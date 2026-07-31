// TODO 人工审查点：1.字段对齐 OpenAI 协议 2.Default 3.序列化兼容 4.stream_enabled 默认值 5.ts-rs 类型导出
// NOTE OpenAI 兼容协议数据类型
//       P4.4：所有 Serialize struct 派生 TS，cargo test 时自动生成 .ts 到 ./bindings/llm/
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::config::{
    DEFAULT_LLM_MAX_CONTEXT_LENGTH, DEFAULT_LLM_MAX_TOKENS, DEFAULT_LLM_MODEL_TYPE,
    DEFAULT_LLM_STREAM_ENABLED, DEFAULT_LLM_TEMPERATURE,
};

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../bindings/llm/LlmConfig.ts")]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub temperature: f64,
    pub max_tokens: u32,
    /// 是否启用流式输出（SSE），控制 orchestrator 调用流式/非流式生成分支
    pub stream_enabled: bool,
    /// 模型类型/提供商分类（openai/anthropic/azure/deepseek/local...），空表示未指定
    #[serde(default)]
    pub model_type: String,
    /// 最大上下文长度（0 = 未设置/不限）
    #[serde(default)]
    pub max_context_length: u32,
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
            model_type: DEFAULT_LLM_MODEL_TYPE.to_string(),
            max_context_length: DEFAULT_LLM_MAX_CONTEXT_LENGTH,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../bindings/llm/ChatMessage.ts")]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../bindings/llm/ChatRequest.ts")]
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../bindings/llm/ConnectionTestResult.ts")]
pub struct ConnectionTestResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub message: String,
}
