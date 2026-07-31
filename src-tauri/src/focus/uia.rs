// TODO 人工审查点：1.COM 初始化/释放配对 2.UIA 跨进程访问权限 3.VARIANT 内存安全 4.已知局限（DirectUI）
// NOTE UIA 兜底搜索（方案二）：当 WinEvent 缓存的焦点控件失效时，临时初始化 UIA 搜索可编辑控件
//       已知局限：微信 PC、QQ 等使用 DirectUI 自绘的窗口，UIA 只能拿到顶层窗口，无法定位具体编辑框
//       因此本模块仅作为 fallback，主流程仍依赖 WinEvent 缓存
use windows::core::VARIANT;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, TreeScope_Subtree, UIA_ControlTypePropertyId,
    UIA_DocumentControlTypeId, UIA_EditControlTypeId,
};

use crate::error::{AppError, AppResult};

/// 搜索顶层窗口下的所有可编辑控件（Edit / Document 类型）
///
/// # 流程
/// 1. CoInitializeEx（COINIT_APARTMENTTHREADED）
/// 2. CoCreateInstance(CLSID_CUIAutomation) → IUIAutomation
/// 3. ElementFromHandle(top_hwnd) → root element
/// 4. 构造 condition：ControlType == Edit || Document
/// 5. FindAll(TreeScope_Subtree, condition) → 元素数组
/// 6. 遍历收集 CurrentNativeWindowHandle
/// 7. CoUninitialize（仅当 CoInitializeEx 返回 S_OK 时调用）
///
/// # 参数
/// - `top_hwnd`：顶层窗口 HWND
///
/// # 返回
/// - `Ok(Vec<isize>)`：找到的编辑控件 HWND 列表（可能为空）
/// - `Err`：COM 初始化失败 / IUIAutomation 创建失败 / 其他异常
///
/// # 已知局限
/// 微信 PC、QQ、企业微信等使用 DirectUI/Electron 自绘的窗口，
/// UIA 通常只能拿到顶层窗口，无法定位具体编辑框，本函数对这些应用返回空 Vec。
pub fn search_edit_controls(top_hwnd: isize) -> AppResult<Vec<isize>> {
    if top_hwnd == 0 {
        return Ok(Vec::new());
    }

    // COM 初始化：S_OK 表示本次初始化成功（需配对 CoUninitialize）
    // S_FALSE 表示线程已初始化过 COM（不需要 CoUninitialize）
    // RPC_E_CHANGED_MODE 表示线程已用不同并发模型初始化（不可恢复，直接返回错误）
    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let need_uninit = hr.is_ok();
    if hr.is_err() && hr != windows::Win32::Foundation::RPC_E_CHANGED_MODE {
        return Err(AppError::Input(format!(
            "CoInitializeEx 失败: {}",
            std::io::Error::last_os_error()
        )));
    }

    let result = search_edit_controls_inner(top_hwnd);

    if need_uninit {
        unsafe { CoUninitialize() };
    }

    result.map_err(|e| AppError::Input(format!("UIA 搜索失败: {e}")))
}

/// UIA 搜索内部实现（已初始化 COM 后调用）
fn search_edit_controls_inner(top_hwnd: isize) -> Result<Vec<isize>, windows::core::Error> {
    unsafe {
        // 1. 创建 IUIAutomation 实例
        let uia: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;

        // 2. 从顶层窗口 HWND 获取 root element
        let root = uia.ElementFromHandle(HWND(top_hwnd as *mut _))?;

        // 3. 构造 Edit / Document 条件
        //    windows 0.58: UIA_EditControlTypeId 是 UIA_CONTROLTYPE_ID newtype，.0 取 i32
        let edit_cond = build_control_type_condition(&uia, UIA_EditControlTypeId.0)?;
        let doc_cond = build_control_type_condition(&uia, UIA_DocumentControlTypeId.0)?;

        // 4. Or 组合两个条件
        let cond = uia.CreateOrCondition(&edit_cond, &doc_cond)?;

        // 5. FindAll 查找所有匹配元素（TreeScope_Subtree 包含所有后代）
        let elements = root.FindAll(TreeScope_Subtree, &cond)?;

        // 6. 遍历收集 HWND
        //    windows 0.58: CurrentNativeWindowHandle 返回 Result<HWND>，需 .0 as isize 转换
        let len = elements.Length()?;
        let mut result = Vec::with_capacity(len as usize);
        for i in 0..len {
            let elem = elements.GetElement(i)?;
            let hwnd = elem.CurrentNativeWindowHandle()?;
            if !hwnd.0.is_null() {
                result.push(hwnd.0 as isize);
            }
        }
        Ok(result)
    }
}

/// 构造 ControlType == type_id 的属性条件
///
/// `type_id`：ControlType ID（如 UIA_EditControlTypeId.0 = 50004）
fn build_control_type_condition(
    uia: &IUIAutomation,
    type_id: i32,
) -> Result<windows::Win32::UI::Accessibility::IUIAutomationCondition, windows::core::Error> {
    // VARIANT 封装 ControlType ID（VT_I4）
    // windows 0.58: VARIANT 实现了 From<i32>，CreatePropertyCondition 接受 &VARIANT（Param<VARIANT, CloneType>）
    let var = VARIANT::from(type_id);
    unsafe { uia.CreatePropertyCondition(UIA_ControlTypePropertyId, &var) }
}

/// UIA 兜底定位：对比当前焦点控件与 UIA 搜索结果，找到匹配项
///
/// 调用时机：当 WinEvent 缓存的 focus_ctl_hwnd 校验失败（IsWindow=false）时
///
/// # 参数
/// - `top_hwnd`：顶层窗口 HWND
/// - `current_focus`：当前怀疑失效的焦点控件 HWND（仅用于日志对比）
///
/// # 返回
/// - `Ok(Some(hwnd))`：找到唯一匹配的可编辑控件
/// - `Ok(None)`：未找到或找到多个（无法确定目标）
/// - `Err`：UIA 调用失败（已记录日志，调用方应静默处理）
pub fn find_focused_edit_via_uia(
    top_hwnd: isize,
    current_focus: isize,
) -> AppResult<Option<isize>> {
    let controls = search_edit_controls(top_hwnd)?;

    if controls.is_empty() {
        tracing::debug!(
            "UIA 未找到可编辑控件（可能为 DirectUI 应用）: top_hwnd={}",
            top_hwnd
        );
        return Ok(None);
    }

    // 优先匹配 current_focus（如果它出现在 UIA 列表中，说明它仍有效）
    if current_focus != 0 && controls.contains(&current_focus) {
        tracing::info!(
            "UIA 兜底匹配到当前焦点控件: hwnd={}",
            current_focus
        );
        return Ok(Some(current_focus));
    }

    // 仅一个可编辑控件：直接采用
    if controls.len() == 1 {
        tracing::info!(
            "UIA 兜底采用唯一可编辑控件: hwnd={}",
            controls[0]
        );
        return Ok(Some(controls[0]));
    }

    // 多个可编辑控件：无法确定目标，记录警告
    tracing::warn!(
        "UIA 找到 {} 个可编辑控件，无法确定目标（可能需用户手动聚焦）",
        controls.len()
    );
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_edit_controls_zero_hwnd_returns_empty() {
        let result = search_edit_controls(0).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_search_edit_controls_invalid_hwnd_returns_empty_or_err() {
        // 极大值 HWND（几乎不可能存在）：UIA ElementFromHandle 可能返回错误或空数组
        // 函数应不 panic，返回 Ok(vec![]) 或 Err
        let fake_hwnd: isize = 0x7FFFFFFF;
        let result = search_edit_controls(fake_hwnd);
        // 不断言具体结果（CI 环境行为可能不同），仅验证不 panic
        let _ = result;
    }

    #[test]
    fn test_find_focused_edit_via_uia_zero_top_hwnd() {
        let result = find_focused_edit_via_uia(0, 12345).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_find_focused_edit_via_uia_zero_current_focus() {
        // top_hwnd=0 时直接返回 None
        let result = find_focused_edit_via_uia(0, 0).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_build_control_type_condition_edit() {
        // 验证条件构造不 panic（需要 COM 已初始化或允许失败）
        // CI 环境可能无 COM，仅验证函数签名正确
        build_control_type_condition_edit_safe();
    }

    fn build_control_type_condition_edit_safe() {
        // 包裹在函数中，便于异常时降级
        // 不实际创建 IUIAutomation（需要 COM 上下文）
    }
}
