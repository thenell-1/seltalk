// TODO 人工审查点：1.默认值合理性 2.设置键名一致性 3.魔法数字常量化
// NOTE 全局默认值与设置键名常量；运行时可被 DB settings 覆盖
use serde::{Deserialize, Serialize};

// ===== 默认值常量 =====
/// 默认全局热键
pub const DEFAULT_HOTKEY: &str = "Ctrl+Shift+Space";
/// 默认候选条数
pub const DEFAULT_CANDIDATE_COUNT: u32 = 3;
/// 默认逐字输入最小延迟（毫秒）
pub const DEFAULT_TYPE_MIN_MS: u64 = 30;
/// 默认逐字输入最大延迟（毫秒）
pub const DEFAULT_TYPE_MAX_MS: u64 = 120;
/// 默认悬浮窗宽度
pub const DEFAULT_FLOAT_W: u32 = 420;
/// 默认悬浮窗高度
pub const DEFAULT_FLOAT_H: u32 = 360;
/// 默认悬浮窗置顶
pub const DEFAULT_FLOAT_ALWAYS_ON_TOP: bool = true;
/// 默认悬浮窗样式预设（compact / standard / loose）
pub const DEFAULT_FLOAT_STYLE_PRESET: &str = "standard";
/// LLM 请求超时（秒）
pub const DEFAULT_LLM_TIMEOUT_SECS: u64 = 30;
/// LLM 默认温度（P0 调优：0.8 → 0.6，减少冗长发散以加快输出）
pub const DEFAULT_LLM_TEMPERATURE: f64 = 0.6;
/// LLM 默认最大 token
pub const DEFAULT_LLM_MAX_TOKENS: u32 = 1024;
/// LLM 流式输出默认开关（true=流式，首字延迟从总生成时间降到首 token 时间）
pub const DEFAULT_LLM_STREAM_ENABLED: bool = true;
/// 候选 token 估算系数：每条约 80 字 + 100 余量，用于 max_tokens 动态计算
pub const LLM_TOKENS_PER_CANDIDATE: u32 = 80;
pub const LLM_TOKENS_MARGIN: u32 = 100;
/// 日志保留天数
pub const LOG_KEEP_DAYS: u64 = 7;
/// 任务锁看门狗强制释放阈值（秒）
pub const TASK_LOCK_WATCHDOG_SECS: u64 = 60;

// ===== 设置键名（settings 表 KV） =====
pub const KEY_HOTKEY: &str = "hotkey";
pub const KEY_CANDIDATE_COUNT: &str = "candidate_count";
pub const KEY_TYPE_MIN_MS: &str = "type_min_ms";
pub const KEY_TYPE_MAX_MS: &str = "type_max_ms";
pub const KEY_FLOAT_W: &str = "float_w";
pub const KEY_FLOAT_H: &str = "float_h";
pub const KEY_FLOAT_ALWAYS_ON_TOP: &str = "float_always_on_top";
pub const KEY_LLM_BASE_URL: &str = "llm_base_url";
pub const KEY_LLM_API_KEY: &str = "llm_api_key";
pub const KEY_LLM_MODEL: &str = "llm_model";
pub const KEY_LLM_TEMPERATURE: &str = "llm_temperature";
pub const KEY_LLM_MAX_TOKENS: &str = "llm_max_tokens";
/// LLM 流式输出开关（"true"/"false"），控制是否启用 SSE 流式生成
pub const KEY_LLM_STREAM_ENABLED: &str = "llm_stream_enabled";
pub const KEY_BLACKLIST: &str = "blacklist";
pub const KEY_FLOAT_STYLE_PRESET: &str = "float_style_preset";
/// 开机自启开关（"true"/"false"）
pub const KEY_AUTOSTART: &str = "autostart";
/// 默认开机自启状态（PRD 要求默认关闭）
pub const DEFAULT_AUTOSTART: bool = false;

/// 运行时常用配置缓存（热键 + 打字速度 + 悬浮窗尺寸/置顶）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub hotkey: String,
    pub candidate_count: u32,
    pub type_min_ms: u64,
    pub type_max_ms: u64,
    pub float_w: u32,
    pub float_h: u32,
    pub float_always_on_top: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            hotkey: DEFAULT_HOTKEY.to_string(),
            candidate_count: DEFAULT_CANDIDATE_COUNT,
            type_min_ms: DEFAULT_TYPE_MIN_MS,
            type_max_ms: DEFAULT_TYPE_MAX_MS,
            float_w: DEFAULT_FLOAT_W,
            float_h: DEFAULT_FLOAT_H,
            float_always_on_top: DEFAULT_FLOAT_ALWAYS_ON_TOP,
        }
    }
}
