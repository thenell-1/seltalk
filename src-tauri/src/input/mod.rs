// TODO 人工审查点：1.逐字延迟随机化 2.中断标志每字检查 3.焦点漂移检测 4.换行符处理
// NOTE 输入模拟入口：type_text 逐字 SendInput + 随机延迟 + 中断检查 + 焦点校验
pub mod sendinput;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use rand::Rng;

use crate::error::{AppError, AppResult};

/// 换行符统一处理：将 \r\n / \r 规整为 \n，避免 SendInput 重复注入回车
fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// 逐字模拟真人输入
///
/// - `text`：要输入的完整文本
/// - `min_ms` / `max_ms`：每字之间的随机延迟区间（毫秒）
/// - `interrupt`：中断标志，每字前检查；为 true 立即停止并返回 `Interrupted`
/// - `focus_hwnd`：调用方传入的预期前台窗口句柄；输入过程中若焦点漂移则停止
///
/// 返回 `Ok(())` 表示全部输入完成；`Err(Interrupted)` 表示被中断。
pub fn type_text(
    text: &str,
    min_ms: u64,
    max_ms: u64,
    interrupt: &Arc<AtomicBool>,
    focus_hwnd: isize,
) -> AppResult<()> {
    if text.is_empty() {
        return Ok(());
    }
    if min_ms > max_ms {
        return Err(AppError::Input(format!(
            "延迟区间非法: min({min_ms}) > max({max_ms})"
        )));
    }

    let normalized = normalize_newlines(text);
    let mut rng = rand::thread_rng();

    for ch in normalized.chars() {
        // 1. 中断检查（每字前）
        if interrupt.load(Ordering::Relaxed) {
            tracing::info!("输入被中断，已停止");
            return Err(AppError::Interrupted);
        }

        // 2. 焦点漂移检测：若前台窗口变化则停止，防止错输窗口
        let current_hwnd = sendinput::get_foreground_hwnd();
        if focus_hwnd != 0 && current_hwnd != focus_hwnd {
            return Err(AppError::Input(format!(
                "焦点已漂移，停止输入: 预期 {focus_hwnd}, 当前 {current_hwnd}"
            )));
        }

        // 3. 发送字符
        sendinput::send_char(ch)?;

        // 4. 随机延迟（模拟真人节奏）；换行稍长
        let (lo, hi) = if ch == '\n' {
            (min_ms.saturating_add(40), max_ms.saturating_add(80))
        } else {
            (min_ms, max_ms)
        };
        let delay = rng.gen_range(lo..=hi);
        thread::sleep(Duration::from_millis(delay));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_newlines_crlf() {
        assert_eq!(normalize_newlines("a\r\nb"), "a\nb");
    }

    #[test]
    fn test_normalize_newlines_cr_only() {
        assert_eq!(normalize_newlines("a\rb"), "a\nb");
    }

    #[test]
    fn test_normalize_newlines_lf_preserved() {
        assert_eq!(normalize_newlines("a\nb"), "a\nb");
    }

    #[test]
    fn test_type_text_empty_returns_ok() {
        let flag = Arc::new(AtomicBool::new(false));
        // 空文本应立即返回 Ok(())，unwrap 本身即验证成功
        type_text("", 10, 20, &flag, 0).unwrap();
    }

    #[test]
    fn test_type_text_invalid_range_errors() {
        let flag = Arc::new(AtomicBool::new(false));
        let r = type_text("x", 100, 50, &flag, 0);
        assert!(matches!(r, Err(AppError::Input(_))));
    }

    #[test]
    fn test_type_text_interrupted_immediately() {
        let flag = Arc::new(AtomicBool::new(true));
        let r = type_text("hello", 1, 2, &flag, 0);
        assert!(matches!(r, Err(AppError::Interrupted)));
    }
}
