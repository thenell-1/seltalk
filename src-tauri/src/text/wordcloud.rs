// TODO 人工审查点：1.分词规则合理性 2.大文本性能 3.正则编译缓存 4.停用词过滤
// NOTE 词频统计：从候选文本中提取词语并计数，用于高频词云展示
//       分词策略：中文连续2字及以上、英文单词2字母及以上、数字串2位及以上
//       不引入 jieba 等重型分词依赖，保持轻量化；统计精度满足词云可视化即可
use std::collections::HashMap;

use regex::Regex;

/// 中文停用词表（高频无意义词，不计入词频）
const STOP_WORDS: &[&str] = &[
    "的", "了", "是", "在", "我", "有", "和", "就", "不", "人", "都", "一", "上", "也", "很",
    "到", "说", "要", "去", "你", "会", "着", "没有", "看", "好", "自己", "这", "那", "它",
    "他", "她", "们", "什么", "怎么", "可以", "这个", "那个", "一个", "没有", "不是", "不要",
    "的话", "然后", "因为", "所以", "但是", "不过", "还是", "就是", "只是", "已经", "一下",
    "觉得", "知道", "现在", "这样", "那样", "怎么", "为什么", "其实", "可能", "应该", "或者",
];

/// 编译分词正则（once_cell 或 OnceLock 缓存，避免重复编译）
static WORD_PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

/// 获取编译好的分词正则
fn get_pattern() -> &'static Regex {
    WORD_PATTERN.get_or_init(|| {
        // 匹配：中文连续2字及以上 | 英文字母2个及以上 | 数字2位及以上
        Regex::new(r"[\u4e00-\u9fa5]{2,}|[A-Za-z]{2,}|\d{2,}").expect("词频正则编译失败")
    })
}

/// 判断是否为停用词
fn is_stop_word(word: &str) -> bool {
    STOP_WORDS.contains(&word)
}

/// 从文本中提取词语并统计词频
///
/// # 参数
/// - `text`：待统计的文本（通常为用户选中的候选回复）
///
/// # 返回
/// 按词频降序排列的 (词语, 次数) 列表
#[allow(dead_code)]
pub fn count_words(text: &str) -> Vec<(String, u32)> {
    let re = get_pattern();
    let mut freq: HashMap<String, u32> = HashMap::new();

    for m in re.find_iter(text) {
        let word = m.as_str();
        // 跳过停用词
        if is_stop_word(word) {
            continue;
        }
        // 英文统一转小写，避免大小写差异导致重复计数
        let key = if word.chars().all(|c| c.is_ascii_alphabetic()) {
            word.to_lowercase()
        } else {
            word.to_string()
        };
        *freq.entry(key).or_insert(0) += 1;
    }

    let mut vec: Vec<(String, u32)> = freq.into_iter().collect();
    // 按词频降序，词频相同按词字典序（保证测试稳定）
    vec.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    vec
}

/// 从多条文本中批量统计词频
///
/// # 参数
/// - `texts`：多条文本切片
///
/// # 返回
/// 合并后的词频列表（降序）
#[allow(dead_code)]
pub fn count_words_batch(texts: &[&str]) -> Vec<(String, u32)> {
    let mut freq: HashMap<String, u32> = HashMap::new();
    let re = get_pattern();

    for text in texts {
        for m in re.find_iter(text) {
            let word = m.as_str();
            if is_stop_word(word) {
                continue;
            }
            let key = if word.chars().all(|c| c.is_ascii_alphabetic()) {
                word.to_lowercase()
            } else {
                word.to_string()
            };
            *freq.entry(key).or_insert(0) += 1;
        }
    }

    let mut vec: Vec<(String, u32)> = freq.into_iter().collect();
    vec.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    vec
}

/// 仅提取词语列表（不计数），用于词频记录入库
///
/// # 返回
/// 去重后的词语列表
pub fn extract_words(text: &str) -> Vec<String> {
    let re = get_pattern();
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();

    for m in re.find_iter(text) {
        let word = m.as_str();
        if is_stop_word(word) {
            continue;
        }
        let key = if word.chars().all(|c| c.is_ascii_alphabetic()) {
            word.to_lowercase()
        } else {
            word.to_string()
        };
        if seen.insert(key.clone()) {
            result.push(key);
        }
    }
    result
}

/// 统计文本中的词语总数（去重后）
#[allow(dead_code)]
pub fn count_unique_words(text: &str) -> usize {
    extract_words(text).len()
}

/// 验证词频统计结果非空（供调用方快速判断是否需要入库）
#[allow(dead_code)]
pub fn has_words(text: &str) -> bool {
    let re = get_pattern();
    re.find_iter(text).any(|m| !is_stop_word(m.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== 正常流程测试 =====

    #[test]
    fn test_count_chinese_words() {
        // 连续中文字符作为整体词匹配（非按字数切分）
        let result = count_words("你好 世界 你好 朋友");
        // "你好" 出现 2 次，"世界" 和 "朋友" 各 1 次
        let hello_count = result.iter().find(|(w, _)| w == "你好").map(|(_, c)| *c);
        assert_eq!(hello_count, Some(2));
    }

    #[test]
    fn test_count_english_words() {
        let result = count_words("hello world hello");
        let hello_count = result.iter().find(|(w, _)| w == "hello").map(|(_, c)| *c);
        assert_eq!(hello_count, Some(2));
    }

    #[test]
    fn test_count_numbers() {
        let result = count_words("订单号 123456 和 123456");
        let num_count = result.iter().find(|(w, _)| w == "123456").map(|(_, c)| *c);
        assert_eq!(num_count, Some(2));
    }

    #[test]
    fn test_count_mixed() {
        let result = count_words("hello 你好 world 世界 hello");
        assert!(result.len() >= 3);
        // hello 出现 2 次应排第一
        assert_eq!(result[0].0, "hello");
        assert_eq!(result[0].1, 2);
    }

    #[test]
    fn test_count_sorted_descending() {
        let result = count_words("aaa aaa aaa bbb bbb ccc");
        assert_eq!(result[0], ("aaa".to_string(), 3));
        assert_eq!(result[1], ("bbb".to_string(), 2));
        assert_eq!(result[2], ("ccc".to_string(), 1));
    }

    #[test]
    fn test_case_insensitive_english() {
        let result = count_words("Hello hello HELLO");
        let count = result.iter().find(|(w, _)| w == "hello").map(|(_, c)| *c);
        assert_eq!(count, Some(3));
    }

    #[test]
    fn test_extract_words_dedup() {
        let words = extract_words("你好 你好 世界 世界 世界");
        assert_eq!(words.len(), 2);
        assert!(words.contains(&"你好".to_string()));
        assert!(words.contains(&"世界".to_string()));
    }

    #[test]
    fn test_batch_count() {
        let texts = vec!["你好 世界", "你好 朋友"];
        let result = count_words_batch(&texts);
        let hello_count = result.iter().find(|(w, _)| w == "你好").map(|(_, c)| *c);
        assert_eq!(hello_count, Some(2));
    }

    // ===== 边界场景测试 =====

    #[test]
    fn test_empty_text() {
        assert!(count_words("").is_empty());
    }

    #[test]
    fn test_whitespace_only() {
        assert!(count_words("   \n\t  ").is_empty());
    }

    #[test]
    fn test_single_chars_filtered() {
        // 单个中文字符不匹配（需要2字及以上）
        let result = count_words("啊 我 你 他");
        assert!(result.is_empty());
    }

    #[test]
    fn test_single_english_letter_filtered() {
        // 单个英文字母不匹配（需要2字母及以上）
        let result = count_words("a b c d e");
        assert!(result.is_empty());
    }

    #[test]
    fn test_punctuation_only() {
        assert!(count_words("！@#￥%……&*（）").is_empty());
    }

    #[test]
    fn test_stop_words_filtered() {
        let result = count_words("的话 因为 你好");
        // "的话" 和 "因为" 是停用词，应被过滤
        let has_stop = result.iter().any(|(w, _)| w == "的话" || w == "因为");
        assert!(!has_stop);
        // "你好" 应保留
        let has_hello = result.iter().any(|(w, _)| w == "你好");
        assert!(has_hello);
    }

    #[test]
    fn test_has_words_true() {
        assert!(has_words("你好世界"));
    }

    #[test]
    fn test_has_words_false_empty() {
        assert!(!has_words(""));
    }

    #[test]
    fn test_has_words_false_stop_only() {
        assert!(!has_words("的话 因为"));
    }

    #[test]
    fn test_count_unique_words() {
        assert_eq!(count_unique_words("你好 你好 世界"), 2);
    }

    // ===== 错误场景测试 =====

    #[test]
    fn test_count_words_handles_emoji() {
        // emoji 不影响分词，正常中文词应被正确提取
        let result = count_words("🎉 你好世界 🎁");
        assert!(result.iter().any(|(w, _)| w == "你好世界"));
    }

    #[test]
    fn test_count_words_handles_null_bytes() {
        // null 字节不应导致 panic
        let _ = count_words("\0\0\0");
    }
}
