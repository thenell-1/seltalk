// NOTE Tauri commands 模块：前端可调用的后端方法
// 命名规范：动词_名词，与前端 api/index.ts 一一对应

use crate::config::{self, AppConfig, LlmConfig};
use crate::database::HistoryRecord;
use crate::error::AppResult;
use crate::AppState;
use serde::Serialize;
use tauri::{AppHandle, Manager};

/// 系统状态
#[derive(Debug, Serialize)]
pub struct SystemStatus {
    pub running: bool,
    pub llm_mode: String,
    pub last_capture_time: Option<String>,
    pub total_adopted: u64,
}

/// 获取应用配置
#[tauri::command]
pub async fn get_config(app: AppHandle) -> AppResult<AppConfig> {
    config::load_app_config(&app)
}

/// 保存应用配置
#[tauri::command]
pub async fn save_config(app: AppHandle, config: AppConfig) -> AppResult<()> {
    config::save_app_config(&app, &config)
}

/// 获取 LLM 配置
#[tauri::command]
pub async fn get_llm_config(app: AppHandle) -> AppResult<LlmConfig> {
    config::load_llm_config(&app)
}

/// 保存 LLM 配置
#[tauri::command]
pub async fn save_llm_config(app: AppHandle, config: LlmConfig) -> AppResult<()> {
    config::save_llm_config(&app, &config)
}

/// 测试 LLM 连通性（返回具体错误信息）
#[tauri::command]
pub async fn test_llm(app: AppHandle) -> AppResult<String> {
    let app_config = config::load_app_config(&app)?;
    let llm_config = config::load_llm_config(&app)?;
    let mode = crate::llm::LlmMode::from_str(&app_config.llm_mode)?;
    let client = crate::llm::LlmClient::new(llm_config, mode)?;
    client.test_connection().await
}

/// 获取系统状态（从数据库读取真实数据）
#[tauri::command]
pub async fn get_system_status(app: AppHandle) -> AppResult<SystemStatus> {
    let state = app.state::<AppState>();
    let config = config::load_app_config(&app)?;
    let total_adopted = state.db.count_adopted()?;

    Ok(SystemStatus {
        running: true,
        llm_mode: config.llm_mode,
        last_capture_time: None,
        total_adopted,
    })
}

/// 触发生成回复（接入 Orchestrator 主流程，捕获选中文本）
#[tauri::command]
pub async fn generate_reply(app: AppHandle) -> AppResult<String> {
    crate::orchestrator::trigger_capture(&app).await
}

/// 采纳回复并模拟输入
#[tauri::command]
pub async fn adopt_reply(app: AppHandle, request_id: String, reply_text: String) -> AppResult<()> {
    crate::orchestrator::adopt_and_type(&app, &request_id, &reply_text).await
}

/// 显示悬浮窗（动态创建第二个 Tauri 窗口）
/// 位置跟随鼠标光标，类似输入法候选窗行为
#[tauri::command]
pub async fn show_overlay_window(app: AppHandle) -> AppResult<()> {
    use tauri::WebviewWindowBuilder;

    // 获取鼠标光标位置（悬浮窗显示在光标右下方，避免遮挡选中文本）
    let (pos_x, pos_y) = get_overlay_position();
    tracing::info!("悬浮窗目标位置: ({}, {})", pos_x, pos_y);

    // 若窗口已存在则显示并移动到光标位置
    if let Some(window) = app.get_webview_window("overlay") {
        tracing::info!("悬浮窗已存在，显示并移动到光标位置");
        window.show()?;
        use tauri::PhysicalPosition;
        window.set_position(PhysicalPosition::new(pos_x, pos_y))?;
        // NOTE 悬浮窗有 WS_EX_NOACTIVATE，不调用 set_focus 避免抢焦点
        tracing::info!("悬浮窗已显示并移动完成");
        return Ok(());
    }

    tracing::info!("开始创建悬浮窗");
    let window = WebviewWindowBuilder::new(
        &app,
        "overlay",
        tauri::WebviewUrl::App("overlay.html".into()),
    )
    .title("CIM Overlay")
    .inner_size(360.0, 200.0)
    .position(pos_x, pos_y)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .visible(true)
    .build()?;

    // NOTE 设置 WS_EX_NOACTIVATE（不抢焦点），对输入法场景至关重要
    #[cfg(target_os = "windows")]
    {
        let hwnd_ptr = window.hwnd().map_err(|e| {
            crate::error::AppError::Config(format!("获取窗口句柄失败: {e}"))
        })?;
        set_no_activate(hwnd_ptr.0 as isize)?;
    }

    tracing::info!("悬浮窗创建成功");
    Ok(())
}

/// 显示管理面板窗口
#[tauri::command]
pub async fn show_panel_window(app: AppHandle) -> AppResult<()> {
    if let Some(window) = app.get_webview_window("panel") {
        window.show()?;
        window.set_focus()?;
    }
    Ok(())
}

/// 分页查询历史回复记录
#[tauri::command]
pub async fn list_history(
    app: AppHandle,
    page: Option<u32>,
    page_size: Option<u32>,
) -> AppResult<Vec<HistoryRecord>> {
    let state = app.state::<AppState>();
    let page = page.unwrap_or(0);
    let page_size = page_size.unwrap_or(20).min(100);
    state.db.list_history(page, page_size)
}

/// Windows 专用：为窗口添加 WS_EX_NOACTIVATE 扩展样式
#[cfg(target_os = "windows")]
fn set_no_activate(hwnd_raw: isize) -> AppResult<()> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE,
    };

    // NOTE 将 isize 转换为 windows-rs 的 HWND 类型
    let hwnd = HWND(hwnd_raw as *mut std::ffi::c_void);

    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_NOACTIVATE.0 as isize);
    }
    Ok(())
}

/// 悬浮窗尺寸常量（物理像素）
const OVERLAY_WIDTH: f64 = 360.0;
const OVERLAY_HEIGHT: f64 = 200.0;
/// 光标偏移量（避免悬浮窗遮挡光标）
const CURSOR_OFFSET: f64 = 20.0;

/// 获取悬浮窗显示位置（鼠标光标右下方，不超出屏幕边界）
/// 返回物理像素坐标
#[cfg(target_os = "windows")]
fn get_overlay_position() -> (f64, f64) {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
    };

    let mut point = POINT { x: 0, y: 0 };
    unsafe {
        if GetCursorPos(&mut point).is_ok() {
            let cursor_x = point.x as f64;
            let cursor_y = point.y as f64;
            let screen_w = GetSystemMetrics(SM_CXSCREEN) as f64;
            let screen_h = GetSystemMetrics(SM_CYSCREEN) as f64;

            // 默认显示在光标右下方
            let mut x = cursor_x + CURSOR_OFFSET;
            let mut y = cursor_y + CURSOR_OFFSET;

            // 边界检查：超出右侧则显示在光标左侧
            if x + OVERLAY_WIDTH > screen_w {
                x = cursor_x - OVERLAY_WIDTH - CURSOR_OFFSET;
            }
            // 边界检查：超出底部则显示在光标上方
            if y + OVERLAY_HEIGHT > screen_h {
                y = cursor_y - OVERLAY_HEIGHT - CURSOR_OFFSET;
            }

            // 确保不超出屏幕边界
            if x < 0.0 {
                x = 0.0;
            }
            if y < 0.0 {
                y = 0.0;
            }

            (x, y)
        } else {
            tracing::warn!("获取鼠标位置失败，使用默认位置");
            (100.0, 100.0)
        }
    }
}

/// 非 Windows 平台的占位实现
#[cfg(not(target_os = "windows"))]
fn get_overlay_position() -> (f64, f64) {
    (100.0, 100.0)
}
