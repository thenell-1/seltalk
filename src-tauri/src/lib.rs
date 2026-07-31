// TODO 人工审查点：1.插件注册 2.状态注入 3.setup 流程 4.命令注册 5.热键注册 6.窗口事件拦截 7.FocusManager 启停 8.快捷键路由
// NOTE Tauri 应用入口：注册基础设施模块 + setup 初始化日志/DB/状态/托盘/热键/窗口
//       P-FOCUS-MGR: 启动时初始化 FocusManager，退出时通过 RunEvent::Exit 清理钩子
//       P-FLOAT-SHORTCUT: 主热键触发 trigger，悬浮窗快捷键转发到前端
mod clipboard;
mod commands;
mod config;
mod db;
mod error;
mod focus;
mod hotkey;
mod input;
mod llm;
mod logger;
mod orchestrator;
mod security;
mod state;
mod text;
mod tray;
mod window;

use std::sync::OnceLock;

use tauri::{Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::{
    Builder as ShortcutBuilder, Shortcut, ShortcutEvent, ShortcutState,
};

/// 日志 WorkerGuard 全局保活（防止非阻塞 writer 缓冲丢失）
// tracing-appender 0.2.5: 模块名为 non_blocking（带下划线），非 nonblocking
static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

/// 主热键 Shortcut 对象（启动时初始化，用于 handler 内判断当前 shortcut 是否为主热键）
/// 通过 OnceLock 共享给 handler 闭包，避免每次触发读 DB
/// 使用对象比较，避免字符串 Display 格式差异：
///   tauri-plugin-global-shortcut 2.x 的 Display 输出 Code 格式（如 "alt+KeyX"），
///   与 DB 存储的用户友好格式（如 "Alt+X"）字符串不一致，字符串比较会误判
static MAIN_SHORTCUT: OnceLock<Shortcut> = OnceLock::new();

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 全局热键插件：
    // - 主热键（如 Ctrl+Shift+Space）→ orchestrator::trigger
    // - 悬浮窗快捷键（Tab/Up/Down/R/Esc/Ctrl+1/2/3）→ emit "float-shortcut" 到前端
    let shortcut_plugin = ShortcutBuilder::new()
        .with_handler(|app: &tauri::AppHandle, shortcut: &Shortcut, event: ShortcutEvent| {
            if !matches!(event.state, ShortcutState::Pressed) {
                return;
            }

            // 判断是否为主热键（用 Shortcut 对象比较，避免字符串 Display 格式差异）
            // NOTE tauri-plugin-global-shortcut 2.x 的 Display 输出 Code 格式（如 "alt+KeyX"），
            //      与 DB 存储的用户友好格式（如 "Alt+X"）字符串不一致，字符串比较会误判，
            //      必须用 Shortcut 对象本身的 PartialEq 比较
            let is_main = MAIN_SHORTCUT
                .get()
                .map(|ms| shortcut == ms)
                .unwrap_or(false);

            // Display 字符串仅用于日志和转发到前端（不再用于主热键判断）
            let shortcut_str = format!("{}", shortcut);

            tracing::debug!(
                "热键事件: shortcut='{}', is_main={}",
                shortcut_str,
                is_main
            );

            if is_main {
                // 主热键：触发主链路
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    orchestrator::trigger(app).await;
                });
            } else {
                // 悬浮窗快捷键：转发到前端，由前端路由到 handleKeydown 等价逻辑
                // 仅在悬浮窗可见时处理（前端会自行判断 state 是否为 ready/loading 等）
                if let Err(e) = app.emit_to(window::FLOAT_LABEL, "float-shortcut", &shortcut_str) {
                    tracing::warn!("转发悬浮窗快捷键失败 '{}': {e}", shortcut_str);
                }
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

            // 2. 数据库（P1.1：返回连接池而非单 Connection）
            let config_dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&config_dir)?;
            let db_path = config_dir.join("seltalk.db");
            let pool = db::init_db(&db_path)?;
            // 取首个连接执行种子数据填充（仅首次启动写入默认 Prompt 模板）
            {
                let conn = pool
                    .get()
                    .map_err(|e| {
                        crate::error::AppError::Db(rusqlite::Error::ToSqlConversionFailure(
                            Box::new(e),
                        ))
                    })?;
                db::seed_if_empty(&conn)?;
                // 老用户 KV 单份 LLM 配置 → 迁移为一条 active llm_profiles 记录（幂等，新用户无 KV 时 no-op）
                db::llm_profiles::ensure_default_profile(&conn)?;
            }

            // 3. 全局状态注入（连接池）
            let app_state = state::AppState::new(pool)?;

            // 3a. 从 DB 加载配置 → 刷新缓存
            let hotkey_str = {
                let db = app_state.db()?;
                let cfg = orchestrator::load_config_from_db(&db)?;
                let hotkey = cfg.hotkey.clone();
                if let Ok(mut cache) = app_state.config_cache.write() {
                    *cache = cfg;
                }
                hotkey
            };

            app.manage(app_state);

            // 4. 系统托盘
            tray::setup(app.handle())?;

            // 5. 恢复悬浮窗状态（不显示，仅应用尺寸/位置/置顶 + WS_EX_NOACTIVATE）
            //    返回悬浮窗 HWND，供 FocusManager 过滤本程序窗口
            let float_hwnd = window::restore_on_startup(app.handle())?;

            // 5a. 获取管理面板 HWND（已可见）
            let manager_hwnd = app
                .get_webview_window("manager")
                .and_then(|w| w.hwnd().ok())
                .map(|h| h.0 as isize)
                .unwrap_or(0);

            // 6. 启动 FocusManager（WinEvent 钩子 + 焦点缓存）
            {
                let state = app.state::<state::AppState>();
                if let Err(e) = state.focus.start(float_hwnd, manager_hwnd) {
                    tracing::error!("FocusManager 启动失败: {e}");
                }
            }

            // 7. 缓存主热键 Shortcut 供 handler 路由（容错：解析失败仅记录，不阻断启动）
            //    用对象比较，避免字符串 Display 格式差异（"Alt+X" → Display 为 "alt+KeyX"）
            match hotkey::parse_shortcut(&hotkey_str) {
                Ok(sc) => {
                    let _ = MAIN_SHORTCUT.set(sc);
                }
                Err(e) => {
                    tracing::error!(
                        "主热键解析失败 '{}', handler 无法路由主热键: {e}",
                        hotkey_str
                    );
                }
            }

            // 8. 从配置注册全局热键
            if let Err(e) = hotkey::register(app.handle(), &hotkey_str) {
                tracing::warn!("热键注册失败（将在设置面板中重新配置）: {e}");
            }

            // 9. 悬浮窗关闭拦截：隐藏而非关闭（保持窗口常驻，用完即隐）
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
        .build(tauri::generate_context!())
        .expect("SelTalk 启动失败")
        .run(|app_handle: &tauri::AppHandle, event| {
            // 退出清理：停止 WinEvent 钩子，避免工作线程泄漏
            if let RunEvent::Exit = event {
                if let Some(state) = app_handle.try_state::<state::AppState>() {
                    if let Err(e) = state.focus.shutdown() {
                        tracing::error!("FocusManager 关闭失败: {e}");
                    }
                }
            }
        });
}
