// TODO 人工审查点：1.KEYEVENTF_UNICODE 绕输入法 2.down+up 配对 3.焦点校验 4.错误码
// NOTE Windows SendInput 逐字输入：Unicode 直注，中英文均不乱码
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};

use crate::error::{AppError, AppResult};

/// 输入单个字符（Unicode down + up）
pub fn send_char(ch: char) -> AppResult<()> {
    let scan = (ch as u32 & 0xFFFF) as u16;
    let input_down = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: scan,
                dwFlags: KEYEVENTF_UNICODE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let input_up = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: scan,
                dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        let n = SendInput(&[input_down, input_up], std::mem::size_of::<INPUT>() as i32);
        if n == 0 {
            return Err(AppError::Input(format!(
                "SendInput 失败: {}",
                std::io::Error::last_os_error()
            )));
        }
    }
    Ok(())
}

/// 获取前台窗口句柄（用于焦点校验）
pub fn get_foreground_hwnd() -> isize {
    unsafe { GetForegroundWindow().0 as isize }
}

/// 获取前台窗口标题（诊断用：记录热键触发时的焦点窗口，便于排查焦点漂移）
pub fn get_foreground_title() -> AppResult<Option<String>> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Ok(None);
        }
        let mut buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut buf);
        if len <= 0 {
            return Ok(Some(String::new()));
        }
        let s = String::from_utf16_lossy(&buf[..len as usize]);
        Ok(Some(s))
    }
}
