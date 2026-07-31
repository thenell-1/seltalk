<script setup lang="ts">
// TODO 人工审查点：1.键盘/滚轮事件处理 2.事件监听生命周期 3.选中输入错误处理 4.置顶模式同步 5.滚轮防抖与原文框滚动兼容
//                    6.透明度持久化防抖 7.模板切换即时生效 8.编辑模式 Tab/Esc 拦截 9.图标三态切换
// NOTE 悬浮窗主组件：监听候选事件 → 展示原文+候选列表 → 键盘/滚轮导航 → Tab/点击确认 → ESC取消
//       增强：透明度滑块（持久化）+ 双置顶模式循环 + 模板下拉选择器 + 候选编辑 + iconfont 矢量图标
import { ref, onMounted, onUnmounted, nextTick } from "vue";
import {
  onCandidatesLoading,
  onCandidatesReady,
  onCandidatesError,
  onCandidatesStream,
  onFloatShortcut,
  typeCandidate,
  cancel,
  regenerateCandidates,
  switchPromptByIndex,
  cyclePinMode,
  getPinMode,
  setFloatOpacity,
  getFloatOpacity,
  promptList,
  promptSetDefault,
  wordCreate,
  startDragging,
  getAllSettings,
  type CandidatesPayload,
  type UnlistenFn,
  type PinMode,
  type PromptTemplate,
} from "@/lib/api";
import Icon from "@/components/Icon.vue";

// ===== 状态 =====
type ViewState = "loading" | "ready" | "error";

const state = ref<ViewState>("loading");
const originText = ref("");
const candidates = ref<string[]>([]);
const selectedIndex = ref(0);
const errorMsg = ref("");
const pinMode = ref<PinMode>("Normal");
const isTyping = ref(false);
const stylePreset = ref("standard");
/** 流式生成累积文本（loading 状态下渐进显示，让用户即时看到生成进度） */
const streamText = ref("");

// 透明度（0.30~1.0，默认 1.0）
const opacity = ref(1.0);
/** 透明度弹窗显隐 */
const showOpacityPopover = ref(false);

// 模板下拉选择器
const templates = ref<PromptTemplate[]>([]);
const showTemplateDropdown = ref(false);
/** 当前默认模板名（下拉按钮显示） */
const currentTemplateName = ref("");

// 候选编辑模式
const isEditing = ref(false);
const editingText = ref("");
const editSaving = ref(false);

// 滚轮切换：防抖锁 + 切换方向（用于过渡动画方向控制）
const switchDir = ref<"up" | "down">("down");
// 列表过渡动画：偏移量与透明度（瞬间偏移→过渡回正，产生滑动效果）
const listOffset = ref(0);
const listOpacity = ref(1);
// 禁用过渡标志：设偏移瞬间为 true，回正时 false 以启用 transition
const noTransition = ref(false);

/** 滚轮防抖间隔（毫秒），避免一次滚动触发多次切换 */
const WHEEL_THROTTLE_MS = 160;
/** 切换动画时长（毫秒），与 CSS transition 保持一致 */
const SWITCH_ANIM_MS = 180;
/** 滚轮最小有效滚动距离，过滤触控板/不精密滚轮的微小抖动 */
const WHEEL_MIN_DELTA = 8;
/** 透明度持久化防抖间隔（毫秒） */
const OPACITY_DEBOUNCE_MS = 300;
/** 透明度下限（低于此值窗口不可用） */
const OPACITY_MIN = 0.3;
/** 透明度上限 */
const OPACITY_MAX = 1.0;

let wheelLock = false;
let switchTimer: number | null = null;
let opacityTimer: number | null = null;

// 事件监听清理函数
let unlistenReady: UnlistenFn | null = null;
let unlistenError: UnlistenFn | null = null;
let unlistenLoading: UnlistenFn | null = null;
let unlistenStream: UnlistenFn | null = null;
// P-FLOAT-SHORTCUT：悬浮窗快捷键监听（WS_EX_NOACTIVATE 下替代 keydown）
let unlistenFloatShortcut: UnlistenFn | null = null;

// DOM 引用（用于显式注册非 passive 滚轮监听，确保跨浏览器可 preventDefault）
const shellRef = ref<HTMLElement | null>(null);
// 编辑框引用（用于进入编辑模式后自动聚焦）
const editTextareaRef = ref<HTMLTextAreaElement | null>(null);

// ===== 事件处理 =====

/** 刷新悬浮窗样式预设（fire-and-forget，保证切换后下次呼出生效） */
async function refreshStylePreset(): Promise<void> {
  try {
    const all = await getAllSettings();
    stylePreset.value = all["float_style_preset"] || "standard";
  } catch {
    // 忽略，沿用当前值
  }
}

/** 候选开始生成：悬浮窗切到 loading 状态（热键按下后立即触发，给用户即时响应反馈） */
function handleCandidatesLoading(): void {
  state.value = "loading";
  errorMsg.value = "";
  // 新一轮生成开始，清空上轮流式残留
  streamText.value = "";
  // 退出编辑模式（新一轮生成覆盖编辑态）
  isEditing.value = false;
}

/** 候选就绪：填充列表，重置选中，切换到 ready 状态 */
function handleCandidatesReady(payload: CandidatesPayload): void {
  // 刷新样式预设（不阻塞渲染）
  void refreshStylePreset();
  originText.value = payload.origin;
  candidates.value = payload.candidates;
  selectedIndex.value = 0;
  errorMsg.value = "";
  // 候选就绪，清空流式文本（切换为切分好的候选列表）
  streamText.value = "";
  isEditing.value = false;
  state.value = "ready";
  // 确保 DOM 更新后滚动到第一项
  nextTick(() => scrollToSelected());
}

/** 候选生成错误：切换到 error 状态 */
function handleCandidatesError(msg: string): void {
  errorMsg.value = msg;
  streamText.value = "";
  isEditing.value = false;
  state.value = "error";
}

/** 流式增量：追加到 streamText，loading 状态下渐进显示生成内容（首字延迟降到首 token 时间） */
function handleCandidatesStream(delta: string): void {
  streamText.value += delta;
}

/**
 * 切换选中项并触发平滑过渡动画
 * 原理：先瞬间将列表偏移到反方向（无过渡），下一帧回正（有过渡），产生滑动回弹效果
 */
function switchTo(newIndex: number, dir: "up" | "down"): void {
  if (candidates.value.length === 0) return;
  selectedIndex.value = newIndex;
  switchDir.value = dir;

  // 阶段1：禁用过渡 + 瞬间偏移到反方向（向下切换→列表从上方滑入）
  noTransition.value = true;
  listOpacity.value = 0.45;
  listOffset.value = dir === "down" ? -8 : 8;

  // 阶段2：下一帧启用过渡 + 回正，触发滑动动画
  requestAnimationFrame(() => {
    noTransition.value = false;
    listOpacity.value = 1;
    listOffset.value = 0;
  });

  // 动画结束后清理标志（避免重复切换时定时器堆积）
  if (switchTimer !== null) {
    window.clearTimeout(switchTimer);
  }
  switchTimer = window.setTimeout(() => {
    noTransition.value = false;
    switchTimer = null;
  }, SWITCH_ANIM_MS + 40);

  scrollToSelected();
}

/** 键盘导航：↑↓ 切换候选，R 重新生成，Tab 确认，Esc 取消，E 编辑，Ctrl+1/2/3 切换模板 */
function handleKeydown(event: KeyboardEvent): void {
  // 编辑模式下：Tab 输入编辑文本，Esc 退出编辑，其余键不拦截（允许在 textarea 内自由编辑）
  if (isEditing.value) {
    if (event.key === "Tab") {
      event.preventDefault();
      void doEditConfirm();
    } else if (event.key === "Escape") {
      event.preventDefault();
      exitEdit();
    }
    return;
  }

  // Ctrl+1/2/3 切换 Prompt 模板（任意状态可用，输入中除外）
  if (event.ctrlKey && ["1", "2", "3"].includes(event.key)) {
    if (isTyping.value) return;
    event.preventDefault();
    void doSwitchPrompt(parseInt(event.key, 10) - 1);
    return;
  }

  // R 键重新生成：ready/error 状态可用，loading/typing 状态忽略
  if (event.key === "r" || event.key === "R") {
    if (state.value === "ready" || state.value === "error") {
      event.preventDefault();
      void doRegenerate();
    }
    return;
  }

  // E 键进入编辑模式：仅 ready 状态可用
  if (event.key === "e" || event.key === "E") {
    if (state.value === "ready" && candidates.value.length > 0 && !isTyping.value) {
      event.preventDefault();
      enterEdit();
    }
    return;
  }

  if (state.value !== "ready" || candidates.value.length === 0) {
    if (event.key === "Escape") {
      void doCancel();
    }
    return;
  }

  switch (event.key) {
    case "ArrowDown":
      event.preventDefault();
      switchTo((selectedIndex.value + 1) % candidates.value.length, "down");
      break;
    case "ArrowUp":
      event.preventDefault();
      switchTo((selectedIndex.value - 1 + candidates.value.length) % candidates.value.length, "up");
      break;
    case "Tab":
      event.preventDefault();
      void doConfirm();
      break;
    case "Escape":
      event.preventDefault();
      void doCancel();
      break;
  }
}

/**
 * 悬浮窗快捷键路由（P-FLOAT-SHORTCUT）
 *
 * 由于悬浮窗设置了 WS_EX_NOACTIVATE 扩展样式（不抢焦点），无法直接接收 keydown 事件。
 * 后端通过全局热键注册 Tab/Up/Down/R/Escape/Ctrl+1/2/3，触发后 emit "float-shortcut" 事件，
 * 前端在此函数中根据热键字符串路由到等价的 handleKeydown 逻辑。
 *
 * 热键字符串统一转小写比较（tauri Shortcut Display 在不同平台可能输出 "Tab"/"tab" 等）
 */
function handleFloatShortcut(shortcut: string): void {
  const key = shortcut.toLowerCase();

  // 编辑模式下：Tab 输入编辑文本，Esc 退出编辑（其余快捷键不响应，避免干扰编辑）
  if (isEditing.value) {
    if (key === "tab") {
      void doEditConfirm();
    } else if (key === "escape") {
      exitEdit();
    }
    return;
  }

  // Ctrl+1/2/3 切换 Prompt 模板（任意状态可用，输入中除外）
  if (key === "ctrl+1" || key === "ctrl+2" || key === "ctrl+3") {
    if (isTyping.value) return;
    const idx = parseInt(key.slice(-1), 10) - 1;
    void doSwitchPrompt(idx);
    return;
  }

  // R 键重新生成：ready/error 状态可用，loading/typing 状态忽略
  if (key === "r") {
    if (state.value === "ready" || state.value === "error") {
      void doRegenerate();
    }
    return;
  }

  // 非 ready 状态下仅响应 Escape
  if (state.value !== "ready" || candidates.value.length === 0) {
    if (key === "escape") {
      void doCancel();
    }
    return;
  }

  // ready 状态下的导航键
  switch (key) {
    case "down":
      switchTo((selectedIndex.value + 1) % candidates.value.length, "down");
      break;
    case "up":
      switchTo((selectedIndex.value - 1 + candidates.value.length) % candidates.value.length, "up");
      break;
    case "tab":
      void doConfirm();
      break;
    case "escape":
      void doCancel();
      break;
  }
}

/**
 * 滚轮切换候选（防抖）
 * - 向下滚（deltaY > 0）→ 下一条
 * - 向上滚（deltaY < 0）→ 上一条
 * - 鼠标在原文框上滚动时不拦截，允许原生滚动查看长文本
 */
function handleWheel(event: WheelEvent): void {
  // 编辑模式不拦截滚轮（允许 textarea 内滚动）
  if (isEditing.value) return;
  if (state.value !== "ready" || candidates.value.length <= 1) return;

  // 原文框内允许原生滚动，不拦截（兼容长原文查看）
  const target = event.target as HTMLElement | null;
  if (target?.closest(".origin-box")) return;

  // 阻止页面/容器滚动，由我们接管切换
  event.preventDefault();

  // 防抖：一次滚动动作只触发一次切换
  if (wheelLock) return;
  // 过滤微小抖动（触控板/低精度滚轮）
  if (Math.abs(event.deltaY) < WHEEL_MIN_DELTA) return;

  wheelLock = true;
  window.setTimeout(() => {
    wheelLock = false;
  }, WHEEL_THROTTLE_MS);

  if (event.deltaY > 0) {
    switchTo((selectedIndex.value + 1) % candidates.value.length, "down");
  } else {
    switchTo((selectedIndex.value - 1 + candidates.value.length) % candidates.value.length, "up");
  }
}

/** 确认选中：调用逐字输入 */
async function doConfirm(): Promise<void> {
  if (candidates.value.length === 0 || isTyping.value) return;
  const text = candidates.value[selectedIndex.value];
  if (!text) return;

  isTyping.value = true;
  try {
    await typeCandidate(text);
  } catch (e) {
    errorMsg.value = `输入失败: ${e}`;
    state.value = "error";
  } finally {
    isTyping.value = false;
  }
}

/** 取消：关闭悬浮窗 */
async function doCancel(): Promise<void> {
  try {
    await cancel();
  } catch (e) {
    console.error("取消失败:", e);
  }
}

/** R 键重新生成：用上次文本 + 更高 temperature 重试
 *  - 仅 ready/error 状态可用（loading 中忽略，不中断当前请求）
 *  - 输入中（is_typing）忽略
 *  - 状态切换由 onCandidatesLoading 事件处理（后端 emit candidates-loading）
 */
async function doRegenerate(): Promise<void> {
  if (isTyping.value) return;
  if (state.value === "loading") return;
  try {
    await regenerateCandidates();
    // 状态切换由 onCandidatesLoading 事件处理（后端会 emit "candidates-loading"）
  } catch (e) {
    errorMsg.value = `重新生成失败: ${e}`;
    state.value = "error";
  }
}

/** Ctrl+1/2/3 切换 Prompt 模板（切换后用户可按 R 重新生成） */
async function doSwitchPrompt(index: number): Promise<void> {
  try {
    const name = await switchPromptByIndex(index);
    // 切换成功：刷新本地模板列表 + 显示提示，用户可按 R 重新生成
    currentTemplateName.value = name;
    await loadTemplates();
    errorMsg.value = `已切换模板：${name}（按 R 重新生成）`;
  } catch (e) {
    errorMsg.value = `切换模板失败: ${e}`;
    state.value = "error";
  }
}

/** 循环切换置顶模式（Off → Normal → Temp → Off），更新图标三态 */
async function doCyclePin(): Promise<void> {
  try {
    pinMode.value = await cyclePinMode();
    // 关闭弹窗（避免切换后弹窗残留）
    showOpacityPopover.value = false;
    showTemplateDropdown.value = false;
  } catch (e) {
    console.error("切换置顶失败:", e);
  }
}

// ===== 透明度调节 =====

/** 滑块拖动：实时更新 CSS（opacity ref → --float-opacity 变量）+ 防抖持久化 */
function onOpacitySlider(event: Event): void {
  const target = event.target as HTMLInputElement;
  const val = parseFloat(target.value);
  // 钳制到合法范围（防御性：滑块 min/max 已限制，此处双保险）
  opacity.value = Math.min(OPACITY_MAX, Math.max(OPACITY_MIN, val));
  schedulePersistOpacity();
}

/** 防抖持久化透明度到 settings KV（避免拖动时频繁写 DB） */
function schedulePersistOpacity(): void {
  if (opacityTimer !== null) {
    window.clearTimeout(opacityTimer);
  }
  opacityTimer = window.setTimeout(() => {
    void setFloatOpacity(opacity.value).catch((e) => {
      console.warn("透明度持久化失败:", e);
    });
    opacityTimer = null;
  }, OPACITY_DEBOUNCE_MS);
}

/** 切换透明度弹窗显隐 */
function toggleOpacityPopover(): void {
  showOpacityPopover.value = !showOpacityPopover.value;
  // 关闭另一个弹窗（互斥）
  if (showOpacityPopover.value) {
    showTemplateDropdown.value = false;
  }
}

// ===== 模板下拉选择器 =====

/** 加载模板列表 + 更新当前默认模板名 */
async function loadTemplates(): Promise<void> {
  try {
    templates.value = await promptList();
    const def = templates.value.find((t) => t.is_default);
    currentTemplateName.value = def?.name ?? (templates.value.length > 0 ? templates.value[0].name : "");
  } catch (e) {
    console.warn("加载模板列表失败:", e);
  }
}

/** 切换模板下拉显隐 */
function toggleTemplateDropdown(): void {
  showTemplateDropdown.value = !showTemplateDropdown.value;
  // 关闭另一个弹窗（互斥）
  if (showTemplateDropdown.value) {
    showOpacityPopover.value = false;
  }
}

/** 选择模板：调 promptSetDefault 即时生效（invalidate_cache 已在后端处理） */
async function onTemplateSelect(tpl: PromptTemplate): Promise<void> {
  const id = tpl.id;
  if (id === null) return;
  try {
    await promptSetDefault(id);
    currentTemplateName.value = tpl.name;
    showTemplateDropdown.value = false;
    // 切换成功提示（不切到 error 状态，仅在 errorMsg 显示提示文字）
    errorMsg.value = `已切换：${tpl.name}（按 R 重新生成）`;
  } catch (e) {
    errorMsg.value = `切换模板失败: ${e}`;
    state.value = "error";
  }
}

/** 解析标签字符串为数组（逗号分隔） */
function parseTags(tags: string): string[] {
  return tags
    .split(",")
    .map((t) => t.trim())
    .filter((t) => t.length > 0);
}

// ===== 候选编辑模式 =====

/** 进入编辑模式：预填当前选中候选，聚焦 textarea */
function enterEdit(): void {
  if (candidates.value.length === 0) return;
  editingText.value = candidates.value[selectedIndex.value] ?? "";
  isEditing.value = true;
  // 关闭弹窗
  showOpacityPopover.value = false;
  showTemplateDropdown.value = false;
  // DOM 更新后自动聚焦
  nextTick(() => {
    editTextareaRef.value?.focus();
    // 光标移到末尾
    const el = editTextareaRef.value;
    if (el) {
      const len = el.value.length;
      el.setSelectionRange(len, len);
    }
  });
}

/** 退出编辑模式，回到 ready 状态 */
function exitEdit(): void {
  isEditing.value = false;
  editingText.value = "";
}

/** 编辑模式 Tab：将编辑后的文本逐字输入到当前活动窗口 */
async function doEditConfirm(): Promise<void> {
  if (isTyping.value) return;
  const text = editingText.value.trim();
  if (!text) return;

  isTyping.value = true;
  try {
    await typeCandidate(text);
    // 输入完成后退出编辑模式
    isEditing.value = false;
    editingText.value = "";
  } catch (e) {
    errorMsg.value = `输入失败: ${e}`;
    state.value = "error";
    isEditing.value = false;
  } finally {
    isTyping.value = false;
  }
}

/** 将编辑后的文本存入本地词库（保持编辑模式，可继续 Tab 输入） */
async function doSaveToLexicon(): Promise<void> {
  const text = editingText.value.trim();
  if (!text) return;
  editSaving.value = true;
  try {
    await wordCreate(text, "AI候选");
    // 提示成功（在 errorMsg 位置显示，不切到 error 状态）
    errorMsg.value = `已存入词库：${text.slice(0, 20)}${text.length > 20 ? "…" : ""}`;
    // 保持编辑模式，用户可继续 Tab 输入
  } catch (e) {
    errorMsg.value = `存入词库失败: ${e}`;
  } finally {
    editSaving.value = false;
  }
}

/** 滚动到选中项 */
function scrollToSelected(): void {
  const el = document.querySelector<HTMLElement>(`.candidate-item[data-index="${selectedIndex.value}"]`);
  el?.scrollIntoView({ block: "nearest", behavior: "smooth" });
}

/**
 * 鼠标单击候选项：切换选中并直接确认输入（P-FLOAT-SHORTCUT 配套优化）
 *
 * - 鼠标用户单击即确认（替代原"单击选中 + 双击确认"两步操作，降低交互成本）
 * - 键盘用户仍可通过 Tab 确认（handleFloatShortcut 路由）
 * - 输入中（is_typing）忽略，防止重复触发
 */
function handleClickItem(index: number): void {
  if (isTyping.value) return;
  if (candidates.value.length === 0) return;
  // 先切换选中到点击的项（触发切换动画，selectedIndex 同步更新）
  if (index !== selectedIndex.value) {
    const dir = index > selectedIndex.value ? "down" : "up";
    switchTo(index, dir);
  }
  // 直接确认输入（doConfirm 内部会读取最新的 selectedIndex）
  void doConfirm();
}

// ===== 生命周期 =====
onMounted(async () => {
  unlistenReady = await onCandidatesReady(handleCandidatesReady);
  unlistenError = await onCandidatesError(handleCandidatesError);
  unlistenLoading = await onCandidatesLoading(handleCandidatesLoading);
  unlistenStream = await onCandidatesStream(handleCandidatesStream);
  // P-FLOAT-SHORTCUT：注册悬浮窗快捷键监听（WS_EX_NOACTIVATE 下替代 keydown）
  unlistenFloatShortcut = await onFloatShortcut(handleFloatShortcut);
  window.addEventListener("keydown", handleKeydown);
  // 滚轮事件显式注册为非 passive，确保能 preventDefault（兼容 Chrome/Firefox/Safari/Edge）
  shellRef.value?.addEventListener("wheel", handleWheel, { passive: false });
  // 初始加载样式预设
  await refreshStylePreset();
  // 初始化置顶模式 + 透明度 + 模板列表
  try {
    pinMode.value = await getPinMode();
  } catch (e) {
    console.warn("加载置顶模式失败:", e);
  }
  try {
    opacity.value = await getFloatOpacity();
  } catch (e) {
    console.warn("加载透明度失败:", e);
  }
  await loadTemplates();
});

onUnmounted(() => {
  unlistenReady?.();
  unlistenError?.();
  unlistenLoading?.();
  unlistenStream?.();
  unlistenFloatShortcut?.();
  window.removeEventListener("keydown", handleKeydown);
  shellRef.value?.removeEventListener("wheel", handleWheel);
  if (switchTimer !== null) {
    window.clearTimeout(switchTimer);
  }
  if (opacityTimer !== null) {
    window.clearTimeout(opacityTimer);
  }
});
</script>

<template>
  <div
    ref="shellRef"
    class="float-shell"
    :class="[`preset-${stylePreset}`, { typing: isTyping }]"
    :style="{ '--float-opacity': opacity }"
  >
    <!-- 顶部拖拽栏 -->
    <div class="float-drag-region" @mousedown="startDragging">
      <div class="float-title-group">
        <span class="float-title">择言</span>
        <!-- 模板下拉选择器 -->
        <button
          class="template-selector"
          title="切换 Prompt 模板"
          @mousedown.stop
          @click="toggleTemplateDropdown"
        >
          <span class="template-name">{{ currentTemplateName || "无模板" }}</span>
          <Icon name="chevron-down" :size="12" />
        </button>
      </div>
      <div class="float-actions">
        <!-- 透明度滑块按钮 -->
        <button
          class="icon-btn"
          :class="{ active: showOpacityPopover }"
          title="窗口透明度"
          @mousedown.stop
          @click="toggleOpacityPopover"
        >
          <Icon name="opacity" :size="14" />
        </button>
        <!-- 置顶循环按钮（三态图标） -->
        <button
          class="icon-btn"
          :class="{ active: pinMode !== 'Off' }"
          :title="pinMode === 'Off' ? '置顶：关' : pinMode === 'Normal' ? '置顶：普通' : '置顶：临时'"
          @mousedown.stop
          @click="doCyclePin"
        >
          <Icon :name="pinMode === 'Normal' ? 'pin' : pinMode === 'Temp' ? 'pin-clock' : 'pin-off'" :size="14" />
        </button>
        <!-- 关闭按钮 -->
        <button
          class="icon-btn"
          title="关闭 (ESC)"
          @mousedown.stop
          @click="doCancel"
        >
          <Icon name="close" :size="14" />
        </button>
      </div>

      <!-- 透明度弹窗 -->
      <div v-if="showOpacityPopover" class="opacity-popover" @mousedown.stop>
        <div class="popover-label">透明度</div>
        <input
          type="range"
          class="opacity-slider"
          :min="OPACITY_MIN"
          :max="OPACITY_MAX"
          step="0.05"
          :value="opacity"
          @input="onOpacitySlider"
        />
        <span class="opacity-value">{{ Math.round(opacity * 100) }}%</span>
      </div>

      <!-- 模板下拉列表 -->
      <div v-if="showTemplateDropdown" class="template-dropdown" @mousedown.stop>
        <div v-if="templates.length === 0" class="dropdown-empty">
          暂无模板
        </div>
        <template v-else>
          <div
            v-for="tpl in templates"
            :key="tpl.id ?? 0"
            class="template-item"
            :class="{ active: tpl.is_default }"
            @click="onTemplateSelect(tpl)"
          >
            <div class="template-item-name">{{ tpl.name }}</div>
            <div v-if="parseTags(tpl.tags).length > 0" class="template-item-tags">
              <span v-for="tag in parseTags(tpl.tags)" :key="tag" class="mini-tag">{{ tag }}</span>
            </div>
            <Icon v-if="tpl.is_default" name="check" :size="12" class="template-check" />
          </div>
        </template>
      </div>
    </div>

    <!-- 加载中（流式开启时渐进显示生成内容，首字延迟降到首 token 时间） -->
    <div v-if="state === 'loading'" class="float-body">
      <div v-if="streamText" class="stream-box">
        <div class="stream-label">生成中</div>
        <div class="stream-text">{{ streamText }}<Icon name="cursor" :size="12" class="stream-cursor" /></div>
      </div>
      <div v-else class="stream-empty">
        <span class="hint-text">正在生成回复…</span>
      </div>
    </div>

    <!-- 错误 -->
    <div v-else-if="state === 'error'" class="float-body center-state">
      <div class="error-box">
        <Icon name="warn" :size="20" class="error-icon" />
        <span class="error-text">{{ errorMsg }}</span>
      </div>
      <button class="retry-btn" @click="doCancel">关闭</button>
    </div>

    <!-- 候选列表 -->
    <div v-else class="float-body">
      <!-- 编辑模式 -->
      <template v-if="isEditing">
        <div class="edit-box">
          <div class="origin-label">编辑候选</div>
          <textarea
            ref="editTextareaRef"
            v-model="editingText"
            class="edit-textarea"
            placeholder="编辑文本…"
            @keydown="handleKeydown"
          ></textarea>
          <div class="edit-actions">
            <button
              class="edit-btn primary"
              :disabled="isTyping || !editingText.trim()"
              title="输入到当前窗口 (Tab)"
              @click="doEditConfirm"
            >
              <Icon name="check" :size="12" />
              {{ isTyping ? "输入中" : "Tab 输入" }}
            </button>
            <button
              class="edit-btn"
              :disabled="editSaving || !editingText.trim()"
              title="存入本地词库"
              @click="doSaveToLexicon"
            >
              <Icon name="save" :size="12" />
              存入词库
            </button>
            <button class="edit-btn" title="取消编辑 (Esc)" @click="exitEdit">
              <Icon name="close" :size="12" />
            </button>
          </div>
        </div>
      </template>

      <!-- 常规候选展示 -->
      <template v-else>
        <!-- 原始文本 -->
        <div class="origin-box">
          <div class="origin-label">原文</div>
          <div class="origin-text">{{ originText }}</div>
        </div>

        <!-- 候选列表（滚轮切换 + 切换过渡动画） -->
        <div
          class="candidate-list"
          :class="{ 'no-transition': noTransition }"
          :style="{ transform: `translateY(${listOffset}px)`, opacity: listOpacity }"
        >
          <div
            v-for="(item, index) in candidates"
            :key="index"
            class="candidate-item"
            :data-index="index"
            :class="{ selected: index === selectedIndex }"
            @click="handleClickItem(index)"
          >
            <span class="candidate-index">{{ index + 1 }}</span>
            <span class="candidate-text">{{ item }}</span>
            <button
              v-if="index === selectedIndex"
              class="edit-trigger"
              title="编辑 (E)"
              @click.stop="enterEdit"
            >
              <Icon name="edit" :size="12" />
            </button>
          </div>
        </div>
      </template>
    </div>

    <!-- 底部操作栏：切换指示器 + 位置 + 确认按钮 -->
    <div class="float-footer">
      <template v-if="isEditing">
        <span class="hint-text">Tab 输入 · Esc 取消编辑</span>
      </template>
      <template v-else-if="state === 'ready' && candidates.length > 0">
        <!-- 圆点指示器：当前选中位置 -->
        <div class="indicator">
          <span
            v-for="(_, i) in candidates"
            :key="i"
            class="dot"
            :class="{ active: i === selectedIndex }"
          />
        </div>
        <!-- 位置文字：2/3 -->
        <span class="position-text">{{ selectedIndex + 1 }}/{{ candidates.length }}</span>
        <!-- 确认按钮：鼠标左键点击触发确认 -->
        <button
          class="confirm-btn"
          :class="{ pulse: isTyping }"
          :disabled="isTyping"
          title="确认输入 (Tab)"
          @click="doConfirm"
        >
          <Icon name="check" :size="13" />
          <span>{{ isTyping ? "输入中" : "确认" }}</span>
        </button>
      </template>
      <span v-else class="hint-text">↑↓/滚轮 切换 · E 编辑 · Tab 确认 · ESC 关闭</span>
    </div>
  </div>
</template>

<style scoped>
.float-shell {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  /* 透明度调节：双重 background 兜底
     1. var(--st-bg)：不透明背景兜底，确保 --st-bg-rgb 未定义或 rgba 解析失败时仍可见
     2. rgba(var(--st-bg-rgb), var(--float-opacity, 1))：半透明背景（若 CSS 变量可用则覆盖兜底）
     --float-opacity 由 opacity ref 动态绑定（0.3~1.0），--st-bg-rgb 来自 base.css */
  background: var(--st-bg);
  background: rgba(var(--st-bg-rgb), var(--float-opacity, 1));
  border-radius: var(--st-radius);
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.18);
  border: 1px solid var(--st-border);
  overflow: hidden;
}

.float-shell.typing {
  opacity: 0.7;
}

/* 拖拽栏 */
.float-drag-region {
  position: relative;
  -webkit-app-region: drag;
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 6px 0 10px;
  background: var(--st-bg-soft);
  border-bottom: 1px solid var(--st-border);
  flex-shrink: 0;
  cursor: grab;
}

.float-title-group {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.float-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--st-text-soft);
  flex-shrink: 0;
}

/* 模板下拉选择器 */
.template-selector {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  max-width: 120px;
  padding: 2px 4px;
  border: none;
  background: transparent;
  border-radius: 4px;
  color: var(--st-text-soft);
  font-size: 11px;
  cursor: pointer;
  transition: background 0.15s;
  -webkit-app-region: no-drag;
}

.template-selector:hover {
  background: rgba(0, 0, 0, 0.08);
}

.template-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.float-actions {
  display: flex;
  gap: 2px;
  -webkit-app-region: no-drag;
}

.icon-btn {
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  border-radius: 4px;
  font-size: 12px;
  color: var(--st-text-soft);
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.15s;
}

.icon-btn:hover {
  background: rgba(0, 0, 0, 0.08);
}

.icon-btn.active {
  color: var(--st-primary);
}

/* 透明度弹窗 */
.opacity-popover {
  position: absolute;
  top: 30px;
  right: 70px;
  z-index: 100;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  background: var(--st-bg);
  border: 1px solid var(--st-border);
  border-radius: 6px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.15);
  -webkit-app-region: no-drag;
}

.popover-label {
  font-size: 11px;
  color: var(--st-text-soft);
  flex-shrink: 0;
}

.opacity-slider {
  width: 100px;
  cursor: pointer;
}

.opacity-value {
  font-size: 11px;
  color: var(--st-text);
  font-variant-numeric: tabular-nums;
  min-width: 32px;
  text-align: right;
}

/* 模板下拉列表 */
.template-dropdown {
  position: absolute;
  top: 30px;
  left: 10px;
  z-index: 100;
  min-width: 180px;
  max-width: 280px;
  max-height: 280px;
  overflow-y: auto;
  padding: 4px;
  background: var(--st-bg);
  border: 1px solid var(--st-border);
  border-radius: 6px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.15);
  -webkit-app-region: no-drag;
}

.dropdown-empty {
  padding: 12px;
  font-size: 12px;
  color: var(--st-text-soft);
  text-align: center;
}

.template-item {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 6px 8px;
  border-radius: 4px;
  cursor: pointer;
  transition: background 0.15s;
}

.template-item:hover {
  background: var(--st-bg-soft);
}

.template-item.active {
  background: rgba(32, 128, 240, 0.1);
}

.template-item-name {
  font-size: 12px;
  color: var(--st-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  padding-right: 16px;
}

.template-item-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 2px;
}

.mini-tag {
  font-size: 10px;
  padding: 0 4px;
  border-radius: 2px;
  background: var(--st-bg-soft);
  color: var(--st-text-soft);
  line-height: 16px;
}

.template-item.active .mini-tag {
  background: rgba(32, 128, 240, 0.15);
  color: var(--st-primary);
}

.template-check {
  position: absolute;
  right: 6px;
  top: 6px;
  color: var(--st-primary);
}

/* 主体 */
.float-body {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  padding: 8px;
  gap: 8px;
}

.center-state {
  align-items: center;
  justify-content: center;
}

.hint-text {
  font-size: 13px;
  color: var(--st-text-soft);
}

/* 流式生成渐进显示 */
.stream-box {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 4px;
  overflow: hidden;
}

.stream-label {
  font-size: 10px;
  color: var(--st-text-soft);
}

.stream-text {
  flex: 1;
  font-size: 13px;
  color: var(--st-text);
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-all;
  overflow-y: auto;
}

.stream-cursor {
  display: inline-block;
  color: var(--st-primary);
  animation: stream-blink 1s step-end infinite;
  vertical-align: text-bottom;
}

.stream-empty {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}

@keyframes stream-blink {
  0%,
  50% {
    opacity: 1;
  }
  51%,
  100% {
    opacity: 0;
  }
}

.error-box {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
}

.error-icon {
  color: var(--st-danger);
  line-height: 1;
}

.error-text {
  font-size: 13px;
  color: var(--st-danger);
  text-align: center;
  padding: 0 12px;
  word-break: break-all;
}

.retry-btn {
  margin-top: 12px;
  padding: 4px 16px;
  border: 1px solid var(--st-border);
  border-radius: 4px;
  background: var(--st-bg);
  font-size: 12px;
  color: var(--st-text-soft);
}

.retry-btn:hover {
  background: var(--st-bg-soft);
}

/* 原文框 */
.origin-box {
  border: 1px solid var(--st-border);
  border-radius: 4px;
  padding: 6px 8px;
  background: var(--st-bg-soft);
  flex-shrink: 0;
  max-height: 80px;
  overflow-y: auto;
}

.origin-label {
  font-size: 10px;
  color: var(--st-text-soft);
  margin-bottom: 2px;
}

.origin-text {
  font-size: 12px;
  color: var(--st-text);
  line-height: 1.4;
  white-space: pre-wrap;
  word-break: break-all;
}

/* 候选列表 + 切换过渡动画 */
.candidate-list {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
  /* 过渡：切换时由偏移回正产生滑动效果 */
  transition: transform 0.18s ease, opacity 0.18s ease;
  will-change: transform, opacity;
}

/* 瞬间偏移阶段禁用过渡 */
.candidate-list.no-transition {
  transition: none;
}

.candidate-item {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  padding: 6px 8px;
  border-radius: 4px;
  cursor: pointer;
  transition: background 0.2s ease, transform 0.15s ease;
}

.candidate-item:hover {
  background: var(--st-bg-soft);
}

.candidate-item.selected {
  background: rgba(32, 128, 240, 0.12);
  transform: translateX(2px);
}

.candidate-index {
  font-size: 10px;
  color: var(--st-text-soft);
  flex-shrink: 0;
  margin-top: 2px;
  min-width: 12px;
}

.candidate-item.selected .candidate-index {
  color: var(--st-primary);
  font-weight: 600;
}

.candidate-text {
  flex: 1;
  font-size: 13px;
  color: var(--st-text);
  line-height: 1.4;
  white-space: pre-wrap;
  word-break: break-all;
}

/* 编辑触发按钮（选中项右侧） */
.edit-trigger {
  flex-shrink: 0;
  width: 20px;
  height: 20px;
  border: none;
  background: transparent;
  border-radius: 3px;
  color: var(--st-text-soft);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.15s, background 0.15s;
  margin-top: 1px;
}

.candidate-item.selected .edit-trigger {
  opacity: 0.7;
}

.edit-trigger:hover {
  opacity: 1;
  background: rgba(0, 0, 0, 0.1);
  color: var(--st-primary);
}

/* 编辑模式 */
.edit-box {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 6px;
  overflow: hidden;
}

.edit-textarea {
  flex: 1;
  width: 100%;
  padding: 8px;
  border: 1px solid var(--st-border);
  border-radius: 4px;
  background: var(--st-bg-soft);
  color: var(--st-text);
  font-size: 13px;
  line-height: 1.5;
  font-family: var(--st-font-family);
  resize: none;
  outline: none;
}

.edit-textarea:focus {
  border-color: var(--st-primary);
  background: var(--st-bg);
}

.edit-actions {
  display: flex;
  gap: 6px;
  flex-shrink: 0;
}

.edit-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 5px 10px;
  border: 1px solid var(--st-border);
  border-radius: 4px;
  background: var(--st-bg);
  color: var(--st-text-soft);
  font-size: 12px;
  cursor: pointer;
  transition: background 0.15s, opacity 0.15s;
}

.edit-btn:hover:not(:disabled) {
  background: var(--st-bg-soft);
}

.edit-btn.primary {
  background: var(--st-primary);
  color: #fff;
  border-color: var(--st-primary);
}

.edit-btn.primary:hover:not(:disabled) {
  opacity: 0.9;
}

.edit-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* 底部操作栏 */
.float-footer {
  height: 34px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 10px;
  border-top: 1px solid var(--st-border);
  background: var(--st-bg-soft);
  flex-shrink: 0;
  gap: 8px;
}

/* 非 ready 状态提示文字居中 */
.float-footer .hint-text {
  flex: 1;
  text-align: center;
}

/* 圆点指示器 */
.indicator {
  display: flex;
  gap: 4px;
  align-items: center;
}

.dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--st-border);
  transition: all 0.22s ease;
}

.dot.active {
  background: var(--st-primary);
  width: 16px;
  border-radius: 3px;
}

/* 位置文字 */
.position-text {
  font-size: 11px;
  color: var(--st-text-soft);
  font-variant-numeric: tabular-nums;
  margin-left: 2px;
}

/* 确认按钮 */
.confirm-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 4px 14px;
  border: none;
  border-radius: 4px;
  background: var(--st-primary);
  color: #fff;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  transition: transform 0.1s ease, box-shadow 0.15s ease, opacity 0.15s ease;
  box-shadow: 0 2px 6px rgba(32, 128, 240, 0.3);
  -webkit-user-select: none;
  -moz-user-select: none;
  user-select: none;
}

.confirm-btn:hover:not(:disabled) {
  box-shadow: 0 3px 10px rgba(32, 128, 240, 0.45);
}

.confirm-btn:active:not(:disabled) {
  transform: scale(0.94);
}

.confirm-btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
  box-shadow: none;
}

.confirm-btn.pulse {
  animation: btnPulse 1s infinite;
}

@keyframes btnPulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.6; }
}

/* 样式预设：紧凑 */
.preset-compact .candidate-text { font-size: 12px; line-height: 1.35; }
.preset-compact .candidate-item { padding: 4px 6px; }
.preset-compact .float-body { padding: 6px; gap: 6px; }
.preset-compact .origin-text { font-size: 11px; }
.preset-compact .candidate-list { gap: 1px; }

/* 样式预设：宽松 */
.preset-loose .candidate-text { font-size: 15px; line-height: 1.5; }
.preset-loose .candidate-item { padding: 8px 10px; }
.preset-loose .float-body { padding: 10px; gap: 10px; }
.preset-loose .origin-text { font-size: 13px; }
.preset-loose .candidate-list { gap: 4px; }
</style>
