// TODO 人工审查点：1.invoke 封装错误处理 2.事件监听清理 3.类型安全 4.窗口操作
// NOTE Tauri API 封装：统一命令调用 + 事件监听 + 窗口操作，前端各组件共用
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

/** 事件监听清理函数类型 */
export type UnlistenFn = () => void;

// ===== 类型定义 =====

/** 应用运行时配置 */
export interface AppConfig {
  hotkey: string;
  candidate_count: number;
  type_min_ms: number;
  type_max_ms: number;
  float_w: number;
  float_h: number;
  float_always_on_top: boolean;
}

/** Prompt 模板 */
export interface PromptTemplate {
  id: number | null;
  name: string;
  template: string;
  is_default: boolean;
  created_at: string;
  updated_at: string;
}

/** 候选数据载荷（Rust 发送到前端） */
export interface CandidatesPayload {
  origin: string;
  candidates: string[];
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

/** 切换悬浮窗置顶 */
export function toggleFloatAlwaysOnTop(): Promise<boolean> {
  return invoke<boolean>("toggle_float_always_on_top");
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

export function promptCreate(name: string, template: string): Promise<number> {
  return invoke<number>("prompt_create", { name, template });
}

export function promptUpdate(id: number, name: string, template: string): Promise<void> {
  return invoke<void>("prompt_update", { id, name, template });
}

export function promptDelete(id: number): Promise<void> {
  return invoke<void>("prompt_delete", { id });
}

export function promptSetDefault(id: number): Promise<void> {
  return invoke<void>("prompt_set_default", { id });
}

// ===== LLM 命令 =====

/** LLM 连通性测试结果 */
export interface ConnectionTestResult {
  /** 是否连接成功 */
  ok: boolean;
  /** 请求耗时（毫秒） */
  latency_ms: number;
  /** 结果消息（成功为"连接成功"，失败为错误详情） */
  message: string;
}

/** 测试 LLM 连通性，返回结果（ok/延迟/消息） */
export function testLlmConnection(): Promise<ConnectionTestResult> {
  return invoke<ConnectionTestResult>("test_llm_connection");
}

// ===== 词库命令 =====

/** 词库条目 */
export interface WordEntry {
  id: number | null;
  word: string;
  category: string;
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

/** 批量导入结果 */
export interface BatchResult {
  imported: number;
  skipped: number;
  errors: string[];
}

/** 批量导入单条结构 */
export interface BatchImportEntry {
  word: string;
  category: string;
}

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

/** 词频条目（对应 word_freq 表一行） */
export interface WordFreqEntry {
  /** 词语 */
  word: string;
  /** 使用次数 */
  count: number;
  /** 最后使用时间（RFC3339 格式） */
  last_used_at: string | null;
}

/** 词频统计概览 */
export interface WordFreqOverview {
  /** 不同的词语总数 */
  total_words: number;
  /** 累计使用总次数 */
  total_usage: number;
}

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

// ===== 开机自启命令 =====

/** 查询开机自启状态 */
export function autostartGet(): Promise<boolean> {
  return invoke<boolean>("autostart_get");
}

/** 设置开机自启 */
export function autostartSet(enabled: boolean): Promise<void> {
  return invoke<void>("autostart_set", { enabled });
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

// ===== 窗口操作 =====

/** 开始拖拽窗口 */
export function startDragging(): Promise<void> {
  return getCurrentWindow().startDragging();
}

/** 隐藏当前窗口 */
export function hideCurrentWindow(): Promise<void> {
  return getCurrentWindow().hide();
}
