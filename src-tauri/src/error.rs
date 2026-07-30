// NOTE 统一错误类型，便于 commands 返回标准化错误信息
// 包含 PRD 11.3 错误码定义，支持 error 事件推送（错误码 + 中文提示）

use serde::Serialize;

/// 应用错误类型
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("配置错误：{0}")]
    Config(String),

    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),

    #[error("序列化错误：{0}")]
    Serde(#[from] serde_json::Error),

    #[error("Tauri 错误：{0}")]
    Tauri(#[from] tauri::Error),
}

/// 序列化为前端可识别的字符串
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

/// 结果类型别名
pub type AppResult<T> = Result<T, AppError>;

// ============================================================================
// PRD 11.3 错误码定义
// ============================================================================

/// 错误码与中文提示（PRD 11.3）
/// silent=true 的错误前端静默处理，不弹窗提示
#[derive(Debug, Clone, Serialize)]
pub struct ErrorInfo {
    pub code: &'static str,
    pub message: &'static str,
    pub silent: bool,
}

/// 根据错误信息推断错误码（PRD 11.3 对照表）
/// 返回 (错误码, 中文提示, 是否静默)
pub fn classify_error(err: &AppError) -> ErrorInfo {
    let msg = err.to_string();

    // E001: 非微信/QQ 窗口触发（静默）
    if msg.contains("非微信") || msg.contains("非目标") || msg.contains("当前前台窗口") {
        return ErrorInfo {
            code: "E001",
            message: "非微信/QQ窗口",
            silent: true,
        };
    }

    // E003: Ctrl+C 备选也失败
    if msg.contains("Ctrl+C") || msg.contains("未捕获到文本") || msg.contains("未捕获到选中文本") {
        return ErrorInfo {
            code: "E003",
            message: "文本捕获失败，请重新选中文本",
            silent: false,
        };
    }

    // E004: 选中文本为空或过短（静默）
    if msg.contains("空") && (msg.contains("文本") || msg.contains("输入")) {
        return ErrorInfo {
            code: "E004",
            message: "选中文本为空",
            silent: true,
        };
    }

    // E005: LLM 云端调用超时
    if msg.contains("超时") || msg.contains("timed out") || msg.contains("timeout") {
        return ErrorInfo {
            code: "E005",
            message: "AI 响应超时，请稍后重试",
            silent: false,
        };
    }

    // E006: LLM 云端余额不足
    if msg.contains("余额") || msg.contains("insufficient") || msg.contains("quota") {
        return ErrorInfo {
            code: "E006",
            message: "云端模型余额不足",
            silent: false,
        };
    }

    // E007: LLM 本地未启动
    if msg.contains("本地") && (msg.contains("未启动") || msg.contains("连接")) {
        return ErrorInfo {
            code: "E007",
            message: "本地模型未启动，请检查 Ollama",
            silent: false,
        };
    }

    // E008: LLM 全部失败（含请求失败、状态码错误等）
    if msg.contains("LLM") || msg.contains("云端") || msg.contains("请求") || msg.contains("API") {
        return ErrorInfo {
            code: "E008",
            message: "AI 生成失败，请稍后重试",
            silent: false,
        };
    }

    // E010: 窗口类名未识别
    if msg.contains("类名") || msg.contains("窗口") {
        return ErrorInfo {
            code: "E010",
            message: "当前窗口暂不支持，可在设置中添加",
            silent: false,
        };
    }

    // E014: 配置保存失败
    if msg.contains("配置") && (msg.contains("保存") || msg.contains("写入") || msg.contains("权限")) {
        return ErrorInfo {
            code: "E014",
            message: "配置保存失败，请检查文件权限",
            silent: false,
        };
    }

    // 默认：未知错误
    ErrorInfo {
        code: "E000",
        message: "操作失败，请稍后重试",
        silent: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_non_wechat_window() {
        let err = AppError::Config("当前前台窗口非微信/QQ".to_string());
        let info = classify_error(&err);
        assert_eq!(info.code, "E001");
        assert!(info.silent);
    }

    #[test]
    fn test_classify_capture_failed() {
        let err = AppError::Config("Ctrl+C 也未捕获到文本".to_string());
        let info = classify_error(&err);
        assert_eq!(info.code, "E003");
        assert!(!info.silent);
    }

    #[test]
    fn test_classify_llm_timeout() {
        let err = AppError::Config("请求云端 LLM 失败: operation timed out".to_string());
        let info = classify_error(&err);
        assert_eq!(info.code, "E005");
        assert!(!info.silent);
    }

    #[test]
    fn test_classify_llm_general_failure() {
        let err = AppError::Config("云端 LLM 返回错误状态: 500".to_string());
        let info = classify_error(&err);
        assert_eq!(info.code, "E008");
        assert!(!info.silent);
    }

    #[test]
    fn test_classify_empty_text() {
        let err = AppError::Config("输入文本不能为空".to_string());
        let info = classify_error(&err);
        assert_eq!(info.code, "E004");
        assert!(info.silent);
    }

    #[test]
    fn test_classify_config_save() {
        let err = AppError::Config("配置保存失败，权限不足".to_string());
        let info = classify_error(&err);
        assert_eq!(info.code, "E014");
        assert!(!info.silent);
    }
}
