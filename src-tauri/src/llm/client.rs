// TODO 人工审查点：1.兼容性 wrapper 签名一致 2.无逻辑重复 3.clone 开销可接受 4.委托正确
// NOTE P6.1: 向后兼容 shim —— 保留原有自由函数签名，内部委托给 OpenAiProvider
//       现有调用点（llm/mod.rs::generate_candidates/_stream、commands.rs::test_llm_connection）
//       无需任何修改即可平滑迁移到 Provider 架构。
//
//       迁移完成后，新代码应直接使用 OpenAiProvider 或 LlmProvider trait，
//       本模块仅为兼容旧调用点而保留，后续可在所有调用点迁移后移除。
use crate::error::AppResult;

use super::provider::{LlmProvider, OpenAiProvider};
use super::types::{ChatRequest, ChatResponse, ConnectionTestResult, LlmConfig};

/// 非流式 chat 请求（向后兼容 shim，委托给 OpenAiProvider）
///
/// 原签名：`(client: &reqwest::Client, cfg: &LlmConfig, req: &ChatRequest)`
/// 内部构造临时 `OpenAiProvider` 并委托调用。
///
/// 性能：`reqwest::Client` clone 为 Arc 浅拷贝，`LlmConfig` 为小结构体，开销可忽略
pub async fn chat(
    client: &reqwest::Client,
    cfg: &LlmConfig,
    req: &ChatRequest,
) -> AppResult<ChatResponse> {
    OpenAiProvider::new(client.clone(), cfg.clone())
        .chat(req)
        .await
}

/// 流式 chat 请求（向后兼容 shim，委托给 OpenAiProvider）
///
/// 原签名含 `impl FnMut(&str)` 回调，内部转发为 `&mut (dyn FnMut(&str) + Send)` 以匹配 trait 方法。
/// `+ Send` 约束保证 future 可在 Tauri `async_runtime::spawn` 中使用（现有调用点的闭包均满足）。
pub async fn chat_stream(
    client: &reqwest::Client,
    cfg: &LlmConfig,
    req: &ChatRequest,
    mut on_chunk: impl FnMut(&str) + Send,
) -> AppResult<String> {
    OpenAiProvider::new(client.clone(), cfg.clone())
        .chat_stream(req, &mut on_chunk)
        .await
}

/// 连通性测试（向后兼容 shim，委托给 OpenAiProvider）
pub async fn test_connection(
    client: &reqwest::Client,
    cfg: &LlmConfig,
) -> AppResult<ConnectionTestResult> {
    OpenAiProvider::new(client.clone(), cfg.clone())
        .test_connection()
        .await
}
