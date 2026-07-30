// TODO 人工审查点：1.纯读不破坏剪贴板 2.无文本时返回空串 3.初始化失败处理
// NOTE 剪贴板入口：纯读取文本（get_text 不修改剪贴板内容，无需备份/还原）
use clipboard_rs::{Clipboard, ClipboardContext};

use crate::error::{AppError, AppResult};

/// 读取剪贴板纯文本
///
/// get_text 为纯读操作（OpenClipboard → GetClipboardData → CloseClipboard），
/// 不修改剪贴板内容，因此无需全格式备份/还原，避免富文本场景下的性能开销。
///
/// 注意：剪贴板存放图片/文件等非文本内容时，`get_text` 返回 `Err`，
/// 属正常情况而非错误，建议调用方使用 [`read_text_or_empty`]。
pub fn read_text() -> AppResult<String> {
    let ctx = new_ctx()?;
    ctx.get_text()
        .map_err(|e| AppError::Clipboard(format!("读取文本失败: {e}")))
}

/// 读取剪贴板纯文本；无文本或读取失败时返回空串（不视作错误）
///
/// 剪贴板可能存放图片/文件等非文本内容，此时 `get_text` 返回 `Err`，
/// 属正常情况，统一返回空串，由调用方按"为空"静默处理。
/// 这与原 `backup_and_read_text` 的宽松语义一致，避免无文本时误报错误。
pub fn read_text_or_empty() -> String {
    match read_text() {
        Ok(t) => t,
        Err(e) => {
            tracing::debug!("剪贴板无文本或读取失败: {e}");
            String::new()
        }
    }
}

fn new_ctx() -> AppResult<ClipboardContext> {
    ClipboardContext::new().map_err(|e| AppError::Clipboard(format!("剪贴板初始化失败: {e}")))
}
