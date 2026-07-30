// TODO 人工审查点：1.插件注册 2.状态注入 3.setup 流程 4.命令注册 5.热键注册 6.窗口事件拦截
// NOTE Tauri 应用入口：注册基础设施模块 + setup 初始化日志/DB/状态/托盘/热键/窗口
mod clipboard;
mod commands;
mod config;
mod db;
mod error;
mod hotkey;
mod input;
mod llm;
mod logger;
mod orchestrator;
mod state;
mod text;
mod tray;
mod window;

use std::sync::OnceLock;

use tauri::{Manager, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::{
    Builder as ShortcutBuilder, Shortcut, ShortcutEvent, ShortcutState,
};

/// 日志 WorkerGuard 全局保活（防止非阻塞 writer 缓冲丢失）
static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 全局热键插件：按下时触发主链路
    let shortcut_plugin = ShortcutBuilder::new()
        .with_handler(|app: &tauri::AppHandle, _shortcut: &Shortcut, event: ShortcutEvent| {
            if matches!(event.state, ShortcutState::Pressed) {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    orchestrator::trigger(app).await;
                });
            }
        })
        .build();

    tauri::Builder::default()
        .plugin(shortcut_plugin)
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(commands::make_handler())
        .setup(|app| {
            // 1. 日志
            let log_dir = app.path().app_log_dir()?;
            let guard = logger::init_logger(&log_dir)?;
            let _ = LOG_GUARD.set(guard);

            // 2. 数据库
            let config_dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&config_dir)?;
            let db_path = config_dir.join("seltalk.db");
            let conn = db::init_db(&db_path)?;
            db::seed_if_empty(&conn)?;

            // 3. 全局状态注入
            let app_state = state::AppState::new(conn)?;

            // 3a. 从 DB 加载配置 → 刷新缓存
            {
                let db_lock = app_state
                    .db
                    .lock()
                    .map_err(|e| crate::error::AppError::Config(format!("DB 锁中毒: {e}")))?;
                let cfg = orchestrator::load_config_from_db(&db_lock)?;
                if let Ok(mut cache) = app_state.config_cache.write() {
                    *cache = cfg;
                }
            }

            app.manage(app_state);

            // 4. 系统托盘
            tray::setup(app.handle())?;

            // 5. 恢复悬浮窗状态（不显示，仅应用尺寸/位置/置顶）
            window::restore_on_startup(app.handle())?;

            // 6. 从配置注册全局热键
            let hotkey_str = {
                let state = app.state::<state::AppState>();
                state
                    .config_cache
                    .read()
                    .map(|c| c.hotkey.clone())
                    .unwrap_or_else(|_| config::DEFAULT_HOTKEY.to_string())
            };
            if let Err(e) = hotkey::register(app.handle(), &hotkey_str) {
                tracing::warn!("热键注册失败（将在设置面板中重新配置）: {e}");
            }

            // 7. 悬浮窗关闭拦截：隐藏而非关闭（保持窗口常驻，用完即隐）
            if let Some(float_win) = app.get_webview_window(window::FLOAT_LABEL) {
                let app_handle = app.handle().clone();
                float_win.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = window::hide_float(&app_handle);
                    }
                });
            }

            tracing::info!("SelTalk 启动完成");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("SelTalk 启动失败");
}
