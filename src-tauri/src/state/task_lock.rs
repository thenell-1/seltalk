// TODO 人工审查点：1.Mutex 中毒处理 2.显式释放配对 3.看门狗强制释放（在 orchestrator 实现）
// NOTE 任务锁：显式 acquire/release 模式，适用于需要跨 await 边界持有的主链路场景
use std::sync::Mutex;

use crate::error::{AppError, AppResult};

/// 显式获取锁（适用于需要跨 await 边界持有的场景，如 orchestrator 主链路）
/// 调用方需在结束时显式调用 `release` 归还
pub fn acquire(lock: &Mutex<bool>) -> AppResult<()> {
    let mut guard = lock
        .lock()
        .map_err(|e| AppError::Config(format!("任务锁中毒: {e}")))?;
    if *guard {
        return Err(AppError::Busy);
    }
    *guard = true;
    tracing::debug!("任务锁已获取（显式）");
    Ok(())
}

/// 显式释放锁（与 `acquire` 配对）
pub fn release(lock: &Mutex<bool>) {
    if let Ok(mut g) = lock.lock() {
        *g = false;
    }
    tracing::debug!("任务锁已释放（显式）");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acquire_second_fails() {
        let lock = Mutex::new(false);
        assert!(acquire(&lock).is_ok());
        // 第二次获取应返回 Busy
        let r2 = acquire(&lock);
        assert!(matches!(r2.err().unwrap(), AppError::Busy));
    }

    #[test]
    fn test_release_allows_reacquire() {
        let lock = Mutex::new(false);
        assert!(acquire(&lock).is_ok());
        release(&lock);
        // 释放后可重新获取
        assert!(acquire(&lock).is_ok());
    }

    #[test]
    fn test_release_when_already_free_is_safe() {
        // 对未占用的锁调用 release 不应 panic
        let lock = Mutex::new(false);
        release(&lock);
        assert!(acquire(&lock).is_ok());
    }
}
