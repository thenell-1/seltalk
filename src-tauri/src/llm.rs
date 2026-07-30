// TODO 人工审查点：1.API密钥安全存储 2.请求超时与重试策略 3.本地Ollama兼容性 4.错误信息中文化
// NOTE F4 LLM 服务模块：基于 reqwest 实现 OpenAI 兼容协议调用
// 支持两种模式：云端（DeepSeek 等 OpenAI 兼容接口）和本地（Ollama）
// 输入：捕获清洗后的文本 + 系统提示词
// 输出：多条候选回复

use crate::config::LlmConfig;
use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// 默认请求超时时间（秒）
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// 系统提示词：用于指导 LLM 生成回复
const SYSTEM_PROMPT: &str = "你是一个聊天回复助手。根据用户给出的对方消息，生成3条简短、自然、得体的候选回复。每条回复单独一行，不要编号。回复风格应贴近日常聊天。";

/// 聊天消息角色
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

/// 聊天消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
}

/// LLM 客户端
#[derive(Debug, Clone)]
pub struct LlmClient {
    config: LlmConfig,
    mode: LlmMode,
    http_client: reqwest::Client,
}

/// LLM 调用模式
#[derive(Debug, Clone, PartialEq)]
pub enum LlmMode {
    Cloud,
    Local,
}

impl LlmMode {
    /// 从字符串解析模式
    pub fn from_str(s: &str) -> AppResult<Self> {
        match s.trim().to_lowercase().as_str() {
            "cloud" => Ok(Self::Cloud),
            "local" => Ok(Self::Local),
            _ => Err(AppError::Config(format!(
                "不支持的 LLM 模式: {s}，请使用 cloud 或 local"
            ))),
        }
    }
}

/// 生成回复的请求参数
#[derive(Debug, Clone)]
pub struct GenerateParams {
    pub captured_text: String,
    pub candidate_count: u32,
}

impl LlmClient {
    /// 创建新的 LLM 客户端
    /// NOTE 强制 IPv4 + User-Agent + 连接超时
    /// IPv6 连接 TCP 握手成功但 TLS/HTTP 层超时（MTU/路由问题），强制 IPv4 解决
    pub fn new(config: LlmConfig, mode: LlmMode) -> AppResult<Self> {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .connect_timeout(Duration::from_secs(10))
            .user_agent("CreativeInputMethod/0.1.0")
            .local_address(std::net::IpAddr::from(std::net::Ipv4Addr::new(0, 0, 0, 0)))
            .build()
            .map_err(|e| AppError::Config(format!("创建 HTTP 客户端失败: {e}")))?;

        Ok(Self {
            config,
            mode,
            http_client,
        })
    }

    /// 生成候选回复
    pub async fn generate_replies(&self, params: &GenerateParams) -> AppResult<Vec<String>> {
        tracing::info!("调用 LLM 生成回复，模式: {:?}", self.mode);

        let raw_text = match self.mode {
            LlmMode::Cloud => self.call_cloud_api(params).await?,
            LlmMode::Local => self.call_local_api(params).await?,
        };

        let replies = parse_replies(&raw_text, params.candidate_count as usize);
        if replies.is_empty() {
            return Err(AppError::Config("LLM 未返回有效回复".to_string()));
        }
        Ok(replies)
    }

    /// 测试 LLM 连通性（返回成功消息或具体错误）
    pub async fn test_connection(&self) -> AppResult<String> {
        let params = GenerateParams {
            captured_text: "你好".to_string(),
            candidate_count: 1,
        };
        match self.generate_replies(&params).await {
            Ok(replies) => {
                let preview = replies.first().map(|s| s.as_str()).unwrap_or("(空)");
                Ok(format!("连接成功，示例回复：{preview}"))
            }
            Err(e) => {
                tracing::warn!("LLM 连接测试失败: {e}");
                Err(e)
            }
        }
    }

    /// 测试 LLM 连通性（旧接口，仅返回布尔值）
    pub async fn health_check(&self) -> AppResult<bool> {
        let params = GenerateParams {
            captured_text: "你好".to_string(),
            candidate_count: 1,
        };
        match self.generate_replies(&params).await {
            Ok(_) => Ok(true),
            Err(e) => {
                tracing::warn!("LLM 健康检查失败: {e}");
                Ok(false)
            }
        }
    }

    /// 调用云端 OpenAI 兼容接口
    async fn call_cloud_api(&self, params: &GenerateParams) -> AppResult<String> {
        let api_key = self.config.cloud_api_key.trim();
        if api_key.is_empty() {
            return Err(AppError::Config("云端 API 密钥未配置".to_string()));
        }

        let endpoint = self.config.cloud_endpoint.trim();
        let model = self.config.cloud_model.trim();
        if model.is_empty() {
            return Err(AppError::Config("云端模型名称未配置".to_string()));
        }

        let url = format!("{}/chat/completions", endpoint.trim_end_matches('/'));
        let body = CloudRequestBody {
            model: model.to_string(),
            messages: build_messages(&params.captured_text),
            temperature: 0.7,
            max_tokens: 500,
            n: params.candidate_count,
        };

        // NOTE 诊断日志：输出请求URL和模型名（隐藏密钥），便于排查连接问题
        tracing::info!(
            "云端 LLM 请求: url={url}, model={model}, key前缀={}",
            &api_key[..api_key.len().min(8)]
        );

        let resp = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {api_key}"))
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                // NOTE 输出完整错误源链，便于诊断网络/TLS/代理问题
                let mut msg = format!("请求云端 LLM 失败: {e}");
                let mut source = std::error::Error::source(&e);
                while let Some(s) = source {
                    msg.push_str(&format!("\n  源: {s}"));
                    source = s.source();
                }
                tracing::error!("{msg}");
                AppError::Config(msg)
            })?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::Config(format!(
                "云端 LLM 返回错误状态: {status}，响应: {text}"
            )));
        }

        let result: CloudResponse = resp
            .json()
            .await
            .map_err(|e| AppError::Config(format!("解析云端 LLM 响应失败: {e}")))?;

        let combined = result
            .choices
            .iter()
            .map(|c| c.message.content.clone())
            .collect::<Vec<_>>()
            .join("\n");

        Ok(combined)
    }

    /// 调用本地 Ollama 接口
    async fn call_local_api(&self, params: &GenerateParams) -> AppResult<String> {
        let endpoint = self.config.local_endpoint.trim();
        let model = self.config.local_model.trim();
        if model.is_empty() {
            return Err(AppError::Config("本地模型名称未配置".to_string()));
        }

        let url = format!("{}/api/chat", endpoint.trim_end_matches('/'));
        let body = OllamaRequestBody {
            model: model.to_string(),
            messages: build_messages(&params.captured_text),
            stream: false,
            options: OllamaOptions { temperature: 0.7 },
        };

        let resp = self
            .http_client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Config(format!("请求本地 Ollama 失败: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::Config(format!(
                "本地 Ollama 返回错误状态: {status}，响应: {text}"
            )));
        }

        let result: OllamaResponse = resp
            .json()
            .await
            .map_err(|e| AppError::Config(format!("解析 Ollama 响应失败: {e}")))?;

        Ok(result.message.content)
    }
}

/// 构建消息列表
fn build_messages(captured_text: &str) -> Vec<ChatMessage> {
    vec![
        ChatMessage {
            role: MessageRole::System,
            content: SYSTEM_PROMPT.to_string(),
        },
        ChatMessage {
            role: MessageRole::User,
            content: format!("对方消息：{captured_text}"),
        },
    ]
}

/// 解析 LLM 返回的文本为多条候选回复
fn parse_replies(raw: &str, expected_count: usize) -> Vec<String> {
    raw.lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .take(expected_count.max(1))
        .collect()
}

/// 云端请求体（OpenAI 兼容）
#[derive(Debug, Serialize)]
struct CloudRequestBody {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    max_tokens: u32,
    n: u32,
}

/// 云端响应体
#[derive(Debug, Deserialize)]
struct CloudResponse {
    choices: Vec<CloudChoice>,
}

#[derive(Debug, Deserialize)]
struct CloudChoice {
    message: CloudMessage,
}

#[derive(Debug, Deserialize)]
struct CloudMessage {
    content: String,
}

/// Ollama 请求体
#[derive(Debug, Serialize)]
struct OllamaRequestBody {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    options: OllamaOptions,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    temperature: f32,
}

/// Ollama 响应体
#[derive(Debug, Deserialize)]
struct OllamaResponse {
    message: OllamaMessage,
}

#[derive(Debug, Deserialize)]
struct OllamaMessage {
    content: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> LlmConfig {
        LlmConfig {
            cloud_api_key: "test-key".to_string(),
            cloud_endpoint: "https://api.deepseek.com/v1".to_string(),
            cloud_model: "deepseek-chat".to_string(),
            local_endpoint: "http://localhost:11434".to_string(),
            local_model: "qwen2.5:7b".to_string(),
        }
    }

    #[test]
    fn test_build_messages_normal() {
        let messages = build_messages("你好啊");
        assert_eq!(messages.len(), 2);
        assert!(matches!(messages[0].role, MessageRole::System));
        assert!(matches!(messages[1].role, MessageRole::User));
        assert!(messages[1].content.contains("你好啊"));
    }

    #[test]
    fn test_parse_replies_normal() {
        let raw = "好的，没问题\n收到\n稍等一下";
        let replies = parse_replies(raw, 3);
        assert_eq!(replies.len(), 3);
        assert_eq!(replies[0], "好的，没问题");
    }

    #[test]
    fn test_parse_replies_empty() {
        let replies = parse_replies("", 3);
        assert!(replies.is_empty());
    }

    #[test]
    fn test_parse_replies_only_whitespace() {
        let replies = parse_replies("   \n  \n  ", 3);
        assert!(replies.is_empty());
    }

    #[test]
    fn test_parse_replies_fewer_than_expected() {
        let raw = "好的\n收到";
        let replies = parse_replies(raw, 5);
        assert_eq!(replies.len(), 2);
    }

    #[tokio::test]
    async fn test_llm_client_creation() {
        let config = test_config();
        let client = LlmClient::new(config, LlmMode::Cloud);
        assert!(client.is_ok());
    }

    #[tokio::test]
    async fn test_cloud_api_missing_key() {
        let mut config = test_config();
        config.cloud_api_key = "  ".to_string();
        let client = LlmClient::new(config, LlmMode::Cloud).unwrap();
        let params = GenerateParams {
            captured_text: "你好".to_string(),
            candidate_count: 1,
        };
        let result = client.call_cloud_api(&params).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("API 密钥"));
    }

    #[test]
    fn test_llm_mode_from_str_cloud() {
        let mode = LlmMode::from_str("cloud");
        assert!(matches!(mode.unwrap(), LlmMode::Cloud));
    }

    #[test]
    fn test_llm_mode_from_str_local() {
        let mode = LlmMode::from_str("local");
        assert!(matches!(mode.unwrap(), LlmMode::Local));
    }

    #[test]
    fn test_llm_mode_from_str_invalid() {
        let mode = LlmMode::from_str("invalid");
        assert!(mode.is_err());
    }
}
