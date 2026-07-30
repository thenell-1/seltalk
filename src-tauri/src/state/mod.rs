// TODO 人工审查点：1.Mutex 中毒处理 2.Client 超时 3.状态注入 Tauri 4.看门狗时间戳 5.缓存写时失效一致性
// NOTE 全局应用状态：持有 DB 连接、HTTP 客户端、任务锁、中断标志、配置缓存、任务获取时间、运行时缓存
pub mod task_lock;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use regex::Regex;
use rusqlite::Connection;

use crate::config::{AppConfig, DEFAULT_LLM_TIMEOUT_SECS};
use crate::error::{AppError, AppResult};
use crate::llm::types::LlmConfig;

pub struct AppState {
    /// SQLite 连接（串行化访问）
    pub db: Mutex<Connection>,
    /// HTTP 客户端（LLM 请求复用）
    pub http: reqwest::Client,
    /// 任务锁：热键触发互斥，同一时间只允许一次主链路
    pub task_lock: Mutex<bool>,
    /// 任务锁获取时间戳（看门狗用：检测卡死后强制释放）
    pub task_acquired_at: Mutex<Option<Instant>>,
    /// 输入中断标志：逐字输入时每字前检查
    pub interrupt: Arc<AtomicBool>,
    /// 运行时配置缓存（避免每次读 DB）
    pub config_cache: RwLock<AppConfig>,
    /// 热键触发时的前台目标窗口句柄（逐字输入前校验焦点）
    pub target_hwnd: Mutex<isize>,
    /// 是否正在逐字输入（区分"悬浮窗显示中"与"输入进行中"，防止 cancel/trigger 竞态）
    pub is_typing: AtomicBool,
    /// 热键是否已暂停（托盘"暂停热键"开关，true 时热键触发被忽略）
    pub hotkey_paused: AtomicBool,
    // ===== P0 运行时缓存（写时失效：任何设置变更递增 config_version）=====
    /// LLM 配置缓存（避免每次触发从 DB 加载）
    pub llm_cfg_cache: RwLock<LlmConfig>,
    /// 默认 Prompt 模板缓存
    pub prompt_cache: RwLock<Option<String>>,
    /// 黑名单编译后的正则缓存（避免每次触发重新编译）
    pub blacklist_cache: RwLock<Vec<Regex>>,
    /// 启用词库拼接字符串缓存（用于 {{words}} 注入）
    pub words_cache: RwLock<String>,
    /// 配置版本号：任何设置/模板/黑名单/词库变更时 +1，trigger 入口比对决定是否重载缓存
    pub config_version: AtomicU64,
    /// 上次缓存加载时的配置版本号（小于 config_version 则缓存已过期，需重载）
    pub cache_loaded_version: AtomicU64,
}

impl AppState {
    pub fn new(db: Connection) -> AppResult<Self> {
        let http = reqwest::Client::builder()
            // 连接超时分级：连接阶段（DNS+TCP+TLS）5s 快速失败，
            // 避免网络异常时拖到整体 30s 超时才反馈，便于快速诊断网络问题
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(DEFAULT_LLM_TIMEOUT_SECS))
            .build()
            .map_err(|e| AppError::Llm(format!("HTTP 客户端初始化失败: {e}")))?;
        Ok(Self {
            db: Mutex::new(db),
            http,
            task_lock: Mutex::new(false),
            task_acquired_at: Mutex::new(None),
            interrupt: Arc::new(AtomicBool::new(false)),
            config_cache: RwLock::new(AppConfig::default()),
            target_hwnd: Mutex::new(0),
            is_typing: AtomicBool::new(false),
            hotkey_paused: AtomicBool::new(false),
            llm_cfg_cache: RwLock::new(LlmConfig::default()),
            prompt_cache: RwLock::new(None),
            blacklist_cache: RwLock::new(Vec::new()),
            words_cache: RwLock::new(String::new()),
            // 初始 config_version=1 / cache_loaded_version=0，保证首次触发 cache_stale()=true 走懒加载
            config_version: AtomicU64::new(1),
            cache_loaded_version: AtomicU64::new(0),
        })
    }

    /// 配置是否已变更（缓存是否过期）。true 表示需要重载缓存。
    pub fn cache_stale(&self) -> bool {
        self.cache_loaded_version.load(Ordering::Relaxed) != self.config_version.load(Ordering::Relaxed)
    }

    /// 标记缓存已与当前 config_version 同步（重载完成后调用）
    pub fn mark_cache_synced(&self) {
        self.cache_loaded_version
            .store(self.config_version.load(Ordering::Relaxed), Ordering::Relaxed);
    }

    /// 使缓存失效（任何写操作后调用）：递增 config_version
    pub fn invalidate_cache(&self) {
        self.config_version.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_state() -> AppState {
        let conn = Connection::open_in_memory().unwrap();
        AppState::new(conn).unwrap()
    }

    #[test]
    fn test_cache_stale_initial() {
        // 初始 config_version=1 / cache_loaded_version=0 → 首次触发应走懒加载
        let state = new_state();
        assert!(state.cache_stale());
    }

    #[test]
    fn test_cache_invalidate_and_sync_cycle() {
        let state = new_state();
        // 初始 stale
        assert!(state.cache_stale());
        // 同步后不再 stale
        state.mark_cache_synced();
        assert!(!state.cache_stale());
        // 写操作失效后又 stale
        state.invalidate_cache();
        assert!(state.cache_stale());
        // 再次同步
        state.mark_cache_synced();
        assert!(!state.cache_stale());
    }

    #[test]
    fn test_invalidate_cache_monotonic() {
        let state = new_state();
        let v0 = state.config_version.load(Ordering::Relaxed);
        state.invalidate_cache();
        state.invalidate_cache();
        let v1 = state.config_version.load(Ordering::Relaxed);
        // 两次失效后版本号递增 2
        assert_eq!(v1, v0 + 2);
    }
}
