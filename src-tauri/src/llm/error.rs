// TODO 人工审查点：1.状态码覆盖完整性 2.body 截断安全 3.网络错误分类 4.中文提示一致性
// NOTE LLM 错误友好化：HTTP 状态码 / 网络错误 → 中文提示 + 安全截断 body
// 安全策略：401/403/429 等鉴权/限流错误不显示 body（避免泄露鉴权细节）；
//           其他错误显示截断后的 body（≤200 字符，辅助开发者排查）

use crate::error::AppError;

/// body 截断长度上限（避免泄露服务端 stack trace 等敏感信息）
const MAX_BODY_LEN: usize = 200;

/// 截断 body 到安全长度，超出加省略号
///
/// 抽取为纯函数便于单元测试
fn truncate_body(body: &str) -> String {
    if body.chars().count() <= MAX_BODY_LEN {
        body.to_string()
    } else {
        let truncated: String = body.chars().take(MAX_BODY_LEN).collect();
        format!("{truncated}…")
    }
}

/// 根据 HTTP 状态码生成友好错误提示
///
/// - 鉴权/限流类（401/403/429）：仅返回中文提示，不附加 body
/// - 其他错误：中文提示 + 截断后的 body 详情（辅助排查）
pub fn format_http_error(status: u16, body: &str) -> AppError {
    let friendly = match status {
        400 => "请求格式错误，请检查 Prompt 模板或模型参数".to_string(),
        401 => "API Key 无效或已过期，请到设置中检查".to_string(),
        403 => "API Key 权限不足或模型无访问权限".to_string(),
        404 => "模型不存在，请检查 model 名称".to_string(),
        408 => "请求超时，请稍后重试或检查网络".to_string(),
        413 => "请求内容过大，请缩短文本".to_string(),
        429 => "请求过于频繁，请稍后再试".to_string(),
        500..=599 => format!("服务端异常（{status}），请稍后再试或更换服务商"),
        _ => format!("请求错误（{status}）"),
    };

    // 安全相关错误不附加 body（避免泄露鉴权/限流细节）
    const SAFE_STATUSES: [u16; 5] = [401, 403, 408, 413, 429];
    if SAFE_STATUSES.contains(&status) {
        AppError::Llm(friendly)
    } else {
        let body_detail = truncate_body(body.trim());
        if body_detail.is_empty() {
            AppError::Llm(friendly)
        } else {
            AppError::Llm(format!("{friendly} | 详情: {body_detail}"))
        }
    }
}

/// 将 reqwest 网络错误转为友好提示
///
/// 分类：超时 / 连接失败 / 响应解析失败 / 其他
pub fn format_network_error(e: reqwest::Error) -> AppError {
    if e.is_timeout() {
        AppError::Llm("网络超时，请检查网络连接或增加超时时间".into())
    } else if e.is_connect() {
        AppError::Llm("无法连接到服务器，请检查 base_url 或网络".into())
    } else if e.is_decode() {
        AppError::Llm("响应解析失败，请检查 API 是否兼容 OpenAI 格式".into())
    } else {
        // 其他网络错误保留原始信息（辅助排查，无安全风险）
        AppError::Llm(format!("网络错误: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== 正常流程 =====

    #[test]
    fn test_format_http_error_400_with_body() {
        // 400 应返回友好提示 + 截断 body（非安全状态码）
        let e = format_http_error(400, "invalid model field");
        match e {
            AppError::Llm(msg) => {
                assert!(msg.contains("请求格式错误"));
                assert!(msg.contains("invalid model field"));
            }
            _ => panic!("应为 Llm 错误"),
        }
    }

    #[test]
    fn test_format_http_error_500_with_body() {
        // 5xx 应返回服务端异常提示 + body 详情
        let e = format_http_error(500, "NullPointerException at line 42");
        match e {
            AppError::Llm(msg) => {
                assert!(msg.contains("服务端异常"));
                assert!(msg.contains("NullPointerException"));
            }
            _ => panic!("应为 Llm 错误"),
        }
    }

    // ===== 边界场景（安全状态码不泄露 body）=====

    #[test]
    fn test_format_http_error_401_no_body_leak() {
        // 401 安全状态码：不应包含 body 内容（可能含鉴权细节）
        let e = format_http_error(401, "Bearer token expired: sk-secret-key-12345");
        match e {
            AppError::Llm(msg) => {
                assert!(msg.contains("API Key 无效"));
                assert!(!msg.contains("secret-key"), "401 不应泄露 body: {msg}");
                assert!(!msg.contains("Bearer"), "401 不应泄露鉴权头: {msg}");
            }
            _ => panic!("应为 Llm 错误"),
        }
    }

    #[test]
    fn test_format_http_error_403_no_body_leak() {
        let e = format_http_error(403, "permission denied for model gpt-4");
        match e {
            AppError::Llm(msg) => {
                assert!(msg.contains("权限不足"));
                assert!(!msg.contains("permission denied"));
            }
            _ => panic!("应为 Llm 错误"),
        }
    }

    #[test]
    fn test_format_http_error_429_no_body_leak() {
        let e = format_http_error(429, "rate limit exceeded, retry after 60s");
        match e {
            AppError::Llm(msg) => {
                assert!(msg.contains("频繁"));
                assert!(!msg.contains("rate limit"));
            }
            _ => panic!("应为 Llm 错误"),
        }
    }

    // ===== 极端场景 =====

    #[test]
    fn test_truncate_body_short() {
        // 短 body 不截断
        let result = truncate_body("short");
        assert_eq!(result, "short");
    }

    #[test]
    fn test_truncate_body_long() {
        // 长 body 截断到 200 字符 + 省略号
        let long_body = "a".repeat(500);
        let result = truncate_body(&long_body);
        assert_eq!(result.chars().count(), MAX_BODY_LEN + 1); // 200 + 省略号
        assert!(result.ends_with('…'));
    }

    #[test]
    fn test_truncate_body_empty() {
        // 空 body 返回空字符串
        let result = truncate_body("");
        assert_eq!(result, "");
    }

    #[test]
    fn test_truncate_body_unicode_safe() {
        // Unicode 字符按字符数截断（非字节），避免乱码
        let long_body = "你好".repeat(150); // 300 字符
        let result = truncate_body(&long_body);
        assert_eq!(result.chars().count(), MAX_BODY_LEN + 1);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn test_format_http_error_unknown_status() {
        // 未知状态码（如 599）应归类为 5xx 服务端异常
        let e = format_http_error(599, "gateway timeout");
        match e {
            AppError::Llm(msg) => {
                assert!(msg.contains("599"));
                assert!(msg.contains("服务端异常"));
            }
            _ => panic!("应为 Llm 错误"),
        }
    }

    #[test]
    fn test_format_http_error_empty_body() {
        // 空 body 的非安全状态码：仅返回友好提示，不附加 " | 详情:"
        let e = format_http_error(500, "   ");
        match e {
            AppError::Llm(msg) => {
                assert!(msg.contains("服务端异常"));
                assert!(!msg.contains("详情"));
            }
            _ => panic!("应为 Llm 错误"),
        }
    }
}
