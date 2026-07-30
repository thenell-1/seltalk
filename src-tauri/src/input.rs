// TODO 人工审查点：1.SendInput权限与兼容性 2. surrogate pair 处理 3. 延迟随机性 4. 输入中断处理
// NOTE F6 键盘模拟模块：使用 SendInput Unicode 方案逐字输入文本
// 原理：对每个字符构造 KEYBDINPUT，设置 wScan 为 Unicode 码点，dwFlags 设 KEYEVENTF_UNICODE
// 逐字发送 KEYDOWN + KEYUP，字符间插入随机延迟，模拟人类打字节奏
// 支持 ESC 键中断输入（PRD US-3/5.5.3）：输入过程中检测 ESC，立即停止并保留已输入内容

use crate::error::{AppError, AppResult};
use rand::Rng;
use std::thread;
use std::time::Duration;

/// 默认打字延迟下限（毫秒）
const DEFAULT_DELAY_MIN: u64 = 50;
/// 默认打字延迟上限（毫秒）
const DEFAULT_DELAY_MAX: u64 = 150;

/// 打字配置
#[derive(Debug, Clone)]
pub struct TypingConfig {
    pub delay_min_ms: u64,
    pub delay_max_ms: u64,
}

impl Default for TypingConfig {
    fn default() -> Self {
        Self {
            delay_min_ms: DEFAULT_DELAY_MIN,
            delay_max_ms: DEFAULT_DELAY_MAX,
        }
    }
}

/// 输入结果（PRD US-3：ESC 中断后已输入内容保留）
#[derive(Debug, Clone, PartialEq)]
pub enum TypingStatus {
    /// 输入全部完成
    Completed,
    /// 用户按 ESC 中断，已输入内容保留
    Interrupted,
}

/// 逐字输入文本（主入口）
/// 返回 TypingStatus 表示是完成还是被 ESC 中断
pub fn type_text(text: &str, config: &TypingConfig) -> AppResult<TypingStatus> {
    if text.is_empty() {
        return Err(AppError::Config("输入文本不能为空".to_string()));
    }

    tracing::info!("开始逐字输入，字符数: {}", text.chars().count());

    for ch in text.chars() {
        // NOTE 每个字符输入前检查 ESC 是否被按下（PRD US-3）
        if is_escape_pressed() {
            tracing::info!("检测到 ESC 键，中断输入，已输入内容保留");
            return Ok(TypingStatus::Interrupted);
        }

        type_single_char(ch)?;
        let delay = random_delay(config.delay_min_ms, config.delay_max_ms);
        thread::sleep(Duration::from_millis(delay));
    }

    tracing::info!("逐字输入完成");
    Ok(TypingStatus::Completed)
}

/// 检测 ESC 键是否被按下（Windows 平台使用 GetAsyncKeyState）
#[cfg(target_os = "windows")]
fn is_escape_pressed() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    // NOTE VK_ESCAPE = 0x1B，GetAsyncKeyState 返回 i16，最高位为1表示按键被按下
    let state = unsafe { GetAsyncKeyState(0x1B) };
    (state as u16 & 0x8000) != 0
}

#[cfg(not(target_os = "windows"))]
fn is_escape_pressed() -> bool {
    false
}

/// 输入单个字符
fn type_single_char(ch: char) -> AppResult<()> {
    let code = ch as u32;
    // NOTE 基本多文种平面（BMP）字符直接输入
    // 补充平面字符（如部分 emoji）需 surrogate pair，此处暂不支持
    if code > 0xFFFF {
        return Err(AppError::Config(format!(
            "暂不支持补充平面字符: U+{code:04X}"
        )));
    }

    send_unicode_key(code, true)?;
    send_unicode_key(code, false)?;
    Ok(())
}

/// 发送 Unicode 按键事件
fn send_unicode_key(scan_code: u32, key_down: bool) -> AppResult<()> {
    #[cfg(target_os = "windows")]
    {
        send_unicode_key_windows(scan_code, key_down)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (scan_code, key_down);
        Err(AppError::Config("当前系统不支持键盘模拟".to_string()))
    }
}

/// Windows 平台 SendInput 实现
#[cfg(target_os = "windows")]
fn send_unicode_key_windows(scan_code: u32, key_down: bool) -> AppResult<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, KEYBDINPUT, KEYEVENTF_UNICODE, INPUT, INPUT_TYPE,
    };

    let mut flags = KEYEVENTF_UNICODE;
    if !key_down {
        flags |= windows::Win32::UI::Input::KeyboardAndMouse::KEYEVENTF_KEYUP;
    }

    let kb_input = KEYBDINPUT {
        wVk: windows::Win32::UI::Input::KeyboardAndMouse::VK_0,
        wScan: scan_code as u16,
        dwFlags: flags,
        time: 0,
        dwExtraInfo: 0,
    };

    let input = INPUT {
        r#type: INPUT_TYPE(1), // INPUT_KEYBOARD
        Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 { ki: kb_input },
    };

    let result = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    if result == 0 {
        return Err(AppError::Config(format!(
            "SendInput 调用失败，错误码: {}",
            std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
        )));
    }
    Ok(())
}

/// 生成随机延迟（毫秒）
fn random_delay(min_ms: u64, max_ms: u64) -> u64 {
    if min_ms >= max_ms {
        return min_ms;
    }
    rand::thread_rng().gen_range(min_ms..=max_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_random_delay_normal() {
        let delay = random_delay(50, 150);
        assert!(delay >= 50 && delay <= 150);
    }

    #[test]
    fn test_random_delay_min_equal_max() {
        let delay = random_delay(100, 100);
        assert_eq!(delay, 100);
    }

    #[test]
    fn test_random_delay_min_greater_than_max() {
        let delay = random_delay(150, 50);
        assert_eq!(delay, 150);
    }

    #[test]
    fn test_type_text_empty() {
        let config = TypingConfig::default();
        let result = type_text("", &config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("不能为空"));
    }

    #[test]
    fn test_type_single_char_supplementary_plane() {
        // emoji 属于补充平面，应返回错误
        let result = type_single_char('🎉');
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("补充平面"));
    }

    #[test]
    fn test_type_single_char_bmp() {
        // 基本多文种平面字符应正常（Windows 平台）
        #[cfg(target_os = "windows")]
        {
            let result = type_single_char('你');
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_typing_config_default() {
        let config = TypingConfig::default();
        assert_eq!(config.delay_min_ms, DEFAULT_DELAY_MIN);
        assert_eq!(config.delay_max_ms, DEFAULT_DELAY_MAX);
    }
}
