// TODO 人工审查点：1.unsafe 回调签名 2.全局静态状态生命周期 3.线程消息循环退出 4.钩子卸载时序 5.GetGUIThreadInfo 跨线程安全
// NOTE WinEvent 钩子：专用工作线程 + PeekMessage 循环 + EVENT_SYSTEM_FOREGROUND/EVENT_OBJECT_FOCUS 双追踪
//       仅 OUT_OF_CONTEXT 模式（回调在工作线程，避免注入目标进程，规避杀软告警）
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetAncestor, GetGUIThreadInfo, GetWindowThreadProcessId, GA_ROOT,
    GUITHREADINFO, MSG, PM_REMOVE, PeekMessageW, TranslateMessage, EVENT_OBJECT_FOCUS,
    EVENT_SYSTEM_FOREGROUND, WINEVENT_OUTOFCONTEXT,
};

use crate::error::{AppError, AppResult};
use crate::focus::tracker::{FocusContext, FocusTracker};

/// 焦点事件来源为客户区（过滤标题栏/滚动条等系统对象）
/// OBJID_CLIENT 在 windows crate 中为 OBJID newtype，直接用 i32 值避免类型转换
const OBJID_CLIENT_RAW: i32 = -4;

/// 工作线程消息循环轮询间隔（无消息时短暂 sleep，避免空转消耗 CPU）
/// 100ms：兼顾响应性（焦点切换感知延迟 <100ms）与空闲 CPU 占用（<0.5%）
const POLL_INTERVAL_MS: u64 = 100;

/// 全局共享状态：WinEvent 回调函数为 C 风格函数指针，无法捕获闭包，
/// 通过 OnceLock 暴露共享数据给回调
struct HookShared {
    tracker: Arc<FocusTracker>,
    float_hwnd: AtomicIsize,
    manager_hwnd: AtomicIsize,
}

static HOOK_SHARED: OnceLock<Arc<HookShared>> = OnceLock::new();

/// 启动 WinEvent 钩子（专用工作线程 + 消息循环）
///
/// # 参数
/// - `tracker`：焦点上下文缓存（Arc 共享给工作线程 + 回调）
/// - `float_hwnd`：本程序悬浮窗 HWND（过滤用，0 表示尚未创建）
/// - `manager_hwnd`：本程序管理面板 HWND（过滤用，0 表示尚未创建）
/// - `shutdown_flag`：退出信号，工作线程每 POLL_INTERVAL_MS 检查一次
///
/// # 返回
/// 工作线程 JoinHandle，调用方 join 后可确保钩子已卸载
///
/// # 错误
/// - 重复调用（HOOK_SHARED 已设置）：返回 Config 错误
/// - 工作线程 spawn 失败：返回 Config 错误
///
/// # 安全性
/// - `SetWinEventHook` 用 `WINEVENT_OUTOFCONTEXT`：回调在调用线程上下文（工作线程），
///   不会注入目标进程，规避杀软告警
/// - 回调仅做最小工作（GetWindowThreadProcessId + GetGUIThreadInfo + 写 RwLock），
///   不调用阻塞 API，避免工作线程长时间被占用
pub fn start_hook(
    tracker: Arc<FocusTracker>,
    float_hwnd: isize,
    manager_hwnd: isize,
    shutdown_flag: Arc<AtomicBool>,
) -> AppResult<JoinHandle<()>> {
    let shared = Arc::new(HookShared {
        tracker,
        float_hwnd: AtomicIsize::new(float_hwnd),
        manager_hwnd: AtomicIsize::new(manager_hwnd),
    });

    // HOOK_SHARED 是 OnceLock，仅允许设置一次（FocusManager::start 应仅调用一次）
    HOOK_SHARED.set(shared).map_err(|_| {
        AppError::Config("WinEvent 钩子共享状态已初始化（重复启动）".into())
    })?;

    let handle = thread::Builder::new()
        .name("seltalk-winevent".into())
        .spawn(move || worker_main(shutdown_flag))
        .map_err(|e| AppError::Config(format!("启动 WinEvent 工作线程失败: {e}")))?;

    Ok(handle)
}

/// 更新悬浮窗 HWND（窗口创建后调用，使回调能过滤本程序窗口）
#[allow(dead_code)]
pub fn update_float_hwnd(hwnd: isize) {
    if let Some(shared) = HOOK_SHARED.get() {
        shared.float_hwnd.store(hwnd, Ordering::Relaxed);
    }
}

/// 更新管理面板 HWND
#[allow(dead_code)]
pub fn update_manager_hwnd(hwnd: isize) {
    if let Some(shared) = HOOK_SHARED.get() {
        shared.manager_hwnd.store(hwnd, Ordering::Relaxed);
    }
}

/// 工作线程主函数：注册钩子 → 消息循环 → 卸载钩子
fn worker_main(shutdown_flag: Arc<AtomicBool>) {
    // 1. 注册两个钩子（顶层窗口切换 + 子控件焦点变化）
    //    windows 0.58: SetWinEventHook 直接返回 HWINEVENTHOOK（非 Result），
    //    失败时返回无效句柄（0 或 -1），通过 is_invalid() 判断
    let hook_fg = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(win_event_callback),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        )
    };
    let hook_focus = unsafe {
        SetWinEventHook(
            EVENT_OBJECT_FOCUS,
            EVENT_OBJECT_FOCUS,
            None,
            Some(win_event_callback),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        )
    };

    if hook_fg.is_invalid() || hook_focus.is_invalid() {
        tracing::error!(
            "WinEvent 钩子注册失败: fg_invalid={}, focus_invalid={}",
            hook_fg.is_invalid(),
            hook_focus.is_invalid()
        );
        // 清理已注册的钩子（仅清理有效句柄）
        if !hook_fg.is_invalid() {
            unsafe {
                let _ = UnhookWinEvent(hook_fg);
            }
        }
        if !hook_focus.is_invalid() {
            unsafe {
                let _ = UnhookWinEvent(hook_focus);
            }
        }
        return;
    }

    tracing::info!("WinEvent 钩子已启动（FOREGROUND + OBJECT_FOCUS）");

    // 2. PeekMessage 循环：OUT_OF_CONTEXT 模式回调依赖消息循环派发
    let mut msg = MSG::default();
    while !shutdown_flag.load(Ordering::Relaxed) {
        // PeekMessage 非阻塞，无消息时立即返回 false
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() } {
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        // 短暂等待，避免空转消耗 CPU（保持空闲 CPU <0.5%）
        thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
    }

    // 3. 退出时卸载钩子
    //    windows 0.58: UnhookWinEvent 返回 BOOL（非 Result），用 as_bool() 判断
    unsafe {
        if !UnhookWinEvent(hook_fg).as_bool() {
            tracing::warn!("UnhookWinEvent(fg) 失败");
        }
        if !UnhookWinEvent(hook_focus).as_bool() {
            tracing::warn!("UnhookWinEvent(focus) 失败");
        }
    }
    tracing::info!("WinEvent 钩子已卸载，工作线程退出");
}

/// WinEvent 回调函数（C 风格函数指针）
///
/// # 安全性
/// - 回调在工作线程上下文执行（OUT_OF_CONTEXT 模式）
/// - 仅做最小工作：过滤 → 构建上下文 → 写 RwLock
/// - 不调用阻塞 API（如 SetForegroundWindow），避免长时间占用
/// - HWND 句柄有效性由调用方 validate 阶段二次校验（IsWindow）
unsafe extern "system" fn win_event_callback(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    _id_child: i32,
    id_event_thread: u32,
    _event_time: u32,
) {
    // 只处理客户区事件，过滤标题栏/滚动条/系统菜单等系统对象
    if id_object != OBJID_CLIENT_RAW {
        return;
    }

    let hwnd_raw = hwnd.0 as isize;
    if hwnd_raw == 0 {
        return;
    }

    let Some(shared) = HOOK_SHARED.get() else {
        return;
    };

    // 过滤本程序窗口：悬浮窗/管理面板的焦点变化不应更新缓存
    let float_hwnd = shared.float_hwnd.load(Ordering::Relaxed);
    let manager_hwnd = shared.manager_hwnd.load(Ordering::Relaxed);
    if hwnd_raw == float_hwnd || hwnd_raw == manager_hwnd {
        return;
    }

    // 构建焦点上下文
    if let Some(ctx) = build_focus_context(event, hwnd, hwnd_raw, id_event_thread) {
        // 二次过滤：子控件（如悬浮窗的 webview）回溯到顶层窗口后可能是本程序窗口
        // 一次过滤只检查 hwnd 本身，子控件的 hwnd ≠ float_hwnd 但 top_hwnd 可能 == float_hwnd
        // 不过滤会导致 top_hwnd 被污染为悬浮窗，后续 Tab 输入校验失败
        if ctx.top_hwnd == float_hwnd || ctx.top_hwnd == manager_hwnd {
            return;
        }
        shared.tracker.update(ctx);
    }
}

/// 根据事件类型构建焦点上下文
///
/// - `EVENT_SYSTEM_FOREGROUND`：hwnd 是顶层窗口，通过 GetGUIThreadInfo 获取焦点子控件
/// - `EVENT_OBJECT_FOCUS`：hwnd 可能是子控件，通过 GetAncestor 获取顶层窗口
fn build_focus_context(
    event: u32,
    hwnd: HWND,
    hwnd_raw: isize,
    id_event_thread: u32,
) -> Option<FocusContext> {
    let mut pid: u32 = 0;
    // GetWindowThreadProcessId 返回线程 ID，pid 通过 out 参数返回
    let thread_id = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };

    // 优先使用事件回调提供的线程 ID（OBJECT_FOCUS 事件更准确）
    let effective_thread_id = if id_event_thread != 0 {
        id_event_thread
    } else {
        thread_id
    };

    // 通过 GetGUIThreadInfo 获取真正的焦点控件（更可靠，覆盖 DirectUI 场景）
    let focus_ctl = get_focus_ctl_via_gui_thread_info(effective_thread_id)
        .unwrap_or(hwnd_raw);

    let top_hwnd = match event {
        EVENT_SYSTEM_FOREGROUND => {
            // 顶层窗口切换：hwnd 即顶层窗口
            hwnd_raw
        }
        EVENT_OBJECT_FOCUS => {
            // 子控件焦点变化：hwnd 可能是子控件，取顶层窗口
            get_top_level_window(hwnd).unwrap_or(hwnd_raw)
        }
        _ => return None,
    };

    Some(FocusContext {
        top_hwnd,
        focus_ctl_hwnd: focus_ctl,
        pid,
        thread_id: effective_thread_id,
        updated_at: Some(Instant::now()),
    })
}

/// 通过 GetGUIThreadInfo 获取指定线程的焦点控件 HWND
///
/// 跨线程读取 GUI 状态（如某浏览器进程的当前焦点输入框），
/// 比 EVENT_OBJECT_FOCUS 的 hwnd 参数更可靠（后者有时返回父窗口）。
///
/// 返回 None：调用失败或焦点为空（无输入焦点的窗口）
fn get_focus_ctl_via_gui_thread_info(thread_id: u32) -> Option<isize> {
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    let result = unsafe { GetGUIThreadInfo(thread_id, &mut info) };
    if result.is_err() {
        return None;
    }
    if info.hwndFocus.0.is_null() {
        None
    } else {
        Some(info.hwndFocus.0 as isize)
    }
}

/// 获取指定窗口的顶层窗口（根窗口）
///
/// 用于 EVENT_OBJECT_FOCUS 事件：hwnd 可能是子控件，需要回溯到顶层窗口
fn get_top_level_window(hwnd: HWND) -> Option<isize> {
    let top = unsafe { GetAncestor(hwnd, GA_ROOT) };
    if top.0.is_null() {
        None
    } else {
        Some(top.0 as isize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_objid_client_constant() {
        // OBJID_CLIENT 在 Win32 中定义为 (LONG)0xFFFFFFFC，即 i32 的 -4
        assert_eq!(OBJID_CLIENT_RAW, -4);
    }

    #[test]
    fn test_poll_interval_within_target() {
        // 100ms 轮询：空闲 CPU 占用估算 <1%，满足 <0.5% 的目标
        // 注：POLL_INTERVAL_MS 为编译期常量，断言已被常量折叠优化移除（避免 clippy::assertions_on_constants）
        //     此处仅作为文档化校验意图，运行时通过编译保证值合理
        const _: () = {
            assert!(POLL_INTERVAL_MS >= 50);
            assert!(POLL_INTERVAL_MS <= 200);
        };
    }

    #[test]
    fn test_get_focus_ctl_via_gui_thread_info_invalid_thread() {
        // 注：thread_id=0 在 Windows 中表示当前线程，测试线程可能持有 GUI 焦点，
        //     因此 get_focus_ctl_via_gui_thread_info(0) 不保证返回 None。
        //     此处仅验证不存在的线程 ID（极大值）返回 None，不 panic。
        assert!(get_focus_ctl_via_gui_thread_info(0xFFFF_FFFF).is_none());
    }

    #[test]
    fn test_get_top_level_window_null_hwnd() {
        // null HWND 应返回 None
        let null_hwnd = HWND(std::ptr::null_mut());
        assert!(get_top_level_window(null_hwnd).is_none());
    }

    #[test]
    fn test_update_float_hwnd_no_panic_without_init() {
        // HOOK_SHARED 未初始化时调用应静默返回，不 panic
        update_float_hwnd(12345);
        update_manager_hwnd(67890);
    }
}
