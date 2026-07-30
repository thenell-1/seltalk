// TODO 人工审查点：1.锁生命周期管理 2.异步 spawn 安全 3.中断竞态 4.错误路径锁释放 5.看门狗强制释放
// NOTE 主链路编排：trigger(热键→生成→显示) + type_candidate(选中→逐字输入) + cancel(取消)
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::config::{
    AppConfig, DEFAULT_CANDIDATE_COUNT, DEFAULT_FLOAT_ALWAYS_ON_TOP, DEFAULT_FLOAT_H,
    DEFAULT_FLOAT_W, DEFAULT_HOTKEY, DEFAULT_TYPE_MAX_MS, DEFAULT_TYPE_MIN_MS,
    KEY_BLACKLIST, KEY_CANDIDATE_COUNT, KEY_FLOAT_ALWAYS_ON_TOP, KEY_FLOAT_H, KEY_FLOAT_W,
    KEY_HOTKEY, KEY_TYPE_MAX_MS, KEY_TYPE_MIN_MS, TASK_LOCK_WATCHDOG_SECS,
};
use crate::db::{settings, word_freq, words};
use crate::error::{AppError, AppResult};
use crate::state::{task_lock, AppState};
use crate::{clipboard, input, llm, text, window};

/// 发送给悬浮窗的候选数据载荷
#[derive(Clone, Serialize)]
struct CandidatesPayload {
    /// 原始识别文本（清洗后）
    origin: String,
    /// AI 候选回复列表
    candidates: Vec<String>,
}

/// 从 DB settings 表加载完整运行时配置
pub fn load_config_from_db(db: &rusqlite::Connection) -> AppResult<AppConfig> {
    let get = |k: &str| settings::get_setting(db, k).ok().flatten();
    Ok(AppConfig {
        hotkey: get(KEY_HOTKEY).unwrap_or_else(|| DEFAULT_HOTKEY.to_string()),
        candidate_count: get(KEY_CANDIDATE_COUNT)
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_CANDIDATE_COUNT),
        type_min_ms: get(KEY_TYPE_MIN_MS)
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_TYPE_MIN_MS),
        type_max_ms: get(KEY_TYPE_MAX_MS)
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_TYPE_MAX_MS),
        float_w: get(KEY_FLOAT_W)
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_FLOAT_W),
        float_h: get(KEY_FLOAT_H)
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_FLOAT_H),
        float_always_on_top: get(KEY_FLOAT_ALWAYS_ON_TOP)
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(DEFAULT_FLOAT_ALWAYS_ON_TOP),
    })
}

/// 刷新运行时缓存（LLM 配置/模板/黑名单/词库），仅在 cache_stale 时由 trigger 调用
///
/// 一次 DB lock 完成四项加载，写入 RwLock 缓存并 `mark_synced`。
/// 后续 trigger 步骤直接读缓存，避免每次触发重复 DB 读 + 正则编译开销。
fn refresh_runtime_cache(state: &AppState) -> AppResult<()> {
    let db = state
        .db
        .lock()
        .map_err(|e| AppError::Config(format!("DB 锁中毒: {e}")))?;

    // 1. LLM 配置（含 base_url/api_key/model/temperature/max_tokens/stream_enabled）
    let cfg = llm::load_llm_config(&db)?;
    if let Ok(mut cache) = state.llm_cfg_cache.write() {
        *cache = cfg;
    }

    // 2. 默认 Prompt 模板（缺失时回退内置默认）
    let template = crate::db::prompts::prompt_get_default(&db)?
        .map(|p| p.template)
        .unwrap_or_else(|| "回复：{{origin}}".into());
    if let Ok(mut cache) = state.prompt_cache.write() {
        *cache = Some(template);
    }

    // 3. 黑名单（解析 JSON → 编译正则，缓存编译结果，避免每次触发重新编译）
    let blacklist_json = settings::get_setting(&db, KEY_BLACKLIST)
        .ok()
        .flatten()
        .unwrap_or_default();
    let patterns = text::filter::parse_blacklist_json(&blacklist_json);
    let compiled = text::filter::compile_patterns(&patterns);
    if let Ok(mut cache) = state.blacklist_cache.write() {
        *cache = compiled;
    }

    // 4. 启用词库拼接字符串（用于 {{words}} 变量注入）
    let words_list = words::word_get_enabled(&db).unwrap_or_default();
    let words_joined = words_list.join("、");
    if let Ok(mut cache) = state.words_cache.write() {
        *cache = words_joined;
    }

    state.mark_cache_synced();
    tracing::debug!("运行时缓存已刷新");
    Ok(())
}

/// 释放任务锁并清除获取时间戳（看门狗配套）
fn release_task_lock(state: &AppState) {
    task_lock::release(&state.task_lock);
    if let Ok(mut t) = state.task_acquired_at.lock() {
        *t = None;
    }
}

/// 尝试获取任务锁；忙时检查看门狗，卡死（超过 TASK_LOCK_WATCHDOG_SECS 秒）则强制释放后重试
/// 成功返回 true，失败（忙或异常）返回 false
fn acquire_with_watchdog(state: &AppState) -> bool {
    match task_lock::acquire(&state.task_lock) {
        Ok(()) => {
            if let Ok(mut t) = state.task_acquired_at.lock() {
                *t = Some(Instant::now());
            }
            true
        }
        Err(AppError::Busy) => {
            // 看门狗：检查任务锁是否卡死（获取后超时未释放）
            let stale = state
                .task_acquired_at
                .lock()
                .map(|t| {
                    t.map(|i| i.elapsed().as_secs() > TASK_LOCK_WATCHDOG_SECS)
                        .unwrap_or(false)
                })
                .unwrap_or(false);
            if !stale {
                tracing::info!("任务忙，忽略重复触发");
                return false;
            }
            tracing::error!("任务锁卡死超过 {TASK_LOCK_WATCHDOG_SECS}s，看门狗强制释放");
            release_task_lock(state);
            // 强制释放后重新获取
            match task_lock::acquire(&state.task_lock) {
                Ok(()) => {
                    if let Ok(mut t) = state.task_acquired_at.lock() {
                        *t = Some(Instant::now());
                    }
                    tracing::info!("看门狗释放后重新获取锁成功");
                    true
                }
                Err(e) => {
                    tracing::error!("看门狗释放后重新获取锁失败: {e}");
                    false
                }
            }
        }
        Err(e) => {
            tracing::error!("任务锁获取异常: {e}");
            false
        }
    }
}

/// 热键触发入口（异步）：立即显示悬浮窗(loading)→读取剪贴板→LLM 生成→填充候选
///
/// 任务锁在入口获取，由 `type_candidate` 或 `cancel` 释放。
/// 任何错误路径均确保锁被释放。
///
/// 性能设计：悬浮窗在 LLM 请求**之前**就显示 loading，用户按下热键后可在
/// 1 秒内看到响应；LLM 完成后再发送候选数据，悬浮窗自动切换到 ready 状态。
pub async fn trigger(app: AppHandle) {
    let state = app.state::<AppState>();

    // 1. 获取任务锁（忙时直接忽略；卡死时看门狗强制释放后重试）
    if !acquire_with_watchdog(&state) {
        return;
    }

    // 2. 重置中断标志 + 记录目标窗口 + 诊断日志
    state.interrupt.store(false, Ordering::Relaxed);
    state.is_typing.store(false, Ordering::Relaxed);
    let hwnd = input::sendinput::get_foreground_hwnd();
    if let Ok(mut h) = state.target_hwnd.lock() {
        *h = hwnd;
    }
    // 诊断：记录热键触发时的前台窗口标题，便于排查焦点漂移
    if let Ok(Some(title)) = input::sendinput::get_foreground_title() {
        let label: &str = if title.is_empty() { "(无标题)" } else { title.as_str() };
        tracing::debug!("热键触发，前台窗口: {}", label);
    }

    // 3. 刷新配置缓存（仅 cache_stale 时执行，避免每次触发重复 DB 加载）
    if state.cache_stale() {
        // 3a. AppConfig（热键/打字速度/悬浮窗尺寸等）
        let cfg = {
            let db = match state.db.lock() {
                Ok(db) => db,
                Err(e) => {
                    tracing::error!("DB 锁中毒: {e}");
                    release_task_lock(&state);
                    return;
                }
            };
            match load_config_from_db(&db) {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("配置加载失败: {e}");
                    release_task_lock(&state);
                    return;
                }
            }
        };
        if let Ok(mut cache) = state.config_cache.write() {
            *cache = cfg;
        }
        // 3b. 运行时缓存（LLM 配置/模板/黑名单/词库）
        if let Err(e) = refresh_runtime_cache(&state) {
            tracing::error!("运行时缓存刷新失败: {e}");
            release_task_lock(&state);
            return;
        }
    }

    // 4. 读取剪贴板纯文本（get_text 为纯读操作，通常 <10ms，不破坏剪贴板内容）
    //    无文本时返回空串，由后续空判断静默忽略（不显示悬浮窗，避免闪现）
    let raw_text = clipboard::read_text_or_empty();

    // 5. 清洗文本；为空则静默忽略（不显示悬浮窗，避免无文本时闪现后消失）
    let cleaned = text::clean::clean(&raw_text);
    if cleaned.is_empty() {
        tracing::info!("剪贴板文本为空，忽略本次触发");
        release_task_lock(&state);
        return;
    }

    // 6. 【性能优化】有文本才显示悬浮窗(loading)，让用户在 1 秒内感知响应。
    //    后续 LLM 完成后再发送候选数据，悬浮窗自动切换到 ready 状态。
    let _ = app.emit_to(window::FLOAT_LABEL, "candidates-loading", ());
    if let Err(e) = window::show_float(&app) {
        tracing::warn!("悬浮窗显示失败: {e}");
    }

    // 7. 黑名单过滤 + 词库拼接（从运行时缓存读，无 DB 操作）
    //    缓存由步骤3 refresh_runtime_cache 填充；此处仅 clone 编译后的正则与词串
    let (filtered_text, words_str) = {
        let compiled = state
            .blacklist_cache
            .read()
            .map(|c| c.clone())
            .unwrap_or_default();
        let hit = compiled.iter().any(|re| re.is_match(&cleaned));
        let filtered = text::filter::apply_blacklist(&cleaned, &compiled);
        if hit {
            tracing::info!("黑名单命中，已对敏感片段脱敏后再送 LLM");
        }
        let words_str = state
            .words_cache
            .read()
            .map(|c| c.clone())
            .unwrap_or_default();
        (filtered, words_str)
    };

    // 8. LLM 生成候选（从缓存读 cfg + template；按 stream_enabled 选流式/非流式分支）
    //    流式分支：边生成边 emit "candidates-stream"，首字延迟从总生成时间降到首 token 时间
    let n = state
        .config_cache
        .read()
        .map(|c| c.candidate_count)
        .unwrap_or(DEFAULT_CANDIDATE_COUNT);
    let (cfg, template) = {
        let cfg = state
            .llm_cfg_cache
            .read()
            .map(|c| c.clone())
            .unwrap_or_default();
        let template = state
            .prompt_cache
            .read()
            .map(|c| c.clone().unwrap_or_else(|| "回复：{{origin}}".into()))
            .unwrap_or_else(|_| "回复：{{origin}}".into());
        (cfg, template)
    };

    let gen_result: AppResult<Vec<String>> = if cfg.stream_enabled {
        llm::generate_candidates_stream(&app, &filtered_text, n, &words_str, &cfg, &template).await
    } else {
        llm::generate_candidates(&app, &filtered_text, n, &words_str, &cfg, &template).await
    };
    let candidates = match gen_result {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("LLM 生成失败: {e}");
            // 通知悬浮窗切换到 error 状态（悬浮窗已显示，无需再 show）
            let _ = app.emit_to(window::FLOAT_LABEL, "candidates-error", e.to_string());
            release_task_lock(&state);
            return;
        }
    };

    if candidates.is_empty() {
        tracing::info!("无候选回复，忽略");
        let _ = window::hide_float(&app);
        release_task_lock(&state);
        return;
    }

    // 9. 发送候选数据到悬浮窗（origin 为脱敏后文本），悬浮窗自动切换到 ready
    let payload = CandidatesPayload {
        origin: filtered_text,
        candidates,
    };
    if let Err(e) = app.emit_to(window::FLOAT_LABEL, "candidates-ready", payload) {
        tracing::error!("事件发送失败: {e}");
        let _ = window::hide_float(&app);
        release_task_lock(&state);
        return;
    }

    tracing::info!("主链路触发完成，等待用户选择");
    // 锁保持，由 type_candidate 或 cancel 释放
}

/// 用户选中候选 → 逐字输入（阻塞型，需 spawn_blocking 调用）
///
/// 调用方负责在异步上下文中用 `spawn_blocking` 包装本函数。
/// 函数返回后锁已释放。
pub fn do_type_candidate(app: &AppHandle, text: &str) -> AppResult<()> {
    let state = app.state::<AppState>();

    // 标记输入中
    state.is_typing.store(true, Ordering::Relaxed);

    // 1. 隐藏悬浮窗
    if let Err(e) = window::hide_float(app) {
        tracing::warn!("隐藏悬浮窗失败: {e}");
    }

    // 2. 等待焦点回归目标窗口（Windows 在窗口隐藏后会自动切换焦点）
    std::thread::sleep(Duration::from_millis(120));

    // 3. 读取目标窗口句柄 + 打字速度
    let hwnd = state
        .target_hwnd
        .lock()
        .map(|h| *h)
        .unwrap_or(0);
    let (min_ms, max_ms) = state
        .config_cache
        .read()
        .map(|c| (c.type_min_ms, c.type_max_ms))
        .unwrap_or((DEFAULT_TYPE_MIN_MS, DEFAULT_TYPE_MAX_MS));

    // 4. 逐字输入（每字前检查中断 + 焦点漂移）
    let interrupt = state.interrupt.clone();
    let result = input::type_text(text, min_ms, max_ms, &interrupt, hwnd);

    // 5. 标记输入结束 + 释放锁
    state.is_typing.store(false, Ordering::Relaxed);
    state.interrupt.store(false, Ordering::Relaxed);
    release_task_lock(&state);

    match result {
        Ok(()) => {
            tracing::info!("逐字输入完成");
            // 词频记录异步化：spawn 独立线程执行 DB 写入，不阻塞命令返回
            // （失败不影响主流程；AppHandle/Vec<String> 均 Send，可安全 move 到新线程）
            let words = text::wordcloud::extract_words(text);
            if !words.is_empty() {
                let app_clone = app.clone();
                std::thread::spawn(move || {
                    // state 借用 app_clone；guard 绑定为局部变量，drop 顺序明确（guard 先于 state 释放）
                    let state = app_clone.state::<AppState>();
                    let guard = state.db.lock();
                    if let Ok(db) = guard {
                        if let Err(e) = word_freq::record_batch(&db, &words) {
                            tracing::warn!("词频记录失败: {e}");
                        }
                    }
                });
            }
            Ok(())
        }
        Err(AppError::Interrupted) => {
            tracing::info!("输入被用户中断");
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// 取消本次会话（ESC / 点击窗外 / 关闭按钮）
///
/// - 若正在逐字输入：仅设置中断标志，由 `do_type_candidate` 释放锁
/// - 若悬浮窗显示中：隐藏悬浮窗 + 释放锁
pub fn do_cancel(app: &AppHandle) -> AppResult<()> {
    let state = app.state::<AppState>();

    // 设置中断标志（无论何种状态都设置，确保输入循环停止）
    state.interrupt.store(true, Ordering::Relaxed);

    if state.is_typing.load(Ordering::Relaxed) {
        // 正在输入中：不释放锁，由 do_type_candidate 完成后释放
        tracing::info!("取消请求已发送（输入进行中，等待中断生效）");
    } else {
        // 悬浮窗显示中：隐藏 + 释放锁
        if let Err(e) = window::hide_float(app) {
            tracing::warn!("隐藏悬浮窗失败: {e}");
        }
        release_task_lock(&state);
        tracing::info!("会话已取消，锁已释放");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_config_defaults_when_empty() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(crate::db::schema::SCHEMA_SQL).unwrap();
        let cfg = load_config_from_db(&conn).unwrap();
        assert_eq!(cfg.hotkey, DEFAULT_HOTKEY);
        assert_eq!(cfg.candidate_count, DEFAULT_CANDIDATE_COUNT);
        assert_eq!(cfg.type_min_ms, DEFAULT_TYPE_MIN_MS);
    }

    #[test]
    fn test_load_config_from_db_values() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(crate::db::schema::SCHEMA_SQL).unwrap();
        settings::set_setting(&conn, KEY_HOTKEY, "Alt+Q").unwrap();
        settings::set_setting(&conn, KEY_CANDIDATE_COUNT, "5").unwrap();
        let cfg = load_config_from_db(&conn).unwrap();
        assert_eq!(cfg.hotkey, "Alt+Q");
        assert_eq!(cfg.candidate_count, 5);
    }

    #[test]
    fn test_refresh_runtime_cache_populates_and_syncs() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(crate::db::schema::SCHEMA_SQL).unwrap();
        crate::db::seed_if_empty(&conn).unwrap();
        let state = AppState::new(conn).unwrap();

        // 初始 stale（首次触发应走懒加载）
        assert!(state.cache_stale());
        // 刷新缓存
        refresh_runtime_cache(&state).unwrap();
        // 同步后不再 stale
        assert!(!state.cache_stale());
        // 默认模板已填充（seed 后应有默认模板）
        {
            let tpl = state.prompt_cache.read().unwrap();
            assert!(tpl.is_some());
        }
        // 黑名单/词库缓存可读（不 panic 即可）
        let _bl = state.blacklist_cache.read().unwrap();
        let _words = state.words_cache.read().unwrap();
    }

    #[test]
    fn test_refresh_runtime_cache_picks_up_changes() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(crate::db::schema::SCHEMA_SQL).unwrap();
        crate::db::seed_if_empty(&conn).unwrap();
        let state = AppState::new(conn).unwrap();

        // 首次刷新
        refresh_runtime_cache(&state).unwrap();
        assert!(!state.cache_stale());

        // 修改 LLM 模型 → 失效 → 再次刷新应加载新值
        {
            let db = state.db.lock().unwrap();
            settings::set_setting(&db, crate::config::KEY_LLM_MODEL, "gpt-test").unwrap();
        }
        state.invalidate_cache();
        assert!(state.cache_stale());
        refresh_runtime_cache(&state).unwrap();
        let cfg = state.llm_cfg_cache.read().unwrap().clone();
        assert_eq!(cfg.model, "gpt-test");
    }
}
