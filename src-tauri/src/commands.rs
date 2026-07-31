// TODO 人工审查点：1.命令错误转字符串 2.spawn_blocking 阻塞型操作 3.状态访问安全 4.参数校验
// NOTE Tauri 命令层：前端 ↔ Rust 桥接，所有 #[tauri::command] 集中定义
use std::collections::HashMap;

use serde::Deserialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

use crate::config::{
    AppConfig, DEFAULT_AUTOSTART, DEFAULT_FLOAT_STYLE_PRESET, KEY_AUTOSTART, KEY_BLACKLIST,
    KEY_FLOAT_STYLE_PRESET,
};
use crate::db::{history, llm_profiles, prompts, settings, word_freq, words, window_state};
use crate::error::err_to_string;
use crate::orchestrator;
use crate::state::AppState;
use crate::{hotkey, llm, text::filter, tray, window};

// ===== 主链路命令 =====

/// 用户选中候选 → 逐字输入
#[tauri::command]
async fn type_candidate(app: AppHandle, text: String) -> Result<(), String> {
    let app_clone = app.clone();
    let result = tokio::task::spawn_blocking(move || {
        orchestrator::do_type_candidate(&app_clone, &text)
    })
    .await
    .map_err(|e| format!("输入任务异常: {e}"))?;

    err_to_string(result)
}

/// 取消本次会话（ESC / 点击窗外 / 关闭按钮）
#[tauri::command]
fn cancel(app: AppHandle) -> Result<(), String> {
    err_to_string(orchestrator::do_cancel(&app))
}

/// R 键重新生成候选（用上次过滤后文本 + 更高 temperature 重试）
#[tauri::command]
async fn regenerate_candidates(app: AppHandle) -> Result<(), String> {
    orchestrator::regenerate(app).await;
    Ok(())
}

/// Ctrl+1/2/3 快捷切换 Prompt 模板（按索引切换，0-based）
/// 切换后失效缓存，下次 trigger 会用新模板
#[tauri::command]
fn switch_prompt_by_index(app: AppHandle, index: usize) -> Result<String, String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;

    let prompts = prompts::prompt_list(&db).map_err(|e| e.to_string())?;
    if index >= prompts.len() {
        return Err(format!("模板索引超出范围（共 {} 个模板）", prompts.len()));
    }

    let target = &prompts[index];
    let target_id = target.id.ok_or("模板 ID 为空")?;
    let target_name = target.name.clone();

    prompts::prompt_set_default(&db, target_id).map_err(|e| e.to_string())?;
    // 失效缓存，下次 trigger / regenerate 会重新加载新模板
    state.invalidate_cache();

    tracing::info!("切换 Prompt 模板: {} (index={})", target_name, index);
    Ok(target_name)
}

// ===== 悬浮窗命令 =====

/// 循环切换悬浮窗置顶模式（Off → Normal → Temp → Off），返回切换后的模式
///
/// - Off: 不置顶
/// - Normal: 普通置顶（持久化到 window_state.always_on_top，不受窗口关闭影响）
/// - Temp: 临时置顶（仅悬浮窗可见期间有效，隐藏后自动失效）
#[tauri::command]
fn cycle_pin_mode(app: AppHandle) -> Result<window::PinMode, String> {
    err_to_string(window::cycle_pin_mode(&app))
}

/// 读取当前置顶模式（供前端初始化图标）
#[tauri::command]
fn get_pin_mode(app: AppHandle) -> Result<window::PinMode, String> {
    err_to_string(window::get_pin_mode(&app))
}

/// 设置悬浮窗透明度（钳制到合法范围 + 持久化到 settings KV）
///
/// 透明度仅前端 CSS 使用，不进入运行时缓存（无需 invalidate_cache）
#[tauri::command]
fn set_float_opacity(app: AppHandle, opacity: f64) -> Result<(), String> {
    err_to_string(window::set_opacity(&app, opacity))
}

/// 读取悬浮窗透明度（缺失返回默认 1.0）
#[tauri::command]
fn get_float_opacity(app: AppHandle) -> Result<f64, String> {
    err_to_string(window::get_opacity(&app))
}

/// 前端在拖拽/缩放结束后保存窗口状态
#[tauri::command]
fn save_float_state(
    app: AppHandle,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    always_on_top: bool,
) -> Result<(), String> {
    let state = window_state::WindowState {
        x,
        y,
        w,
        h,
        always_on_top,
    };
    err_to_string(window::save_state(&app, &state))
}

// ===== 设置命令 =====

/// 读取全部设置（KV），未配置的默认项补充默认值
#[tauri::command]
fn get_all_settings(app: AppHandle) -> Result<HashMap<String, String>, String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    let mut map = err_to_string(settings::get_all_settings(&db))?;
    // 补充 float_style_preset 默认值（未配置时前端直接拿到默认值，无需自行 fallback）
    map.entry(KEY_FLOAT_STYLE_PRESET.to_string())
        .or_insert_with(|| DEFAULT_FLOAT_STYLE_PRESET.to_string());
    Ok(map)
}

/// 写入单个设置项
#[tauri::command]
fn set_setting(app: AppHandle, key: String, value: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    err_to_string(settings::set_setting(&db, &key, &value))?;
    // 写时失效：任何设置变更都使运行时缓存过期（LLM 配置/黑名单/词库等）
    state.invalidate_cache();
    Ok(())
}

/// 读取应用配置（结构化）
#[tauri::command]
fn get_app_config(app: AppHandle) -> Result<AppConfig, String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    err_to_string(orchestrator::load_config_from_db(&db))
}

/// 更新全局热键（保存 DB + 重新注册）
#[tauri::command]
fn update_hotkey(app: AppHandle, hotkey: String) -> Result<(), String> {
    // 1. 校验热键格式 + 系统保留键黑名单（禁止 Ctrl+C 等冲突键）
    hotkey::validate_shortcut(&hotkey).map_err(|e| e.to_string())?;

    // 2. 保存到 DB
    {
        let state = app.state::<AppState>();
        let db = state.db().map_err(|e| e.to_string())?;
        settings::set_setting(&db, crate::config::KEY_HOTKEY, &hotkey)
            .map_err(|e| e.to_string())?;
        // 热键属配置项，使运行时缓存过期
        state.invalidate_cache();
    }

    // 3. 重新注册
    err_to_string(hotkey::register(&app, &hotkey))
}

// ===== Prompt 模板命令 =====

#[tauri::command]
fn prompt_list(app: AppHandle) -> Result<Vec<prompts::PromptTemplate>, String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    err_to_string(prompts::prompt_list(&db))
}

#[tauri::command]
fn prompt_create(
    app: AppHandle,
    name: String,
    template: String,
    tags: Option<String>,
) -> Result<i64, String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    let tags = tags.unwrap_or_default();
    let id = err_to_string(prompts::prompt_create(&db, &name, &template, &tags))?;
    // 模板变更使默认模板缓存过期
    state.invalidate_cache();
    Ok(id)
}

#[tauri::command]
fn prompt_update(
    app: AppHandle,
    id: i64,
    name: String,
    template: String,
    tags: Option<String>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    let tags = tags.unwrap_or_default();
    err_to_string(prompts::prompt_update(&db, id, &name, &template, &tags))?;
    state.invalidate_cache();
    Ok(())
}

/// 查询全库去重后的标签列表（供前端标签自动补全）
#[tauri::command]
fn prompt_all_tags(app: AppHandle) -> Result<Vec<String>, String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    err_to_string(prompts::prompt_all_tags(&db))
}

#[tauri::command]
fn prompt_delete(app: AppHandle, id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    err_to_string(prompts::prompt_delete(&db, id))?;
    state.invalidate_cache();
    Ok(())
}

#[tauri::command]
fn prompt_set_default(app: AppHandle, id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    err_to_string(prompts::prompt_set_default(&db, id))?;
    state.invalidate_cache();
    Ok(())
}

// ===== LLM 命令 =====

/// 测试 LLM 接口连通性（复用 client::test_connection，返回 ok/延迟/消息）
#[tauri::command]
async fn test_llm_connection(
    app: AppHandle,
) -> Result<llm::types::ConnectionTestResult, String> {
    let state = app.state::<AppState>();
    let cfg = {
        let db = state.db().map_err(|e| e.to_string())?;
        llm::load_llm_config(&db).map_err(|e| e.to_string())?
    };

    if cfg.base_url.is_empty() || cfg.api_key.is_empty() || cfg.model.is_empty() {
        return Ok(llm::types::ConnectionTestResult {
            ok: false,
            latency_ms: 0,
            message: "请先配置接口地址、密钥和模型名称".into(),
        });
    }

    err_to_string(llm::client::test_connection(&state.http, &cfg).await)
}

// ===== LLM 配置档案命令 =====

/// 查询全部 LLM 配置档案（active 优先，按更新时间倒序）
#[tauri::command]
fn llm_profile_list(app: AppHandle) -> Result<Vec<llm_profiles::LlmProfile>, String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    err_to_string(llm_profiles::llm_profile_list(&db))
}

/// 查询当前生效的 LLM 配置档案
#[tauri::command]
fn get_active_llm_profile(app: AppHandle) -> Result<Option<llm_profiles::LlmProfile>, String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    err_to_string(llm_profiles::llm_profile_get_active(&db))
}

/// 新建 LLM 配置档案并设为当前生效（新建即切换，主链路立即使用）
#[tauri::command]
fn llm_profile_create(
    app: AppHandle,
    input: llm_profiles::LlmProfileInput,
) -> Result<i64, String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    let id = err_to_string(llm_profiles::llm_profile_create(&db, &input))?;
    // 配置变更使运行时缓存过期，下次 trigger 重新加载新 active 配置
    state.invalidate_cache();
    Ok(id)
}

/// 更新指定 LLM 配置档案（保留 is_active 状态不变）
#[tauri::command]
fn llm_profile_update(
    app: AppHandle,
    id: i64,
    input: llm_profiles::LlmProfileInput,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    err_to_string(llm_profiles::llm_profile_update(&db, id, &input))?;
    state.invalidate_cache();
    Ok(())
}

/// 删除指定 LLM 配置档案（若删除的是 active，自动提升剩余首条）
#[tauri::command]
fn llm_profile_delete(app: AppHandle, id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    err_to_string(llm_profiles::llm_profile_delete(&db, id))?;
    state.invalidate_cache();
    Ok(())
}

/// 将指定 LLM 配置档案设为当前生效（下拉切换：互斥置位，主链路立即使用新配置）
#[tauri::command]
fn llm_profile_set_active(app: AppHandle, id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    err_to_string(llm_profiles::llm_profile_set_active(&db, id))?;
    state.invalidate_cache();
    Ok(())
}

// ===== 词库命令 =====

/// 查询词库列表（支持搜索/分类/启禁用筛选）
#[tauri::command]
fn word_list(
    app: AppHandle,
    search: Option<String>,
    category: Option<String>,
    enabled_only: Option<bool>,
) -> Result<Vec<words::WordEntry>, String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    let filter = words::WordFilter {
        search,
        category,
        enabled_only: enabled_only.unwrap_or(false),
    };
    err_to_string(words::word_list(&db, &filter))
}

/// 新增词条
#[tauri::command]
fn word_create(app: AppHandle, word: String, category: String) -> Result<i64, String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    let id = err_to_string(words::word_create(&db, &word, &category))?;
    // 词库变更使词库拼接缓存过期
    state.invalidate_cache();
    Ok(id)
}

/// 更新词条
#[tauri::command]
fn word_update(
    app: AppHandle,
    id: i64,
    word: String,
    category: String,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    err_to_string(words::word_update(&db, id, &word, &category))?;
    state.invalidate_cache();
    Ok(())
}

/// 删除词条
#[tauri::command]
fn word_delete(app: AppHandle, id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    err_to_string(words::word_delete(&db, id))?;
    state.invalidate_cache();
    Ok(())
}

/// 切换词条启禁用
#[tauri::command]
fn word_toggle_enable(app: AppHandle, id: i64, enabled: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    err_to_string(words::word_toggle_enable(&db, id, enabled))?;
    state.invalidate_cache();
    Ok(())
}

/// 批量导入词条（前端传入 [{word, category}, ...]，重复跳过）
#[derive(Debug, Deserialize)]
struct BatchImportEntry {
    word: String,
    category: String,
}

#[tauri::command]
fn word_batch_import(
    app: AppHandle,
    entries: Vec<BatchImportEntry>,
) -> Result<words::BatchResult, String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    let pairs: Vec<(String, String)> = entries
        .into_iter()
        .map(|e| (e.word, e.category))
        .collect();
    let result = err_to_string(words::word_batch_import(&db, &pairs))?;
    state.invalidate_cache();
    Ok(result)
}

/// 导出全部词库为 JSON 字符串
#[tauri::command]
fn word_export_json(app: AppHandle) -> Result<String, String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    err_to_string(words::word_export_json(&db))
}

/// 获取全部分类（用于前端筛选下拉）
#[tauri::command]
fn word_categories(app: AppHandle) -> Result<Vec<String>, String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    err_to_string(words::word_categories(&db))
}

// ===== Prompt 渲染预览命令 =====

/// 渲染模板预览（不入库，前端编辑时实时预览）
#[tauri::command]
fn prompt_render_preview(
    template: String,
    vars: HashMap<String, String>,
) -> Result<String, String> {
    err_to_string(llm::prompt::render_template(&template, &vars))
}

/// 提取模板中的 {{var}} 变量名列表
#[tauri::command]
fn prompt_extract_variables(template: String) -> Result<Vec<String>, String> {
    Ok(llm::prompt::extract_variables(&template))
}

// ===== 剪贴板处理模式命令 =====

/// 读取剪贴板处理模式（"A"=兼容复原 / "B"=纯净只读，默认 "B"）
#[tauri::command]
fn get_clipboard_mode(app: AppHandle) -> Result<String, String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    Ok(orchestrator::read_clipboard_mode(&db))
}

/// 设置剪贴板处理模式（校验 mode ∈ {A, B} + 写 settings + invalidate_cache）
///
/// - "A"：兼容复原模式（快照→读文本→复原，会新增 Win+V 历史）
/// - "B"：纯净只读模式（默认，不修改剪贴板，Win+V 历史无杂乱）
#[tauri::command]
fn set_clipboard_mode(app: AppHandle, mode: String) -> Result<(), String> {
    // 1. 校验 mode ∈ {A, B}
    use crate::config::{CLIPBOARD_MODE_A, CLIPBOARD_MODE_B};
    if mode != CLIPBOARD_MODE_A && mode != CLIPBOARD_MODE_B {
        return Err(format!(
            "非法剪贴板模式: {mode}（仅支持 \"A\"=兼容复原 或 \"B\"=纯净只读）"
        ));
    }
    // 2. 写 settings
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    err_to_string(settings::set_setting(
        &db,
        crate::config::KEY_CLIPBOARD_MODE,
        &mode,
    ))?;
    // 3. 失效缓存（虽然 clipboard_mode 不在运行时缓存，但保持一致性）
    state.invalidate_cache();
    tracing::info!("剪贴板处理模式已切换: {mode}");
    Ok(())
}

// ===== 黑名单命令 =====

/// 读取黑名单正则列表（未配置时返回默认规则）
#[tauri::command]
fn blacklist_get(app: AppHandle) -> Result<Vec<String>, String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    match settings::get_setting(&db, KEY_BLACKLIST) {
        Ok(Some(json)) => Ok(filter::parse_blacklist_json(&json)),
        _ => {
            // 未配置时返回默认黑名单（不入库，仅展示）
            Ok(filter::default_patterns())
        }
    }
}

/// 保存黑名单正则列表（序列化为 JSON 存 settings）
#[tauri::command]
fn blacklist_set(app: AppHandle, patterns: Vec<String>) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    let json = filter::serialize_blacklist(&patterns);
    err_to_string(settings::set_setting(&db, KEY_BLACKLIST, &json))?;
    // 黑名单变更使编译后的正则缓存过期
    state.invalidate_cache();
    Ok(())
}

// ===== 词频命令 =====

/// 查询高频词列表（按使用次数降序，取前 limit 条）
#[tauri::command]
fn word_freq_list(app: AppHandle, limit: Option<u32>) -> Result<Vec<word_freq::WordFreqEntry>, String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    // 默认取前 100 条，上限 500（db 层有钳制保护）
    let limit = limit.unwrap_or(100);
    err_to_string(word_freq::top(&db, limit))
}

/// 重置词频表（清空全部记录）
#[tauri::command]
fn word_freq_reset(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    err_to_string(word_freq::reset(&db))
}

/// 获取词频统计概览（总词数 + 总使用次数）
/// P4.4：派生 TS，cargo test 时自动生成 .ts 到 ./bindings/commands/
#[derive(Debug, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../bindings/commands/WordFreqOverview.ts")]
struct WordFreqOverview {
    /// 不同的词语总数
    #[ts(type = "number")]
    total_words: i64,
    /// 累计使用总次数
    #[ts(type = "number")]
    total_usage: i64,
}

#[tauri::command]
fn word_freq_overview(app: AppHandle) -> Result<WordFreqOverview, String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    let total_words = err_to_string(word_freq::count_total(&db))?;
    let total_usage = err_to_string(word_freq::count_total_usage(&db))?;
    Ok(WordFreqOverview {
        total_words,
        total_usage,
    })
}

// ===== 历史记录命令 =====

/// 历史记录分页查询响应（含分页元信息）
/// P4.4：派生 TS，cargo test 时自动生成 .ts 到 ./bindings/commands/
#[derive(Debug, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../bindings/commands/HistoryListResult.ts")]
struct HistoryListResult {
    /// 当前页的历史记录列表
    items: Vec<history::HistoryEntry>,
    /// 满足搜索条件的总记录数（用于前端分页计算）
    #[ts(type = "number")]
    total: i64,
}

/// 查询历史记录列表（按时间倒序，支持搜索 + 分页）
///
/// - `search`：可选搜索关键字（模糊匹配 origin 或 selected）
/// - `limit`：每页条数，默认 20，上限 500（db 层钳制保护）
/// - `offset`：偏移量，0-based
#[tauri::command]
fn history_list(
    app: AppHandle,
    search: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<HistoryListResult, String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    let filter = history::HistoryFilter {
        search,
        limit: limit.unwrap_or(20),
        offset: offset.unwrap_or(0),
    };
    let items = err_to_string(history::history_list(&db, &filter))?;
    let total = err_to_string(history::history_count(
        &db,
        filter.search.as_deref(),
    ))?;
    Ok(HistoryListResult { items, total })
}

/// 删除单条历史记录
#[tauri::command]
fn history_delete(app: AppHandle, id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    err_to_string(history::history_delete(&db, id))
}

/// 清空全部历史记录
#[tauri::command]
fn history_clear(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    err_to_string(history::history_clear(&db))
}

// ===== 热键暂停/恢复命令 =====

/// 查询当前热键是否已暂停
#[tauri::command]
fn hotkey_is_paused(app: AppHandle) -> Result<bool, String> {
    Ok(tray::is_hotkey_paused(&app))
}

// ===== 开机自启命令 =====

/// 查询开机自启状态（优先读系统实际状态，回退到 DB 配置）
#[tauri::command]
fn autostart_get(app: AppHandle) -> Result<bool, String> {
    // 优先查系统实际状态
    match app.autolaunch().is_enabled() {
        Ok(enabled) => Ok(enabled),
        Err(e) => {
            tracing::warn!("查询系统自启状态失败，回退到 DB 配置: {e}");
            let state = app.state::<AppState>();
            let db = state.db().map_err(|e| e.to_string())?;
            Ok(settings::get_setting(&db, KEY_AUTOSTART)
                .ok()
                .flatten()
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(DEFAULT_AUTOSTART))
        }
    }
}

/// 设置开机自启（同步系统注册表 + 持久化到 DB）
#[tauri::command]
fn autostart_set(app: AppHandle, enabled: bool) -> Result<(), String> {
    // 1. 同步系统自启配置
    if enabled {
        app.autolaunch()
            .enable()
            .map_err(|e| format!("启用开机自启失败: {e}"))?;
    } else {
        app.autolaunch()
            .disable()
            .map_err(|e| format!("禁用开机自启失败: {e}"))?;
    }

    // 2. 持久化到 DB（供前端快速读取初始状态）
    let state = app.state::<AppState>();
    let db = state.db().map_err(|e| e.to_string())?;
    let value = if enabled { "true" } else { "false" };
    err_to_string(settings::set_setting(&db, KEY_AUTOSTART, value))
}

// ===== 命令注册（必须在命令定义同模块内调用 generate_handler!） =====

/// 构造 Tauri 命令处理器（generate_handler! 需与 #[tauri::command] 同作用域）
pub fn make_handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool {
    tauri::generate_handler![
        type_candidate,
        cancel,
        regenerate_candidates,
        switch_prompt_by_index,
        // 悬浮窗置顶模式（Off/Normal/Temp 循环）+ 透明度
        cycle_pin_mode,
        get_pin_mode,
        set_float_opacity,
        get_float_opacity,
        save_float_state,
        get_all_settings,
        set_setting,
        get_app_config,
        update_hotkey,
        prompt_list,
        prompt_create,
        prompt_update,
        prompt_delete,
        prompt_set_default,
        prompt_all_tags,
        prompt_render_preview,
        prompt_extract_variables,
        // 剪贴板处理模式（A=兼容复原 / B=纯净只读）
        get_clipboard_mode,
        set_clipboard_mode,
        test_llm_connection,
        llm_profile_list,
        get_active_llm_profile,
        llm_profile_create,
        llm_profile_update,
        llm_profile_delete,
        llm_profile_set_active,
        word_list,
        word_create,
        word_update,
        word_delete,
        word_toggle_enable,
        word_batch_import,
        word_export_json,
        word_categories,
        blacklist_get,
        blacklist_set,
        word_freq_list,
        word_freq_reset,
        word_freq_overview,
        history_list,
        history_delete,
        history_clear,
        hotkey_is_paused,
        autostart_get,
        autostart_set,
    ]
}
