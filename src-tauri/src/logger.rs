// TODO 人工审查点：1.WorkerGuard 生命周期 2.日志滚动策略 3.目录创建 4.旧日志清理
// NOTE tracing 日志初始化：按日滚动输出到 log/ 目录，启动时清理过期日志，WorkerGuard 必须在 run() 持有
use std::path::Path;
use std::time::{Duration, SystemTime};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::config::LOG_KEEP_DAYS;
use crate::error::AppResult;

/// 初始化日志，返回 WorkerGuard（必须保活，否则日志丢失）
pub fn init_logger(log_dir: &Path) -> AppResult<WorkerGuard> {
    std::fs::create_dir_all(log_dir)?;

    // 启动时清理过期日志（失败不阻断启动）
    if let Err(e) = cleanup_old_logs(log_dir, LOG_KEEP_DAYS) {
        // 清理失败仅记录到 stderr，此时日志系统尚未完全就绪
        eprintln!("清理旧日志失败（不阻断启动）: {e}");
    }

    let file_appender = tracing_appender::rolling::daily(log_dir, "seltalk.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(if cfg!(debug_assertions) { "debug" } else { "info" }));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_target(false).with_writer(std::io::stderr))
        .with(
            fmt::layer()
                .with_ansi(false)
                .with_writer(non_blocking),
        )
        .init();

    tracing::info!("日志系统已初始化，输出目录: {}", log_dir.display());
    tracing::info!("日志保留天数: {LOG_KEEP_DAYS} 天");
    Ok(guard)
}

/// 清理过期日志：删除修改时间早于 keep_days 天前的 `seltalk.log.*` 文件
///
/// NOTE tracing_appender 的 daily rolling 不自动删除旧文件，需手动清理。
///       按文件修改时间判断过期，不依赖文件名日期解析，更稳健。
pub fn cleanup_old_logs(log_dir: &Path, keep_days: u64) -> AppResult<()> {
    cleanup_old_logs_at(log_dir, keep_days, SystemTime::now())
}

/// 清理过期日志的内部实现（接受当前时间参数，便于测试注入）
fn cleanup_old_logs_at(log_dir: &Path, keep_days: u64, now: SystemTime) -> AppResult<()> {
    let threshold = now
        .checked_sub(Duration::from_secs(keep_days * 24 * 3600))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let mut deleted = 0u32;
    for entry in std::fs::read_dir(log_dir)? {
        let entry = entry?;
        let path = entry.path();
        // 仅清理 seltalk.log.* 滚动文件，保留当前正在写的 seltalk.log（无日期后缀）
        let name_matches = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with("seltalk.log.") && n.len() > "seltalk.log.".len())
            .unwrap_or(false);
        if !name_matches {
            continue;
        }
        // 按修改时间判断是否过期
        let mtime = match entry.metadata()?.modified() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if mtime < threshold {
            if let Err(e) = std::fs::remove_file(&path) {
                tracing::warn!("删除旧日志失败 {:?}: {e}", path);
            } else {
                deleted += 1;
            }
        }
    }
    if deleted > 0 {
        tracing::info!("已清理 {deleted} 个过期日志文件");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_cleanup_removes_old_logs() {
        // 用"未来时间"作为 now，使刚创建的文件相对算作"过期"
        let dir = std::env::temp_dir().join("st_logger_test_old");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let old_file = dir.join("seltalk.log.2020-01-01");
        fs::write(&old_file, "old").unwrap();

        // now 设为 365 天后，keep_days=7 → 刚创建的文件算作过期
        let future = SystemTime::now() + Duration::from_secs(365 * 24 * 3600);
        cleanup_old_logs_at(&dir, 7, future).unwrap();
        assert!(!old_file.exists(), "旧日志应被删除");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_cleanup_keeps_recent_logs() {
        let dir = std::env::temp_dir().join("st_logger_test_recent");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let recent_file = dir.join("seltalk.log.2026-07-30");
        fs::write(&recent_file, "recent").unwrap();

        // now 为当前时间，keep_days=7 → 刚创建的文件不算过期
        cleanup_old_logs_at(&dir, 7, SystemTime::now()).unwrap();
        assert!(recent_file.exists(), "新日志不应被删除");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_cleanup_skips_non_log_file() {
        let dir = std::env::temp_dir().join("st_logger_test_skip");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let other = dir.join("readme.txt");
        fs::write(&other, "keep me").unwrap();

        let future = SystemTime::now() + Duration::from_secs(365 * 24 * 3600);
        cleanup_old_logs_at(&dir, 7, future).unwrap();
        assert!(other.exists(), "非日志文件不应被删除");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_cleanup_empty_dir_is_safe() {
        let dir = std::env::temp_dir().join("st_logger_test_empty");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // 空目录不应报错
        cleanup_old_logs_at(&dir, 7, SystemTime::now()).unwrap();
        fs::remove_dir_all(&dir).ok();
    }
}
