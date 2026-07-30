// TODO 人工审查点：1.窗口标题编码（UTF-16→String）2.进程名匹配规则 3.多显示器坐标处理
// NOTE F1 窗口识别模块：通过进程名+窗口类名双重验证识别微信/QQ 窗口

use crate::error::{AppError, AppResult};
use serde::Serialize;
use std::path::Path;
use windows::Win32::Foundation::{BOOL, CloseHandle, HWND, LPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_FORMAT, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId,
    GetWindowTextW, IsWindowVisible,
};

/// 目标窗口类型
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum TargetWindowType {
    WeChat,
    QQ,
    Unknown,
}

impl std::fmt::Display for TargetWindowType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TargetWindowType::WeChat => write!(f, "WeChat"),
            TargetWindowType::QQ => write!(f, "QQ"),
            TargetWindowType::Unknown => write!(f, "Unknown"),
        }
    }
}

/// 窗口信息
#[derive(Debug, Clone, Serialize)]
pub struct WindowInfo {
    pub hwnd: isize,
    pub title: String,
    pub class_name: String,
    pub process_name: String,
    pub window_type: TargetWindowType,
    pub monitor_left: i32,
    pub monitor_top: i32,
    pub monitor_width: i32,
    pub monitor_height: i32,
}

// 微信与 QQ 的进程名和窗口类名（经验值）
const WECHAT_PROCESS_NAMES: &[&str] = &["WeChat.exe", "Weixin.exe"];
const WECHAT_CLASS_NAMES: &[&str] = &["WeChatMainWndForPC", "Weixin"];
const QQ_PROCESS_NAMES: &[&str] = &["QQ.exe"];
const QQ_CLASS_NAMES: &[&str] = &["TXGuiFoundation"];

/// 判断当前前台窗口是否为微信或 QQ
/// 返回 Some(WindowInfo) 如果是目标窗口，否则 None
pub fn detect_foreground_target() -> AppResult<Option<WindowInfo>> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0 as isize == 0 {
        tracing::warn!("GetForegroundWindow 返回空句柄，无前台窗口");
        return Ok(None);
    }
    let info = get_window_info(hwnd)?;
    if matches!(info.window_type, TargetWindowType::Unknown) {
        // NOTE 诊断日志：输出实际窗口信息，便于排查微信/QQ 新版本进程名/类名不匹配问题
        tracing::warn!(
            "前台窗口未识别为目标窗口: 进程名={}, 类名={}, 标题={}",
            info.process_name,
            info.class_name,
            info.title
        );
        return Ok(None);
    }
    tracing::info!(
        "识别到目标窗口: 类型={}, 进程名={}, 类名={}",
        info.window_type,
        info.process_name,
        info.class_name
    );
    Ok(Some(info))
}

/// 获取窗口完整信息
fn get_window_info(hwnd: HWND) -> AppResult<WindowInfo> {
    let title = get_window_title(hwnd)?;
    let class_name = get_window_class_name(hwnd)?;
    let process_name = get_process_name(hwnd)?;
    let window_type = identify_window_type(&process_name, &class_name);
    let (m_left, m_top, m_width, m_height) = get_monitor_rect(hwnd)?;

    Ok(WindowInfo {
        hwnd: hwnd.0 as isize,
        title,
        class_name,
        process_name,
        window_type,
        monitor_left: m_left,
        monitor_top: m_top,
        monitor_width: m_width,
        monitor_height: m_height,
    })
}

/// 获取窗口标题
fn get_window_title(hwnd: HWND) -> AppResult<String> {
    let mut buffer = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    if len == 0 {
        return Ok(String::new());
    }
    Ok(String::from_utf16_lossy(&buffer[..len as usize]))
}

/// 获取窗口类名
fn get_window_class_name(hwnd: HWND) -> AppResult<String> {
    let mut buffer = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, &mut buffer) };
    if len == 0 {
        return Ok(String::new());
    }
    Ok(String::from_utf16_lossy(&buffer[..len as usize]))
}

/// 通过窗口句柄获取进程名
fn get_process_name(hwnd: HWND) -> AppResult<String> {
    let mut process_id: u32 = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut process_id as *mut u32));
    }
    if process_id == 0 {
        return Ok(String::new());
    }

    let handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, process_id) }
        .map_err(|e| AppError::Config(format!("打开进程失败: {e}")))?;

    // 使用 QueryFullProcessImageNameW 获取完整路径
    use windows::Win32::System::Threading::QueryFullProcessImageNameW;
    let mut buffer = [0u16; 1024];
    let mut len = buffer.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
        .map_err(|e| AppError::Config(format!("查询进程路径失败: {e}")))?;
    }
    let _ = unsafe { CloseHandle(handle) };

    let full_path = String::from_utf16_lossy(&buffer[..len as usize]);
    let file_name = Path::new(&full_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    Ok(file_name)
}

/// 通过进程名和类名双重验证识别窗口类型
fn identify_window_type(process_name: &str, class_name: &str) -> TargetWindowType {
    // 微信：进程名匹配 或 类名匹配
    if WECHAT_PROCESS_NAMES.iter().any(|p| p.eq_ignore_ascii_case(process_name))
        || WECHAT_CLASS_NAMES.iter().any(|c| c.eq_ignore_ascii_case(class_name))
    {
        return TargetWindowType::WeChat;
    }
    // QQ：进程名匹配 且 类名匹配（QQ 类名 TXGuiFoundation 较通用，需进程名配合）
    if QQ_PROCESS_NAMES.iter().any(|p| p.eq_ignore_ascii_case(process_name))
        && QQ_CLASS_NAMES.iter().any(|c| c.eq_ignore_ascii_case(class_name))
    {
        return TargetWindowType::QQ;
    }
    TargetWindowType::Unknown
}

/// 获取窗口所在显示器的矩形区域
fn get_monitor_rect(hwnd: HWND) -> AppResult<(i32, i32, i32, i32)> {
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let success = unsafe { GetMonitorInfoW(monitor, &mut info) };
    if !success.as_bool() {
        return Err(AppError::Config("获取显示器信息失败".to_string()));
    }
    let left = info.rcMonitor.left;
    let top = info.rcMonitor.top;
    let width = info.rcMonitor.right - info.rcMonitor.left;
    let height = info.rcMonitor.bottom - info.rcMonitor.top;
    Ok((left, top, width, height))
}

/// 枚举所有可见窗口（调试用，后续可扩展为查找特定窗口）
#[allow(dead_code)]
pub fn enumerate_visible_windows() -> AppResult<Vec<WindowInfo>> {
    let windows: Vec<WindowInfo> = Vec::new();
    let windows_box = Box::new(windows);
    let raw = Box::into_raw(windows_box);

    unsafe {
        EnumWindows(
            Some(enum_proc),
            LPARAM(raw as isize),
        )
        .map_err(|e| AppError::Config(format!("枚举窗口失败: {e}")))?;
    }

    let windows = unsafe { *Box::from_raw(raw) };
    Ok(windows)
}

/// EnumWindows 回调函数
extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let visible = unsafe { IsWindowVisible(hwnd) }.as_bool();
    if !visible {
        return BOOL(1);
    }

    let windows_box: *mut Vec<WindowInfo> = lparam.0 as *mut Vec<WindowInfo>;
    if let Ok(info) = get_window_info(hwnd) {
        if !matches!(info.window_type, TargetWindowType::Unknown) {
            unsafe {
                (*windows_box).push(info);
            }
        }
    }
    BOOL(1)
}
