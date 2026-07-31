// TODO 人工审查点：1.热键格式转换 2.重复注册清理 3.插件 Builder 用法 4.热键变更后重注册 5.悬浮窗快捷键单键注册风险
// NOTE 全局热键：通过 tauri-plugin-global-shortcut 注册，触发后调用 orchestrator
//       悬浮窗可见期间动态注册 Tab/方向键/R/Esc/Ctrl+1/2/3，hide 时注销（WS_EX_NOACTIVATE 配套）
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::error::{AppError, AppResult};

/// 将 PRD 风格热键（"Ctrl+Shift+Space"）转为插件接受的 accelerator（"ctrl+shift+space"）
pub fn normalize_hotkey(raw: &str) -> String {
    raw.trim().to_lowercase().replace(' ', "")
}

/// 解析并校验热键字符串
pub fn parse_shortcut(raw: &str) -> AppResult<Shortcut> {
    let acc = normalize_hotkey(raw);
    acc.parse::<Shortcut>()
        .map_err(|e| AppError::Hotkey(format!("热键解析失败 '{raw}': {e}")))
}

/// 系统保留热键判断：禁止注册会与系统/应用快捷键冲突的组合
///
/// 规则：
/// - 禁止 `Ctrl+单字母`（如 Ctrl+C/V/X/Z/A/S 等，这些是通用编辑/系统快捷键）
/// - 禁止 `Alt+F4`（关闭窗口）和 `Alt+Tab`（切换窗口）
/// - 允许 `Ctrl+Shift+字母`、`Ctrl+Alt+字母`、`Alt+字母` 等多修饰键组合
pub fn is_reserved(raw: &str) -> bool {
    let normalized = normalize_hotkey(raw);
    if matches!(normalized.as_str(), "alt+f4" | "alt+tab") {
        return true;
    }
    // Ctrl+单字母（仅 Ctrl 一个修饰键 + 单个字母主键，如 "ctrl+c"）
    if let Some(rest) = normalized.strip_prefix("ctrl+") {
        if rest.len() == 1
            && rest
                .chars()
                .next()
                .map(|c| c.is_ascii_alphabetic())
                .unwrap_or(false)
        {
            return true;
        }
    }
    false
}

/// 完整校验热键：格式合法 + 非系统保留键
pub fn validate_shortcut(raw: &str) -> AppResult<()> {
    parse_shortcut(raw)?;
    if is_reserved(raw) {
        return Err(AppError::Hotkey(format!(
            "热键 '{raw}' 与系统快捷键冲突，请更换为含多个修饰键的组合（如 Ctrl+Shift+Space / Alt+X）"
        )));
    }
    Ok(())
}

/// 注销全部已注册热键
pub fn unregister_all(app: &AppHandle) -> AppResult<()> {
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| AppError::Hotkey(format!("注销热键失败: {e}")))?;
    tracing::info!("已注销全部热键");
    Ok(())
}

/// 注册指定热键（先注销全部，避免重复）
pub fn register(app: &AppHandle, raw: &str) -> AppResult<()> {
    unregister_all(app)?;
    let shortcut = parse_shortcut(raw)?;
    app.global_shortcut()
        .register(shortcut)
        .map_err(|e| AppError::Hotkey(format!("注册热键失败 '{raw}': {e}")))?;
    tracing::info!("热键已注册: {raw}");
    Ok(())
}

/// 悬浮窗可见期间需要动态注册的快捷键列表
///
/// 设计原则：
/// - 单键（Tab/Up/Down/R/Escape）：WS_EX_NOACTIVATE 后 webview 不接收键盘事件，
///   需通过全局热键转发到前端（仅悬浮窗可见期间注册，hide 时注销）
/// - Ctrl+1/2/3：与主热键（如 Ctrl+Shift+Space）不冲突
///
/// 单键注册风险：Win32 RegisterHotKey 可能拒绝纯单键（无修饰键），
/// 失败的快捷键记录到日志，前端通过鼠标点击候选项作为兜底确认方式。
pub const FLOAT_SHORTCUTS: &[&str] =
    &["Tab", "Up", "Down", "R", "Escape", "Ctrl+1", "Ctrl+2", "Ctrl+3"];

/// 注册悬浮窗可见期间的快捷键（不注销主热键）
///
/// 调用时机：show_float 之后（[window/mod.rs::show_float](file:///D:/vibecoding/择言（SelTalk）/src-tauri/src/window/mod.rs) 中调用）
///
/// 失败处理：逐个注册，失败的快捷键记录到日志但不影响其他。
/// 调用方（前端）应提供鼠标点击候选作为兜底交互。
pub fn register_float_shortcuts(app: &AppHandle) -> AppResult<()> {
    let mut failed: Vec<&str> = Vec::new();

    for &sc in FLOAT_SHORTCUTS {
        match parse_shortcut(sc) {
            Ok(shortcut) => {
                // 已注册的快捷键跳过（避免重复注册错误）
                // Shortcut 实现了 Copy，无需 clone
                if app.global_shortcut().is_registered(shortcut) {
                    continue;
                }
                if let Err(e) = app.global_shortcut().register(shortcut) {
                    tracing::warn!("悬浮窗快捷键注册失败 '{}': {e}", sc);
                    failed.push(sc);
                }
            }
            Err(e) => {
                tracing::warn!("悬浮窗快捷键解析失败 '{}': {e}", sc);
                failed.push(sc);
            }
        }
    }

    if !failed.is_empty() {
        tracing::warn!(
            "悬浮窗快捷键部分注册失败（{:?}），对应功能不可用，可用鼠标点击替代",
            failed
        );
    } else {
        tracing::debug!("悬浮窗快捷键已全部注册");
    }

    Ok(())
}

/// 注销悬浮窗快捷键（保留主热键）
///
/// 调用时机：hide_float 之后
///
/// 注销失败不致命（可能未注册成功），仅记录 debug 日志
pub fn unregister_float_shortcuts(app: &AppHandle) -> AppResult<()> {
    for &sc in FLOAT_SHORTCUTS {
        if let Ok(shortcut) = parse_shortcut(sc) {
            // 仅注销已注册的快捷键（避免无效注销产生日志噪音）
            // Shortcut 实现了 Copy，无需 clone
            if app.global_shortcut().is_registered(shortcut) {
                if let Err(e) = app.global_shortcut().unregister(shortcut) {
                    tracing::debug!("悬浮窗快捷键注销失败 '{}': {e}", sc);
                }
            }
        }
    }
    tracing::debug!("悬浮窗快捷键已注销");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_lowercases_and_strips_spaces() {
        assert_eq!(normalize_hotkey("Ctrl+Shift+Space"), "ctrl+shift+space");
    }

    #[test]
    fn test_normalize_trims_whitespace() {
        assert_eq!(normalize_hotkey("  Ctrl+C  "), "ctrl+c");
    }

    #[test]
    fn test_parse_shortcut_valid() {
        assert!(parse_shortcut("Ctrl+Shift+Space").is_ok());
    }

    #[test]
    fn test_is_reserved_ctrl_single_letter() {
        // Ctrl+单字母系列应被禁止（与复制/粘贴/剪切等系统快捷键冲突）
        assert!(is_reserved("Ctrl+C"));
        assert!(is_reserved("Ctrl+V"));
        assert!(is_reserved("Ctrl+X"));
        assert!(is_reserved("Ctrl+Z"));
        assert!(is_reserved("Ctrl+A"));
        assert!(is_reserved("Ctrl+S"));
    }

    #[test]
    fn test_is_reserved_alt_f4_and_tab() {
        assert!(is_reserved("Alt+F4"));
        assert!(is_reserved("Alt+Tab"));
    }

    #[test]
    fn test_not_reserved_multi_modifier() {
        // 多修饰键组合应被允许
        assert!(!is_reserved("Alt+X"));
        assert!(!is_reserved("Ctrl+Shift+Space"));
        assert!(!is_reserved("Ctrl+Shift+C"));
        assert!(!is_reserved("Ctrl+Alt+D"));
    }

    #[test]
    fn test_validate_rejects_reserved() {
        assert!(validate_shortcut("Ctrl+C").is_err());
        assert!(validate_shortcut("Alt+F4").is_err());
    }

    #[test]
    fn test_validate_accepts_normal() {
        assert!(validate_shortcut("Alt+X").is_ok());
        assert!(validate_shortcut("Ctrl+Shift+Space").is_ok());
    }

    // ===== 悬浮窗快捷键测试 =====

    #[test]
    fn test_float_shortcuts_not_empty() {
        assert!(!FLOAT_SHORTCUTS.is_empty());
    }

    #[test]
    fn test_float_shortcuts_parseable() {
        // 所有悬浮窗快捷键应能解析为合法 Shortcut（不依赖 Tauri 运行时）
        for &sc in FLOAT_SHORTCUTS {
            assert!(parse_shortcut(sc).is_ok(), "悬浮窗快捷键 '{}' 解析失败", sc);
        }
    }

    #[test]
    fn test_float_shortcuts_no_conflict_with_main_hotkey() {
        // 悬浮窗快捷键不应与主热键（Ctrl+Shift+Space）冲突
        let main = normalize_hotkey("Ctrl+Shift+Space");
        for &sc in FLOAT_SHORTCUTS {
            let normalized = normalize_hotkey(sc);
            assert_ne!(
                normalized, main,
                "悬浮窗快捷键 '{}' 与主热键冲突", sc
            );
        }
    }

    #[test]
    fn test_float_shortcuts_contains_required_keys() {
        // 必须包含核心交互键
        let normalized: Vec<String> = FLOAT_SHORTCUTS
            .iter()
            .map(|&s| normalize_hotkey(s))
            .collect();
        assert!(normalized.contains(&"tab".to_string()), "缺少 Tab 键");
        assert!(normalized.contains(&"up".to_string()), "缺少 Up 键");
        assert!(normalized.contains(&"down".to_string()), "缺少 Down 键");
        assert!(normalized.contains(&"r".to_string()), "缺少 R 键");
        assert!(normalized.contains(&"escape".to_string()), "缺少 Escape 键");
    }
}
