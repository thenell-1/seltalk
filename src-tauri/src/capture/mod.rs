// NOTE 文本捕获模块入口
// 包含：窗口识别（window）、UI Automation（uiautomation）、Ctrl+C 备选（clipboard）
// 双轨降级策略：优先 UI Automation，失败则降级到 Ctrl+C

pub mod clipboard;
pub mod uiautomation;
pub mod window;

use crate::error::AppResult;
use window::WindowInfo;

/// 文本捕获结果
#[derive(Debug, Clone)]
pub struct CaptureResult {
    pub text: String,
    pub window: WindowInfo,
    pub method: CaptureMethod,
}

/// 捕获方式
#[derive(Debug, Clone, PartialEq)]
pub enum CaptureMethod {
    UiAutomation,
    Clipboard,
}

/// 双轨捕获选中文本
/// 1. 先用 UI Automation（准确、不抢焦点）
/// 2. 失败则降级到 Ctrl+C（兼容性好、但会临时占用剪贴板）
pub fn capture_selected_text() -> AppResult<CaptureResult> {
    // 1. 先识别前台窗口
    let window = window::detect_foreground_target()?
        .ok_or_else(|| crate::error::AppError::Config("当前前台窗口非微信/QQ".to_string()))?;

    tracing::info!(
        "检测到目标窗口: {} ({})",
        window.window_type,
        window.title
    );

    // 2. 尝试 UI Automation
    match uiautomation::get_selected_text_via_uia() {
        Ok(Some(text)) => {
            tracing::info!("UI Automation 捕获成功");
            return Ok(CaptureResult {
                text,
                window,
                method: CaptureMethod::UiAutomation,
            });
        }
        Ok(None) => {
            tracing::info!("UI Automation 未获取到选中文本，降级到 Ctrl+C");
        }
        Err(e) => {
            tracing::warn!("UI Automation 失败: {e}，降级到 Ctrl+C");
        }
    }

    // 3. 降级到 Ctrl+C
    let text = clipboard::get_selected_text_via_clipboard()?
        .ok_or_else(|| crate::error::AppError::Config("Ctrl+C 也未捕获到文本".to_string()))?;

    Ok(CaptureResult {
        text,
        window,
        method: CaptureMethod::Clipboard,
    })
}
