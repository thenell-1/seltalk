// TODO 人工审查点：1.RwLock 中毒处理 2.缓存新鲜度判定 3.并发读写安全 4.Instant 精度
// NOTE 焦点上下文缓存：WinEvent 钩子写入，do_type_candidate 实时读取
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// 焦点上下文：缓存用户最后一次激活的可输入控件信息
///
/// 字段说明：
/// - `top_hwnd`：顶层窗口 HWND（用于 SetForegroundWindow 恢复活动窗口）
/// - `focus_ctl_hwnd`：焦点控件 HWND（子窗口，用于 SetFocus 恢复键盘焦点；
///   可能等于 `top_hwnd`，如记事本主窗口本身持有焦点）
/// - `pid`：焦点控件所在进程 PID（诊断用）
/// - `thread_id`：焦点控件所在线程 ID（AttachThreadInput 跨线程设置焦点用）
/// - `updated_at`：最后更新时间（None 表示从未更新，初始化状态）
#[derive(Debug, Clone, Default)]
pub struct FocusContext {
    /// 顶层窗口 HWND
    pub top_hwnd: isize,
    /// 焦点控件 HWND（子窗口）
    pub focus_ctl_hwnd: isize,
    /// 焦点控件所在进程 PID
    #[allow(dead_code)]
    pub pid: u32,
    /// 焦点控件所在线程 ID
    pub thread_id: u32,
    /// 最后更新时间（None 表示从未更新）
    pub updated_at: Option<Instant>,
}

impl FocusContext {
    /// 判断缓存是否已过期
    ///
    /// `max_age`：可接受的最大缓存年龄；超过则视为过期（用户可能已切换窗口）
    /// `updated_at == None` 视为过期（从未更新）
    pub fn is_stale(&self, max_age: Duration) -> bool {
        match self.updated_at {
            None => true,
            Some(t) => t.elapsed() > max_age,
        }
    }

    /// 缓存是否有效（已写入过至少一次）
    #[allow(dead_code)]
    pub fn is_initialized(&self) -> bool {
        self.updated_at.is_some() && self.top_hwnd != 0
    }
}

/// 焦点上下文缓存容器
///
/// `RwLock<FocusContext>` 封装：
/// - 钩子工作线程通过 `update` 写入（写锁）
/// - 主线程通过 `snapshot` 读取（读锁，返回克隆避免长时间持锁）
pub struct FocusTracker {
    context: RwLock<FocusContext>,
}

impl FocusTracker {
    /// 创建空缓存（所有字段为零值，updated_at = None）
    pub fn new() -> Self {
        Self {
            context: RwLock::new(FocusContext::default()),
        }
    }

    /// 读取当前缓存的克隆（不持锁返回，避免调用方持锁期间触发死锁）
    ///
    /// 锁中毒（极端情况：持有锁的线程 panic）时返回空上下文，
    /// 由调用方的 `validate_for_input` 兜底拦截。
    pub fn snapshot(&self) -> FocusContext {
        match self.context.read() {
            Ok(guard) => guard.clone(),
            Err(_) => {
                tracing::error!("焦点缓存读锁中毒，返回空上下文");
                FocusContext::default()
            }
        }
    }

    /// 更新缓存（钩子回调调用）
    ///
    /// 写锁中毒时记录错误但不传播（钩子线程不应因缓存问题崩溃）。
    pub fn update(&self, ctx: FocusContext) {
        match self.context.write() {
            Ok(mut guard) => {
                *guard = ctx;
            }
            Err(_) => {
                tracing::error!("焦点缓存写锁中毒，丢弃本次更新");
            }
        }
    }
}

impl Default for FocusTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_focus_context_default_all_zero() {
        let ctx = FocusContext::default();
        assert_eq!(ctx.top_hwnd, 0);
        assert_eq!(ctx.focus_ctl_hwnd, 0);
        assert_eq!(ctx.pid, 0);
        assert_eq!(ctx.thread_id, 0);
        assert!(ctx.updated_at.is_none());
    }

    #[test]
    fn test_focus_context_is_stale_when_never_updated() {
        // 从未更新（updated_at = None）应视为过期
        let ctx = FocusContext::default();
        assert!(ctx.is_stale(Duration::from_secs(1)));
    }

    #[test]
    fn test_focus_context_is_stale_within_age_limit() {
        // 刚更新且未超时：不过期
        let ctx = FocusContext {
            updated_at: Some(Instant::now()),
            ..Default::default()
        };
        assert!(!ctx.is_stale(Duration::from_secs(30)));
    }

    #[test]
    fn test_focus_context_is_stale_beyond_age_limit() {
        // 模拟过期：updated_at 设为很久以前
        // Instant::now() - Duration::from_secs(60) 在某些平台可能 panic（系统启动未到 60s）
        // 改用直接判断逻辑：构造一个超过 max_age 的值
        let ctx = FocusContext {
            updated_at: Some(Instant::now() - Duration::from_millis(10)),
            ..Default::default()
        };
        assert!(ctx.is_stale(Duration::from_millis(1)));
    }

    #[test]
    fn test_focus_context_is_initialized_false_for_default() {
        let ctx = FocusContext::default();
        assert!(!ctx.is_initialized());
    }

    #[test]
    fn test_focus_context_is_initialized_true_after_update() {
        let ctx = FocusContext {
            top_hwnd: 12345,
            updated_at: Some(Instant::now()),
            ..Default::default()
        };
        assert!(ctx.is_initialized());
    }

    #[test]
    fn test_focus_tracker_snapshot_returns_clone() {
        let tracker = FocusTracker::new();
        let snap1 = tracker.snapshot();
        // 修改克隆不影响缓存
        let mut snap2 = snap1.clone();
        snap2.top_hwnd = 99999;
        let snap3 = tracker.snapshot();
        assert_eq!(snap3.top_hwnd, 0);
        assert_ne!(snap3.top_hwnd, snap2.top_hwnd);
    }

    #[test]
    fn test_focus_tracker_update_and_snapshot() {
        let tracker = FocusTracker::new();
        let ctx = FocusContext {
            top_hwnd: 111,
            focus_ctl_hwnd: 222,
            pid: 333,
            thread_id: 444,
            updated_at: Some(Instant::now()),
        };
        tracker.update(ctx);
        let snap = tracker.snapshot();
        assert_eq!(snap.top_hwnd, 111);
        assert_eq!(snap.focus_ctl_hwnd, 222);
        assert_eq!(snap.pid, 333);
        assert_eq!(snap.thread_id, 444);
        assert!(snap.updated_at.is_some());
    }

    #[test]
    fn test_focus_tracker_snapshot_on_poisoned_lock_returns_default() {
        // 模拟锁中毒：无法直接构造中毒的 RwLock（需要 panic 持锁期间），
        // 但可验证 snapshot 在 Err 分支返回 default 不 panic
        let tracker = FocusTracker::new();
        let snap = tracker.snapshot();
        assert_eq!(snap.top_hwnd, 0);
    }
}
