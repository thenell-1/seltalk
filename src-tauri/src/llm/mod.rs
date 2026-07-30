// TODO 人工审查点：1.锁不跨 await 2.配置缺失处理 3.候选切分 4.max_tokens 动态计算 5.流式 emit 失败容错
// NOTE LLM 入口：渲染模板→请求→切分候选（含流式分支）；cfg/template 由 orchestrator 从缓存提供
pub mod client;
pub mod prompt;
pub mod types;

use std::collections::HashMap;

use tauri::{AppHandle, Emitter, Manager};

use crate::config::{
    DEFAULT_LLM_MAX_TOKENS, DEFAULT_LLM_STREAM_ENABLED, DEFAULT_LLM_TEMPERATURE, KEY_LLM_API_KEY,
    KEY_LLM_BASE_URL, KEY_LLM_MAX_TOKENS, KEY_LLM_MODEL, KEY_LLM_STREAM_ENABLED,
    KEY_LLM_TEMPERATURE, LLM_TOKENS_MARGIN, LLM_TOKENS_PER_CANDIDATE,
};
use crate::db::settings;
use crate::error::AppResult;
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
    let full = client::chat_stream(&state.http, cfg, &req, |delta| {
        // 推送 delta 到悬浮窗（失败不影响流式接收）
        let _ = app_for_cb.emit_to(window::FLOAT_LABEL, "candidates-stream", delta.to_string());
    })
    .await?;

    let candidates = split_candidates(&full);
    tracing::info!("流式生成 {} 条候选", candidates.len());
    Ok(candidates)
}

/// 从 settings 表加载 LLM 配置（供 orchestrator 缓存刷新使用）
pub fn load_llm_config(db: &rusqlite::Connection) -> AppResult<LlmConfig> {
    let get = |k: &str| settings::get_setting(db, k).ok().flatten();
    Ok(LlmConfig {
        base_url: get(KEY_LLM_BASE_URL).unwrap_or_default(),
        api_key: get(KEY_LLM_API_KEY).unwrap_or_default(),
        model: get(KEY_LLM_MODEL).unwrap_or_default(),
        temperature: get(KEY_LLM_TEMPERATURE)
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_LLM_TEMPERATURE),
        max_tokens: get(KEY_LLM_MAX_TOKENS)
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_LLM_MAX_TOKENS),
        stream_enabled: get(KEY_LLM_STREAM_ENABLED)
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(DEFAULT_LLM_STREAM_ENABLED),
    })
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
