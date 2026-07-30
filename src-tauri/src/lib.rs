// TODO 人工审查点：1.全局快捷键注册 2.悬浮窗 WS_EX_NOACTIVATE 设置 3.commands 注册完整性 4.系统托盘事件处理
// NOTE Tauri v2 应用核心：双窗口架构 + 全局快捷键F8 + commands 注册 + 事件系统 + 日志 + 数据库 + 系统托盘

mod capture;
mod cleaner;
mod commands;
mod config;
mod database;
mod error;
mod input;
mod llm;
mod logger;
mod orchestrator;

use database::Database;
use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tracing_appender::non_blocking::WorkerGuard;

/// 应用全局状态
pub struct AppState {
    pub db: Arc<Database>,
    pub _log_guard: WorkerGuard,
}

/// 应用入口
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            // 初始化日志系统
            let log_guard = logger::init(app.handle())?;

            // 初始化数据库
            let db = Database::init(app.handle())?;
            tracing::info!("应用启动完成");

            // 管理面板窗口已在 tauri.conf.json 配置，确保显示
            let panel_window = app
                .get_webview_window("panel")
                .expect("管理面板窗口必须存在");
            panel_window.show()?;
            panel_window.set_focus()?;

            // 初始化系统托盘
            setup_tray(app.handle())?;

            // 注册全局快捷键 F8（触发文本捕获+生成回复）
            register_global_shortcut(app.handle())?;

            // 注入全局状态
            app.manage(AppState {
                db: Arc::new(db),
                _log_guard: log_guard,
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_config,
            commands::get_llm_config,
            commands::save_llm_config,
            commands::test_llm,
            commands::get_system_status,
            commands::generate_reply,
            commands::adopt_reply,
            commands::show_overlay_window,
            commands::show_panel_window,
            commands::list_history,
        ])
        .run(tauri::generate_context!())
        .expect("运行 Tauri 应用时发生错误");
}

/// 注册全局快捷键 F8
fn register_global_shortcut(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    // F8 快捷键
    let shortcut: Shortcut = "F8".parse()?;
    app.global_shortcut().on_shortcut(shortcut, move |app, _shortcut, event| {
        // 仅在按键按下时触发（松开不触发）
        if event.state() == ShortcutState::Pressed {
            let app_clone = app.clone();
            tauri::async_runtime::spawn(async move {
                tracing::info!("全局快捷键 F8 触发");
                match orchestrator::trigger_capture(&app_clone).await {
                    Ok(id) => tracing::info!("F8 触发捕获成功: {id}"),
                    Err(e) => {
                        // 静默忽略"非微信/QQ窗口"和"未选中文本"错误
                        let msg = e.to_string();
                        if !msg.contains("非微信") && !msg.contains("未捕获") && !msg.contains("非目标") {
                            tracing::warn!("F8 触发捕获失败: {e}");
                        }
                    }
                }
            });
        }
    })?;
    tracing::info!("全局快捷键 F8 注册成功");
    Ok(())
}

/// 设置系统托盘（F7）
fn setup_tray(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let show_panel = MenuItem::with_id(app, "show_panel", "显示管理面板", true, None::<&str>)?;
    let trigger_capture = MenuItem::with_id(app, "trigger_capture", "触发捕获 (F8)", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&show_panel, &trigger_capture, &quit])?;

    let _tray = TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("创意输入法 - AI 智能回复助手")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            handle_tray_menu_event(app, event.id.as_ref());
        })
        .on_tray_icon_event(|tray, event| {
            handle_tray_icon_event(tray.app_handle(), &event);
        })
        .build(app)?;

    tracing::info!("系统托盘初始化完成");
    Ok(())
}

/// 处理托盘菜单点击事件
fn handle_tray_menu_event(app: &tauri::AppHandle, menu_id: &str) {
    match menu_id {
        "show_panel" => {
            if let Some(window) = app.get_webview_window("panel") {
                let _ = window.show();
                let _ = window.set_focus();
            }
            tracing::info!("托盘菜单：显示管理面板");
        }
        "trigger_capture" => {
            let app_clone = app.clone();
            tauri::async_runtime::spawn(async move {
                match orchestrator::trigger_capture(&app_clone).await {
                    Ok(id) => tracing::info!("托盘触发捕获成功: {id}"),
                    Err(e) => tracing::error!("托盘触发捕获失败: {e}"),
                }
            });
        }
        "quit" => {
            tracing::info!("托盘菜单：退出应用");
            app.exit(0);
        }
        _ => {}
    }
}

/// 处理托盘图标点击事件
fn handle_tray_icon_event(app: &tauri::AppHandle, event: &TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        if let Some(window) = app.get_webview_window("panel") {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}
