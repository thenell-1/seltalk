// TODO 人工审查点：1.trait 设计是否覆盖未来 Provider 2.对象安全 3.错误传播 4.配置校验 5.流式回调线程安全
// NOTE P6.1: LLM Provider trait 抽象 + OpenAI 兼容实现
//       抽象不同 LLM 服务商调用接口，当前提供 OpenAiProvider（OpenAI 兼容 /v1/chat/completions）
//       后续可扩展 AnthropicProvider、LocalProvider 等，由 orchestrator 按 cfg 选择
use std::time::Instant;

use futures_util::StreamExt;
use serde::Deserialize;

use crate::error::{AppError, AppResult};

use super::types::{ChatMessage, ChatRequest, ChatResponse, ConnectionTestResult, LlmConfig};

/// LLM 提供方抽象 trait
///
/// 抽象不同 LLM 服务商的调用接口，支持后续扩展多 Provider
/// （如 OpenAI 兼容、Anthropic、本地模型等）。
///
/// 实现方需保证 Send + Sync 以便在 Tauri 异步命令中跨线程使用。
/// 流式回调 on_chunk 使用 `&mut (dyn FnMut(&str) + Send)`：
/// - `&mut dyn ...` 避免泛型参数（保持 trait 简洁）
/// - `+ Send` 保证 future 跨 await 仍满足 Tauri `spawn` 的 `Send` 约束
///
/// NOTE 当前使用原生 async fn in trait（Rust 1.75+ 稳定），无需 async-trait 依赖。
///      如后续需要 `dyn LlmProvider` 动态分发，可引入 async-trait 或手动装箱 Future。
pub trait LlmProvider: Send + Sync {
    /// 非流式聊天请求，返回完整的 ChatResponse
    ///
    /// 错误：配置不完整 / 网络失败 / HTTP 非 2xx / 响应解析失败
    async fn chat(&self, req: &ChatRequest) -> AppResult<ChatResponse>;

    /// 流式聊天请求，通过 `on_chunk` 回调推送增量内容，返回完整累积文本
    ///
    /// 性能：首字延迟从"整段生成时间"降到"首 token 时间"，悬浮窗可渐进显示生成内容。
    /// 完成后由调用方对返回的完整文本做 `---` 切分得到候选列表。
    ///
    /// `on_chunk` 在每个 delta content 到达时被调用；回调失败不影响流式接收。
    /// 要求 `+ Send` 以便在 Tauri `async_runtime::spawn` 中安全使用。
    async fn chat_stream(
        &self,
        req: &ChatRequest,
        on_chunk: &mut (dyn FnMut(&str) + Send),
    ) -> AppResult<String>;

    /// 连通性测试：发一条最小请求验证配置是否可用
    ///
    /// 返回 `ConnectionTestResult`（含 ok 标志、延迟、消息），即使失败也返回 Ok 包装的结果
    async fn test_connection(&self) -> AppResult<ConnectionTestResult>;
}

/// OpenAI 兼容的 LLM Provider 实现
///
/// 适配所有兼容 OpenAI `/v1/chat/completions` 协议的服务商
/// （OpenAI、DeepSeek、智谱、通义千问、agnes-ai 等）。
///
/// 构造时传入复用的 `reqwest::Client`（由 AppState 持有）和运行时 `LlmConfig`（从 DB 加载）。
pub struct OpenAiProvider {
    /// HTTP 客户端（复用连接池，clone 为 Arc 浅拷贝）
    client: reqwest::Client,
    /// LLM 运行时配置
    cfg: LlmConfig,
}

impl OpenAiProvider {
    /// 构造 Provider
    ///
    /// `client` 由 AppState 持有的复用客户端，`cfg` 从 DB settings 表加载
    pub fn new(client: reqwest::Client, cfg: LlmConfig) -> Self {
        Self { client, cfg }
    }

    /// 校验配置完整性（base_url + model 必填）
    ///
    /// 抽取为私有方法供三个 trait 方法复用，避免重复校验逻辑
    fn validate(&self) -> AppResult<()> {
        if self.cfg.base_url.is_empty() || self.cfg.model.is_empty() {
            return Err(AppError::Llm("LLM 配置不完整：base_url 或 model 为空".into()));
        }
        Ok(())
    }
}

impl LlmProvider for OpenAiProvider {
    async fn chat(&self, req: &ChatRequest) -> AppResult<ChatResponse> {
        self.validate()?;
        let url = build_chat_url(&self.cfg.base_url);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.cfg.api_key)
            .json(req)
            .send()
            .await
            .map_err(super::error::format_network_error)?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(super::error::format_http_error(status.as_u16(), &body));
        }
        resp.json::<ChatResponse>()
            .await
            .map_err(|e| AppError::Llm(format!("解析响应失败: {e}")))
    }

    async fn chat_stream(
        &self,
        req: &ChatRequest,
        on_chunk: &mut (dyn FnMut(&str) + Send),
    ) -> AppResult<String> {
        self.validate()?;
        let url = build_chat_url(&self.cfg.base_url);
        let t_start = Instant::now();

        let resp = self
            .client
            .post(&url)
            // SSE 标准请求头：显式声明接收事件流，避免部分代理/网关缓冲整个响应后才转发
            .header("Accept", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .bearer_auth(&self.cfg.api_key)
            .json(req)
            .send()
            .await
            .map_err(super::error::format_network_error)?;

        // 响应头到达 = 连接+TLS+服务端排队完成（不含模型生成 token）
        // 与首字节拆分后可精确定位瓶颈：响应头慢=网络/排队，首 token 慢=模型生成
        let t_headers = Instant::now();
        tracing::info!(
            "LLM 响应头到达: {}ms（连接+TLS+服务端排队）",
            t_headers.duration_since(t_start).as_millis()
        );

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(super::error::format_http_error(status.as_u16(), &body));
        }

        let mut stream = resp.bytes_stream();
        let mut buffer: Vec<u8> = Vec::new();
        let mut full = String::new();
        let mut first_byte_logged = false;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| AppError::Llm(format!("读取流失败: {e}")))?;
            if !first_byte_logged {
                // 首 token 耗时 = 模型生成首个 token 的时间（响应头之后）
                // 若首 token ≈ 总耗时，说明服务端缓冲整段响应（非真流式），需排查 API/代理
                tracing::info!(
                    "LLM 流式首字节: 总计 {}ms（响应头后 {}ms，模型生成首 token）",
                    t_start.elapsed().as_millis(),
                    t_headers.elapsed().as_millis()
                );
                first_byte_logged = true;
            }
            buffer.extend_from_slice(&chunk);

            // 按行处理已完整接收的内容（以 b'\n' 分隔）
            while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = buffer.drain(..=pos).collect();
                // 每行是完整 UTF-8（SSE 协议保证），用 lossy 转换安全
                let line = String::from_utf8_lossy(&line_bytes);
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                // 解析单行 SSE：None=[DONE]结束，Some=delta 列表（空表示无内容/非 data 行/解析失败）
                match parse_sse_line(line) {
                    None => {
                        tracing::info!(
                            "LLM 流式完成，总耗时 {}ms，输出 {} 字",
                            t_start.elapsed().as_millis(),
                            full.chars().count()
                        );
                        return Ok(full);
                    }
                    Some(deltas) => {
                        for d in deltas {
                            on_chunk(&d);
                            full.push_str(&d);
                        }
                    }
                }
            }
        }

        // 流结束未显式收到 [DONE]（部分接口不发），返回已累积文本
        tracing::info!(
            "LLM 流式结束（未收到 [DONE]），总耗时 {}ms，输出 {} 字",
            t_start.elapsed().as_millis(),
            full.chars().count()
        );
        Ok(full)
    }

    async fn test_connection(&self) -> AppResult<ConnectionTestResult> {
        let start = Instant::now();
        let req = ChatRequest {
            model: self.cfg.model.clone(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "ping".into(),
            }],
            temperature: 0.0,
            max_tokens: 1,
            n: None,
            stream: None,
        };
        match self.chat(&req).await {
            Ok(_) => Ok(ConnectionTestResult {
                ok: true,
                latency_ms: start.elapsed().as_millis() as u64,
                message: "连接成功".into(),
            }),
            Err(e) => Ok(ConnectionTestResult {
                ok: false,
                latency_ms: start.elapsed().as_millis() as u64,
                message: e.to_string(),
            }),
        }
    }
}

// ===== 内部工具函数（从 client.rs 迁移，私有于本模块）=====

/// 构造 chat completions 请求 URL
///
/// 兼容用户填写的 base_url 是否含 /v1 后缀：
/// - `https://api.openai.com` → `https://api.openai.com/v1/chat/completions`
/// - `https://api.openai.com/v1` → `https://api.openai.com/v1/chat/completions`
/// - `https://proxy.com/v1/` → `https://proxy.com/v1/chat/completions`
fn build_chat_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        format!("{trimmed}/chat/completions")
    } else {
        format!("{trimmed}/v1/chat/completions")
    }
}

/// 流式响应中的单个 chunk（OpenAI 兼容 SSE 格式：data: {choices:[{delta:{content}}]}）
#[derive(Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
}

#[derive(Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
}

#[derive(Deserialize)]
struct StreamDelta {
    /// 首个 chunk 可能无 content（仅 role），用 default 兜底
    #[serde(default)]
    content: String,
}

/// 解析单行 SSE，提取 delta content 列表
///
/// 返回值：
/// - `None`：收到 `data: [DONE]`，流结束
/// - `Some(vec)`：该行所有 choice 的 delta.content（空 Vec 表示空行/非 data 行/解析失败/无 content）
///
/// 抽取为纯函数以便单元测试；容错：单行解析失败不报错，返回空 Vec（部分代理插入非标准行）。
fn parse_sse_line(line: &str) -> Option<Vec<String>> {
    let line = line.trim();
    if line.is_empty() {
        return Some(Vec::new());
    }
    let Some(data) = line.strip_prefix("data:") else {
        // 非 data 行（如心跳 ":"），跳过
        return Some(Vec::new());
    };
    let data = data.trim();
    if data == "[DONE]" {
        return None;
    }
    match serde_json::from_str::<StreamChunk>(data) {
        Ok(chunk) => Some(
            chunk
                .choices
                .into_iter()
                .map(|c| c.delta.content)
                .filter(|s| !s.is_empty())
                .collect(),
        ),
        Err(e) => {
            // 单行解析失败不中断流（部分代理插入非标准行），静默跳过
            tracing::debug!("SSE 行解析失败「{data}」: {e}");
            Some(Vec::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== trait 契约测试 =====

    #[test]
    fn test_openai_provider_constructible() {
        // 验证 OpenAiProvider 可正常构造（不触发任何网络请求）
        let client = reqwest::Client::new();
        let cfg = LlmConfig::default();
        let _provider = OpenAiProvider::new(client, cfg);
    }

    #[test]
    fn test_openai_provider_implements_trait() {
        // 编译期验证 OpenAiProvider 实现 LlmProvider trait
        // 若 trait 方法签名变更未同步实现，此处编译失败
        fn assert_provider<T: LlmProvider>(_: &T) {}
        let client = reqwest::Client::new();
        let cfg = LlmConfig::default();
        let provider = OpenAiProvider::new(client, cfg);
        assert_provider(&provider);
    }

    #[test]
    fn test_validate_rejects_empty_config() {
        let client = reqwest::Client::new();
        let cfg = LlmConfig::default(); // base_url 与 model 均为空
        let provider = OpenAiProvider::new(client, cfg);
        assert!(provider.validate().is_err(), "空配置应被校验拦截");
    }

    #[test]
    fn test_validate_accepts_complete_config() {
        let client = reqwest::Client::new();
        let cfg = LlmConfig {
            base_url: "https://api.openai.com".into(),
            api_key: "sk-test".into(),
            model: "gpt-4o-mini".into(),
            ..LlmConfig::default()
        };
        let provider = OpenAiProvider::new(client, cfg);
        assert!(provider.validate().is_ok(), "完整配置应通过校验");
    }

    // ===== URL 构造测试 =====

    #[test]
    fn test_build_url_without_v1() {
        assert_eq!(
            build_chat_url("https://api.openai.com"),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_build_url_with_v1() {
        assert_eq!(
            build_chat_url("https://api.openai.com/v1"),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_build_url_with_v1_trailing_slash() {
        assert_eq!(
            build_chat_url("https://api.openai.com/v1/"),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_build_url_with_trailing_slash_no_v1() {
        assert_eq!(
            build_chat_url("https://proxy.com/"),
            "https://proxy.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_build_url_no_duplicate_v1() {
        // 核心测试：确保不会出现 /v1/v1/chat/completions
        let url = build_chat_url("https://api.openai.com/v1");
        assert!(!url.contains("/v1/v1/"), "URL 不应包含重复的 /v1/: {url}");
    }

    // ===== SSE 流式解析测试 =====

    #[test]
    fn test_parse_sse_line_done() {
        // [DONE] 标记流结束（含/不含空格均应识别）
        assert!(parse_sse_line("data: [DONE]").is_none());
        assert!(parse_sse_line("data:[DONE]").is_none());
    }

    #[test]
    fn test_parse_sse_line_delta() {
        let line = r#"data: {"choices":[{"delta":{"content":"你好"}}]}"#;
        let deltas = parse_sse_line(line).unwrap();
        assert_eq!(deltas, vec!["你好"]);
    }

    #[test]
    fn test_parse_sse_line_empty_content_skipped() {
        // 首个 chunk 通常仅含 role，无 content，应返回空 Vec
        let line = r#"data: {"choices":[{"delta":{"role":"assistant"}}]}"#;
        let deltas = parse_sse_line(line).unwrap();
        assert!(deltas.is_empty());
    }

    #[test]
    fn test_parse_sse_line_non_data_line() {
        // 非 data 行（心跳/注释/空行）→ 空 Vec
        assert!(parse_sse_line(": heartbeat").unwrap().is_empty());
        assert!(parse_sse_line("").unwrap().is_empty());
        assert!(parse_sse_line("event: ping").unwrap().is_empty());
    }

    #[test]
    fn test_parse_sse_line_invalid_json_skipped() {
        // 非法 JSON 不应 panic，静默返回空 Vec
        let deltas = parse_sse_line("data: not json").unwrap();
        assert!(deltas.is_empty());
    }

    #[test]
    fn test_parse_sse_line_multi_choices() {
        // 单行多 choice（理论场景）：全部 content 收集
        let line = r#"data: {"choices":[{"delta":{"content":"a"}},{"delta":{"content":"b"}}]}"#;
        let deltas = parse_sse_line(line).unwrap();
        assert_eq!(deltas, vec!["a", "b"]);
    }
}
