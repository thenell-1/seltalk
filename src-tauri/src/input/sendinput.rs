// TODO 人工审查点：1.KEYEVENTF_UNICODE 绕输入法 2.down+up 配对 3.焦点校验 4.错误码 5.焦点恢复时机 6.AttachThreadInput 资源释放
// NOTE Windows SendInput 逐字输入：Unicode 直注，中英文均不乱码
//       P-FOCUS-MGR: 焦点恢复相关函数（restore_foreground/set_foreground_window/is_foreground/
//                     to_hwnd/get_foreground_hwnd）已迁移到 focus::restore 模块，
//                     此处通过 `pub use` 重导出保持外部调用方兼容（input::sendinput::xxx 仍可用）
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};

use crate::error::{AppError, AppResult};

// ===== P-FOCUS-MGR: 重导出已迁移到 focus::restore 的函数（保持外部调用方兼容） =====
// 调用方仍可使用 input::sendinput::restore_foreground / set_foreground_window / is_foreground /
// get_foreground_hwnd / to_hwnd，实际实现位于 crate::focus::restore
// 注：部分函数当前 crate 内未直接使用（orchestrator 已改用 FocusManager），
//     但作为公共 API 保留供未来扩展或外部调用方使用
#[allow(unused_imports)]
pub use crate::focus::restore::{
    get_foreground_hwnd, is_foreground, restore_foreground, set_foreground_window, to_hwnd,
};

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

#[cfg(test)]
mod tests {
    use super::*;

    // ===== get_foreground_title 基本测试 =====
    #[test]
    fn test_get_foreground_title_returns_option() {
        // 在测试环境中应返回 Ok(Some(String)) 或 Ok(None)（无桌面会话）
        // 仅验证不 panic
        let result = get_foreground_title();
        assert!(result.is_ok());
    }

    // ===== 重导出函数的可访问性测试 =====
    // 注：to_hwnd / set_foreground_window / is_foreground / restore_foreground / get_foreground_hwnd
    // 的实际测试位于 focus::restore 模块，此处仅验证重导出可编译通过（类型/函数可见）

    #[test]
    fn test_reexported_get_foreground_hwnd_is_callable() {
        // 重导出的函数应可直接调用（验证 pub use 生效）
        let _ = get_foreground_hwnd();
    }

    #[test]
    fn test_reexported_to_hwnd_zero() {
        let h = to_hwnd(0);
        assert!(h.0.is_null(), "重导出的 to_hwnd(0) 应返回 null 指针");
    }

    #[test]
    fn test_reexported_is_foreground_zero_returns_false() {
        assert!(!is_foreground(0));
    }

    #[test]
    fn test_reexported_set_foreground_window_zero_returns_false() {
        assert!(!set_foreground_window(0));
    }

    #[test]
    fn test_reexported_restore_foreground_zero_returns_false() {
        assert!(!restore_foreground(0, 3, 10));
    }
}
