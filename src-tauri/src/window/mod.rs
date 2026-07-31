// TODO 人工审查点：1.边界矫正算法 2.多显示器适配 3.状态持久化时序 4.窗口不存在兜底 5.WS_EX_NOACTIVATE 应用时机 6.临时置顶与持久置顶隔离
// NOTE 悬浮窗管理：显示/隐藏/置顶模式/透明度/尺寸位置记忆 + 屏幕边界矫正 + 鼠标跟随弹出
//       P-NOACTIVATE: 启动时一次性应用 WS_EX_NOACTIVATE，杜绝悬浮窗抢焦点（最高发定位偏移 BUG 根因）
//       双置顶模式：Normal=持久化到 window_state.always_on_top；Temp=运行时 temp_on_top，隐藏即失效
mod cursor;

use std::ffi::c_void;
use std::sync::atomic::Ordering;

use serde::Serialize;
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewWindow};
use ts_rs::TS;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE,
};

use crate::config::{
    DEFAULT_FLOAT_ALWAYS_ON_TOP, DEFAULT_FLOAT_FOLLOW_CURSOR, DEFAULT_FLOAT_H, DEFAULT_FLOAT_OPACITY,
    DEFAULT_FLOAT_W, KEY_FLOAT_FOLLOW_CURSOR, KEY_FLOAT_OPACITY, MAX_FLOAT_OPACITY, MIN_FLOAT_OPACITY,
};
use crate::db::{settings, window_state};
use crate::error::{AppError, AppResult};
use crate::hotkey;
use crate::state::AppState;

/// 悬浮窗 label（与 tauri.conf.json 一致）
pub const FLOAT_LABEL: &str = "float";

/// 鼠标偏移常量：避免光标压在悬浮窗上，悬浮窗弹在鼠标右下方 +offset 处
const CURSOR_OFFSET: i32 = 16;

/// 悬浮窗置顶模式
/// - Off: 不置顶
/// - Normal: 普通置顶（持久化到 window_state.always_on_top，不受窗口关闭影响）
/// - Temp: 临时置顶（仅悬浮窗可见期间有效，隐藏后自动失效）
///
/// P4.4：派生 TS，cargo test 时自动生成 .ts 到 ./bindings/commands/
#[derive(Debug, Clone, Copy, Serialize, TS, PartialEq, Eq)]
#[ts(export, export_to = "../bindings/commands/PinMode.ts")]
pub enum PinMode {
    Off,
    Normal,
    Temp,
}

/// 根据持久 always_on_top + 运行时 temp_on_top 判定当前置顶模式
///
/// 抽取为纯函数便于单元测试：Temp 优先级最高（运行时覆盖持久状态）
fn pin_mode(normal: bool, temp: bool) -> PinMode {
    if temp {
        PinMode::Temp
    } else if normal {
        PinMode::Normal
    } else {
        PinMode::Off
    }
}

/// 循环切换：Off → Normal → Temp → Off
fn next_pin_mode(current: PinMode) -> PinMode {
    match current {
        PinMode::Off => PinMode::Normal,
        PinMode::Normal => PinMode::Temp,
        PinMode::Temp => PinMode::Off,
    }
}

/// 读取透明度配置（缺失返回默认，并钳制到合法范围）
fn read_opacity_setting(app: &AppHandle) -> AppResult<f64> {
    let state = app.state::<AppState>();
    let db = state.db()?;
    Ok(settings::get_setting(&db, KEY_FLOAT_OPACITY)
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_FLOAT_OPACITY)
        .clamp(MIN_FLOAT_OPACITY, MAX_FLOAT_OPACITY))
}

/// 获取悬浮窗实例
pub fn get_float(app: &AppHandle) -> AppResult<WebviewWindow> {
    app.get_webview_window(FLOAT_LABEL)
        .ok_or_else(|| AppError::Window("悬浮窗不存在".into()))
}

/// 从 DB 加载窗口状态；缺失则返回默认
pub fn load_state(app: &AppHandle) -> AppResult<window_state::WindowState> {
    let state = app.state::<AppState>();
    let db = state.db()?;
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
    // P1.1：从连接池获取连接（替代原 state.db.lock()）
    let db = state.db()?;
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

/// 计算悬浮窗鼠标跟随位置所需的所有输入参数（逻辑像素）
///
/// 抽取为结构体避免函数签名参数过多（clippy::too_many_arguments），提升可读性
#[derive(Debug, Clone, Copy)]
struct CursorPositionInput {
    /// 鼠标逻辑坐标
    mouse_x: i32,
    mouse_y: i32,
    /// 显示器工作区（鼠标所在显示器，逻辑像素）
    mon_x: i32,
    mon_y: i32,
    mon_w: i32,
    mon_h: i32,
    /// 悬浮窗尺寸
    float_w: i32,
    float_h: i32,
    /// 鼠标偏移量（避免光标压在悬浮窗上）
    offset: i32,
}

/// 纯函数：根据鼠标位置、显示器工作区、悬浮窗尺寸计算最佳弹出位置
///
/// 算法（避让优先级从高到低）：
/// 1. 初始位置 = 鼠标 + offset（默认弹在右下方）
/// 2. 右边界超出 → 反向到鼠标左侧（mouse_x - offset - float_w）
/// 3. 下边界超出 → 反向到鼠标上方（mouse_y - offset - float_h）
/// 4. 仍超出左/上边界 → 贴边（取显示器起点）
///
/// 抽取为纯函数便于单元测试，不依赖 Windows API 或 Tauri 运行时
fn calc_cursor_position(input: CursorPositionInput) -> (i32, i32) {
    let CursorPositionInput {
        mouse_x: mx,
        mouse_y: my,
        mon_x,
        mon_y,
        mon_w,
        mon_h,
        float_w,
        float_h,
        offset,
    } = input;

    let mut x = mx + offset;
    let mut y = my + offset;

    // 右边界超出 → 弹到鼠标左侧
    if x + float_w > mon_x + mon_w {
        x = mx - offset - float_w;
    }
    // 下边界超出 → 弹到鼠标上方
    if y + float_h > mon_y + mon_h {
        y = my - offset - float_h;
    }
    // 仍超出左/上边界 → 贴边（兜底，悬浮窗比屏幕大或反向后仍越界）
    if x < mon_x {
        x = mon_x;
    }
    if y < mon_y {
        y = mon_y;
    }
    (x, y)
}

/// 读取 follow_cursor 配置（每次触发读一次 DB，频率低，不入主缓存）
///
/// 配置缺失时返回默认值（DEFAULT_FLOAT_FOLLOW_CURSOR=true），保证首次使用直觉化体验
fn read_follow_cursor_setting(app: &AppHandle) -> AppResult<bool> {
    let state = app.state::<AppState>();
    // P1.1：从连接池获取连接（替代原 state.db.lock()）
    let db = state.db()?;
    Ok(settings::get_setting(&db, KEY_FLOAT_FOLLOW_CURSOR)
        .ok()
        .flatten()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(DEFAULT_FLOAT_FOLLOW_CURSOR))
}

/// 将悬浮窗移动到鼠标附近（含智能避让与多显示器适配）
///
/// 流程：
/// 1. 取鼠标位置 + 所在显示器工作区（物理像素）
/// 2. 取窗口 scale_factor 把物理像素转换为逻辑像素（Tauri set_position 用逻辑像素）
/// 3. 调用纯函数 calc_cursor_position 计算最佳位置
/// 4. set_position 应用（仅位置，不重复 set_size/置顶，避免与 apply_state 冲突）
///
/// 多显示器 DPI 差异说明：窗口隐藏时 scale_factor 返回上次显示器的值，
/// 鼠标当前所在显示器可能 DPI 不同，会引入 16px 内的轻微偏差，可接受
pub fn move_float_to_cursor(app: &AppHandle) -> AppResult<()> {
    let win = get_float(app)?;

    // 1. 鼠标位置 + 所在显示器工作区（物理像素）
    let pt = cursor::get_cursor_position()?;
    let monitor = cursor::get_monitor_work_area(pt)?;

    // 2. 当前窗口尺寸（从缓存读，避免窗口未显示时拿不到尺寸）
    let s = load_state(app)?;

    // 3. 物理像素 → 逻辑像素（Tauri set_position 用逻辑像素）
    //    取窗口所在显示器 scale_factor（窗口隐藏时返回上次显示器的值）
    let scale = win.scale_factor().unwrap_or(1.0);
    let to_logical = |v: i32| -> i32 { (v as f64 / scale) as i32 };

    let mx = to_logical(pt.x);
    let my = to_logical(pt.y);
    let mon_x = to_logical(monitor.x);
    let mon_y = to_logical(monitor.y);
    let mon_w = to_logical(monitor.width);
    let mon_h = to_logical(monitor.height);

    // 4. 纯函数计算位置（避让算法）
    let input = CursorPositionInput {
        mouse_x: mx,
        mouse_y: my,
        mon_x,
        mon_y,
        mon_w,
        mon_h,
        float_w: s.w as i32,
        float_h: s.h as i32,
        offset: CURSOR_OFFSET,
    };
    let (x, y) = calc_cursor_position(input);

    // 5. 应用位置（仅 set_position，不重复 set_size/置顶）
    win.set_position(LogicalPosition::new(x as f64, y as f64))
        .map_err(|e| AppError::Window(format!("跟随鼠标设置位置失败: {e}")))?;

    // 用 info 级别记录关键路径（debug 级别在 tauri dev 默认未输出，改用 info 保证可观测）
    tracing::info!(
        "悬浮窗跟随鼠标: 鼠标({}, {}) → 弹出位置({}, {})",
        mx,
        my,
        x,
        y
    );
    Ok(())
}

/// 应用 WS_EX_NOACTIVATE 扩展样式到悬浮窗
///
/// 启动时一次性调用（在 `restore_on_startup` 内）：
/// - 读取当前 GWL_EXSTYLE
/// - 追加 WS_EX_NOACTIVATE 位
/// - 写回 GWL_EXSTYLE
///
/// 效果：悬浮窗点击/显示时不被激活为活动窗口，避免抢夺原输入框焦点。
/// 配合动态全局热键转发（show 时注册 Tab/方向键/R/Esc）保证键盘交互。
fn apply_noactivate_style(hwnd_raw: isize) -> AppResult<()> {
    if hwnd_raw == 0 {
        return Err(AppError::Window("HWND 为 0，无法应用 WS_EX_NOACTIVATE".into()));
    }
    let hwnd = HWND(hwnd_raw as *mut c_void);

    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if ex_style == 0 {
            // 返回 0 可能是样式为 0 也可能失败，记录但不阻断
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() != Some(0) {
                tracing::warn!("GetWindowLongPtrW 返回 0，可能失败: {err}");
            }
        }
        // 已设置则跳过（幂等）
        if (ex_style & (WS_EX_NOACTIVATE.0 as isize)) != 0 {
            tracing::debug!("WS_EX_NOACTIVATE 已设置，跳过重复应用");
            return Ok(());
        }
        let new_style = ex_style | (WS_EX_NOACTIVATE.0 as isize);
        if SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_style) == 0 {
            let err = std::io::Error::last_os_error();
            // 设置失败可能是权限问题（极罕见），记录但不阻断主流程
            tracing::warn!("SetWindowLongPtrW 设置 WS_EX_NOACTIVATE 失败: {err}");
        }
    }
    tracing::info!("已应用 WS_EX_NOACTIVATE 到悬浮窗 (hwnd={})", hwnd_raw);
    Ok(())
}

/// 显示悬浮窗并应用记忆状态 + 边界矫正（可选跟随鼠标弹出）
///
/// 流程：
/// 1. load_state → correct_state_boundary → apply_state（与原逻辑一致）
/// 2. 读取 follow_cursor 配置；为 true 时调用 move_float_to_cursor 在 show 之前定位
/// 3. show（不再调用 set_focus，WS_EX_NOACTIVATE 已杜绝抢焦点）
/// 4. 注册悬浮窗可见期间的快捷键（Tab/方向键/R/Esc/Ctrl+1/2/3）
/// 5. save_state（矫正可能调整了位置）
///
/// P-NOACTIVATE 改动：
/// - 移除 `win.set_focus()` 调用（WS_EX_NOACTIVATE 后不需要，且会触发活动窗口切换）
/// - show 后注册动态全局热键（替代 webview 直接接收键盘事件）
pub fn show_float(app: &AppHandle) -> AppResult<()> {
    let mut s = load_state(app)?;
    correct_state_boundary(app, &mut s)?;
    apply_state(app, &s)?;

    // 临时置顶覆盖：temp_on_top=true 时强制置顶（覆盖持久 always_on_top）
    // 场景：用户切到"临时置顶"模式后隐藏悬浮窗又呼出，temp 已在 hide 时清零，
    //       此分支仅在 temp 仍为 true（如 cycle 后未隐藏直接再 trigger）时生效
    let temp = app
        .state::<AppState>()
        .temp_on_top
        .load(Ordering::Relaxed);
    if temp {
        let win = get_float(app)?;
        win.set_always_on_top(true)
            .map_err(|e| AppError::Window(format!("临时置顶设置失败: {e}")))?;
    }

    // 跟随鼠标弹出（失败不阻塞显示，沿用上次位置）
    let follow_cursor = read_follow_cursor_setting(app).unwrap_or(DEFAULT_FLOAT_FOLLOW_CURSOR);
    if follow_cursor {
        if let Err(e) = move_float_to_cursor(app) {
            tracing::warn!("悬浮窗跟随鼠标失败，沿用上次位置: {e}");
        }
    }

    let win = get_float(app)?;
    win.show()
        .map_err(|e| AppError::Window(format!("显示悬浮窗失败: {e}")))?;

    // Windows 上对隐藏窗口设置 HWND_TOPMOST 可能不生效，需在 show 后重新设置置顶
    // 否则悬浮窗显示后 TOPMOST 状态可能丢失，被前台窗口遮挡（用户看不见悬浮窗的常见根因）
    let top_state = if temp { true } else { s.always_on_top };
    if let Err(e) = win.set_always_on_top(top_state) {
        tracing::warn!("显示后重新设置置顶失败: {e}");
    }

    // 注：不再调用 win.set_focus() —— WS_EX_NOACTIVATE 已让悬浮窗不抢活动窗口，
    // 键盘交互通过动态注册的全局热键转发到前端（hotkey::register_float_shortcuts）
    // 矫正可能调整了位置，持久化（注意：跟随鼠标的位置不持久化，下次仍跟随）
    let _ = save_state(app, &s);

    // 注册悬浮窗可见期间的快捷键（失败仅记录日志，键盘交互降级为鼠标点击）
    if let Err(e) = hotkey::register_float_shortcuts(app) {
        tracing::warn!("悬浮窗快捷键注册失败（键盘交互可能受限）: {e}");
    }

    // 诊断日志：记录窗口实际可见性与置顶状态，便于排查"悬浮窗不显示"问题
    tracing::info!(
        "悬浮窗已显示: visible={}, always_on_top={}",
        win.is_visible().unwrap_or(false),
        win.is_always_on_top().unwrap_or(false),
    );
    Ok(())
}

/// 隐藏悬浮窗，并持久化当前位置/尺寸/置顶
///
/// 双置顶模式关键改动：
/// - always_on_top 用底层持久值（load_state），而非 win.is_always_on_top()
///   避免临时置顶（temp_on_top）的运行时 true 泄漏到 DB 持久值
/// - temp_on_top 在隐藏时自动清零（"临时置顶"仅悬浮窗可见期间有效）
///
/// P-NOACTIVATE 改动：hide 后注销悬浮窗快捷键，恢复原输入框的 Tab/方向键等正常输入
pub fn hide_float(app: &AppHandle) -> AppResult<()> {
    let win = get_float(app)?;
    let app_state = app.state::<AppState>();

    // 读取底层持久状态（always_on_top 不受 temp 影响）
    let mut underlying = load_state(app)?;
    // 持久化当前实际位置/尺寸（always_on_top 保留底层值）
    if let (Ok(pos), Ok(size)) = (win.outer_position(), win.outer_size()) {
        let scale = win.scale_factor().unwrap_or(1.0);
        let p = pos.to_logical::<i32>(scale);
        let s = size.to_logical::<u32>(scale);
        underlying.x = p.x;
        underlying.y = p.y;
        underlying.w = s.width;
        underlying.h = s.height;
    }
    // 临时置顶：隐藏即失效（swap 保证读取并清零的原子性）
    let was_temp = app_state.temp_on_top.swap(false, Ordering::Relaxed);
    if was_temp {
        tracing::info!("临时置顶已随悬浮窗隐藏而失效");
    }
    let _ = save_state(app, &underlying);

    win.hide()
        .map_err(|e| AppError::Window(format!("隐藏悬浮窗失败: {e}")))?;

    // 注销悬浮窗快捷键（恢复原输入框的正常键盘输入）
    if let Err(e) = hotkey::unregister_float_shortcuts(app) {
        tracing::warn!("悬浮窗快捷键注销失败: {e}");
    }

    tracing::info!("悬浮窗已隐藏");
    Ok(())
}

/// 循环切换置顶模式（Off → Normal → Temp → Off），返回切换后的模式
///
/// 状态映射：
/// - Off: always_on_top=false(持久), temp_on_top=false(运行时)
/// - Normal: always_on_top=true(持久), temp_on_top=false —— 长期有效，重启仍置顶
/// - Temp: always_on_top=false(持久), temp_on_top=true —— 隐藏即失效，恢复到 Off
pub fn cycle_pin_mode(app: &AppHandle) -> AppResult<PinMode> {
    let app_state = app.state::<AppState>();
    let mut s = load_state(app)?;
    let temp = app_state.temp_on_top.load(Ordering::Relaxed);
    let current = pin_mode(s.always_on_top, temp);
    let next = next_pin_mode(current);

    match next {
        PinMode::Off => {
            s.always_on_top = false;
            app_state.temp_on_top.store(false, Ordering::Relaxed);
        }
        PinMode::Normal => {
            s.always_on_top = true;
            app_state.temp_on_top.store(false, Ordering::Relaxed);
        }
        PinMode::Temp => {
            // 临时置顶：底层 always_on_top 设 false（隐藏后恢复到 Off），运行时 temp=true
            s.always_on_top = false;
            app_state.temp_on_top.store(true, Ordering::Relaxed);
        }
    }
    save_state(app, &s)?;

    // 应用到窗口：effective = always_on_top || temp_on_top
    let win = get_float(app)?;
    let effective = s.always_on_top || app_state.temp_on_top.load(Ordering::Relaxed);
    win.set_always_on_top(effective)
        .map_err(|e| AppError::Window(format!("切换置顶失败: {e}")))?;

    tracing::info!("置顶模式切换: {:?} → {:?}", current, next);
    Ok(next)
}

/// 读取当前置顶模式（供前端初始化图标）
pub fn get_pin_mode(app: &AppHandle) -> AppResult<PinMode> {
    let app_state = app.state::<AppState>();
    let s = load_state(app)?;
    let temp = app_state.temp_on_top.load(Ordering::Relaxed);
    Ok(pin_mode(s.always_on_top, temp))
}

/// 设置悬浮窗透明度（钳制到合法范围 + 持久化）
///
/// 透明度仅前端 CSS 使用，不进入运行时缓存（无需 invalidate_cache）
pub fn set_opacity(app: &AppHandle, opacity: f64) -> AppResult<()> {
    let clamped = opacity.clamp(MIN_FLOAT_OPACITY, MAX_FLOAT_OPACITY);
    let state = app.state::<AppState>();
    let db = state.db()?;
    settings::set_setting(&db, KEY_FLOAT_OPACITY, &clamped.to_string())?;
    tracing::debug!("透明度已保存: {clamped}");
    Ok(())
}

/// 读取悬浮窗透明度（缺失返回默认）
pub fn get_opacity(app: &AppHandle) -> AppResult<f64> {
    read_opacity_setting(app)
}

/// 启动时恢复悬浮窗状态（不显示，仅应用尺寸/位置/置顶配置）
///
/// P-NOACTIVATE 改动：启动时一次性应用 WS_EX_NOACTIVATE 扩展样式，
/// 杜绝悬浮窗后续 show 时抢夺焦点（最高发定位偏移 BUG 根因）。
/// 返回悬浮窗 HWND，供 FocusManager 更新过滤白名单。
pub fn restore_on_startup(app: &AppHandle) -> AppResult<isize> {
    let s = load_state(app)?;
    apply_state(app, &s)?;
    let _ = correct_boundary(app);

    // 应用 WS_EX_NOACTIVATE：启动时一次性，避免每次 show 重复设置
    let win = get_float(app)?;
    let hwnd_raw = win
        .hwnd()
        .map(|h| h.0 as isize)
        .map_err(|e| AppError::Window(format!("获取悬浮窗 HWND 失败: {e}")))?;
    if let Err(e) = apply_noactivate_style(hwnd_raw) {
        tracing::warn!("应用 WS_EX_NOACTIVATE 失败（不阻断启动）: {e}");
    }

    tracing::info!("悬浮窗状态已恢复 (hwnd={})", hwnd_raw);
    Ok(hwnd_raw)
}

#[cfg(test)]
mod tests {
    // 注意：窗口操作强依赖 Tauri 运行时，此处仅测试纯函数逻辑
    // 集成测试在端到端验证阶段覆盖

    use super::{calc_cursor_position, next_pin_mode, pin_mode, CursorPositionInput, PinMode};

    #[test]
    fn test_float_label_constant() {
        assert_eq!(super::FLOAT_LABEL, "float");
    }

    // 测试辅助：构造输入参数（默认 1920×1080 屏幕 + 420×360 悬浮窗 + 16 偏移）
    fn make_input(mx: i32, my: i32) -> CursorPositionInput {
        CursorPositionInput {
            mouse_x: mx,
            mouse_y: my,
            mon_x: 0,
            mon_y: 0,
            mon_w: 1920,
            mon_h: 1080,
            float_w: 420,
            float_h: 360,
            offset: 16,
        }
    }

    // ===== calc_cursor_position 正常流程 =====

    #[test]
    fn test_calc_center_position() {
        // 鼠标在中央 (960, 540)，期望弹在右下方（+16 偏移）
        let (x, y) = calc_cursor_position(make_input(960, 540));
        assert_eq!(x, 976);
        assert_eq!(y, 556);
    }

    // ===== calc_cursor_position 边界场景 =====

    #[test]
    fn test_calc_right_bottom_corner() {
        // 鼠标在右下角 (1900, 1060)：右边界 + 下边界都超出 → 反向弹到鼠标左上方
        let (x, y) = calc_cursor_position(make_input(1900, 1060));
        assert_eq!(x, 1900 - 16 - 420); // 1464
        assert_eq!(y, 1060 - 16 - 360); // 684
    }

    #[test]
    fn test_calc_right_edge_only() {
        // 仅右边界超出（鼠标在右侧中部）
        let (x, y) = calc_cursor_position(make_input(1900, 540));
        assert_eq!(x, 1900 - 16 - 420); // 1464，反向到左侧
        assert_eq!(y, 540 + 16); // 556，y 仍在下方
    }

    #[test]
    fn test_calc_top_left_corner() {
        // 鼠标在左上角 (0, 0)，正常偏移即可（不触发反向）
        let (x, y) = calc_cursor_position(make_input(0, 0));
        assert_eq!(x, 16);
        assert_eq!(y, 16);
    }

    #[test]
    fn test_calc_second_monitor() {
        // 多显示器：第二显示器在右侧，起点 (1920, 0)，尺寸 1920×1080
        // 鼠标在第二显示器右下角 (3820, 1060)
        let input = CursorPositionInput {
            mouse_x: 3820,
            mouse_y: 1060,
            mon_x: 1920,
            mon_y: 0,
            mon_w: 1920,
            mon_h: 1080,
            float_w: 420,
            float_h: 360,
            offset: 16,
        };
        let (x, y) = calc_cursor_position(input);
        assert_eq!(x, 3820 - 16 - 420); // 3384
        assert_eq!(y, 1060 - 16 - 360); // 684
    }

    #[test]
    fn test_calc_negative_monitor_origin() {
        // 第二显示器在左侧（负坐标），起点 (-1920, 0)，尺寸 1920×1080
        // 鼠标在第二显示器中央 (-960, 540)
        let input = CursorPositionInput {
            mouse_x: -960,
            mouse_y: 540,
            mon_x: -1920,
            mon_y: 0,
            mon_w: 1920,
            mon_h: 1080,
            float_w: 420,
            float_h: 360,
            offset: 16,
        };
        let (x, y) = calc_cursor_position(input);
        assert_eq!(x, -960 + 16); // -944
        assert_eq!(y, 540 + 16); // 556
    }

    // ===== calc_cursor_position 极端场景 =====

    #[test]
    fn test_calc_window_larger_than_screen() {
        // 悬浮窗比屏幕大（极端兜底）：应贴左上角
        let input = CursorPositionInput {
            mouse_x: 100,
            mouse_y: 100,
            mon_x: 0,
            mon_y: 0,
            mon_w: 800,
            mon_h: 600,
            float_w: 1000,
            float_h: 700,
            offset: 16,
        };
        // x=100+16=116 → 116+1000=1116 > 800 反向 → 100-16-1000=-916 < 0 贴边=0
        let (x, y) = calc_cursor_position(input);
        assert_eq!(x, 0);
        assert_eq!(y, 0);
    }

    #[test]
    fn test_calc_zero_offset() {
        // 偏移为 0（边界值）
        let mut input = make_input(100, 100);
        input.offset = 0;
        let (x, y) = calc_cursor_position(input);
        assert_eq!(x, 100);
        assert_eq!(y, 100);
    }

    #[test]
    fn test_calc_just_at_right_boundary() {
        // 鼠标刚好让窗口右边界贴边（不超出）：不应反向
        // 屏幕宽 1920，悬浮窗 420，offset 16
        // 鼠标 x = 1920 - 420 - 16 = 1484，此时 x+offset+float_w = 1500+420=1920 = 边界
        // 不应反向（条件是 > 而非 >=）
        let (x, y) = calc_cursor_position(make_input(1484, 540));
        assert_eq!(x, 1484 + 16); // 1500
        assert_eq!(y, 540 + 16);
    }

    #[test]
    fn test_calc_one_pixel_over_right_boundary() {
        // 鼠标 x = 1485 → 1501+420=1921 > 1920 触发反向
        let (x, _y) = calc_cursor_position(make_input(1485, 540));
        assert_eq!(x, 1485 - 16 - 420); // 1049
    }

    // ===== pin_mode / next_pin_mode 纯函数测试 =====

    #[test]
    fn test_pin_mode_off() {
        // 两个标志都为 false → Off
        assert_eq!(pin_mode(false, false), PinMode::Off);
    }

    #[test]
    fn test_pin_mode_normal() {
        // always_on_top=true, temp=false → Normal
        assert_eq!(pin_mode(true, false), PinMode::Normal);
    }

    #[test]
    fn test_pin_mode_temp_overrides_normal() {
        // temp=true 优先级最高（即使 always_on_top 也为 true，仍判定为 Temp）
        assert_eq!(pin_mode(true, true), PinMode::Temp);
        assert_eq!(pin_mode(false, true), PinMode::Temp);
    }

    #[test]
    fn test_next_pin_mode_cycle() {
        // 完整循环：Off → Normal → Temp → Off
        assert_eq!(next_pin_mode(PinMode::Off), PinMode::Normal);
        assert_eq!(next_pin_mode(PinMode::Normal), PinMode::Temp);
        assert_eq!(next_pin_mode(PinMode::Temp), PinMode::Off);
    }

    #[test]
    fn test_pin_cycle_returns_to_start() {
        // 三次循环后回到起点
        let start = PinMode::Off;
        let after_one = next_pin_mode(start);
        let after_two = next_pin_mode(after_one);
        let after_three = next_pin_mode(after_two);
        assert_eq!(after_one, PinMode::Normal);
        assert_eq!(after_two, PinMode::Temp);
        assert_eq!(after_three, PinMode::Off);
        assert_eq!(after_three, start);
    }
}

