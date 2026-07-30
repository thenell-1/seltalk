// TODO 人工审查点：1.字段对齐 OpenAI 协议 2.Default 3.序列化兼容 4.stream_enabled 默认值
// NOTE OpenAI 兼容协议数据类型
use serde::{Deserialize, Serialize};

use crate::config::{DEFAULT_LLM_MAX_TOKENS, DEFAULT_LLM_STREAM_ENABLED, DEFAULT_LLM_TEMPERATURE};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub temperature: f64,
    pub max_tokens: u32,
    /// 是否启用流式输出（SSE），控制 orchestrator 调用流式/非流式生成分支
    pub stream_enabled: bool,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            api_key: String::new(),
            model: String::new(),
            temperature: DEFAULT_LLM_TEMPERATURE,
            max_tokens: DEFAULT_LLM_MAX_TOKENS,
            stream_enabled: DEFAULT_LLM_STREAM_ENABLED,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: f64,
    pub max_tokens: u32,
    /// 部分接口不支持 n，None 时不下发
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    /// 是否启用流式输出（SSE）。None 时不下发，Some(true) 启用流式
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<ChatChoice>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatChoice {
    pub message: ChatMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionTestResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub message: String,
}
