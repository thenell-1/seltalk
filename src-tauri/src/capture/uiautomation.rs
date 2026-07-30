// TODO 人工审查点：1.uiautomation crate 在新版微信/QQ 的兼容性 2.焦点元素查找性能 3.异常降级逻辑
// NOTE F2 UI Automation 模块：首选方案，通过 COM 接口读取选中文本

use crate::error::{AppError, AppResult};
use uiautomation::UIAutomation;
use uiautomation::controls::ControlType;
use uiautomation::patterns::{UITextPattern, UIValuePattern};

/// 通过 UI Automation 获取前台窗口的选中文本
/// 流程：获取焦点元素 → 尝试 TextPattern.GetSelection → 失败则尝试 ValuePattern
pub fn get_selected_text_via_uia() -> AppResult<Option<String>> {
    let automation = UIAutomation::new()
        .map_err(|e| AppError::Config(format!("初始化 UI Automation 失败: {e}")))?;

    let element = automation
        .get_focused_element()
        .map_err(|e| AppError::Config(format!("获取焦点元素失败: {e}")))?;

    // 方案1：尝试 TextPattern.GetSelection（最准确）
    if let Ok(text_pattern) = element.get_pattern::<UITextPattern>() {
        let selections = text_pattern
            .get_selection()
            .map_err(|e| AppError::Config(format!("获取选区失败: {e}")))?;
        if !selections.is_empty() {
            let text_ranges: Vec<String> = selections
                .iter()
                .filter_map(|r: &uiautomation::patterns::UITextRange| r.get_text(-1).ok())
                .collect();
            let combined = text_ranges.join("");
            if !combined.is_empty() {
                tracing::debug!("UI Automation TextPattern 获取成功: {} 字符", combined.len());
                return Ok(Some(combined));
            }
        }
    }

    // 方案2：尝试 ValuePattern（部分控件不支持 TextPattern）
    if let Ok(value_pattern) = element.get_pattern::<UIValuePattern>() {
        // ValuePattern 没有 GetSelection，但可获取整个值（仅当控件全部选中时有效）
        let value = value_pattern
            .get_value()
            .map_err(|e| AppError::Config(format!("获取值失败: {e}")))?;
        if !value.is_empty() {
            tracing::debug!("UI Automation ValuePattern 获取值: {} 字符", value.len());
            // 注意：ValuePattern 返回的是整个文本，无法区分是否选中
            // 仅在文本较短（≤200字符）时使用，避免误捕获整段文本
            if value.chars().count() <= 200 {
                return Ok(Some(value));
            }
        }
    }

    // 方案3：尝试获取焦点元素的 Name 属性（兜底）
    let name = element
        .get_name()
        .map_err(|e| AppError::Config(format!("获取名称失败: {e}")))?;
    let control_type = element
        .get_control_type()
        .unwrap_or(ControlType::Custom);

    tracing::debug!(
        "UI Automation 无法获取选中文本，焦点元素: name='{}', type={:?}",
        name,
        control_type
    );
    Ok(None)
}
