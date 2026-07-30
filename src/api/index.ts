// NOTE Tauri IPC 封装层：统一管理前后端通信
// 严格遵循 PRD 4.3.2 事件定义：capture.triggered / llm.generating / llm.done / typing.done

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ============================================================================
// 类型定义
// ============================================================================

/** 系统配置（JSON 持久化） */
export interface AppConfig {
  trigger_key: string;
  candidate_count: number;
  typing_speed: number;
  typing_delay_min: number;
  typing_delay_max: number;
  llm_mode: "cloud" | "local";
}

/** LLM 模型配置 */
export interface LlmConfig {
  cloud_api_key: string;
  cloud_endpoint: string;
  cloud_model: string;
  local_endpoint: string;
  local_model: string;
}

/** capture.triggered 事件 payload（PRD 4.3.2） */
export interface CaptureTriggeredPayload {
  request_id: string;
  window_title: string;
}

/** llm.done 事件 payload（PRD 4.3.2） */
export interface LlmDonePayload {
  request_id: string;
  captured_text: string;
  candidates: string[];
  window_title: string;
}

/** 系统状态 */
export interface SystemStatus {
  running: boolean;
  llm_mode: string;
  last_capture_time: string | null;
  total_adopted: number;
}

/** 历史回复记录 */
export interface HistoryRecord {
  id: number | null;
  captured_text: string;
  reply_text: string;
  adopted: boolean;
  llm_mode: string;
  created_at: string;
}

// ============================================================================
// Command 调用封装
// ============================================================================

/** 获取系统配置 */
export async function getConfig(): Promise<AppConfig> {
  return invoke<AppConfig>("get_config");
}

/** 保存系统配置 */
export async function saveConfig(config: AppConfig): Promise<void> {
  await invoke("save_config", { config });
}

/** 获取 LLM 配置 */
export async function getLlmConfig(): Promise<LlmConfig> {
  return invoke<LlmConfig>("get_llm_config");
}

/** 保存 LLM 配置 */
export async function saveLlmConfig(config: LlmConfig): Promise<void> {
  await invoke("save_llm_config", { config });
}

/** 测试 LLM 连通性，返回成功消息或抛出错误 */
export async function testLlm(): Promise<string> {
  return invoke<string>("test_llm");
}

/** 获取系统状态 */
export async function getSystemStatus(): Promise<SystemStatus> {
  return invoke<SystemStatus>("get_system_status");
}

/** 分页查询历史回复 */
export async function listHistory(
  page: number = 0,
  pageSize: number = 20
): Promise<HistoryRecord[]> {
  return invoke<HistoryRecord[]>("list_history", { page, pageSize });
}

/** 触发生成回复（捕获选中文本），返回 request_id */
export async function generateReply(): Promise<string> {
  return invoke<string>("generate_reply");
}

/** 采纳回复并模拟逐字输入 */
export async function adoptReply(requestId: string, replyText: string): Promise<void> {
  await invoke("adopt_reply", { requestId, replyText });
}

/** 显示悬浮窗 */
export async function showOverlayWindow(): Promise<void> {
  await invoke("show_overlay_window");
}

/** 显示管理面板 */
export async function showPanelWindow(): Promise<void> {
  await invoke("show_panel_window");
}

// ============================================================================
// Event 监听封装（PRD 4.3.2）
// ============================================================================

/** 监听 capture.triggered 事件（选中文本已捕获，前端准备显示浮窗） */
export async function onCaptureTriggered(
  callback: (payload: { payload: CaptureTriggeredPayload }) => void
): Promise<UnlistenFn> {
  return listen<CaptureTriggeredPayload>("capture.triggered", callback as any);
}

/** 监听 llm.generating 事件（LLM 生成中，浮窗显示占位） */
export async function onLlmGenerating(callback: () => void): Promise<UnlistenFn> {
  return listen("llm.generating", callback);
}

/** 监听 llm.done 事件（LLM 完成，推送候选） */
export async function onLlmDone(
  callback: (payload: { payload: LlmDonePayload }) => void
): Promise<UnlistenFn> {
  return listen<LlmDonePayload>("llm.done", callback as any);
}

/** 监听 typing.started 事件（逐字输入开始） */
export async function onTypingStarted(callback: () => void): Promise<UnlistenFn> {
  return listen("typing.started", callback);
}

/** 监听 typing.done 事件（逐字输入完成） */
export async function onTypingDone(callback: () => void): Promise<UnlistenFn> {
  return listen("typing.done", callback);
}

/** 监听 typing.interrupted 事件（用户按 ESC 中断输入，PRD US-3） */
export async function onTypingInterrupted(callback: () => void): Promise<UnlistenFn> {
  return listen("typing.interrupted", callback);
}

/** 监听错误事件 */
export async function onError(
  callback: (payload: { payload: { code: string; message: string } }) => void
): Promise<UnlistenFn> {
  return listen<{ code: string; message: string }>("error", callback as any);
}
