// TODO 人工审查点：1.正则安全编译 2.替换策略 3.性能（大文本） 4.默认黑名单合理性
// NOTE 黑名单过滤：正则匹配命中后替换为 ***，防止隐私数据送入 LLM
use regex::Regex;

/// 默认黑名单正则：手机号、身份证、邮箱
/// NOTE 邮箱用 ASCII 字符类，避免 regex crate 默认 Unicode 模式下 \w 匹配中文导致替换范围过大
pub const DEFAULT_BLACKLIST: &[&str] = &[
    r"1[3-9]\d{9}",                          // 手机号
    r"\d{17}[\dXx]",                         // 身份证
    r"[A-Za-z0-9.+-]+@[A-Za-z0-9-]+\.[A-Za-z0-9.-]+", // 邮箱
];

/// 返回默认黑名单正则列表（未配置时供前端展示）
pub fn default_patterns() -> Vec<String> {
    DEFAULT_BLACKLIST.iter().map(|s| s.to_string()).collect()
}

/// 编译黑名单正则列表（无效正则跳过并记录警告）
pub fn compile_patterns(patterns: &[String]) -> Vec<Regex> {
    let mut compiled = Vec::new();
    for p in patterns {
        match Regex::new(p) {
            Ok(re) => compiled.push(re),
            Err(e) => tracing::warn!("黑名单正则编译失败「{}」: {}", p, e),
        }
    }
    compiled
}

/// 判断文本是否命中黑名单（保留为公共 API，供后续命中检测命令使用）
#[allow(dead_code)]
pub fn is_blacklisted(text: &str, patterns: &[Regex]) -> bool {
    patterns.iter().any(|re| re.is_match(text))
}

/// 应用黑名单过滤：将命中部分替换为 `***`
pub fn apply_blacklist(text: &str, patterns: &[Regex]) -> String {
    let mut result = text.to_string();
    for re in patterns {
        result = re.replace_all(&result, "***").to_string();
    }
    result
}

/// 从 settings 表的 JSON 字符串解析黑名单正则列表
pub fn parse_blacklist_json(json: &str) -> Vec<String> {
    if json.trim().is_empty() {
        return Vec::new();
    }
    match serde_json::from_str::<Vec<String>>(json) {
        Ok(list) => list,
        Err(e) => {
            tracing::warn!("黑名单 JSON 解析失败: {}", e);
            Vec::new()
        }
    }
}

/// 将黑名单正则列表序列化为 JSON 字符串
pub fn serialize_blacklist(patterns: &[String]) -> String {
    serde_json::to_string(patterns).unwrap_or_else(|_| "[]".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phone_detected() {
        let patterns = compile_patterns(&[r"1[3-9]\d{9}".to_string()]);
        assert!(is_blacklisted("我的手机是13812345678", &patterns));
    }

    #[test]
    fn test_id_card_detected() {
        let patterns = compile_patterns(&[r"\d{17}[\dXx]".to_string()]);
        assert!(is_blacklisted("身份证号110101199001011234", &patterns));
    }

    #[test]
    fn test_email_detected() {
        let patterns = compile_patterns(&[r"[A-Za-z0-9.+-]+@[A-Za-z0-9-]+\.[A-Za-z0-9.-]+".to_string()]);
        assert!(is_blacklisted("联系我test@example.com", &patterns));
    }

    #[test]
    fn test_apply_blacklist_replaces() {
        let patterns = compile_patterns(&[r"1[3-9]\d{9}".to_string()]);
        let result = apply_blacklist("手机13812345678联系", &patterns);
        assert_eq!(result, "手机***联系");
    }

    #[test]
    fn test_apply_blacklist_multiple_patterns() {
        let patterns = compile_patterns(&[
            r"1[3-9]\d{9}".to_string(),
            r"[A-Za-z0-9.+-]+@[A-Za-z0-9-]+\.[A-Za-z0-9.-]+".to_string(),
        ]);
        let result = apply_blacklist("手机13812345678邮箱test@x.com", &patterns);
        assert_eq!(result, "手机***邮箱***");
    }

    #[test]
    fn test_apply_blacklist_no_match() {
        let patterns = compile_patterns(&[r"1[3-9]\d{9}".to_string()]);
        let result = apply_blacklist("没有敏感信息", &patterns);
        assert_eq!(result, "没有敏感信息");
    }

    #[test]
    fn test_invalid_regex_skipped() {
        let patterns = compile_patterns(&["[invalid".to_string()]);
        assert!(patterns.is_empty());
    }

    #[test]
    fn test_parse_blacklist_json_valid() {
        // JSON 字符串内反斜杠需转义为 \\d，才能表示正则 \d
        let json = r#"["1[3-9]\\d{9}","\\d{17}[\\dXx]"]"#;
        let list = parse_blacklist_json(json);
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_parse_blacklist_json_empty() {
        let list = parse_blacklist_json("");
        assert!(list.is_empty());
    }

    #[test]
    fn test_parse_blacklist_json_invalid() {
        let list = parse_blacklist_json("not json");
        assert!(list.is_empty());
    }

    #[test]
    fn test_serialize_roundtrip() {
        let patterns = vec!["1[3-9]\\d{9}".to_string(), "test".to_string()];
        let json = serialize_blacklist(&patterns);
        let parsed = parse_blacklist_json(&json);
        assert_eq!(parsed, patterns);
    }

    #[test]
    fn test_default_blacklist_compiles() {
        let patterns: Vec<String> = DEFAULT_BLACKLIST.iter().map(|s| s.to_string()).collect();
        let compiled = compile_patterns(&patterns);
        assert_eq!(compiled.len(), 3);
    }
}
