// TODO 人工审查点：1.空输入处理 2.分隔符优先级 3.去空白
// NOTE LLM 候选切分：优先 --- 分隔，其次双换行，最后整段作单条
pub fn split_candidates(raw: &str) -> Vec<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Vec::new();
    }
    // 优先按 --- 分隔（默认 Prompt 模板要求）
    let parts: Vec<&str> = raw.split("---").collect();
    if parts.len() > 1 {
        return collect_non_empty(&parts);
    }
    // 再按双换行分隔
    let parts: Vec<&str> = raw.split("\n\n").collect();
    if parts.len() > 1 {
        return collect_non_empty(&parts);
    }
    // 单条
    vec![raw.to_string()]
}

fn collect_non_empty(parts: &[&str]) -> Vec<String> {
    parts
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_by_separator() {
        let r = split_candidates("你好\n---\n在的\n---\n怎么了");
        assert_eq!(r, vec!["你好", "在的", "怎么了"]);
    }

    #[test]
    fn test_split_by_double_newline() {
        let r = split_candidates("第一条\n\n第二条\n\n第三条");
        assert_eq!(r, vec!["第一条", "第二条", "第三条"]);
    }

    #[test]
    fn test_split_single_returns_one() {
        let r = split_candidates("只有一条回复");
        assert_eq!(r, vec!["只有一条回复"]);
    }

    #[test]
    fn test_split_empty() {
        assert!(split_candidates("").is_empty());
        assert!(split_candidates("   \n  ").is_empty());
    }
}
