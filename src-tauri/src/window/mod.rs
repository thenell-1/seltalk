// TODO 人工审查点：1.边界矫正算法 2.多显示器适配 3.状态持久化时序 4.窗口不存在兜底
// NOTE 悬浮窗管理：显示/隐藏/置顶/尺寸位置记忆 + 屏幕边界矫正
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewWindow};

use crate::config::{DEFAULT_FLOAT_ALWAYS_ON_TOP, DEFAULT_FLOAT_H, DEFAULT_FLOAT_W};
use crate::db::{settings, window_state};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// 悬浮窗 label（与 tauri.conf.json 一致）
pub const FLOAT_LABEL: &str = "float";

/// 获取悬浮窗实例
pub fn get_float(app: &AppHandle) -> AppResult<WebviewWindow> {
    app.get_webview_window(FLOAT_LABEL)
        .ok_or_else(|| AppError::Window("悬浮窗不存在".into()))
}

/// 从 DB 加载窗口状态；缺失则返回默认
pub fn load_state(app: &AppHandle) -> AppResult<window_state::WindowState> {
    let state = app.state::<AppState>();
    let db = state
        .db
        .lock()
        .map_err(|e| AppError::Config(format!("DB 锁中毒: {e}")))?;
    if let Some(s) = window_state::window_state_load(&db, FLOAT_LABEL)? {
        Ok(s)
    } else {
        // DB 无记录时用 settings 表中的默认尺寸/置顶
        let w = settings::get_setting(&db, crate::config::KEY_FLOAT_W)
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_FLOAT_W);
        let h = settings::get_setting(&db, crate::config::KEY_FLOAT_H)
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_FLOAT_H);
        let top = settings::get_setting(&db, crate::config::KEY_FLOAT_ALWAYS_ON_TOP)
            .ok()
            .flatten()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(DEFAULT_FLOAT_ALWAYS_ON_TOP);
        Ok(window_state::WindowState {
            x: 100,
            y: 100,
            w,
            h,
            always_on_top: top,
        })
    }
}

/// 持久化窗口状态到 DB
pub fn save_state(app: &AppHandle, s: &window_state::WindowState) -> AppResult<()> {
    let state = app.state::<AppState>();
    let db = state
        .db
        .lock()
        .map_err(|e| AppError::Config(format!("DB 锁中毒: {e}")))?;
    window_state::window_state_save(&db, FLOAT_LABEL, s)
}

/// 应用窗口状态到实际窗口（位置/尺寸/置顶）
pub fn apply_state(app: &AppHandle, s: &window_state::WindowState) -> AppResult<()> {
    let win = get_float(app)?;
    win.set_position(LogicalPosition::new(s.x as f64, s.y as f64))
        .map_err(|e| AppError::Window(format!("设置位置失败: {e}")))?;
    win.set_size(LogicalSize::new(s.w as f64, s.h as f64))
        .map_err(|e| AppError::Window(format!("设置尺寸失败: {e}")))?;
    win.set_always_on_top(s.always_on_top)
        .map_err(|e| AppError::Window(format!("设置置顶失败: {e}")))?;
    Ok(())
}

/// 对窗口状态做屏幕边界矫正（纯计算，仅修改 s，不写窗口/DB）
/// 抽取为独立函数，供 show_float / correct_boundary 复用，避免重复 load/apply
fn correct_state_boundary(app: &AppHandle, s: &mut window_state::WindowState) -> AppResult<()> {
    let win = get_float(app)?;

    // 优先取窗口当前所在显示器，取不到则取主显示器
    let monitor = win
        .current_monitor()
        .map_err(|e| AppError::Window(format!("获取当前显示器失败: {e}")))?
        .or_else(|| {
            win.primary_monitor()
                .map_err(|e| AppError::Window(format!("获取主显示器失败: {e}")))
                .ok()
                .flatten()
        });

    let Some(monitor) = monitor else {
        tracing::warn!("无法获取显示器信息，跳过边界矫正");
        return Ok(());
    };

    let pos = monitor.position().to_logical::<i32>(monitor.scale_factor());
    let size = monitor.size().to_logical::<u32>(monitor.scale_factor());

    let min_x = pos.x;
    let min_y = pos.y;
    let max_x = pos.x + size.width as i32 - s.w as i32;
    let max_y = pos.y + size.height as i32 - s.h as i32;

    // 横向矫正：窗口比屏幕宽或超出左/右边界时贴边
    if max_x < min_x || s.x < min_x {
        s.x = min_x;
    } else if s.x > max_x {
        s.x = max_x;
    }

    // 纵向矫正：窗口比屏幕高或超出上/下边界时贴边
    if max_y < min_y || s.y < min_y {
        s.y = min_y;
    } else if s.y > max_y {
        s.y = max_y;
    }
    Ok(())
}

/// 屏幕边界矫正：若窗口超出当前显示器可视区，拉回可视区域内（含 apply + save）
pub fn correct_boundary(app: &AppHandle) -> AppResult<window_state::WindowState> {
    let mut s = load_state(app)?;
    correct_state_boundary(app, &mut s)?;
    apply_state(app, &s)?;
    save_state(app, &s)?;
    tracing::debug!("边界矫正完成: x={}, y={}, w={}, h={}", s.x, s.y, s.w, s.h);
    Ok(s)
}

/// 显示悬浮窗并应用记忆状态 + 边界矫正
/// 性能优化：load/apply/save 各一次，边界矫正内联避免重复窗口操作
pub fn show_float(app: &AppHandle) -> AppResult<()> {
    let mut s = load_state(app)?;
    correct_state_boundary(app, &mut s)?;
    apply_state(app, &s)?;
    let win = get_float(app)?;
    win.show()
        .map_err(|e| AppError::Window(format!("显示悬浮窗失败: {e}")))?;
    win.set_focus()
        .map_err(|e| AppError::Window(format!("聚焦悬浮窗失败: {e}")))?;
    // 矫正可能调整了位置，持久化
    let _ = save_state(app, &s);
    tracing::info!("悬浮窗已显示");
    Ok(())
}

/// 隐藏悬浮窗，并持久化当前位置/尺寸/置顶
pub fn hide_float(app: &AppHandle) -> AppResult<()> {
    let win = get_float(app)?;
    // 持久化当前实际状态
    if let (Ok(pos), Ok(size), Ok(top)) =
        (win.outer_position(), win.outer_size(), win.is_always_on_top())
    {
        let scale = win.scale_factor().unwrap_or(1.0);
        let p = pos.to_logical::<i32>(scale);
        let s = size.to_logical::<u32>(scale);
        let state = window_state::WindowState {
            x: p.x,
            y: p.y,
            w: s.width,
            h: s.height,
            always_on_top: top,
        };
        let _ = save_state(app, &state);
    }
    win.hide()
        .map_err(|e| AppError::Window(format!("隐藏悬浮窗失败: {e}")))?;
    tracing::info!("悬浮窗已隐藏");
    Ok(())
}

/// 切换置顶状态；返回切换后的状态
pub fn toggle_always_on_top(app: &AppHandle) -> AppResult<bool> {
    let win = get_float(app)?;
    let current = win
        .is_always_on_top()
        .map_err(|e| AppError::Window(format!("读取置顶状态失败: {e}")))?;
    let next = !current;
    win.set_always_on_top(next)
        .map_err(|e| AppError::Window(format!("切换置顶失败: {e}")))?;
    // 持久化
    let mut s = load_state(app)?;
    s.always_on_top = next;
    save_state(app, &s)?;
    tracing::info!("置顶已切换为: {next}");
    Ok(next)
}

/// 启动时恢复悬浮窗状态（不显示，仅应用尺寸/位置/置顶配置）
pub fn restore_on_startup(app: &AppHandle) -> AppResult<()> {
    let s = load_state(app)?;
    apply_state(app, &s)?;
    let _ = correct_boundary(app);
    tracing::info!("悬浮窗状态已恢复");
    Ok(())
}

#[cfg(test)]
mod tests {
    // 注意：窗口操作强依赖 Tauri 运行时，此处仅测试纯函数逻辑
    // 集成测试在端到端验证阶段覆盖

    #[test]
    fn test_float_label_constant() {
        assert_eq!(super::FLOAT_LABEL, "float");
    }
}
