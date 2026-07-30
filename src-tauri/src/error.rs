// TODO 人工审查点：1.错误变体覆盖 2.thiserror 派生 3.命令侧字符串转换
// NOTE 统一错误类型：所有模块返回 AppResult<T>，命令层用 err_to_string 转 String 给前端
use thiserror::Error;

/// 应用统一错误类型
#[derive(Debug, Error)]
pub enum AppError {
    #[error("数据库错误: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("剪贴板错误: {0}")]
    Clipboard(String),
    #[error("LLM 请求错误: {0}")]
    Llm(String),
    #[error("输入模拟错误: {0}")]
    Input(String),
    #[error("热键错误: {0}")]
    Hotkey(String),
    #[error("窗口错误: {0}")]
    Window(String),
    #[error("配置错误: {0}")]
    Config(String),
    #[error("任务忙，已忽略重复触发")]
    Busy,
    #[error("输入已中断")]
    Interrupted,
    #[error("序列化错误: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

pub type AppResult<T> = Result<T, AppError>;

/// 命令层错误转字符串（前端消费）
pub fn err_to_string<T, E: std::fmt::Display>(res: Result<T, E>) -> Result<T, String> {
    res.map_err(|e| e.to_string())
}
