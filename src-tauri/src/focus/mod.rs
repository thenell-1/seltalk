// TODO 人工审查点：1.钩子生命周期管理 2.焦点恢复重试 3.UIA fallback 触发时机 4.并发安全 5.shutdown 时序
// NOTE 焦点管理器模块：统一编排 WinEvent 钩子 + 焦点缓存 + 实时校验 + 恢复 + UIA 兜底
//       AppState 通过 Arc<FocusManager> 共享给主链路，do_type_candidate 调用 validate_and_restore
pub mod hook;
pub mod restore;
pub mod tracker;
pub mod uia;
pub mod validate;

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::error::{AppError, AppResult};
use crate::focus::tracker::{FocusContext, FocusTracker};
use crate::focus::validate::validate_for_input;

/// 焦点管理器：封装焦点追踪 + 校验 + 恢复的完整能力
///
/// 生命周期：
/// 1. `FocusManager::new()` 创建实例（不启动钩子）
/// 2. `start(float_hwnd, manager_hwnd)` 启动 WinEvent 钩子（专用工作线程）
/// 3. `snapshot()` / `validate_and_restore()` 运行时调用
/// 4. `shutdown()` 退出时调用，停止钩子并等待工作线程结束
pub struct FocusManager {
    /// 焦点上下文缓存（WinEvent 钩子写入，主线程读取）
    tracker: Arc<FocusTracker>,
    /// 本程序悬浮窗 HWND（过滤用）
    float_hwnd: AtomicIsize,
    /// 本程序管理面板 HWND（过滤用）
    manager_hwnd: AtomicIsize,
    /// WinEvent 工作线程句柄（shutdown 时 join）
    worker_thread: Mutex<Option<JoinHandle<()>>>,
    /// 工作线程退出信号
    shutdown_flag: Arc<AtomicBool>,
}

impl FocusManager {
    /// 创建焦点管理器（不启动钩子，需调用 start）
    pub fn new() -> AppResult<Self> {
        Ok(Self {
            tracker: Arc::new(FocusTracker::new()),
            float_hwnd: AtomicIsize::new(0),
            manager_hwnd: AtomicIsize::new(0),
            worker_thread: Mutex::new(None),
            shutdown_flag: Arc::new(AtomicBool::new(false)),
        })
    }

    /// 启动 WinEvent 钩子（启动后自动追踪焦点变化）
    ///
    /// # 参数
    /// - `float_hwnd`：本程序悬浮窗 HWND（用于过滤，避免缓存被自身窗口污染）
    /// - `manager_hwnd`：本程序管理面板 HWND
    ///
    /// # 错误
    /// - 重复调用 start：返回 Config 错误
    /// - 工作线程 spawn 失败：返回 Config 错误
    pub fn start(&self, float_hwnd: isize, manager_hwnd: isize) -> AppResult<()> {
        // 已启动则拒绝重复调用
        if let Ok(guard) = self.worker_thread.lock() {
            if guard.is_some() {
                return Err(AppError::Config(
                    "WinEvent 钩子已启动（重复调用 start）".into(),
                ));
            }
        }

        self.float_hwnd.store(float_hwnd, Ordering::Relaxed);
        self.manager_hwnd.store(manager_hwnd, Ordering::Relaxed);

        let handle = hook::start_hook(
            self.tracker.clone(),
            float_hwnd,
            manager_hwnd,
            self.shutdown_flag.clone(),
        )?;

        if let Ok(mut guard) = self.worker_thread.lock() {
            *guard = Some(handle);
        }

        tracing::info!(
            "FocusManager 已启动: float_hwnd={}, manager_hwnd={}",
            float_hwnd,
            manager_hwnd
        );
        Ok(())
    }

    /// 停止 WinEvent 钩子并等待工作线程退出
    ///
    /// 在 RunEvent::Exit 时调用，确保钩子正确卸载，避免内存泄漏。
    /// 工作线程最多在 POLL_INTERVAL_MS (100ms) 内响应退出信号。
    pub fn shutdown(&self) -> AppResult<()> {
        // 设置退出信号
        self.shutdown_flag.store(true, Ordering::Relaxed);

        // join 工作线程（等待其卸载钩子并退出）
        let handle = if let Ok(mut guard) = self.worker_thread.lock() {
            guard.take()
        } else {
            None
        };

        if let Some(h) = handle {
            // 等待最多 1 秒，避免卡死（POLL_INTERVAL_MS=100ms，正常 200ms 内退出）
            // 注：join 不接受 timeout，直接 join（工作线程应在 200ms 内退出）
            if let Err(e) = h.join() {
                tracing::error!("WinEvent 工作线程 join 失败: {:?}", e);
            }
        }

        tracing::info!("FocusManager 已关闭");
        Ok(())
    }

    /// 更新悬浮窗 HWND（窗口创建后调用，使钩子能过滤本程序窗口）
    ///
    /// 适用场景：窗口在 start 之后才创建（实际项目中常见）
    #[allow(dead_code)]
    pub fn update_float_hwnd(&self, hwnd: isize) {
        self.float_hwnd.store(hwnd, Ordering::Relaxed);
        hook::update_float_hwnd(hwnd);
    }

    /// 更新管理面板 HWND
    #[allow(dead_code)]
    pub fn update_manager_hwnd(&self, hwnd: isize) {
        self.manager_hwnd.store(hwnd, Ordering::Relaxed);
        hook::update_manager_hwnd(hwnd);
    }

    /// 读取当前焦点上下文的克隆（不阻塞钩子线程）
    ///
    /// 仅用于诊断或非关键路径；输入前置校验应使用 `validate_and_restore`。
    #[allow(dead_code)]
    pub fn snapshot(&self) -> FocusContext {
        self.tracker.snapshot()
    }

    /// 实时校验焦点并恢复到目标控件
    ///
    /// 在 do_type_candidate 入口调用，确保输入前焦点已正确恢复。
    ///
    /// # 流程
    /// 1. 实时读取焦点上下文（不允许使用 trigger 时的缓存）
    /// 2. validate_for_input 校验有效性
    /// 3. 校验失败 → 尝试 UIA 兜底（搜索可编辑控件）
    /// 4. UIA 兜底成功 → 用 UIA 结果构建新的 FocusContext
    /// 5. restore_foreground 恢复顶层窗口焦点
    /// 6. set_focus_to_ctl 恢复焦点控件键盘焦点
    ///
    /// # 返回
    /// - `Ok(FocusContext)`：校验通过 + 焦点已恢复，可安全输入
    /// - `Err(AppError::Input(_))`：校验失败（焦点丢失/窗口最小化/UIA 兜底无效）
    pub fn validate_and_restore(&self) -> AppResult<FocusContext> {
        // 1. 实时读取焦点上下文
        let mut ctx = self.tracker.snapshot();
        let float_hwnd = self.float_hwnd.load(Ordering::Relaxed);
        let manager_hwnd = self.manager_hwnd.load(Ordering::Relaxed);

        // 2. 校验有效性
        let validate_result = validate_for_input(&ctx, float_hwnd, manager_hwnd);

        if let Err(e) = validate_result {
            // 3. 校验失败 → UIA 兜底
            tracing::warn!("焦点校验失败，尝试 UIA 兜底: {}", e);

            if ctx.top_hwnd != 0 {
                match uia::find_focused_edit_via_uia(ctx.top_hwnd, ctx.focus_ctl_hwnd) {
                    Ok(Some(ctl_hwnd)) => {
                        // 4. UIA 兜底成功：更新 focus_ctl_hwnd
                        ctx.focus_ctl_hwnd = ctl_hwnd;
                        // 重新校验（top_hwnd 应仍有效）
                        if let Err(e2) = validate_for_input(&ctx, float_hwnd, manager_hwnd) {
                            tracing::warn!("UIA 兜底后校验仍失败: {}", e2);
                            return Err(e2);
                        }
                        tracing::info!("UIA 兜底成功，继续恢复焦点");
                    }
                    Ok(None) => {
                        tracing::warn!("UIA 兜底未找到可编辑控件");
                        return Err(e);
                    }
                    Err(uia_err) => {
                        tracing::warn!("UIA 兜底调用失败: {}", uia_err);
                        return Err(e);
                    }
                }
            } else {
                // top_hwnd 为 0，UIA 无法兜底
                return Err(e);
            }
        }

        // 5. 恢复顶层窗口前台焦点（带重试）
        if !restore::restore_foreground(ctx.top_hwnd, 3, 30) {
            tracing::warn!(
                "顶层窗口焦点恢复未完全验证，仍尝试设置焦点控件（type_text 有焦点校验兜底）"
            );
        }

        // 6. 恢复焦点控件键盘焦点（跨线程 AttachThreadInput）
        if !restore::set_focus_to_ctl(ctx.focus_ctl_hwnd, ctx.thread_id) {
            // set_focus_to_ctl 失败不致命：部分应用（如记事本）顶层窗口即焦点控件，
            // restore_foreground 已让其获得焦点，set_focus_to_ctl 失败可继续
            tracing::debug!(
                "set_focus_to_ctl 未成功（不致命，继续输入）: ctl_hwnd={}",
                ctx.focus_ctl_hwnd
            );
        }

        // 短暂等待焦点稳定
        std::thread::sleep(std::time::Duration::from_millis(30));

        Ok(ctx)
    }
}

impl Default for FocusManager {
    fn default() -> Self {
        Self::new().expect("FocusManager 初始化失败")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_focus_manager_new_does_not_start_hook() {
        // new() 不应启动工作线程
        let mgr = FocusManager::new().unwrap();
        let guard = mgr.worker_thread.lock().unwrap();
        assert!(guard.is_none());
        assert!(!mgr.shutdown_flag.load(Ordering::Relaxed));
    }

    #[test]
    fn test_focus_manager_snapshot_returns_default_when_not_started() {
        // 未启动时 snapshot 应返回默认（空）上下文
        let mgr = FocusManager::new().unwrap();
        let ctx = mgr.snapshot();
        assert_eq!(ctx.top_hwnd, 0);
        assert!(ctx.updated_at.is_none());
    }

    #[test]
    fn test_update_float_hwnd_no_panic() {
        // 未启动钩子时调用 update_float_hwnd 不应 panic
        let mgr = FocusManager::new().unwrap();
        mgr.update_float_hwnd(12345);
        assert_eq!(mgr.float_hwnd.load(Ordering::Relaxed), 12345);
    }

    #[test]
    fn test_update_manager_hwnd_no_panic() {
        let mgr = FocusManager::new().unwrap();
        mgr.update_manager_hwnd(67890);
        assert_eq!(mgr.manager_hwnd.load(Ordering::Relaxed), 67890);
    }

    #[test]
    fn test_validate_and_restore_uninitialized_returns_err() {
        // 未启动钩子时 validate_and_restore 应返回 Err（上下文未初始化）
        let mgr = FocusManager::new().unwrap();
        let result = mgr.validate_and_restore();
        assert!(result.is_err());
        if let Err(AppError::Input(msg)) = result {
            assert!(msg.contains("未初始化") || msg.contains("过期") || msg.contains("为 0"));
        }
    }

    #[test]
    fn test_shutdown_without_start_no_panic() {
        // 未启动时 shutdown 不应 panic
        let mgr = FocusManager::new().unwrap();
        let result = mgr.shutdown();
        assert!(result.is_ok());
    }
}
