// TODO 人工审查点：1.invoke 封装错误处理 2.事件监听清理 3.类型安全 4.窗口操作 5.ts-rs 类型同步
// NOTE Tauri API 封装：统一命令调用 + 事件监听 + 窗口操作，前端各组件共用
//       P4.4：所有类型从 src-tauri/bindings/ 自动生成的 .ts 文件 import（cargo test 触发）
//             避免手写 interface 与后端 struct 字段漂移
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

// P4.4：从后端自动生成的 .ts 类型文件导入
import type { AppConfig } from "@bindings/config/AppConfig";
import type { PromptTemplate } from "@bindings/db/PromptTemplate";
import type { ConnectionTestResult } from "@bindings/llm/ConnectionTestResult";
import type { WordEntry } from "@bindings/db/WordEntry";
import type { BatchResult } from "@bindings/db/BatchResult";
import type { WordFreqEntry } from "@bindings/db/WordFreqEntry";
import type { HistoryEntry } from "@bindings/db/HistoryEntry";
import type { WordFreqOverview } from "@bindings/commands/WordFreqOverview";
import type { HistoryListResult } from "@bindings/commands/HistoryListResult";
import type { PinMode } from "@bindings/commands/PinMode";
import type { LlmProfile } from "@bindings/db/LlmProfile";
import type { LlmProfileInput } from "@bindings/db/LlmProfileInput";

// 重新导出所有类型，保持前端调用方式不变（import { AppConfig } from "@/lib/api"）
export type {
  AppConfig,
  PromptTemplate,
  ConnectionTestResult,
  WordEntry,
  BatchResult,
  WordFreqEntry,
  HistoryEntry,
  WordFreqOverview,
  HistoryListResult,
  PinMode,
  LlmProfile,
  LlmProfileInput,
};

/** 事件监听清理函数类型 */
export type UnlistenFn = () => void;

/** 候选数据载荷（Rust 发送到前端） */
export interface CandidatesPayload {
  origin: string;
  candidates: string[];
}

/** 批量导入单条结构（前端专用，无对应后端 struct） */
export interface BatchImportEntry {
  word: string;
  category: string;
}

// ===== 命令调用 =====

/** 用户选中候选 → 逐字输入 */
export function typeCandidate(text: string): Promise<void> {
  return invoke<void>("type_candidate", { text });
}

/** 取消本次会话 */
export function cancel(): Promise<void> {
  return invoke<void>("cancel");
}

/** R 键重新生成候选（用上次过滤后文本 + 更高 temperature 重试） */
export function regenerateCandidates(): Promise<void> {
  return invoke<void>("regenerate_candidates");
}

/** Ctrl+1/2/3 切换 Prompt 模板（0-based 索引），返回切换后的模板名 */
export function switchPromptByIndex(index: number): Promise<string> {
  return invoke<string>("switch_prompt_by_index", { index });
}

/** 循环切换悬浮窗置顶模式（Off → Normal → Temp → Off），返回切换后的模式 */
export function cyclePinMode(): Promise<PinMode> {
  return invoke<PinMode>("cycle_pin_mode");
}

/** 读取当前置顶模式（供悬浮窗初始化图标） */
export function getPinMode(): Promise<PinMode> {
  return invoke<PinMode>("get_pin_mode");
}

/** 设置悬浮窗透明度（钳制到 0.30~1.0 + 持久化到 settings KV） */
export function setFloatOpacity(opacity: number): Promise<void> {
  return invoke<void>("set_float_opacity", { opacity });
}

/** 读取悬浮窗透明度（缺失返回默认 1.0） */
export function getFloatOpacity(): Promise<number> {
  return invoke<number>("get_float_opacity");
}

/** 保存悬浮窗状态 */
export function saveFloatState(
  x: number,
  y: number,
  w: number,
  h: number,
  alwaysOnTop: boolean,
): Promise<void> {
  return invoke<void>("save_float_state", {
    x,
    y,
    w,
    h,
    alwaysOnTop,
  });
}

/** 读取全部设置（KV） */
export function getAllSettings(): Promise<Record<string, string>> {
  return invoke<Record<string, string>>("get_all_settings");
}

/** 写入单个设置项 */
export function setSetting(key: string, value: string): Promise<void> {
  return invoke<void>("set_setting", { key, value });
}

/** 读取应用配置 */
export function getAppConfig(): Promise<AppConfig> {
  return invoke<AppConfig>("get_app_config");
}

/** 更新热键 */
export function updateHotkey(hotkey: string): Promise<void> {
  return invoke<void>("update_hotkey", { hotkey });
}

// ===== Prompt 命令 =====

export function promptList(): Promise<PromptTemplate[]> {
  return invoke<PromptTemplate[]>("prompt_list");
}

export function promptCreate(
  name: string,
  template: string,
  tags?: string,
): Promise<number> {
  return invoke<number>("prompt_create", { name, template, tags: tags ?? null });
}

export function promptUpdate(
  id: number,
  name: string,
  template: string,
  tags?: string,
): Promise<void> {
  return invoke<void>("prompt_update", { id, name, template, tags: tags ?? null });
}

export function promptDelete(id: number): Promise<void> {
  return invoke<void>("prompt_delete", { id });
}

export function promptSetDefault(id: number): Promise<void> {
  return invoke<void>("prompt_set_default", { id });
}

/** 查询全库去重后的标签列表（供前端标签自动补全） */
export function promptAllTags(): Promise<string[]> {
  return invoke<string[]>("prompt_all_tags");
}

// ===== LLM 命令 =====

/** 测试 LLM 连通性，返回结果（ok/延迟/消息） */
export function testLlmConnection(): Promise<ConnectionTestResult> {
  return invoke<ConnectionTestResult>("test_llm_connection");
}

// ===== LLM 配置档案命令 =====

/** 查询全部 LLM 配置档案（active 优先，按更新时间倒序） */
export function llmProfileList(): Promise<LlmProfile[]> {
  return invoke<LlmProfile[]>("llm_profile_list");
}

/** 查询当前生效的 LLM 配置档案 */
export function getActiveLlmProfile(): Promise<LlmProfile | null> {
  return invoke<LlmProfile | null>("get_active_llm_profile");
}

/** 新建 LLM 配置档案并设为当前生效（新建即切换，主链路立即使用） */
export function llmProfileCreate(input: LlmProfileInput): Promise<number> {
  return invoke<number>("llm_profile_create", { input });
}

/** 更新指定 LLM 配置档案（保留 is_active 状态不变） */
export function llmProfileUpdate(id: number, input: LlmProfileInput): Promise<void> {
  return invoke<void>("llm_profile_update", { id, input });
}

/** 删除指定 LLM 配置档案（若删除的是 active，自动提升剩余首条） */
export function llmProfileDelete(id: number): Promise<void> {
  return invoke<void>("llm_profile_delete", { id });
}

/** 将指定 LLM 配置档案设为当前生效（下拉切换：互斥置位，主链路立即使用新配置） */
export function llmProfileSetActive(id: number): Promise<void> {
  return invoke<void>("llm_profile_set_active", { id });
}

// ===== 词库命令 =====

/** 查询词库列表（可选筛选） */
export function wordList(
  search?: string,
  category?: string,
  enabledOnly?: boolean,
): Promise<WordEntry[]> {
  return invoke<WordEntry[]>("word_list", {
    search: search ?? null,
    category: category ?? null,
    enabledOnly: enabledOnly ?? false,
  });
}

/** 新增词条 */
export function wordCreate(word: string, category: string): Promise<number> {
  return invoke<number>("word_create", { word, category });
}

/** 更新词条 */
export function wordUpdate(
  id: number,
  word: string,
  category: string,
): Promise<void> {
  return invoke<void>("word_update", { id, word, category });
}

/** 删除词条 */
export function wordDelete(id: number): Promise<void> {
  return invoke<void>("word_delete", { id });
}

/** 切换词条启禁用 */
export function wordToggleEnable(id: number, enabled: boolean): Promise<void> {
  return invoke<void>("word_toggle_enable", { id, enabled });
}

/** 批量导入词条（重复跳过） */
export function wordBatchImport(
  entries: BatchImportEntry[],
): Promise<BatchResult> {
  return invoke<BatchResult>("word_batch_import", { entries });
}

/** 导出全部词库为 JSON 字符串 */
export function wordExportJson(): Promise<string> {
  return invoke<string>("word_export_json");
}

/** 获取全部分类 */
export function wordCategories(): Promise<string[]> {
  return invoke<string[]>("word_categories");
}

// ===== Prompt 渲染预览命令 =====

/** 渲染模板预览（不入库） */
export function promptRenderPreview(
  template: string,
  vars: Record<string, string>,
): Promise<string> {
  return invoke<string>("prompt_render_preview", { template, vars });
}

/** 提取模板中的 {{var}} 变量名列表 */
export function promptExtractVariables(template: string): Promise<string[]> {
  return invoke<string[]>("prompt_extract_variables", { template });
}

// ===== 黑名单命令 =====

/** 读取黑名单正则列表 */
export function blacklistGet(): Promise<string[]> {
  return invoke<string[]>("blacklist_get");
}

/** 保存黑名单正则列表 */
export function blacklistSet(patterns: string[]): Promise<void> {
  return invoke<void>("blacklist_set", { patterns });
}

// ===== 词频命令 =====

/** 查询高频词列表（按使用次数降序） */
export function wordFreqList(limit?: number): Promise<WordFreqEntry[]> {
  return invoke<WordFreqEntry[]>("word_freq_list", { limit: limit ?? null });
}

/** 重置词频表（清空全部记录） */
export function wordFreqReset(): Promise<void> {
  return invoke<void>("word_freq_reset");
}

/** 获取词频统计概览（总词数 + 总使用次数） */
export function wordFreqOverview(): Promise<WordFreqOverview> {
  return invoke<WordFreqOverview>("word_freq_overview");
}

// ===== 热键暂停/恢复命令 =====

/** 查询当前热键是否已暂停 */
export function hotkeyIsPaused(): Promise<boolean> {
  return invoke<boolean>("hotkey_is_paused");
}

// ===== 历史记录命令 =====

/**
 * 查询历史记录列表（按时间倒序，支持搜索 + 分页）
 * @param search 可选搜索关键字（模糊匹配 origin 或 selected）
 * @param limit 每页条数，默认 20，上限 500
 * @param offset 偏移量，0-based
 */
export function historyList(
  search?: string,
  limit?: number,
  offset?: number,
): Promise<HistoryListResult> {
  return invoke<HistoryListResult>("history_list", {
    search: search ?? null,
    limit: limit ?? null,
    offset: offset ?? null,
  });
}

/** 删除单条历史记录 */
export function historyDelete(id: number): Promise<void> {
  return invoke<void>("history_delete", { id });
}

/** 清空全部历史记录 */
export function historyClear(): Promise<void> {
  return invoke<void>("history_clear");
}

// ===== 开机自启命令 =====

/** 查询开机自启状态 */
export function autostartGet(): Promise<boolean> {
  return invoke<boolean>("autostart_get");
}

/** 设置开机自启 */
export function autostartSet(enabled: boolean): Promise<void> {
  return invoke<void>("autostart_set", { enabled });
}

// ===== 剪贴板处理模式命令 =====

/**
 * 读取剪贴板处理模式
 * - "A"：兼容复原模式（快照→读文本→复原，会新增 Win+V 历史）
 * - "B"：纯净只读模式（默认，不修改剪贴板，Win+V 历史无杂乱）
 */
export function getClipboardMode(): Promise<string> {
  return invoke<string>("get_clipboard_mode");
}

/** 设置剪贴板处理模式（校验 mode ∈ {A, B} + 写 settings + invalidate_cache） */
export function setClipboardMode(mode: string): Promise<void> {
  return invoke<void>("set_clipboard_mode", { mode });
}

// ===== 事件监听 =====

/** 监听候选开始生成事件（悬浮窗进入 loading 状态，热键按下后立即触发） */
export function onCandidatesLoading(handler: () => void): Promise<UnlistenFn> {
  return listen<null>("candidates-loading", () => {
    handler();
  });
}

/** 监听候选就绪事件 */
export function onCandidatesReady(
  handler: (payload: CandidatesPayload) => void,
): Promise<UnlistenFn> {
  return listen<CandidatesPayload>("candidates-ready", (event) => {
    handler(event.payload);
  });
}

/** 监听候选生成错误事件 */
export function onCandidatesError(handler: (msg: string) => void): Promise<UnlistenFn> {
  return listen<string>("candidates-error", (event) => {
    handler(event.payload);
  });
}

/** 监听流式生成增量 chunk（流式输出开启时，边生成边推送 delta 到悬浮窗渐进显示） */
export function onCandidatesStream(handler: (delta: string) => void): Promise<UnlistenFn> {
  return listen<string>("candidates-stream", (event) => {
    handler(event.payload);
  });
}

/**
 * 监听悬浮窗快捷键事件（P-FLOAT-SHORTCUT）
 *
 * 由于悬浮窗设置了 WS_EX_NOACTIVATE 扩展样式（不抢焦点），无法直接接收 keydown 事件。
 * 后端通过全局热键注册 Tab/Up/Down/R/Escape/Ctrl+1/2/3，触发后 emit "float-shortcut"
 * 事件到前端，payload 为热键字符串（如 "tab"、"up"、"ctrl+1"）。
 *
 * 前端根据 payload 路由到等价的 handleKeydown 逻辑，实现键盘交互。
 */
export function onFloatShortcut(handler: (shortcut: string) => void): Promise<UnlistenFn> {
  return listen<string>("float-shortcut", (event) => {
    handler(event.payload);
  });
}

// ===== 窗口操作 =====

/** 开始拖拽窗口 */
export function startDragging(): Promise<void> {
  return getCurrentWindow().startDragging();
}

/** 隐藏当前窗口 */
export function hideCurrentWindow(): Promise<void> {
  return getCurrentWindow().hide();
}
