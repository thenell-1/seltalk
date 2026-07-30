// NOTE 日志模块：基于 tracing + tracing-appender
// 日志文件位于 %APPDATA%\CreativeInputMethod\logs\app.log.YYYY-MM-DD
// 按天轮转，保留最近 7 天

use crate::error::{AppError, AppResult};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// 初始化日志系统
/// 返回 WorkerGuard，必须在主线程持有否则日志写入会停止
pub fn init(app: &AppHandle) -> AppResult<WorkerGuard> {
    let log_dir = log_dir(app)?;
    let file_appender = tracing_appender::rolling::daily(&log_dir, "app.log");
    let (non_blocking_file, guard) = tracing_appender::non_blocking(file_appender);

    // 日志级别：开发环境 DEBUG，发布环境 INFO
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(if cfg!(debug_assertions) { "debug" } else { "info" }));

    // 控制台输出（开发时可见）
    let console_layer = fmt::layer()
        .with_target(true)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_level(true);

    // 文件输出（持久化）
    let file_layer = fmt::layer()
        .with_writer(non_blocking_file)
        .with_target(true)
        .with_ansi(false) // 文件中不要 ANSI 颜色码
        .with_level(true);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer)
        .with(file_layer)
        .init();

    tracing::info!("日志系统已初始化，日志目录: {}", log_dir.display());
    Ok(guard)
}

/// 获取日志目录
fn log_dir(app: &AppHandle) -> AppResult<PathBuf> {
    let dir = app
        .path()
        .app_log_dir()
        .map_err(|e| AppError::Config(format!("获取日志目录失败: {e}")))?;
    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}
