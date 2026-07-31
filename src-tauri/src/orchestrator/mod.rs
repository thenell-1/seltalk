// TODO 人工审查点：1.锁生命周期管理 2.异步 spawn 安全 3.中断竞态 4.错误路径锁释放 5.看门狗强制释放
// NOTE 主链路编排：trigger(热键→生成→显示) + type_candidate(选中→逐字输入) + cancel(取消)
use std::sync::atomic::Ordering;
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::config::{
    AppConfig, DEFAULT_CANDIDATE_COUNT, DEFAULT_CLIPBOARD_MODE, DEFAULT_FLOAT_ALWAYS_ON_TOP,
    DEFAULT_FLOAT_H, DEFAULT_FLOAT_W, DEFAULT_HOTKEY, DEFAULT_TYPE_MAX_MS, DEFAULT_TYPE_MIN_MS,
    KEY_BLACKLIST, KEY_CANDIDATE_COUNT, KEY_CLIPBOARD_MODE, KEY_FLOAT_ALWAYS_ON_TOP, KEY_FLOAT_H,
    KEY_FLOAT_W, KEY_HOTKEY, KEY_TYPE_MAX_MS, KEY_TYPE_MIN_MS, TASK_LOCK_WATCHDOG_SECS,
};
use crate::db::{history, prompts, settings, word_freq, words};
use crate::error::{AppError, AppResult};
use crate::state::{task_lock, AppState};
use crate::{clipboard, input, llm, text, window};

/// 请求体最大字符数：避免超长文本撑爆 LLM token 限制或触发 413
const MAX_REQUEST_CHARS: usize = 5000;

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

/// 读取剪贴板处理模式（"A"=兼容复原 / "B"=纯净只读，默认 B）
///
/// 缺失或读取失败时返回默认值 `DEFAULT_CLIPBOARD_MODE`（"B"）。
/// 抽取为纯函数便于单元测试。
pub fn read_clipboard_mode(db: &rusqlite::Connection) -> String {
    settings::get_setting(db, KEY_CLIPBOARD_MODE)
        .ok()
        .flatten()
        .unwrap_or_else(|| DEFAULT_CLIPBOARD_MODE.to_string())
}

/// 刷新运行时缓存（LLM 配置/模板/黑名单/词库），仅在 cache_stale 时由 trigger 调用
///
/// 一次 DB lock 完成四项加载，写入 RwLock 缓存并 `mark_synced`。
/// 后续 trigger 步骤直接读缓存，避免每次触发重复 DB 读 + 正则编译开销。
fn refresh_runtime_cache(state: &AppState) -> AppResult<()> {
    // P1.1：从连接池获取连接（替代原 state.db.lock()）
    let db = state.db()?;

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

    // 2. 重置中断标志 + 诊断日志
    //    P-FOCUS-MGR：目标窗口追踪由 FocusManager 的 WinEvent 钩子自动完成，
    //    无需在 trigger 入口手动快照前台窗口（旧的 target_hwnd 机制已废弃）
    state.interrupt.store(false, Ordering::Relaxed);
    state.is_typing.store(false, Ordering::Relaxed);
    // 诊断：记录热键触发时的前台窗口标题，便于排查焦点漂移
    if let Ok(Some(title)) = input::sendinput::get_foreground_title() {
        let label: &str = if title.is_empty() { "(无标题)" } else { title.as_str() };
        tracing::debug!("热键触发，前台窗口: {}", label);
    }

    // 3. 刷新配置缓存（仅 cache_stale 时执行，避免每次触发重复 DB 加载）
    if state.cache_stale() {
        // 3a. AppConfig（热键/打字速度/悬浮窗尺寸等）
        let cfg = {
            // P1.1：从连接池获取连接（替代原 state.db.lock()）
            let db = match state.db() {
                Ok(db) => db,
                Err(e) => {
                    tracing::error!("DB 连接池获取失败: {e}");
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

    // 4. 读取剪贴板纯文本（按剪贴板处理模式分支）
    //    模式B（默认）：纯读，不修改剪贴板（Win+V 历史无杂乱）
    //    模式A（兼容复原）：快照→读文本→复原（操作后剪贴板恢复原内容，但会新增一条 Win+V 历史）
    //    无文本时返回空串，由后续空判断静默忽略（不显示悬浮窗，避免闪现）
    let raw_text = {
        let mode = match state.db() {
            Ok(db) => read_clipboard_mode(&db),
            Err(e) => {
                tracing::warn!("读取剪贴板模式配置失败，使用默认模式B: {e}");
                DEFAULT_CLIPBOARD_MODE.to_string()
            }
        };
        if mode == "A" {
            tracing::debug!("剪贴板模式A（兼容复原）：快照→读文本→复原");
            clipboard::read_text_with_restore()
        } else {
            clipboard::read_text_or_empty()
        }
    };

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

    // 7.4. 请求体大小限制：截断到 MAX_REQUEST_CHARS 字符，避免超长文本撑爆 LLM token
    let filtered_text: String = if filtered_text.chars().count() > MAX_REQUEST_CHARS {
        tracing::info!(
            "请求文本超长（{} 字符），已截断到 {} 字符",
            filtered_text.chars().count(),
            MAX_REQUEST_CHARS
        );
        filtered_text.chars().take(MAX_REQUEST_CHARS).collect()
    } else {
        filtered_text
    };

    // 7.5. 保存过滤后文本，供 R 键重新生成使用（不重读剪贴板，避免用户已复制其他内容）
    if let Ok(mut t) = state.last_filtered_text.lock() {
        *t = Some(filtered_text.clone());
    }

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
///
/// P-FOCUS-MGR 流程：
/// 1. `FocusManager::validate_and_restore` 实时校验焦点 + 恢复（悬浮窗隐藏前调用）
/// 2. 校验通过后隐藏悬浮窗（此时目标窗口已是前台，隐藏不导致焦点漂移）
/// 3. `type_text` 逐字发送，每字前检测焦点漂移（top_hwnd 比对）
/// 4. 焦点漂移时返回 `AppError::Input`，前端提示用户（悬浮窗已隐藏）
pub fn do_type_candidate(app: &AppHandle, text: &str) -> AppResult<()> {
    let state = app.state::<AppState>();

    // 标记输入中
    state.is_typing.store(true, Ordering::Relaxed);

    // 1. 读取打字速度配置（在隐藏悬浮窗之前读取，避免后续持锁时机不确定）
    let (min_ms, max_ms) = state
        .config_cache
        .read()
        .map(|c| (c.type_min_ms, c.type_max_ms))
        .unwrap_or((DEFAULT_TYPE_MIN_MS, DEFAULT_TYPE_MAX_MS));

    // 2. P-FOCUS-MGR：实时校验焦点 + 恢复到目标控件（在悬浮窗隐藏之前调用）
    //    此时悬浮窗仍可见（WS_EX_NOACTIVATE 不抢焦点），FocusManager 缓存的焦点上下文
    //    即用户最后激活的可输入控件。validate_and_restore 内部：
    //      ① 实时读取焦点上下文（禁止使用 trigger 时的旧缓存）
    //      ② 校验：焦点控件有效 + 非本程序窗口 + 未最小化 + 可见
    //      ③ 校验失败时尝试 UIA 兜底搜索可编辑控件
    //      ④ restore_foreground 恢复顶层窗口前台 + set_focus_to_ctl 恢复子控件键盘焦点
    //    校验失败：返回 Err，悬浮窗保持可见，由前端显示错误提示。
    let ctx = match state.focus.validate_and_restore() {
        Ok(ctx) => ctx,
        Err(e) => {
            tracing::warn!("焦点校验失败，取消本次输入: {e}");
            state.is_typing.store(false, Ordering::Relaxed);
            release_task_lock(&state);
            return Err(e);
        }
    };

    // 3. 隐藏悬浮窗（此时目标窗口已是前台，隐藏不再导致焦点漂移）
    //    注：validate_and_restore 内部已 sleep 30ms 等待焦点稳定，无需再额外等待
    if let Err(e) = window::hide_float(app) {
        tracing::warn!("隐藏悬浮窗失败: {e}");
    }

    // 4. 逐字输入（每字前检查中断 + 焦点漂移检测）
    //    focus_hwnd 为顶层窗口 HWND：type_text 内部每字前比对 GetForegroundWindow，
    //    若用户中途切换窗口（焦点漂移），立即中止并返回 Input 错误。
    //    前端 catch 后提示"目标输入框已失去焦点，无法输入"（悬浮窗已隐藏，等待用户再次操作）
    let interrupt = state.interrupt.clone();
    let result = input::type_text(text, min_ms, max_ms, &interrupt, ctx.top_hwnd);

    // 5. 标记输入结束 + 释放锁
    state.is_typing.store(false, Ordering::Relaxed);
    state.interrupt.store(false, Ordering::Relaxed);

    // 6. 在清理 last_filtered_text 之前先读出 origin（用于历史记录）
    //    读完后立即清理，防止跨会话误用
    let origin_text = state
        .last_filtered_text
        .lock()
        .map(|g| g.clone().unwrap_or_default())
        .unwrap_or_default();
    if let Ok(mut t) = state.last_filtered_text.lock() {
        *t = None;
    }
    release_task_lock(&state);

    match result {
        Ok(()) => {
            tracing::info!("逐字输入完成");
            // 词频 + 历史记录异步化：spawn 独立线程执行 DB 写入，不阻塞命令返回
            // （失败不影响主流程；AppHandle/Vec<String> 均 Send，可安全 move 到新线程）
            let words = text::wordcloud::extract_words(text);
            // 当前模板名 + 模型（从缓存读，避免再次 DB 查询）
            // P1.1：从连接池获取连接（替代原 state.db.lock()）
            let prompt_name = state
                .db()
                .ok()
                .and_then(|db| prompts::prompt_get_default(&db).ok().flatten())
                .map(|p| p.name)
                .unwrap_or_default();
            let model = state
                .llm_cfg_cache
                .read()
                .map(|c| c.model.clone())
                .unwrap_or_default();
            let selected_text = text.to_string();

            let app_clone = app.clone();
            std::thread::spawn(move || {
                // P1.1：从连接池获取连接（替代原 state.db.lock()）
                // 注：spawn 内部不能直接 await（std::thread 非 async），但 state.db() 是同步方法
                let state = app_clone.state::<AppState>();
                let db = match state.db() {
                    Ok(db) => db,
                    Err(e) => {
                        tracing::warn!("DB 连接池获取失败（异步记录跳过）: {e}");
                        return;
                    }
                };
                // 1. 词频记录
                if !words.is_empty() {
                    if let Err(e) = word_freq::record_batch(&db, &words) {
                        tracing::warn!("词频记录失败: {e}");
                    }
                }
                // 2. 历史记录（origin/selected 可能为空，空 origin 仍记录便于追溯选中动作）
                let rec = history::HistoryRecord {
                    origin: &origin_text,
                    selected: &selected_text,
                    prompt_name: &prompt_name,
                    model: &model,
                };
                if let Err(e) = history::record(&db, &rec) {
                    tracing::warn!("历史记录失败: {e}");
                }
            });
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
        // 清理上次过滤后文本（会话已取消，防止跨会话误用）
        if let Ok(mut t) = state.last_filtered_text.lock() {
            *t = None;
        }
        release_task_lock(&state);
        tracing::info!("会话已取消，锁已释放");
    }

    Ok(())
}

/// 重新生成时的 temperature 调整：当前值 +0.2，clamp 到 [0.0, 1.5]
///
/// - 上限 1.5：避免温度过高导致乱码（OpenAI 模型一般 0-2 范围）
/// - 下限 0.0：防止负数输入
/// - 抽取为纯函数便于单元测试
fn next_temperature(current: f64) -> f64 {
    (current + 0.2).clamp(0.0, 1.5)
}

/// R 键重新生成候选：用上次过滤后文本 + 更高 temperature 重试
///
/// 设计要点：
/// - 不重新获取任务锁（复用当前会话锁，仅 ready 状态由前端控制触发）
/// - 不重新读剪贴板（用户可能已复制其他内容）
/// - 提升 temperature 0.2，上限 1.5（避免无限提高导致乱码）
/// - 不写回缓存（仅局部修改 cfg 副本，下次 trigger 仍用原值）
pub async fn regenerate(app: AppHandle) {
    let state = app.state::<AppState>();

    // 1. 读取上次缓存的过滤后文本
    let text = match state.last_filtered_text.lock() {
        Ok(guard) => guard.clone().unwrap_or_default(),
        Err(e) => {
            tracing::error!("读取上次文本失败（锁中毒）: {e}");
            return;
        }
    };
    if text.is_empty() {
        tracing::warn!("无上次文本，无法重新生成");
        return;
    }

    // 2. 通知悬浮窗切到 loading（清空候选 + 显示生成中）
    let _ = app.emit_to(window::FLOAT_LABEL, "candidates-loading", ());

    // 3. 提升 temperature（从缓存读 cfg 副本，不写回，下次 trigger 仍用原值）
    let mut cfg = state
        .llm_cfg_cache
        .read()
        .map(|c| c.clone())
        .unwrap_or_default();
    cfg.temperature = next_temperature(cfg.temperature);
    tracing::info!("重新生成，temperature={}", cfg.temperature);

    // 4. 读取模板 + 词库 + 候选数（从缓存，与 trigger 步骤 8 一致）
    let template = state
        .prompt_cache
        .read()
        .map(|c| c.clone().unwrap_or_else(|| "回复：{{origin}}".into()))
        .unwrap_or_else(|_| "回复：{{origin}}".into());
    let words_str = state
        .words_cache
        .read()
        .map(|c| c.clone())
        .unwrap_or_default();
    let n = state
        .config_cache
        .read()
        .map(|c| c.candidate_count)
        .unwrap_or(DEFAULT_CANDIDATE_COUNT);

    // 5. LLM 生成（流式/非流式分支，与 trigger 一致）
    let gen_result: AppResult<Vec<String>> = if cfg.stream_enabled {
        llm::generate_candidates_stream(&app, &text, n, &words_str, &cfg, &template).await
    } else {
        llm::generate_candidates(&app, &text, n, &words_str, &cfg, &template).await
    };

    // 6. emit 候选或错误（与 trigger 步骤 9 一致）
    match gen_result {
        Ok(candidates) if !candidates.is_empty() => {
            let payload = CandidatesPayload {
                origin: text,
                candidates,
            };
            if let Err(e) = app.emit_to(window::FLOAT_LABEL, "candidates-ready", payload) {
                tracing::error!("重新生成事件发送失败: {e}");
            }
            tracing::info!("重新生成完成");
        }
        Ok(_) => {
            tracing::warn!("重新生成无候选");
            let _ = app.emit_to(window::FLOAT_LABEL, "candidates-error", "无候选".to_string());
        }
        Err(e) => {
            tracing::error!("重新生成失败: {e}");
            let _ = app.emit_to(window::FLOAT_LABEL, "candidates-error", e.to_string());
        }
    }
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

    // ===== read_clipboard_mode 测试 =====

    #[test]
    fn test_read_clipboard_mode_default_when_missing() {
        // 未配置时返回默认值 "B"（纯净只读）
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(crate::db::schema::SCHEMA_SQL).unwrap();
        assert_eq!(read_clipboard_mode(&conn), "B");
    }

    #[test]
    fn test_read_clipboard_mode_reads_value() {
        // 配置为 "A" 时返回 "A"
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(crate::db::schema::SCHEMA_SQL).unwrap();
        settings::set_setting(&conn, KEY_CLIPBOARD_MODE, "A").unwrap();
        assert_eq!(read_clipboard_mode(&conn), "A");
    }

    #[test]
    fn test_read_clipboard_mode_reads_value_b() {
        // 配置为 "B" 时返回 "B"
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(crate::db::schema::SCHEMA_SQL).unwrap();
        settings::set_setting(&conn, KEY_CLIPBOARD_MODE, "B").unwrap();
        assert_eq!(read_clipboard_mode(&conn), "B");
    }

    #[test]
    fn test_refresh_runtime_cache_populates_and_syncs() {
        // P1.1：用连接池替代单 Connection（max_size=1 保证内存库共享）
        let manager = r2d2_sqlite::SqliteConnectionManager::memory();
        let pool = r2d2::Pool::builder().max_size(1).build(manager).unwrap();
        {
            let conn = pool.get().unwrap();
            conn.execute_batch(crate::db::schema::SCHEMA_SQL).unwrap();
            crate::db::seed_if_empty(&conn).unwrap();
        }
        let state = AppState::new(pool).unwrap();

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
        // P1.1：用连接池替代单 Connection（max_size=1 保证内存库共享）
        let manager = r2d2_sqlite::SqliteConnectionManager::memory();
        let pool = r2d2::Pool::builder().max_size(1).build(manager).unwrap();
        {
            let conn = pool.get().unwrap();
            conn.execute_batch(crate::db::schema::SCHEMA_SQL).unwrap();
            crate::db::seed_if_empty(&conn).unwrap();
        }
        let state = AppState::new(pool).unwrap();

        // 首次刷新
        refresh_runtime_cache(&state).unwrap();
        assert!(!state.cache_stale());

        // 修改 LLM 模型 → 失效 → 再次刷新应加载新值
        {
            let db = state.db().unwrap();
            // load_llm_config 现从 active profile 读取：创建一条 active profile（model=gpt-test）
            let input = crate::db::llm_profiles::LlmProfileInput {
                name: "test".to_string(),
                base_url: String::new(),
                api_key: String::new(),
                model: "gpt-test".to_string(),
                model_type: String::new(),
                temperature: 0.6,
                max_tokens: 1024,
                max_context_length: 0,
                stream_enabled: true,
            };
            crate::db::llm_profiles::llm_profile_create(&db, &input).unwrap();
        }
        state.invalidate_cache();
        assert!(state.cache_stale());
        refresh_runtime_cache(&state).unwrap();
        let cfg = state.llm_cfg_cache.read().unwrap().clone();
        assert_eq!(cfg.model, "gpt-test");
    }

    // ===== next_temperature 测试（R 键重新生成）=====

    #[test]
    fn test_next_temperature_increment() {
        // 正常递增：+0.2（f64 浮点精度容差 1e-9，避免 0.7+0.2=0.8999... 比较）
        assert!((next_temperature(0.7) - 0.9).abs() < 1e-9);
        assert!((next_temperature(0.9) - 1.1).abs() < 1e-9);
        assert!((next_temperature(1.1) - 1.3).abs() < 1e-9);
    }

    #[test]
    fn test_next_temperature_at_upper_bound() {
        // 到达上限 1.5 后不再提升
        assert_eq!(next_temperature(1.4), 1.5);  // 1.4+0.2=1.6 → clamp 到 1.5
        assert_eq!(next_temperature(1.5), 1.5);  // 已达上限
    }

    #[test]
    fn test_next_temperature_far_above_bound() {
        // 远超上限：仍 clamp 到 1.5
        assert_eq!(next_temperature(5.0), 1.5);
    }

    #[test]
    fn test_next_temperature_negative_input() {
        // 负数输入：clamp 到 0.0（-0.5+0.2=-0.3 → clamp 到 0.0）
        assert_eq!(next_temperature(-0.5), 0.0);
    }

    #[test]
    fn test_next_temperature_zero_input() {
        // 边界：0 输入应返回 0.2
        assert_eq!(next_temperature(0.0), 0.2);
    }
}
