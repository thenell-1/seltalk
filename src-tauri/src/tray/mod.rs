// TODO 人工审查点：1.托盘图标加载 2.菜单事件路由 3.左键单击显示管理面板 4.退出清理 5.暂停/恢复热键状态同步
// NOTE 系统托盘：后台常驻，左键/菜单项显示管理面板，菜单项暂停-恢复热键/退出
use std::sync::atomic::Ordering;
use std::sync::OnceLock;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::config::{DEFAULT_HOTKEY, KEY_HOTKEY};
use crate::db::settings;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::hotkey;

/// 托盘菜单 ID
const MENU_SHOW: &str = "show_manager";
const MENU_PAUSE_RESUME: &str = "pause_resume_hotkey";
const MENU_QUIT: &str = "quit";

/// 全局保存"暂停/恢复"菜单项句柄，用于动态切换文案
static PAUSE_RESUME_ITEM: OnceLock<MenuItem<Wry>> = OnceLock::new();

/// 创建系统托盘
pub fn setup(app: &AppHandle) -> AppResult<()> {
    let show_item = MenuItem::with_id(app, MENU_SHOW, "显示管理面板", true, None::<&str>)
        .map_err(|e| AppError::Config(format!("创建菜单项失败: {e}")))?;
    let pause_item =
        MenuItem::with_id(app, MENU_PAUSE_RESUME, "暂停热键", true, None::<&str>)
            .map_err(|e| AppError::Config(format!("创建菜单项失败: {e}")))?;
    let quit_item = MenuItem::with_id(app, MENU_QUIT, "退出", true, None::<&str>)
        .map_err(|e| AppError::Config(format!("创建菜单项失败: {e}")))?;
    let menu = Menu::with_items(app, &[&show_item, &pause_item, &quit_item])
        .map_err(|e| AppError::Config(format!("创建菜单失败: {e}")))?;

    // 保存暂停/恢复菜单项句柄，供后续 set_text 切换文案
    let _ = PAUSE_RESUME_ITEM.set(pause_item);

    let icon = app
        .default_window_icon()
        .ok_or_else(|| AppError::Config("默认窗口图标不存在".into()))?
        .clone();

    TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .tooltip("择言 SelTalk")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(handle_menu_event)
        .on_tray_icon_event(handle_tray_event)
        .build(app)
        .map_err(|e| AppError::Config(format!("创建托盘失败: {e}")))?;

    tracing::info!("系统托盘已创建");
    Ok(())
}

/// 菜单事件处理
fn handle_menu_event(app: &AppHandle<Wry>, event: tauri::menu::MenuEvent) {
    match event.id.as_ref() {
        MENU_SHOW => {
            if let Err(e) = show_manager(app) {
                tracing::error!("显示管理面板失败: {e}");
            }
        }
        MENU_PAUSE_RESUME => {
            if let Err(e) = toggle_hotkey_pause(app) {
                tracing::error!("切换热键暂停状态失败: {e}");
            }
        }
        MENU_QUIT => {
            tracing::info!("用户通过托盘退出");
            app.exit(0);
        }
        _ => {}
    }
}

/// 托盘图标事件处理（左键单击显示管理面板）
fn handle_tray_event(_tray: &tauri::tray::TrayIcon<Wry>, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        let app = _tray.app_handle();
        if let Err(e) = show_manager(app) {
            tracing::error!("显示管理面板失败: {e}");
        }
    }
}

/// 切换热键暂停/恢复状态
///
/// - 暂停时：注销全局热键，菜单文案改为"恢复热键"，托盘提示更新
/// - 恢复时：从 DB 读取当前热键配置并重新注册，菜单文案改回"暂停热键"
fn toggle_hotkey_pause(app: &AppHandle<Wry>) -> AppResult<()> {
    let state = app.state::<AppState>();
    let was_paused = state.hotkey_paused.load(Ordering::Relaxed);
    let now_paused = !was_paused;
    state.hotkey_paused.store(now_paused, Ordering::Relaxed);

    if now_paused {
        // 暂停：注销热键
        hotkey::unregister_all(app)?;
        update_pause_menu_text("恢复热键");
        update_tray_tooltip(app, "择言 SelTalk（热键已暂停）")?;
        tracing::info!("热键已暂停");
    } else {
        // 恢复：从 DB 读取热键配置并重新注册
        let hotkey_str = {
            // P1.1：从连接池获取连接（替代原 state.db.lock()）
            let db = state.db()?;
            settings::get_setting(&db, KEY_HOTKEY)
                .ok()
                .flatten()
                .unwrap_or_else(|| DEFAULT_HOTKEY.to_string())
        };
        if let Err(e) = hotkey::register(app, &hotkey_str) {
            tracing::warn!("恢复热键注册失败: {e}");
            // 注册失败时回退为暂停状态
            state.hotkey_paused.store(true, Ordering::Relaxed);
            return Err(e);
        }
        update_pause_menu_text("暂停热键");
        update_tray_tooltip(app, "择言 SelTalk")?;
        tracing::info!("热键已恢复");
    }
    Ok(())
}

/// 更新暂停/恢复菜单项文案
fn update_pause_menu_text(text: &str) {
    if let Some(item) = PAUSE_RESUME_ITEM.get() {
        if let Err(e) = item.set_text(text) {
            tracing::warn!("更新菜单文案失败: {e}");
        }
    }
}

/// 更新托盘 tooltip
fn update_tray_tooltip(app: &AppHandle<Wry>, tooltip: &str) -> AppResult<()> {
    if let Some(tray) = app.tray_by_id("main-tray") {
        tray.set_tooltip(Some(tooltip))
            .map_err(|e| AppError::Config(format!("更新托盘提示失败: {e}")))?;
    }
    Ok(())
}

/// 查询当前热键是否已暂停（供命令层调用）
pub fn is_hotkey_paused(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    state.hotkey_paused.load(Ordering::Relaxed)
}

/// 显示管理面板窗口
fn show_manager(app: &AppHandle<Wry>) -> AppResult<()> {
    if let Some(win) = app.get_webview_window("manager") {
        win.show()
            .map_err(|e| AppError::Window(format!("显示管理面板失败: {e}")))?;
        win.set_focus()
            .map_err(|e| AppError::Window(format!("聚焦管理面板失败: {e}")))?;
    } else {
        tracing::warn!("管理面板窗口不存在");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_menu_ids_are_unique() {
        // 确保菜单 ID 不重复（编译期 + 运行期双重保障）
        assert_ne!(super::MENU_SHOW, super::MENU_PAUSE_RESUME);
        assert_ne!(super::MENU_SHOW, super::MENU_QUIT);
        assert_ne!(super::MENU_PAUSE_RESUME, super::MENU_QUIT);
    }
}
