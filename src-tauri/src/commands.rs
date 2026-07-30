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
use crate::db::{prompts, settings, word_freq, words, window_state};
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

// ===== 悬浮窗命令 =====

/// 切换悬浮窗置顶状态
#[tauri::command]
fn toggle_float_always_on_top(app: AppHandle) -> Result<bool, String> {
    err_to_string(window::toggle_always_on_top(&app))
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
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB 锁中毒: {e}"))?;
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
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB 锁中毒: {e}"))?;
    err_to_string(settings::set_setting(&db, &key, &value))?;
    // 写时失效：任何设置变更都使运行时缓存过期（LLM 配置/黑名单/词库等）
    state.invalidate_cache();
    Ok(())
}

/// 读取应用配置（结构化）
#[tauri::command]
fn get_app_config(app: AppHandle) -> Result<AppConfig, String> {
    let state = app.state::<AppState>();
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB 锁中毒: {e}"))?;
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
        let db = state
            .db
            .lock()
            .map_err(|e| format!("DB 锁中毒: {e}"))?;
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
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB 锁中毒: {e}"))?;
    err_to_string(prompts::prompt_list(&db))
}

#[tauri::command]
fn prompt_create(app: AppHandle, name: String, template: String) -> Result<i64, String> {
    let state = app.state::<AppState>();
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB 锁中毒: {e}"))?;
    let id = err_to_string(prompts::prompt_create(&db, &name, &template))?;
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
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB 锁中毒: {e}"))?;
    err_to_string(prompts::prompt_update(&db, id, &name, &template))?;
    state.invalidate_cache();
    Ok(())
}

#[tauri::command]
fn prompt_delete(app: AppHandle, id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB 锁中毒: {e}"))?;
    err_to_string(prompts::prompt_delete(&db, id))?;
    state.invalidate_cache();
    Ok(())
}

#[tauri::command]
fn prompt_set_default(app: AppHandle, id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB 锁中毒: {e}"))?;
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
        let db = state
            .db
            .lock()
            .map_err(|e| format!("DB 锁中毒: {e}"))?;
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
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB 锁中毒: {e}"))?;
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
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB 锁中毒: {e}"))?;
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
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB 锁中毒: {e}"))?;
    err_to_string(words::word_update(&db, id, &word, &category))?;
    state.invalidate_cache();
    Ok(())
}

/// 删除词条
#[tauri::command]
fn word_delete(app: AppHandle, id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB 锁中毒: {e}"))?;
    err_to_string(words::word_delete(&db, id))?;
    state.invalidate_cache();
    Ok(())
}

/// 切换词条启禁用
#[tauri::command]
fn word_toggle_enable(app: AppHandle, id: i64, enabled: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB 锁中毒: {e}"))?;
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
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB 锁中毒: {e}"))?;
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
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB 锁中毒: {e}"))?;
    err_to_string(words::word_export_json(&db))
}

/// 获取全部分类（用于前端筛选下拉）
#[tauri::command]
fn word_categories(app: AppHandle) -> Result<Vec<String>, String> {
    let state = app.state::<AppState>();
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB 锁中毒: {e}"))?;
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

// ===== 黑名单命令 =====

/// 读取黑名单正则列表（未配置时返回默认规则）
#[tauri::command]
fn blacklist_get(app: AppHandle) -> Result<Vec<String>, String> {
    let state = app.state::<AppState>();
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB 锁中毒: {e}"))?;
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
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB 锁中毒: {e}"))?;
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
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB 锁中毒: {e}"))?;
    // 默认取前 100 条，上限 500（db 层有钳制保护）
    let limit = limit.unwrap_or(100);
    err_to_string(word_freq::top(&db, limit))
}

/// 重置词频表（清空全部记录）
#[tauri::command]
fn word_freq_reset(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB 锁中毒: {e}"))?;
    err_to_string(word_freq::reset(&db))
}

/// 获取词频统计概览（总词数 + 总使用次数）
#[derive(Debug, serde::Serialize)]
struct WordFreqOverview {
    /// 不同的词语总数
    total_words: i64,
    /// 累计使用总次数
    total_usage: i64,
}

#[tauri::command]
fn word_freq_overview(app: AppHandle) -> Result<WordFreqOverview, String> {
    let state = app.state::<AppState>();
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB 锁中毒: {e}"))?;
    let total_words = err_to_string(word_freq::count_total(&db))?;
    let total_usage = err_to_string(word_freq::count_total_usage(&db))?;
    Ok(WordFreqOverview {
        total_words,
        total_usage,
    })
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
            let db = state
                .db
                .lock()
                .map_err(|e| format!("DB 锁中毒: {e}"))?;
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
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB 锁中毒: {e}"))?;
    let value = if enabled { "true" } else { "false" };
    err_to_string(settings::set_setting(&db, KEY_AUTOSTART, value))
}

// ===== 命令注册（必须在命令定义同模块内调用 generate_handler!） =====

/// 构造 Tauri 命令处理器（generate_handler! 需与 #[tauri::command] 同作用域）
pub fn make_handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool {
    tauri::generate_handler![
        type_candidate,
        cancel,
        toggle_float_always_on_top,
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
        prompt_render_preview,
        prompt_extract_variables,
        test_llm_connection,
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
        hotkey_is_paused,
        autostart_get,
        autostart_set,
    ]
}
