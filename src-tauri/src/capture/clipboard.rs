// TODO 人工审查点：1.剪贴板读取后的原始内容恢复 2.按键模拟时序 3.超时处理
// NOTE F2 Ctrl+C 备选模块：当 UI Automation 失败时的降级方案
// 流程：备份当前剪贴板 → 模拟 Ctrl+C → 读取剪贴板 → 恢复原始剪贴板

use crate::error::{AppError, AppResult};
use clipboard_win::{get_clipboard_string, set_clipboard_string};
use std::thread;
use std::time::Duration;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_C,
};

/// 通过模拟 Ctrl+C 获取选中文本
/// 1. 备份当前剪贴板内容
/// 2. 模拟 Ctrl+C 按键
/// 3. 等待剪贴板更新
/// 4. 读取剪贴板内容
/// 5. 恢复原始剪贴板内容
pub fn get_selected_text_via_clipboard() -> AppResult<Option<String>> {
    // 1. 备份剪贴板
    let original = get_clipboard_string().ok();
    tracing::debug!("备份剪贴板: {} 字符", original.as_ref().map(|s| s.len()).unwrap_or(0));

    // 2. 清空剪贴板（确保能检测到新内容）
    let _ = set_clipboard_string("");

    // 3. 模拟 Ctrl+C
    simulate_ctrl_c()?;

    // 4. 等待剪贴板更新（轮询，最多 500ms）
    let mut captured = None;
    for _ in 0..10 {
        thread::sleep(Duration::from_millis(50));
        if let Ok(text) = get_clipboard_string() {
            if !text.is_empty() {
                captured = Some(text);
                break;
            }
        }
    }

    // 5. 恢复原始剪贴板
    if let Some(ref orig) = original {
        let _ = set_clipboard_string(orig);
        tracing::debug!("已恢复原始剪贴板");
    }

    match captured {
        Some(text) => {
            tracing::debug!("Ctrl+C 捕获成功: {} 字符", text.len());
            Ok(Some(text))
        }
        None => {
            tracing::warn!("Ctrl+C 捕获失败：剪贴板未更新");
            Ok(None)
        }
    }
}

/// 模拟 Ctrl+C 按键
fn simulate_ctrl_c() -> AppResult<()> {
    // 按下 Ctrl
    send_key(VK_CONTROL.0 as u16, false)?;
    // 按下 C
    send_key(VK_C.0 as u16, false)?;
    // 释放 C
    send_key(VK_C.0 as u16, true)?;
    // 释放 Ctrl
    send_key(VK_CONTROL.0 as u16, true)?;

    Ok(())
}

/// 发送单个按键
/// key_code: 虚拟键码
/// key_up: true=释放, false=按下
fn send_key(key_code: u16, key_up: bool) -> AppResult<()> {
    let mut flags = 0u32;
    if key_up {
        flags |= KEYEVENTF_KEYUP.0;
    }

    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(key_code),
                wScan: 0,
                dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(flags),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    let result = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    if result == 0 {
        return Err(AppError::Config(format!(
            "SendInput 失败: 错误码 {}",
            std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
        )));
    }
    Ok(())
}
