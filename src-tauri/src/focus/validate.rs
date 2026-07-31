// TODO 人工审查点：1.IsWindow/IsIconic/IsWindowVisible 返回值 2.缓存新鲜度阈值 3.错误信息可读性 4.本程序窗口过滤完整性
// NOTE 输入前置校验：在 do_type_candidate 入口实时校验焦点上下文有效性，防止文本输入到错误窗口
use std::time::Duration;

use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindow, IsWindowVisible};

use crate::error::{AppError, AppResult};
use crate::focus::restore::to_hwnd;
use crate::focus::tracker::FocusContext;

/// 缓存最大可接受年龄（30 秒）
/// 超过此值视为缓存过期（用户可能已切换窗口但 WinEvent 钩子未及时回调）
const MAX_CACHE_AGE_SECS: u64 = 30;

/// 输入前置校验：检查焦点上下文是否可用于安全输入
///
/// 校验链（按用户方案"焦点窗口校验 + 窗口状态校验 + 焦点控件 ≠ 本程序窗口"）：
/// 1. 缓存已初始化（updated_at 不为 None）
/// 2. 缓存未过期（30 秒内）
/// 3. 顶层窗口有效性（IsWindow）
/// 4. 焦点控件有效性（IsWindow）
/// 5. 焦点控件 ≠ 本程序悬浮窗 / 管理面板
/// 6. 顶层窗口未最小化（!IsIconic）
/// 7. 顶层窗口可见（IsWindowVisible）
///
/// # 参数
/// - `ctx`：实时读取的焦点上下文（不允许使用 trigger 时的缓存）
/// - `float_hwnd`：本程序悬浮窗 HWND
/// - `manager_hwnd`：本程序管理面板 HWND
///
/// # 返回
/// - `Ok(())`：所有校验通过，可安全输入
/// - `Err(AppError::Input(_))`：校验失败，附带中文原因
pub fn validate_for_input(
    ctx: &FocusContext,
    float_hwnd: isize,
    manager_hwnd: isize,
) -> AppResult<()> {
    // ① 缓存已初始化
    if ctx.updated_at.is_none() {
        return Err(AppError::Input(
            "焦点上下文未初始化（WinEvent 钩子可能未启动）".into(),
        ));
    }

    // ② 缓存未过期
    if ctx.is_stale(Duration::from_secs(MAX_CACHE_AGE_SECS)) {
        return Err(AppError::Input(format!(
            "焦点上下文已过期（超过 {} 秒未更新）",
            MAX_CACHE_AGE_SECS
        )));
    }

    // ③ 顶层窗口有效性
    if ctx.top_hwnd == 0 {
        return Err(AppError::Input("顶层窗口 HWND 为 0".into()));
    }
    if !is_window_valid(ctx.top_hwnd) {
        return Err(AppError::Input(format!(
            "顶层窗口已失效 (hwnd={})",
            ctx.top_hwnd
        )));
    }

    // ④ 焦点控件有效性
    if ctx.focus_ctl_hwnd == 0 {
        return Err(AppError::Input("焦点控件 HWND 为 0".into()));
    }
    if !is_window_valid(ctx.focus_ctl_hwnd) {
        return Err(AppError::Input(format!(
            "焦点控件已失效 (hwnd={})",
            ctx.focus_ctl_hwnd
        )));
    }

    // ⑤ 焦点控件 ≠ 本程序窗口
    if ctx.focus_ctl_hwnd == float_hwnd {
        return Err(AppError::Input("焦点控件为本程序悬浮窗，禁止输入".into()));
    }
    if ctx.focus_ctl_hwnd == manager_hwnd {
        return Err(AppError::Input("焦点控件为本程序管理面板，禁止输入".into()));
    }
    if ctx.top_hwnd == float_hwnd {
        return Err(AppError::Input("顶层窗口为本程序悬浮窗，禁止输入".into()));
    }
    if ctx.top_hwnd == manager_hwnd {
        return Err(AppError::Input("顶层窗口为本程序管理面板，禁止输入".into()));
    }

    // ⑥ 顶层窗口未最小化
    if is_window_minimized(ctx.top_hwnd) {
        return Err(AppError::Input(format!(
            "目标窗口已最小化 (hwnd={})",
            ctx.top_hwnd
        )));
    }

    // ⑦ 顶层窗口可见
    if !is_window_visible_check(ctx.top_hwnd) {
        return Err(AppError::Input(format!(
            "目标窗口不可见 (hwnd={})",
            ctx.top_hwnd
        )));
    }

    Ok(())
}

/// 校验窗口句柄是否仍有效（IsWindow）
fn is_window_valid(hwnd: isize) -> bool {
    if hwnd == 0 {
        return false;
    }
    unsafe { IsWindow(to_hwnd(hwnd)).as_bool() }
}

/// 校验窗口是否最小化（IsIconic）
fn is_window_minimized(hwnd: isize) -> bool {
    if hwnd == 0 {
        return false;
    }
    unsafe { IsIconic(to_hwnd(hwnd)).as_bool() }
}

/// 校验窗口是否可见（IsWindowVisible）
fn is_window_visible_check(hwnd: isize) -> bool {
    if hwnd == 0 {
        return false;
    }
    unsafe { IsWindowVisible(to_hwnd(hwnd)).as_bool() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn make_valid_ctx() -> FocusContext {
        // 使用当前测试进程的前台窗口（CI 环境可能为 0）
        let hwnd = crate::focus::restore::get_foreground_hwnd();
        FocusContext {
            top_hwnd: hwnd,
            focus_ctl_hwnd: hwnd,
            pid: 0,
            thread_id: 0,
            updated_at: Some(Instant::now()),
        }
    }

    #[test]
    fn test_validate_uninitialized_context() {
        let ctx = FocusContext::default();
        let result = validate_for_input(&ctx, 0, 0);
        assert!(result.is_err());
        assert!(matches!(result, Err(AppError::Input(_))));
    }

    #[test]
    fn test_validate_stale_context() {
        let ctx = FocusContext {
            top_hwnd: 12345,
            focus_ctl_hwnd: 12345,
            updated_at: Some(Instant::now() - Duration::from_secs(60)),
            ..Default::default()
        };
        let result = validate_for_input(&ctx, 0, 0);
        assert!(result.is_err());
        if let Err(AppError::Input(msg)) = result {
            assert!(msg.contains("过期"));
        }
    }

    #[test]
    fn test_validate_zero_top_hwnd() {
        let ctx = FocusContext {
            focus_ctl_hwnd: 12345,
            updated_at: Some(Instant::now()),
            ..Default::default()
        };
        let result = validate_for_input(&ctx, 0, 0);
        assert!(result.is_err());
        if let Err(AppError::Input(msg)) = result {
            assert!(msg.contains("顶层窗口 HWND 为 0"));
        }
    }

    #[test]
    fn test_validate_zero_focus_ctl_hwnd() {
        // 需要有效的 top_hwnd 才能到达 focus_ctl_hwnd 校验分支
        // make_valid_ctx 使用当前前台窗口（CI 环境可能为 0，此时跳过）
        let mut ctx = make_valid_ctx();
        if ctx.top_hwnd == 0 {
            return;
        }
        ctx.focus_ctl_hwnd = 0;
        let result = validate_for_input(&ctx, 0, 0);
        assert!(result.is_err());
        if let Err(AppError::Input(msg)) = result {
            assert!(msg.contains("焦点控件 HWND 为 0"), "实际错误: {}", msg);
        }
    }

    #[test]
    fn test_validate_focus_ctl_is_float_window() {
        // 需要有效的 focus_ctl_hwnd 才能到达"焦点控件 ≠ 本程序窗口"校验分支
        // 使用当前前台窗口（有效）作为 focus_ctl_hwnd 和 float_hwnd
        let ctx = make_valid_ctx();
        if ctx.top_hwnd == 0 {
            return;
        }
        let result = validate_for_input(&ctx, ctx.focus_ctl_hwnd, 0);
        assert!(result.is_err());
        if let Err(AppError::Input(msg)) = result {
            assert!(msg.contains("悬浮窗"), "实际错误: {}", msg);
        }
    }

    #[test]
    fn test_validate_focus_ctl_is_manager_window() {
        // 使用当前前台窗口（有效）作为 focus_ctl_hwnd 和 manager_hwnd
        let ctx = make_valid_ctx();
        if ctx.top_hwnd == 0 {
            return;
        }
        let result = validate_for_input(&ctx, 0, ctx.focus_ctl_hwnd);
        assert!(result.is_err());
        if let Err(AppError::Input(msg)) = result {
            assert!(msg.contains("管理面板"), "实际错误: {}", msg);
        }
    }

    #[test]
    fn test_validate_top_hwnd_is_float_window() {
        // 使用当前前台窗口（有效）作为 top_hwnd 和 float_hwnd
        // focus_ctl_hwnd 保持等于 top_hwnd（有效），使校验能到达"顶层窗口 ≠ 本程序窗口"分支
        let ctx = make_valid_ctx();
        if ctx.top_hwnd == 0 {
            return;
        }
        let result = validate_for_input(&ctx, ctx.top_hwnd, 0);
        assert!(result.is_err());
        if let Err(AppError::Input(msg)) = result {
            assert!(msg.contains("悬浮窗"), "实际错误: {}", msg);
        }
    }

    #[test]
    fn test_validate_nonexistent_hwnd() {
        let ctx = FocusContext {
            top_hwnd: 0x7FFFFFFF, // 极大值，几乎不可能是有效窗口
            focus_ctl_hwnd: 0x7FFFFFFF,
            updated_at: Some(Instant::now()),
            ..Default::default()
        };
        let result = validate_for_input(&ctx, 0, 0);
        assert!(result.is_err());
        if let Err(AppError::Input(msg)) = result {
            // 应在校验 IsWindow 时失败
            assert!(msg.contains("失效") || msg.contains("最小化") || msg.contains("不可见"));
        }
    }

    #[test]
    fn test_is_window_valid_zero() {
        assert!(!is_window_valid(0));
    }

    #[test]
    fn test_is_window_minimized_zero() {
        assert!(!is_window_minimized(0));
    }

    #[test]
    fn test_is_window_visible_check_zero() {
        assert!(!is_window_visible_check(0));
    }
}
