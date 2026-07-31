// TODO 人工审查点：1.锁不跨 await 2.配置缺失处理 3.候选切分 4.max_tokens 动态计算 5.流式 emit 失败容错
// NOTE LLM 入口：渲染模板→请求→切分候选（含流式分支）；cfg/template 由 orchestrator 从缓存提供
//
// P6.1: 模块结构
//   - provider: LlmProvider trait + OpenAiProvider 实现（核心逻辑所在）
//   - client:   向后兼容 shim，保留原自由函数签名委托给 OpenAiProvider
//   - types:    OpenAI 兼容协议数据类型（含 ts-rs 类型导出）
//   - prompt:  Prompt 模板渲染
//   - error:   LLM 错误格式化（网络/HTTP 状态码）
pub mod client;
pub mod error;
pub mod prompt;
pub mod provider;
pub mod types;

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::config::{LLM_TOKENS_MARGIN, LLM_TOKENS_PER_CANDIDATE};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::text::split::split_candidates;
use crate::window;
use types::{ChatMessage, ChatRequest, LlmConfig};

/// 根据候选数动态计算 max_tokens：每条约 80 字 + 100 余量
///
/// 避免 max_tokens 固定 1024 导致：短回复场景浪费 token、长回复场景可能截断。
/// 候选越多配额越大，候选越少越快返回。
pub fn calc_max_tokens(n: u32) -> u32 {
    n.saturating_mul(LLM_TOKENS_PER_CANDIDATE)
        .saturating_add(LLM_TOKENS_MARGIN)
}

/// 渲染 Prompt 模板（注入 origin/n/words 变量）
fn render_with_vars(template: &str, text: &str, n: u32, words: &str) -> AppResult<String> {
    let mut vars = HashMap::new();
    vars.insert("origin".into(), text.to_string());
    vars.insert("n".into(), n.to_string());
    vars.insert("words".into(), words.to_string());
    prompt::render_template(template, &vars)
}

/// 构造 ChatRequest（max_tokens 动态计算，stream 由调用方设置）
fn build_request(cfg: &LlmConfig, rendered: &str, n: u32) -> ChatRequest {
    ChatRequest {
        model: cfg.model.clone(),
        messages: vec![ChatMessage {
            role: "user".into(),
            content: rendered.into(),
        }],
        temperature: cfg.temperature,
        max_tokens: calc_max_tokens(n),
        n: None,
        stream: None,
    }
}

/// 生成候选回复（非流式）：渲染→请求→切分候选
///
/// `cfg` 与 `template` 由调用方（orchestrator）从运行时缓存提供，
/// 避免每次触发重复读 DB。模板缺省时由调用方保证回退到内置默认。
pub async fn generate_candidates(
    app: &AppHandle,
    text: &str,
    n: u32,
    words: &str,
    cfg: &LlmConfig,
    template: &str,
) -> AppResult<Vec<String>> {
    let rendered = render_with_vars(template, text, n, words)?;
    let req = build_request(cfg, &rendered, n);
    let state = app.state::<AppState>();
    let resp = client::chat(&state.http, cfg, &req).await?;
    // 每个 choice 内可能含多条 --- 分隔的候选，逐个切分
    let mut all = Vec::new();
    for choice in resp.choices {
        all.extend(split_candidates(&choice.message.content));
    }
    tracing::info!("非流式生成 {} 条候选", all.len());
    Ok(all)
}

/// 生成候选回复（流式）：边生成边 emit delta 到悬浮窗，完成后切分候选
///
/// 性能：首字延迟从"整段生成时间"降到"首 token 时间"（通常 <500ms），
/// 悬浮窗渐进显示生成中的原文，完成后切换为切分好的候选列表。
///
/// emit 失败不中断流（悬浮窗可能已被用户关闭），仅影响渐进显示。
pub async fn generate_candidates_stream(
    app: &AppHandle,
    text: &str,
    n: u32,
    words: &str,
    cfg: &LlmConfig,
    template: &str,
) -> AppResult<Vec<String>> {
    let rendered = render_with_vars(template, text, n, words)?;
    let mut req = build_request(cfg, &rendered, n);
    req.stream = Some(true);

    let state = app.state::<AppState>();
    let app_for_cb = app.clone();

    // P1.2：用 select! 包裹流式请求与 interrupt 轮询，实现 ESC 取消
    // 每 50ms 检查一次 interrupt 标志，用户按 ESC 后可在 50ms 内中断 LLM 请求
    let interrupt_ref = &state.interrupt;
    let full = tokio::select! {
        result = client::chat_stream(&state.http, cfg, &req, |delta| {
            // 推送 delta 到悬浮窗（失败不影响流式接收）
            let _ = app_for_cb.emit_to(window::FLOAT_LABEL, "candidates-stream", delta.to_string());
        }) => result?,
        _ = async {
            loop {
                if interrupt_ref.load(Ordering::Relaxed) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        } => {
            tracing::info!("LLM 流式请求已取消（用户 ESC）");
            return Err(AppError::Llm("LLM 请求已取消".into()));
        }
    };

    let candidates = split_candidates(&full);
    tracing::info!("流式生成 {} 条候选", candidates.len());
    Ok(candidates)
}

/// 加载当前生效的 LLM 配置（供 orchestrator 缓存刷新使用）
///
/// 数据源：llm_profiles 表中 is_active=1 的记录。无 active 记录时回退 `LlmConfig::default()`，
/// 保证主链路在未配置时不崩溃（orchestrator 会在调用前校验 base_url/api_key/model 非空）。
pub fn load_llm_config(db: &rusqlite::Connection) -> AppResult<LlmConfig> {
    match crate::db::llm_profiles::llm_profile_get_active(db)? {
        Some(p) => Ok(LlmConfig {
            base_url: p.base_url,
            api_key: p.api_key,
            model: p.model,
            temperature: p.temperature,
            max_tokens: p.max_tokens,
            stream_enabled: p.stream_enabled,
            model_type: p.model_type,
            max_context_length: p.max_context_length,
        }),
        None => Ok(LlmConfig::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calc_max_tokens_basic() {
        // n=3 → 3*80+100 = 340
        assert_eq!(calc_max_tokens(3), 340);
    }

    #[test]
    fn test_calc_max_tokens_single() {
        // n=1 → 1*80+100 = 180
        assert_eq!(calc_max_tokens(1), 180);
    }

    #[test]
    fn test_calc_max_tokens_overflow_safe() {
        // 极大值不溢出（saturating）
        let huge = calc_max_tokens(u32::MAX);
        assert!(huge >= LLM_TOKENS_MARGIN);
    }

    #[test]
    fn test_render_with_vars_substitutes() {
        let r = render_with_vars("回复{{n}}条：{{origin}}", "你好", 3, "词1").unwrap();
        assert_eq!(r, "回复3条：你好");
    }
}
