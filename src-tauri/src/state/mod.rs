// TODO 人工审查点：1.Mutex 中毒处理 2.Client 超时 3.状态注入 Tauri 4.看门狗时间戳 5.缓存写时失效一致性 6.连接池获取 7.FocusManager 生命周期
// NOTE 全局应用状态：持有 DB 连接池、HTTP 客户端、任务锁、中断标志、配置缓存、任务获取时间、运行时缓存
//       P1.1：DB 改用 r2d2 连接池，读操作可并发（WAL 模式下读不阻塞写）
//       P-FOCUS-MGR: 注入 FocusManager（WinEvent 钩子 + 焦点缓存 + UIA 兜底）
pub mod task_lock;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use regex::Regex;

use crate::config::{AppConfig, DEFAULT_LLM_TIMEOUT_SECS};
use crate::error::{AppError, AppResult};
use crate::focus::FocusManager;
use crate::llm::types::LlmConfig;

/// SQLite 连接池类型别名（简化 commands.rs 等模块的类型签名）
pub type DbPool = Pool<SqliteConnectionManager>;

pub struct AppState {
    /// SQLite 连接池（P1.1：替代原 Mutex<Connection>，支持并发读）
    pub db: DbPool,
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
    /// 焦点管理器：WinEvent 钩子 + 焦点缓存 + UIA 兜底（P-FOCUS-MGR）
    /// 替代原 target_hwnd 快照机制，实时追踪用户最后激活的可输入控件
    pub focus: Arc<FocusManager>,
    /// 是否正在逐字输入（区分"悬浮窗显示中"与"输入进行中"，防止 cancel/trigger 竞态）
    pub is_typing: AtomicBool,
    /// 热键是否已暂停（托盘"暂停热键"开关，true 时热键触发被忽略）
    pub hotkey_paused: AtomicBool,
    /// 临时置顶标志（运行时，不持久化）：true 时悬浮窗显示期间强制置顶，
    /// 隐藏时自动清零（"临时置顶"模式：仅悬浮窗打开期间有效）
    pub temp_on_top: AtomicBool,
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
    /// 上次 trigger 的过滤后文本（供 R 键重新生成使用，避免重读剪贴板）
    /// - trigger 入口写入；cancel/输入完成时清理，防止跨会话误用
    pub last_filtered_text: Mutex<Option<String>>,
}

impl AppState {
    pub fn new(db: DbPool) -> AppResult<Self> {
        let http = reqwest::Client::builder()
            // 连接超时分级：连接阶段（DNS+TCP+TLS）5s 快速失败，
            // 避免网络异常时拖到整体 30s 超时才反馈，便于快速诊断网络问题
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(DEFAULT_LLM_TIMEOUT_SECS))
            .build()
            .map_err(|e| AppError::Llm(format!("HTTP 客户端初始化失败: {e}")))?;
        Ok(Self {
            db,
            http,
            task_lock: Mutex::new(false),
            task_acquired_at: Mutex::new(None),
            interrupt: Arc::new(AtomicBool::new(false)),
            config_cache: RwLock::new(AppConfig::default()),
            focus: Arc::new(FocusManager::new()?),
            is_typing: AtomicBool::new(false),
            hotkey_paused: AtomicBool::new(false),
            temp_on_top: AtomicBool::new(false),
            llm_cfg_cache: RwLock::new(LlmConfig::default()),
            prompt_cache: RwLock::new(None),
            blacklist_cache: RwLock::new(Vec::new()),
            words_cache: RwLock::new(String::new()),
            // 初始 config_version=1 / cache_loaded_version=0，保证首次触发 cache_stale()=true 走懒加载
            config_version: AtomicU64::new(1),
            cache_loaded_version: AtomicU64::new(0),
            last_filtered_text: Mutex::new(None),
        })
    }

    /// 从连接池获取一个连接（P1.1：替代原 state.db.lock()）
    ///
    /// 返回的 `PooledConnection` 实现了 `Deref<Target=Connection>`，
    /// 可直接传给所有原接受 `&Connection` 的 db 模块函数。
    ///
    /// 错误处理：连接池耗尽或初始化失败时返回 `AppError::Db`
    pub fn db(&self) -> AppResult<r2d2::PooledConnection<SqliteConnectionManager>> {
        self.db
            .get()
            .map_err(|e| AppError::Db(rusqlite::Error::ToSqlConversionFailure(Box::new(e))))
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
        // P1.1：连接池替代单个 Connection
        // 注：SqliteConnectionManager::memory() 每个连接是独立内存库，
        //     测试中用 max_size=1 保证多次 .get() 拿到同一个内存库
        let manager = SqliteConnectionManager::memory();
        let pool = r2d2::Pool::builder()
            .max_size(1)
            .build(manager)
            .unwrap();
        // 在连接池中初始化全部表结构（取一个连接执行 SCHEMA_SQL 后归还）
        {
            let conn = pool.get().unwrap();
            conn.execute_batch(crate::db::schema::SCHEMA_SQL).unwrap();
        }
        AppState::new(pool).unwrap()
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
