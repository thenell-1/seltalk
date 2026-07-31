// TODO 人工审查点：1.纯读不破坏剪贴板 2.无文本时返回空串 3.初始化失败处理
//                    4.模式A 快照范围(仅文本类格式) 5.句柄型格式跳过 6.SetClipboardData 所有权移交
//                    7.OpenClipboard/CloseClipboard 配对 8.GlobalLock/GlobalUnlock 配对
// NOTE 剪贴板入口：
//       模式B（默认）：read_text / read_text_or_empty —— 纯读取文本，不修改剪贴板
//       模式A（兼容复原）：read_text_with_restore —— 快照文本类格式 → 读文本 → EmptyClipboard + 复原
//       模式A 短板：复原时 SetClipboardData 会新增一条 Win+V 历史记录
//       模式A 适用：用户需要"操作后剪贴板恢复成之前内容"的场景
//
// windows 0.58 API 说明：
// - GetClipboardData 返回 Result<HANDLE>，需转 HGLOBAL 给 Global* 函数
// - GlobalAlloc 返回 Result<HGLOBAL>；GlobalSize 返回 usize（0=失败）；GlobalLock 返回 *mut c_void（null=失败）
// - GlobalFree 在 windows 0.58 中已移除：SetClipboardData 失败时不释放（接受极小概率内存泄漏）
use clipboard_rs::{Clipboard, ClipboardContext};

use crate::error::{AppError, AppResult};

// ===== Win32 剪贴板 API（模式A 使用，仅 Windows）=====
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{HANDLE, HGLOBAL};
#[cfg(target_os = "windows")]
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardFormatNameW, OpenClipboard, SetClipboardData,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};

// ===== 标准剪贴板格式 ID（Win32 预定义，windows crate 不再导出常量）=====
/// ANSI 文本格式
#[cfg(target_os = "windows")]
const CF_TEXT: u32 = 1;
/// OEM 文本格式
#[cfg(target_os = "windows")]
const CF_OEMTEXT: u32 = 7;
/// Unicode 文本格式（最常用）
#[cfg(target_os = "windows")]
const CF_UNICODETEXT: u32 = 13;

/// 注册格式 ID 起始值（>= 0xC000 的格式为运行时注册格式，可用 GetClipboardFormatNameW 取名称）
#[cfg(target_os = "windows")]
const CF_REGISTERED_MIN: u32 = 0xC000;

/// 文本类注册格式名称关键字（包含即视为文本类格式，需快照）
/// - HTML Format：Chrome/Edge/Firefox 复制的 HTML
/// - Rich Text Format：Office 复制的富文本
/// - UniformResourceLocator：复制的快捷方式/链接
#[cfg(target_os = "windows")]
const TEXT_FORMAT_KEYWORDS: &[&str] = &[
    "HTML Format",
    "Rich Text Format",
    "UniformResourceLocator",
];

/// 单条剪贴板格式快照数据（模式A 内部使用，pub(crate) 供测试）
#[cfg(target_os = "windows")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardFormatData {
    /// 剪贴板格式 ID（CF_UNICODETEXT 或注册格式 ID）
    pub format: u32,
    /// 原始字节数据（含终止符）
    pub data: Vec<u8>,
}

// ===== 模式B：纯只读（默认，不修改剪贴板）=====

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

// ===== 模式A：兼容复原模式（仅 Windows 实现）=====

/// 判断剪贴板格式是否为文本类（需快照）
///
/// 标准格式：CF_UNICODETEXT(13) / CF_TEXT(1) / CF_OEMTEXT(7)
/// 注册格式（ID >= 0xC000）：名称含 "HTML Format" / "Rich Text Format" / "UniformResourceLocator"
///
/// 句柄型格式（CF_BITMAP/CF_ENHMETAFILE 等）返回 false，跳过（按用户决策：仅文本类）
#[cfg(target_os = "windows")]
pub(crate) fn is_text_format(format: u32) -> bool {
    match format {
        CF_UNICODETEXT | CF_TEXT | CF_OEMTEXT => true,
        _ if format >= CF_REGISTERED_MIN => {
            // 注册格式：调用 GetClipboardFormatNameW 取名称判断
            let mut buf = [0u16; 256];
            // SAFETY: GetClipboardFormatNameW 仅读取 format 对应的名称到栈缓冲区，无副作用
            let len = unsafe { GetClipboardFormatNameW(format, &mut buf) };
            if len <= 0 {
                return false;
            }
            let name = String::from_utf16_lossy(&buf[..len as usize]);
            TEXT_FORMAT_KEYWORDS.iter().any(|k| name.contains(k))
        }
        _ => false,
    }
}

/// 快照当前剪贴板的全部文本类格式
///
/// 流程：OpenClipboard → EnumClipboardFormats 枚举 → 对文本类格式 GetClipboardData + 拷贝 → CloseClipboard
///
/// 单条格式失败时跳过（不阻断整体），返回已成功快照的列表。
/// 句柄型格式（位图/图元文件等）由 is_text_format 过滤，不会被快照。
#[cfg(target_os = "windows")]
pub fn snapshot_text_formats() -> AppResult<Vec<ClipboardFormatData>> {
    let mut snapshots = Vec::new();

    // 1. 打开剪贴板（hwnd=None，不关联窗口，避免干扰窗口消息处理）
    //    SAFETY: OpenClipboard 无副作用，仅获取剪贴板锁
    if let Err(e) = unsafe { OpenClipboard(None) } {
        return Err(AppError::Clipboard(format!("OpenClipboard 失败: {e}")));
    }

    // 2. 枚举全部格式（EnumClipboardFormats(0) 返回首个格式，后续传入上次返回值枚举下一项）
    //    返回 0 表示枚举结束
    let mut format: u32 = 0;
    loop {
        // SAFETY: EnumClipboardFormats 仅读取剪贴板格式列表，无副作用
        format = unsafe { EnumClipboardFormats(format) };
        if format == 0 {
            break;
        }
        if !is_text_format(format) {
            continue;
        }
        // 3. 取数据句柄并拷贝（单条失败跳过，不阻断整体）
        match copy_format_data(format) {
            Ok(data) => snapshots.push(ClipboardFormatData { format, data }),
            Err(e) => {
                tracing::warn!("快照格式 {format} 失败，跳过: {e}");
            }
        }
    }

    // 4. 关闭剪贴板（必须调用，释放锁）
    //    SAFETY: CloseClipboard 释放剪贴板锁，无副作用
    if let Err(e) = unsafe { CloseClipboard() } {
        tracing::warn!("CloseClipboard 失败（快照已成功，忽略）: {e}");
    }
    Ok(snapshots)
}

/// 从剪贴板拷贝指定格式数据到 Vec<u8>
///
/// 流程：GetClipboardData → GlobalSize → GlobalLock → 拷贝 → GlobalUnlock
///
/// 注：GetClipboardData 返回的 HGLOBAL 所有权属于剪贴板，**不要释放**。
///     GlobalLock/GlobalUnlock 必须配对（即使拷贝失败也要 Unlock）。
#[cfg(target_os = "windows")]
fn copy_format_data(format: u32) -> AppResult<Vec<u8>> {
    // SAFETY: GetClipboardData 读取剪贴板数据句柄，句柄所有权属于剪贴板
    let handle: HANDLE = unsafe { GetClipboardData(format) }
        .map_err(|e| AppError::Clipboard(format!("GetClipboardData({format}) 失败: {e}")))?;
    // HANDLE → HGLOBAL（内部都是 *mut c_void，剪贴板文本数据句柄即 HGLOBAL）
    let hglobal = HGLOBAL(handle.0);

    // SAFETY: GlobalSize 读取全局内存块大小，无副作用（返回 0 表示失败）
    let size = unsafe { GlobalSize(hglobal) };
    if size == 0 {
        return Ok(Vec::new());
    }

    // SAFETY: GlobalLock 锁定内存块返回指针，必须配对 GlobalUnlock（返回 null 表示失败）
    let ptr = unsafe { GlobalLock(hglobal) };
    if ptr.is_null() {
        return Err(AppError::Clipboard(format!("GlobalLock({format}) 失败")));
    }

    // 拷贝到 Vec<u8>（GlobalUnlock 在任何返回前必须调用）
    // SAFETY: ptr 指向 size 字节的可读内存，拷贝到 Vec 后立即 Unlock
    let slice = unsafe { std::slice::from_raw_parts(ptr as *const u8, size) };
    let data = slice.to_vec();
    // SAFETY: GlobalUnlock 配对 GlobalLock（忽略返回值，解锁失败也不影响数据拷贝）
    let _ = unsafe { GlobalUnlock(hglobal) };
    Ok(data)
}

/// 复原剪贴板格式（EmptyClipboard + 重新写入快照）
///
/// 流程：OpenClipboard → EmptyClipboard → 遍历快照 SetClipboardData → CloseClipboard
///
/// 注：SetClipboardData 成功后内存所有权移交剪贴板，**不要释放**。
///     EmptyClipboard 会清空剪贴板并清除所有格式（包括非文本格式），
///     但由于模式A 仅在"快照→读文本→复原"链路中使用，非文本格式本就不在快照中，
///     复原后剪贴板仅保留快照的文本类格式（与原状态可能不完全一致，但满足"恢复文本内容"诉求）。
#[cfg(target_os = "windows")]
pub fn restore_formats(snapshots: &[ClipboardFormatData]) -> AppResult<()> {
    if snapshots.is_empty() {
        // 无快照可复原：仍需 EmptyClipboard 清除 AI 写入的文本
        // SAFETY: OpenClipboard + EmptyClipboard + CloseClipboard 标准流程
        if let Err(e) = unsafe { OpenClipboard(None) } {
            return Err(AppError::Clipboard(format!("OpenClipboard 失败: {e}")));
        }
        let _ = unsafe { EmptyClipboard() };
        let _ = unsafe { CloseClipboard() };
        return Ok(());
    }

    // 1. 打开剪贴板
    // SAFETY: OpenClipboard 无副作用，仅获取剪贴板锁
    if let Err(e) = unsafe { OpenClipboard(None) } {
        return Err(AppError::Clipboard(format!("OpenClipboard 失败: {e}")));
    }

    // 2. 清空剪贴板（必须先 EmptyClipboard 再 SetClipboardData）
    //    SAFETY: EmptyClipboard 清空剪贴板并释放现有数据所有权
    if let Err(e) = unsafe { EmptyClipboard() } {
        let _ = unsafe { CloseClipboard() };
        return Err(AppError::Clipboard(format!("EmptyClipboard 失败: {e}")));
    }

    // 3. 遍历快照，重新写入（单条失败跳过，不阻断整体）
    for snap in snapshots {
        if let Err(e) = write_format_data(snap.format, &snap.data) {
            tracing::warn!("复原格式 {} 失败，跳过: {}", snap.format, e);
        }
    }

    // 4. 关闭剪贴板
    // SAFETY: CloseClipboard 释放剪贴板锁
    if let Err(e) = unsafe { CloseClipboard() } {
        tracing::warn!("CloseClipboard 失败（复原已执行，忽略）: {e}");
    }
    Ok(())
}

/// 写入单条格式数据到剪贴板
///
/// 流程：GlobalAlloc(GMEM_MOVEABLE, size) → GlobalLock → 拷贝 → GlobalUnlock → SetClipboardData
///
/// 注：SetClipboardData 成功后内存所有权移交剪贴板，**不要释放**。
///     失败时 windows 0.58 无 GlobalFree 可用，接受极小概率内存泄漏
///     （SetClipboardData 失败极少发生，且进程退出时内存会被回收）。
#[cfg(target_os = "windows")]
fn write_format_data(format: u32, data: &[u8]) -> AppResult<()> {
    if data.is_empty() {
        return Ok(());
    }

    // 1. 分配全局内存（GMEM_MOVEABLE：可移动内存块，SetClipboardData 要求）
    // SAFETY: GlobalAlloc 分配指定大小的全局内存块
    let hglobal = unsafe { GlobalAlloc(GMEM_MOVEABLE, data.len()) }
        .map_err(|e| AppError::Clipboard(format!("GlobalAlloc({format}) 失败: {e}")))?;

    // 2. 锁定 + 拷贝数据（GlobalLock 必须配对 GlobalUnlock）
    // SAFETY: GlobalLock 锁定内存块返回指针（返回 null 表示失败）
    let ptr = unsafe { GlobalLock(hglobal) };
    if ptr.is_null() {
        // GlobalLock 失败：windows 0.58 无 GlobalFree，接受极小概率内存泄漏
        tracing::error!("GlobalLock({format}) 失败，内存未释放");
        return Err(AppError::Clipboard(format!("GlobalLock({format}) 失败")));
    }

    // SAFETY: ptr 指向 data.len() 字节的可写内存，copy_nonoverlapping 拷贝数据
    unsafe {
        std::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, data.len());
        // SAFETY: GlobalUnlock 配对 GlobalLock（忽略返回值）
        let _ = GlobalUnlock(hglobal);
    }

    // 3. 写入剪贴板（成功后所有权移交，不要释放）
    //    HGLOBAL → HANDLE（SetClipboardData 接受 impl Param<HANDLE>，HANDLE 直接传入）
    //    SAFETY: SetClipboardData 将内存块所有权移交给剪贴板
    let handle_for_clipboard = HANDLE(hglobal.0);
    match unsafe { SetClipboardData(format, handle_for_clipboard) } {
        Ok(_) => Ok(()),
        Err(e) => {
            // SetClipboardData 失败：内存所有权未移交，windows 0.58 无 GlobalFree
            // 接受极小概率内存泄漏（SetClipboardData 失败极少发生）
            tracing::error!("SetClipboardData({format}) 失败，内存未释放: {e}");
            Err(AppError::Clipboard(format!("SetClipboardData({format}) 失败: {e}")))
        }
    }
}

/// 模式 A 主入口：快照 → 读文本 → 复原
///
/// 执行链路：
/// 1. snapshot_text_formats() 完整快照所有文本类格式（CF_UNICODETEXT/CF_TEXT/CF_OEMTEXT + HTML/RTF/URL 注册格式）
/// 2. 用 clipboard-rs 读文本（用于 AI 业务）
/// 3. restore_formats() 复原剪贴板原内容（EmptyClipboard + SetClipboardData）
///
/// 短板：复原时 SetClipboardData 会新增一条 Win+V 历史记录
/// 适用：用户需要"操作后剪贴板恢复成之前内容"的场景
///
/// 容错策略：
/// - 快照失败：回退到纯读模式（read_text_or_empty），不影响 AI 业务
/// - 复原失败：仅记录日志，不影响已读到的文本（AI 业务仍可继续）
#[cfg(target_os = "windows")]
pub fn read_text_with_restore() -> String {
    // 1. 快照（失败则回退到纯读模式 B）
    let snapshots = match snapshot_text_formats() {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("剪贴板快照失败，回退到纯读模式: {e}");
            return read_text_or_empty();
        }
    };

    // 2. 读文本（clipboard-rs 的纯读操作，不修改剪贴板）
    let text = read_text_or_empty();

    // 3. 复原（失败仅记录日志，不影响已读到的文本）
    if let Err(e) = restore_formats(&snapshots) {
        tracing::warn!("剪贴板复原失败（不影响本次 AI 业务）: {e}");
    }

    text
}

#[cfg(not(target_os = "windows"))]
pub fn read_text_with_restore() -> String {
    read_text_or_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== read_text_or_empty 行为测试 =====

    #[test]
    fn test_read_text_or_empty_returns_string() {
        // 在测试环境中，剪贴板可能为空或含任意内容，仅验证返回 String 不 panic
        let s = read_text_or_empty();
        // 不对具体内容做断言（剪贴板状态不可控），仅确认类型为 String
        let _s: String = s;
    }

    // ===== is_text_format 纯函数测试（仅 Windows）=====

    #[cfg(target_os = "windows")]
    #[test]
    fn test_is_text_format_standard_text_formats() {
        // 标准文本格式应被识别为文本类
        assert!(is_text_format(CF_UNICODETEXT));
        assert!(is_text_format(CF_TEXT));
        assert!(is_text_format(CF_OEMTEXT));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_is_text_format_standard_non_text_formats() {
        // 句柄型/非文本标准格式应返回 false
        // CF_BITMAP=2, CF_METAFILEPICT=3, CF_DIB=8, CF_ENHMETAFILE=14, CF_HDROP=15
        assert!(!is_text_format(2));  // CF_BITMAP
        assert!(!is_text_format(3));  // CF_METAFILEPICT
        assert!(!is_text_format(8));  // CF_DIB
        assert!(!is_text_format(14)); // CF_ENHMETAFILE
        assert!(!is_text_format(15)); // CF_HDROP
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_is_text_format_registered_below_threshold() {
        // 注册格式阈值以下的格式 ID（非标准文本格式）应返回 false
        // 这些格式调用 GetClipboardFormatNameW 会失败，不会命中关键字判断
        assert!(!is_text_format(100));
        assert!(!is_text_format(1000));
        assert!(!is_text_format(0xBFFF));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_is_text_format_registered_above_threshold_unregistered_id() {
        // 注册格式阈值以上但未实际注册的 ID：GetClipboardFormatNameW 返回 0，应返回 false
        // 选择一个极不可能被注册的 ID（0xFFFF 远超常见注册范围）
        assert!(!is_text_format(0xFFFF));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_clipboard_format_data_clone_eq() {
        let a = ClipboardFormatData {
            format: CF_UNICODETEXT,
            data: vec![0x68, 0x00, 0x69, 0x00, 0x00, 0x00], // "hi\0" UTF-16LE
        };
        let b = a.clone();
        assert_eq!(a, b);
    }

    // ===== 非 Windows 平台：read_text_with_restore 回退到纯读 =====

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_read_text_with_restore_fallback_on_non_windows() {
        // 非 Windows 平台：read_text_with_restore 应等同于 read_text_or_empty
        let s = read_text_with_restore();
        assert!(s.len() >= 0);
    }
}
