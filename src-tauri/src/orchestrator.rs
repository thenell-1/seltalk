// TODO 人工审查点：1.主流程异常恢复策略 2.悬浮窗事件payload结构 3.并发捕获冲突 4.历史记录写入时机
// NOTE Orchestrator 主流程调度：串联 捕获→清洗→LLM→候选展示→采纳输入
// 严格遵循 PRD 4.3.2 事件定义：capture.triggered / llm.generating / llm.done / typing.done

use crate::capture;
use crate::cleaner;
use crate::config;
use crate::database::{self, HistoryRecord};
use crate::error::{classify_error, AppError, AppResult};
use crate::input::{self, TypingConfig};
use crate::llm::{GenerateParams, LlmClient, LlmMode};
use crate::AppState;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use tauri::{AppHandle, Emitter, Manager};

// PRD 4.3.2 事件名定义
const EVENT_CAPTURE_TRIGGERED: &str = "capture.triggered";
const EVENT_LLM_GENERATING: &str = "llm.generating";
const EVENT_LLM_DONE: &str = "llm.done";
const EVENT_TYPING_STARTED: &str = "typing.started";
const EVENT_TYPING_DONE: &str = "typing.done";
const EVENT_TYPING_INTERRUPTED: &str = "typing.interrupted";
const EVENT_ERROR: &str = "error";

/// request_id → 捕获时的窗口句柄（用于采纳后切回原窗口）
static CAPTURE_WINDOWS: LazyLock<Mutex<HashMap<String, isize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 推送 error 事件到前端（PRD 4.3.2 + 11.3：错误码 + 中文提示）
/// silent=true 的错误前端静默处理，不弹窗
fn emit_error(app: &AppHandle, err: &AppError) {
    let info = classify_error(err);
    tracing::warn!("错误 [{}]: {} (silent={})", info.code, info.message, info.silent);
    let _ = app.emit(
        EVENT_ERROR,
        &serde_json::json!({
            "code": info.code,
            "message": info.message,
            "silent": info.silent,
        }),
    );
}

/// capture.triggered 事件 payload（PRD 4.3.2）
#[derive(Debug, Clone, Serialize)]
pub struct CaptureTriggeredPayload {
    pub request_id: String,
    pub window_title: String,
}

/// llm.done 事件 payload（PRD 4.3.2）
#[derive(Debug, Clone, Serialize)]
pub struct LlmDonePayload {
    pub request_id: String,
    pub captured_text: String,
    pub candidates: Vec<String>,
    pub window_title: String,
}

/// 触发捕获并生成候选回复（主流程入口，由 F8 或托盘菜单调用）
pub async fn trigger_capture(app: &AppHandle) -> AppResult<String> {
    let request_id = format!("req_{}", chrono::Utc::now().timestamp_millis());
    tracing::info!("开始捕获流程: {request_id}");

    // 1. 捕获选中文本（spawn_blocking 避免 UI Automation 阻塞异步运行时）
    let capture_result = match tokio::task::spawn_blocking(|| capture::capture_selected_text()).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            emit_error(app, &e);
            return Err(e);
        }
        Err(e) => {
            let err = AppError::Config(format!("捕获任务执行失败: {e}"));
            emit_error(app, &err);
            return Err(err);
        }
    };

    // 记录窗口句柄，用于采纳时切回
    CAPTURE_WINDOWS
        .lock()
        .map_err(|e| AppError::Config(format!("锁获取失败: {e}")))?
        .insert(request_id.clone(), capture_result.window.hwnd);

    // 2. 先创建/显示悬浮窗（必须先创建窗口，Vue 组件 mount 后才能注册事件监听器）
    tracing::info!("步骤2: 显示悬浮窗");
    show_overlay(app).await?;

    // NOTE 短暂等待悬浮窗 Vue 组件 mount 完成并注册事件监听器
    tracing::info!("等待悬浮窗 Vue 组件 mount (300ms)");
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    // 3. 发送 capture.triggered 事件（前端显示来源窗口信息）
    let trigger_payload = CaptureTriggeredPayload {
        request_id: request_id.clone(),
        window_title: capture_result.window.title.clone(),
    };
    tracing::info!("步骤3: 发送 capture.triggered 事件, request_id={}", request_id);
    match app.emit(EVENT_CAPTURE_TRIGGERED, &trigger_payload) {
        Ok(_) => tracing::info!("capture.triggered 事件发送成功"),
        Err(e) => tracing::error!("capture.triggered 事件发送失败: {e}"),
    }

    // 3. 清洗文本
    let cleaned = cleaner::clean_text(&capture_result.text);
    if !cleaner::is_valid_text(&cleaned) {
        // 静默忽略空文本（PRD 5.3.1：无选中文本静默忽略）
        hide_overlay(app);
        let err = AppError::Config("未捕获到选中文本".to_string());
        emit_error(app, &err);
        return Err(err);
    }
    tracing::info!("文本清洗完成，长度: {}", cleaned.len());

    // 4. 发送 llm.generating 事件（悬浮窗显示"AI 思考中…"）
    tracing::info!("步骤4: 发送 llm.generating 事件");
    match app.emit(EVENT_LLM_GENERATING, &serde_json::json!({ "request_id": &request_id })) {
        Ok(_) => tracing::info!("llm.generating 事件发送成功"),
        Err(e) => tracing::error!("llm.generating 事件发送失败: {e}"),
    }

    // 5. 加载配置并创建 LLM 客户端
    let app_config = match config::load_app_config(app) {
        Ok(c) => c,
        Err(e) => { emit_error(app, &e); return Err(e); }
    };
    let llm_config = match config::load_llm_config(app) {
        Ok(c) => c,
        Err(e) => { emit_error(app, &e); return Err(e); }
    };
    if let Err(e) = validate_llm_config(&app_config, &llm_config) {
        emit_error(app, &e);
        return Err(e);
    }
    let mode = match LlmMode::from_str(&app_config.llm_mode) {
        Ok(m) => m,
        Err(e) => { emit_error(app, &e); return Err(e); }
    };
    let client = match LlmClient::new(llm_config, mode) {
        Ok(c) => c,
        Err(e) => { emit_error(app, &e); return Err(e); }
    };

    // 6. 调用 LLM 生成候选回复
    let params = GenerateParams {
        captured_text: cleaned.clone(),
        candidate_count: app_config.candidate_count,
    };
    let candidates = match client.generate_replies(&params).await {
        Ok(c) => c,
        Err(e) => {
            hide_overlay(app);
            emit_error(app, &e);
            return Err(e);
        }
    };
    tracing::info!("LLM 生成 {} 条候选回复", candidates.len());

    // 7. 保存历史记录
    save_history_records(app, &cleaned, &candidates, &app_config.llm_mode)?;

    // 8. 发送 llm.done 事件（推送候选到悬浮窗）
    let done_payload = LlmDonePayload {
        request_id: request_id.clone(),
        captured_text: cleaned,
        candidates: candidates.clone(),
        window_title: capture_result.window.title,
    };
    tracing::info!("步骤8: 发送 llm.done 事件, 候选数: {}", candidates.len());
    match app.emit(EVENT_LLM_DONE, &done_payload) {
        Ok(_) => tracing::info!("llm.done 事件发送成功"),
        Err(e) => tracing::error!("llm.done 事件发送失败: {e}"),
    }

    Ok(request_id)
}

/// 用户采纳某条候选并模拟输入（PRD US-1：Tab 确认→逐字输入）
pub async fn adopt_and_type(
    app: &AppHandle,
    request_id: &str,
    reply_text: &str,
) -> AppResult<()> {
    tracing::info!("用户采纳回复: {request_id}");

    // 1. 隐藏悬浮窗
    hide_overlay(app);

    // 2. 切回原捕获窗口
    if let Some(hwnd) = CAPTURE_WINDOWS
        .lock()
        .map_err(|e| AppError::Config(format!("锁获取失败: {e}")))?
        .get(request_id)
        .copied()
    {
        bring_window_to_foreground(hwnd);
    }

    // 3. 发送 typing.started 事件
    let _ = app.emit(EVENT_TYPING_STARTED, &serde_json::json!({ "request_id": request_id }));

    // 4. 模拟逐字输入（支持 ESC 中断，PRD US-3）
    let app_config = config::load_app_config(app)?;
    let typing_config = TypingConfig {
        delay_min_ms: app_config.typing_delay_min as u64,
        delay_max_ms: app_config.typing_delay_max as u64,
    };
    let reply_owned = reply_text.to_string();
    let typing_result = tokio::task::spawn_blocking(move || {
        input::type_text(&reply_owned, &typing_config)
    })
    .await
    .map_err(|e| AppError::Config(format!("输入任务执行失败: {e}")))??;

    // 5. 根据输入结果发送对应事件
    match typing_result {
        input::TypingStatus::Completed => {
            let _ = app.emit(EVENT_TYPING_DONE, &serde_json::json!({ "request_id": request_id }));
            // 6. 标记历史记录为已采纳
            mark_reply_adopted(app, reply_text)?;
            tracing::info!("采纳并输入完成: {request_id}");
        }
        input::TypingStatus::Interrupted => {
            // NOTE PRD US-3：ESC 中断，已输入内容保留，不标记为已采纳
            let _ = app.emit(
                EVENT_TYPING_INTERRUPTED,
                &serde_json::json!({ "request_id": request_id }),
            );
            tracing::info!("输入被 ESC 中断: {request_id}");
        }
    }

    // 7. 清理窗口记录
    CAPTURE_WINDOWS
        .lock()
        .map_err(|e| AppError::Config(format!("锁获取失败: {e}")))?
        .remove(request_id);

    Ok(())
}

/// 校验 LLM 配置是否可用
fn validate_llm_config(
    app_config: &config::AppConfig,
    llm_config: &config::LlmConfig,
) -> AppResult<()> {
    match app_config.llm_mode.as_str() {
        "cloud" => {
            if llm_config.cloud_api_key.trim().is_empty() {
                return Err(AppError::Config(
                    "云端 API 密钥未配置，请在设置页面填写 LLM API 密钥".to_string(),
                ));
            }
            if llm_config.cloud_endpoint.trim().is_empty() {
                return Err(AppError::Config("云端 API 端点未配置".to_string()));
            }
        }
        "local" => {
            if llm_config.local_endpoint.trim().is_empty() {
                return Err(AppError::Config("本地 Ollama 端点未配置".to_string()));
            }
        }
        other => {
            return Err(AppError::Config(format!(
                "不支持的 LLM 模式: {other}，请使用 cloud 或 local"
            )));
        }
    }
    Ok(())
}

/// 显示悬浮窗
async fn show_overlay(app: &AppHandle) -> AppResult<()> {
    if let Some(window) = app.get_webview_window("overlay") {
        tracing::info!("悬浮窗已存在，调用 show() 显示");
        window.show()?;
        tracing::info!("悬浮窗 show() 调用完成");
    } else {
        tracing::info!("悬浮窗不存在，调用 show_overlay_window 创建");
        crate::commands::show_overlay_window(app.clone()).await?;
        tracing::info!("悬浮窗创建完成");
    }
    Ok(())
}

/// 隐藏悬浮窗
fn hide_overlay(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.hide();
        tracing::info!("悬浮窗已隐藏");
    } else {
        tracing::warn!("悬浮窗不存在，无法隐藏");
    }
}

/// 将窗口切换到前台
fn bring_window_to_foreground(hwnd_raw: isize) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
        };
        let hwnd = HWND(hwnd_raw as *mut std::ffi::c_void);
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
    }
    tracing::info!("已切换到原捕获窗口");
}

/// 保存候选回复到历史记录表
fn save_history_records(
    app: &AppHandle,
    captured_text: &str,
    candidates: &[String],
    llm_mode: &str,
) -> AppResult<()> {
    let state = app.state::<AppState>();
    let now = database::now_utc_string();

    for reply in candidates {
        let record = HistoryRecord {
            id: None,
            captured_text: captured_text.to_string(),
            reply_text: reply.clone(),
            adopted: false,
            llm_mode: llm_mode.to_string(),
            created_at: now.clone(),
        };
        state.db.insert_history(&record)?;
    }
    Ok(())
}

/// 标记匹配的回复为已采纳
fn mark_reply_adopted(app: &AppHandle, reply_text: &str) -> AppResult<()> {
    let state = app.state::<AppState>();
    let records = state.db.list_history(0, 50)?;
    for record in records {
        if let Some(id) = record.id {
            if record.reply_text == reply_text && !record.adopted {
                state.db.mark_adopted(id)?;
                tracing::info!("已标记历史记录 {id} 为已采纳");
                break;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_done_payload_serialization() {
        let payload = LlmDonePayload {
            request_id: "req_123".to_string(),
            captured_text: "你好".to_string(),
            candidates: vec!["好的".to_string(), "收到".to_string()],
            window_title: "微信".to_string(),
        };
        let json = serde_json::to_string(&payload);
        assert!(json.is_ok());
        let json_str = json.unwrap();
        assert!(json_str.contains("req_123"));
        assert!(json_str.contains("candidates"));
    }

    #[test]
    fn test_llm_done_payload_empty_candidates() {
        let payload = LlmDonePayload {
            request_id: "req_empty".to_string(),
            captured_text: "测试".to_string(),
            candidates: vec![],
            window_title: "QQ".to_string(),
        };
        assert_eq!(payload.candidates.len(), 0);
    }

    #[test]
    fn test_request_id_format() {
        let id = format!("req_{}", 1234567890_i64);
        assert!(id.starts_with("req_"));
        assert!(id.len() > 5);
    }

    #[test]
    fn test_capture_windows_store_and_retrieve() {
        let mut map = CAPTURE_WINDOWS.lock().unwrap();
        map.insert("req_test".to_string(), 12345_isize);
        assert_eq!(map.get("req_test"), Some(&12345_isize));
        map.remove("req_test");
        assert_eq!(map.get("req_test"), None);
    }
}
