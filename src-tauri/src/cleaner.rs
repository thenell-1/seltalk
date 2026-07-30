// NOTE F3 文本清洗模块：正则去除无效符号、规范化空白字符
// 输入：原始捕获文本（可能含特殊符号、多余空白、控制字符）
// 输出：清洗后的纯文本

use regex::Regex;
use std::sync::OnceLock;

/// 文本清洗：移除无效字符、规范空白
pub fn clean_text(raw: &str) -> String {
    let trailing_ws = get_trailing_ws_regex();
    let multi_ws = get_multi_ws_regex();
    let control_chars = get_control_chars_regex();
    let multi_newline = get_multi_newline_regex();

    let mut text = raw.to_string();

    // 1. 移除控制字符（保留换行符）
    text = control_chars.replace_all(&text, "").to_string();

    // 2. 规范化空白：多个空格/制表符合并为一个
    text = multi_ws.replace_all(&text, " ").to_string();

    // 3. 去除每行首尾空白
    text = text
        .lines()
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join("\n");

    // 4. 去除首尾空白
    text = trailing_ws.replace_all(&text, "").to_string();

    // 5. 多个连续换行合并为最多两个
    text = multi_newline.replace_all(&text, "\n\n").to_string();

    text.trim().to_string()
}

/// 判断文本是否有效（非空且非纯空白）
pub fn is_valid_text(text: &str) -> bool {
    !text.trim().is_empty()
}

// 使用 OnceLock 缓存正则表达式，避免重复编译
fn get_trailing_ws_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[ \t]+|[ \t]+$").unwrap())
}

fn get_multi_ws_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[ \t]+").unwrap())
}

fn get_control_chars_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // 匹配除换行符外的控制字符（0x00-0x08, 0x0B, 0x0C, 0x0E-0x1F, 0x7F）
    RE.get_or_init(|| Regex::new(r"[\x00-\x08\x0B\x0C\x0E-\x1F\x7F]").unwrap())
}

fn get_multi_newline_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // 匹配3个或更多连续换行符，替换为2个
    RE.get_or_init(|| Regex::new(r"\n{3,}").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_text_normal() {
        let input = "  你好，   世界  ";
        let result = clean_text(input);
        assert_eq!(result, "你好， 世界");
    }

    #[test]
    fn test_clean_text_control_chars() {
        let input = "你好\x00\x07世界";
        let result = clean_text(input);
        assert_eq!(result, "你好世界");
    }

    #[test]
    fn test_clean_text_newlines() {
        let input = "第一行\n\n\n\n第二行";
        let result = clean_text(input);
        assert_eq!(result, "第一行\n\n第二行");
    }

    #[test]
    fn test_is_valid_text_empty() {
        assert!(!is_valid_text(""));
        assert!(!is_valid_text("   "));
        assert!(!is_valid_text("\n\n"));
    }

    #[test]
    fn test_is_valid_text_normal() {
        assert!(is_valid_text("你好"));
        assert!(is_valid_text("Hello World"));
    }
}
