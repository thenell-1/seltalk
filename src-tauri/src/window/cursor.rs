// TODO 人工审查点：1.unsafe 边界 2.多显示器工作区 3.物理像素单位 4.错误传播
// NOTE Windows API 封装：获取鼠标位置 + 鼠标所在显示器的工作区
// 仅返回物理像素，由调用方（window::move_float_to_cursor）根据窗口 scale_factor 转换为逻辑像素

use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITOR_DEFAULTTONEAREST, MONITORINFO,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use crate::error::{AppError, AppResult};

/// 显示器工作区矩形（物理像素）
///
/// 仅包含桌面可见区域（排除任务栏），用于悬浮窗边界计算
#[derive(Debug, Clone, Copy)]
pub struct PhysicalRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// 获取鼠标当前屏幕坐标（物理像素）
///
/// 失败场景：桌面会话未就绪、远程会话异常等罕见情况
pub fn get_cursor_position() -> AppResult<POINT> {
    let mut pt = POINT { x: 0, y: 0 };
    unsafe {
        GetCursorPos(&mut pt)
            .map_err(|e| AppError::Window(format!("GetCursorPos 失败: {e}")))?;
    }
    Ok(pt)
}

/// 获取指定点所在显示器的工作区（物理像素）
///
/// `MONITOR_DEFAULTTONEAREST`：找不到包含该点的显示器时返回最近的，
/// 确保鼠标在任何位置都能取到有效显示器（含跨多显示器间隙）
pub fn get_monitor_work_area(pt: POINT) -> AppResult<PhysicalRect> {
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        if GetMonitorInfoW(hmon, &mut mi).as_bool() {
            Ok(PhysicalRect {
                x: mi.rcWork.left,
                y: mi.rcWork.top,
                width: mi.rcWork.right - mi.rcWork.left,
                height: mi.rcWork.bottom - mi.rcWork.top,
            })
        } else {
            Err(AppError::Window(
                "GetMonitorInfoW 返回 false，可能显示器已断开".into(),
            ))
        }
    }
}
