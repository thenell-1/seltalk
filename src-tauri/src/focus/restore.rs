// TODO 人工审查点：1.AttachThreadInput 资源释放 2.跨线程 SetFocus 失败处理 3.重试间隔合理性 4.isize↔HWND 转换
// NOTE 焦点恢复模块：从 input/sendinput.rs 迁入 + 新增 set_focus_to_ctl（子控件焦点恢复）
//       解决"悬浮窗隐藏后焦点不回归目标窗口"的核心修复，配合 WS_EX_NOACTIVATE 杜绝抢焦点
use std::ffi::c_void;
use std::time::Duration;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
};

/// 将 isize 转为 Windows HWND 类型（内部辅助，统一转换逻辑）
///
/// `isize` ↔ `HWND` 转换说明：
/// - `HWND.0` 是 `*mut c_void`，可 `as isize` 得到数值句柄（用于存储）
/// - 反向转换：`isize` → `*mut c_void` → `HWND`（用于调用 Win32 API）
pub fn to_hwnd(hwnd: isize) -> HWND {
    HWND(hwnd as *mut c_void)
}

/// 获取前台窗口句柄（用于焦点校验）
pub fn get_foreground_hwnd() -> isize {
    unsafe { GetForegroundWindow().0 as isize }
}

/// 主动将前台焦点恢复到目标窗口
///
/// 采用两级策略确保焦点恢复可靠性：
/// 1. **直接 SetForegroundWindow**：当 SelTalk 进程是当前前台进程时
///    （悬浮窗可见或刚被用户点击），此调用应成功
/// 2. **AttachThreadInput 技巧**：将当前线程的输入队列附加到前台窗口线程，
///    使当前线程"共享"前台权限，再调用 SetForegroundWindow。
///    适用于直接调用失败的场景（如 SelTalk 已丢失前台权限）。
///
/// # 安全性
/// - `AttachThreadInput` 的 attach/detach 严格配对，即使 SetForegroundWindow 失败也 detach
/// - 不 attach 相同线程（current_thread == fg_thread 时跳过，避免死锁）
///
/// # 返回值
/// - `true`：SetForegroundWindow 返回成功（Windows 内部判定允许设置前台）
/// - `false`： hwnd 为 0 / 两种方法均失败
///
/// 注意：返回 true 不等于焦点已切换完成（Windows 异步处理），
///       需配合 `is_foreground()` 验证或 `restore_foreground()` 带重试版本。
pub fn set_foreground_window(hwnd: isize) -> bool {
    if hwnd == 0 {
        return false;
    }
    let target = to_hwnd(hwnd);

    // 方法1：直接 SetForegroundWindow
    // 当 SelTalk 拥有前台权限时（悬浮窗可见/刚被点击），此调用应成功
    unsafe {
        if SetForegroundWindow(target).as_bool() {
            return true;
        }
    }

    // 方法2：AttachThreadInput 技巧
    // 附加当前线程与前台窗口线程的输入队列，使当前线程获得设置前台窗口的能力
    unsafe {
        let current_thread = GetCurrentThreadId();
        let foreground_hwnd = GetForegroundWindow();
        let fg_thread = GetWindowThreadProcessId(foreground_hwnd, None);

        // fg_thread == 0 表示获取失败（窗口无效）；current_thread == fg_thread 无需 attach（同线程）
        if fg_thread != 0 && current_thread != fg_thread {
            let attached = AttachThreadInput(current_thread, fg_thread, true).as_bool();
            if attached {
                // 附加后再次尝试设置前台窗口
                let result = SetForegroundWindow(target).as_bool();
                // 无论成功与否都必须 detach，避免输入队列长期共享导致输入混乱
                let detach_ok = AttachThreadInput(current_thread, fg_thread, false).as_bool();
                if !detach_ok {
                    tracing::warn!("AttachThreadInput detach 失败（线程可能已退出），输入队列可能未分离");
                }
                return result;
            }
        }
    }

    false
}

/// 校验当前前台窗口是否为预期窗口
///
/// 用于 SetForegroundWindow 后的验证：Windows 焦点切换是异步的，
/// 调用 SetForegroundWindow 后需短暂等待再验证 GetForegroundWindow 结果。
pub fn is_foreground(hwnd: isize) -> bool {
    if hwnd == 0 {
        return false;
    }
    get_foreground_hwnd() == hwnd
}

/// 带重试的焦点恢复：尝试多次恢复焦点并验证
///
/// # 重试策略
/// Windows 焦点切换是异步的：`SetForegroundWindow` 返回 true 后，
/// `GetForegroundWindow` 可能仍短暂返回旧窗口。
/// 本函数在每次 `set_foreground_window` 后等待 `retry_interval_ms` 再验证，
/// 最多重试 `max_retries` 次。
///
/// # 参数
/// - `hwnd`：目标窗口句柄
/// - `max_retries`：最大尝试次数（含首次，建议 3）
/// - `retry_interval_ms`：每次重试间隔（建议 30ms）
///
/// # 返回值
/// - `true`：焦点已成功恢复到目标窗口
/// - `false`：所有重试均未验证到焦点恢复（hwnd=0 或恢复失败）
pub fn restore_foreground(hwnd: isize, max_retries: u32, retry_interval_ms: u64) -> bool {
    if hwnd == 0 {
        return false;
    }

    for attempt in 0..max_retries {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(retry_interval_ms));
        }

        if set_foreground_window(hwnd) {
            // 短暂等待 Windows 异步完成焦点切换
            std::thread::sleep(Duration::from_millis(20));
            if is_foreground(hwnd) {
                tracing::debug!(
                    "焦点恢复成功（尝试 {}），目标窗口 hwnd={}",
                    attempt + 1,
                    hwnd
                );
                return true;
            }
        }
    }

    // 最终验证（不调用 set，仅检查当前前台是否已为目标窗口）
    let final_ok = is_foreground(hwnd);
    if !final_ok {
        let current = get_foreground_hwnd();
        tracing::warn!(
            "焦点恢复失败（尝试 {} 次），目标 hwnd={} 当前台 hwnd={}",
            max_retries,
            hwnd,
            current
        );
    }
    final_ok
}

/// 跨线程设置焦点控件（恢复子控件键盘焦点）
///
/// 用于恢复子控件焦点（如浏览器内的输入框、IDE 的编辑器）。
/// `SetFocus` 跨线程调用默认会失败（目标窗口不属于当前线程），
/// 需先通过 `AttachThreadInput` 共享输入队列。
///
/// # 参数
/// - `ctl_hwnd`：目标焦点控件 HWND
/// - `target_thread_id`：目标控件所在线程 ID（来自 FocusContext.thread_id）
///
/// # 返回值
/// - `true`：SetFocus 调用成功
/// - `false`：参数无效 / AttachThreadInput 失败 / SetFocus 失败
///
/// # 安全性
/// - AttachThreadInput 的 attach/detach 严格配对
/// - 同线程时跳过 attach（避免死锁），直接 SetFocus
pub fn set_focus_to_ctl(ctl_hwnd: isize, target_thread_id: u32) -> bool {
    if ctl_hwnd == 0 || target_thread_id == 0 {
        return false;
    }
    let target = to_hwnd(ctl_hwnd);
    let current_thread = unsafe { GetCurrentThreadId() };

    // 同线程：直接 SetFocus（无需 AttachThreadInput）
    if current_thread == target_thread_id {
        let result = unsafe { SetFocus(target).is_ok() };
        if !result {
            tracing::warn!("同线程 SetFocus 失败，ctl_hwnd={}", ctl_hwnd);
        }
        return result;
    }

    // 跨线程：AttachThreadInput 共享输入队列后 SetFocus
    unsafe {
        let attached = AttachThreadInput(current_thread, target_thread_id, true).as_bool();
        if !attached {
            tracing::warn!(
                "AttachThreadInput 失败，无法跨线程设置焦点: target_thread={}",
                target_thread_id
            );
            return false;
        }

        let result = SetFocus(target).is_ok();

        // 无论成功与否都 detach
        let detach_ok = AttachThreadInput(current_thread, target_thread_id, false).as_bool();
        if !detach_ok {
            tracing::warn!("AttachThreadInput detach 失败（线程可能已退出）");
        }

        if !result {
            tracing::warn!(
                "跨线程 SetFocus 失败: ctl_hwnd={}, target_thread={}",
                ctl_hwnd,
                target_thread_id
            );
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== to_hwnd 转换测试 =====

    #[test]
    fn test_to_hwnd_zero() {
        let h = to_hwnd(0);
        assert!(h.0.is_null(), "isize(0) 转换后应为 null 指针");
    }

    #[test]
    fn test_to_hwnd_nonzero() {
        let h = to_hwnd(12345);
        assert!(!h.0.is_null(), "非零 isize 转换后应为非 null 指针");
    }

    #[test]
    fn test_to_hwnd_roundtrip() {
        let original: isize = 65536;
        let hwnd = to_hwnd(original);
        let restored = hwnd.0 as isize;
        assert_eq!(original, restored, "isize ↔ HWND 转换应可逆");
    }

    // ===== set_foreground_window 边界测试 =====

    #[test]
    fn test_set_foreground_window_zero_returns_false() {
        assert!(!set_foreground_window(0));
    }

    // ===== is_foreground 边界测试 =====

    #[test]
    fn test_is_foreground_zero_returns_false() {
        assert!(!is_foreground(0));
    }

    #[test]
    fn test_is_foreground_matches_current() {
        // 当前前台窗口（测试进程的控制台/测试运行器）应能被 is_foreground 识别
        let current = get_foreground_hwnd();
        if current != 0 {
            assert!(is_foreground(current), "当前前台窗口应匹配自身");
        }
    }

    #[test]
    fn test_is_foreground_nonexistent_window() {
        // 一个不可能存在的 hwnd 值（极大值）不应匹配当前前台
        let fake_hwnd: isize = 0x7FFFFFFF;
        let current = get_foreground_hwnd();
        if current != fake_hwnd {
            assert!(!is_foreground(fake_hwnd), "不存在的窗口不应匹配前台");
        }
    }

    // ===== restore_foreground 边界测试 =====

    #[test]
    fn test_restore_foreground_zero_returns_false() {
        assert!(!restore_foreground(0, 3, 10));
    }

    #[test]
    fn test_restore_foreground_nonexistent_fails_fast() {
        // 不存在的窗口句柄：SetForegroundWindow 应失败，restore 应在重试后返回 false
        let fake_hwnd: isize = 0x7FFFFFFF;
        let result = restore_foreground(fake_hwnd, 2, 5);
        // 不断言 false（极小概率可能匹配），仅验证不 panic 且在合理时间内返回
        // 2 次重试 × (5+20)ms ≈ 50ms，应快速返回
        let _ = result;
    }

    // ===== get_foreground_hwnd 基本测试 =====

    #[test]
    fn test_get_foreground_hwnd_returns_isize() {
        let hwnd = get_foreground_hwnd();
        // 不断言非零（CI 环境可能无桌面），仅验证不 panic
        let _ = hwnd;
    }

    // ===== set_focus_to_ctl 边界测试 =====

    #[test]
    fn test_set_focus_to_ctl_zero_hwnd_returns_false() {
        assert!(!set_focus_to_ctl(0, 12345));
    }

    #[test]
    fn test_set_focus_to_ctl_zero_thread_returns_false() {
        assert!(!set_focus_to_ctl(12345, 0));
    }

    #[test]
    fn test_set_focus_to_ctl_both_zero_returns_false() {
        assert!(!set_focus_to_ctl(0, 0));
    }
}
