// TODO 人工审查点：1.控制字符过滤 2.零宽字符清理 3.空白规整 4.换行保留
// NOTE 文本清洗：去控制字符/零宽字符，规整空白，保留换行；黑名单过滤后续阶段
/// 清洗剪贴板文本
///
/// 处理步骤：
/// 1. 去除首尾空白
/// 2. 移除零宽字符（U+200B U+200C U+200D U+FEFF）
/// 3. 移除控制字符（保留 \n \t \r）
/// 4. 多个连续空格合并为单个
/// 5. 行首/行尾空格清除
/// 6. 多个连续换行合并为双换行（段落分隔）
pub fn clean(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // 移除零宽字符 + 过滤控制字符（保留 \n \t \r）
    let filtered: String = trimmed
        .chars()
        .filter(|&c| {
            if is_zero_width(c) {
                return false;
            }
            if c.is_control() {
                return c == '\n' || c == '\t' || c == '\r';
            }
            true
        })
        .collect();

    // 规整空白：逐字符处理
    let mut result = String::with_capacity(filtered.len());
    let mut prev = '\0';
    for ch in filtered.chars() {
        match ch {
            ' ' => {
                // 跳过行首空格（前一个是换行或开头）
                if prev == '\n' || prev == '\0' {
                    continue;
                }
                // 跳过连续空格
                if prev == ' ' {
                    continue;
                }
                result.push(ch);
                prev = ch;
            }
            '\n' => {
                // 移除行尾空格
                while result.ends_with(' ') {
                    result.pop();
                }
                // 3+ 连续换行 → 双换行
                if result.ends_with("\n\n") {
                    continue;
                }
                result.push(ch);
                prev = ch;
            }
            _ => {
                result.push(ch);
                prev = ch;
            }
        }
    }

    // 移除尾部残余空格
    while result.ends_with(' ') {
        result.pop();
    }

    result
}

/// 判断是否为零宽字符
fn is_zero_width(c: char) -> bool {
    matches!(c, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_empty() {
        assert_eq!(clean(""), "");
        assert_eq!(clean("   "), "");
    }

    #[test]
    fn test_clean_trims() {
        assert_eq!(clean("  hello  "), "hello");
    }

    #[test]
    fn test_clean_zero_width() {
        assert_eq!(clean("hello\u{200B}world"), "helloworld");
        assert_eq!(clean("\u{FEFF}test"), "test");
    }

    #[test]
    fn test_clean_control_chars() {
        assert_eq!(clean("hello\u{0001}world"), "helloworld");
        assert_eq!(clean("hello\u{0007}world"), "helloworld");
    }

    #[test]
    fn test_clean_preserves_newlines() {
        assert_eq!(clean("line1\nline2"), "line1\nline2");
    }

    #[test]
    fn test_clean_preserves_tabs() {
        assert_eq!(clean("col1\tcol2"), "col1\tcol2");
    }

    #[test]
    fn test_clean_collapses_spaces() {
        assert_eq!(clean("hello     world"), "hello world");
    }

    #[test]
    fn test_clean_collapses_newlines() {
        assert_eq!(clean("para1\n\n\n\npara2"), "para1\n\npara2");
    }

    #[test]
    fn test_clean_mixed() {
        let input = "  hello\u{200B}   world  \n\n\n  foo  ";
        assert_eq!(clean(input), "hello world\n\nfoo");
    }

    #[test]
    fn test_clean_trailing_spaces_before_newline() {
        assert_eq!(clean("hello   \nworld"), "hello\nworld");
    }

    #[test]
    fn test_clean_leading_spaces_after_newline() {
        assert_eq!(clean("hello\n   world"), "hello\nworld");
    }
}
