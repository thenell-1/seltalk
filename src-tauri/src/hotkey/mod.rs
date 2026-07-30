// TODO 人工审查点：1.热键格式转换 2.重复注册清理 3.插件 Builder 用法 4.热键变更后重注册
// NOTE 全局热键：通过 tauri-plugin-global-shortcut 注册，触发后调用 orchestrator
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
}
