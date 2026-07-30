<script setup lang="ts">
// TODO 人工审查点：1.键盘/滚轮事件处理 2.事件监听生命周期 3.选中输入错误处理 4.置顶状态同步 5.滚轮防抖与原文框滚动兼容
// NOTE 悬浮窗主组件：监听候选事件 → 展示原文+候选列表 → 键盘/滚轮导航 → Tab/点击确认 → ESC取消
import { ref, onMounted, onUnmounted, nextTick } from "vue";
import {
  onCandidatesLoading,
  onCandidatesReady,
  onCandidatesError,
  onCandidatesStream,
  type CandidatesPayload,
  type UnlistenFn,
} from "@/lib/api";
import { typeCandidate, cancel, toggleFloatAlwaysOnTop, startDragging, getAllSettings } from "@/lib/api";

// ===== 状态 =====
type ViewState = "loading" | "ready" | "error";

const state = ref<ViewState>("loading");
const originText = ref("");
const candidates = ref<string[]>([]);
const selectedIndex = ref(0);
const errorMsg = ref("");
const alwaysOnTop = ref(true);
const isTyping = ref(false);
const stylePreset = ref("standard");
/** 流式生成累积文本（loading 状态下渐进显示，让用户即时看到生成进度） */
const streamText = ref("");

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

let wheelLock = false;
let switchTimer: number | null = null;

// 事件监听清理函数
let unlistenReady: UnlistenFn | null = null;
let unlistenError: UnlistenFn | null = null;
let unlistenLoading: UnlistenFn | null = null;
let unlistenStream: UnlistenFn | null = null;

// DOM 引用（用于显式注册非 passive 滚轮监听，确保跨浏览器可 preventDefault）
const shellRef = ref<HTMLElement | null>(null);

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
  state.value = "ready";
  // 确保 DOM 更新后滚动到第一项
  nextTick(() => scrollToSelected());
}

/** 候选生成错误：切换到 error 状态 */
function handleCandidatesError(msg: string): void {
  errorMsg.value = msg;
  streamText.value = "";
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

/** 键盘导航：↑↓ 切换候选 */
function handleKeydown(event: KeyboardEvent): void {
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
 * 滚轮切换候选（防抖）
 * - 向下滚（deltaY > 0）→ 下一条
 * - 向上滚（deltaY < 0）→ 上一条
 * - 鼠标在原文框上滚动时不拦截，允许原生滚动查看长文本
 */
function handleWheel(event: WheelEvent): void {
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

/** 切换置顶 */
async function doTogglePin(): Promise<void> {
  try {
    alwaysOnTop.value = await toggleFloatAlwaysOnTop();
  } catch (e) {
    console.error("切换置顶失败:", e);
  }
}

/** 滚动到选中项 */
function scrollToSelected(): void {
  const el = document.querySelector<HTMLElement>(`.candidate-item[data-index="${selectedIndex.value}"]`);
  el?.scrollIntoView({ block: "nearest", behavior: "smooth" });
}

/** 鼠标单击候选项：仅选中（不确认），并触发切换动画 */
function handleClickItem(index: number): void {
  if (index === selectedIndex.value) return;
  const dir = index > selectedIndex.value ? "down" : "up";
  switchTo(index, dir);
}

// ===== 生命周期 =====
onMounted(async () => {
  unlistenReady = await onCandidatesReady(handleCandidatesReady);
  unlistenError = await onCandidatesError(handleCandidatesError);
  unlistenLoading = await onCandidatesLoading(handleCandidatesLoading);
  unlistenStream = await onCandidatesStream(handleCandidatesStream);
  window.addEventListener("keydown", handleKeydown);
  // 滚轮事件显式注册为非 passive，确保能 preventDefault（兼容 Chrome/Firefox/Safari/Edge）
  shellRef.value?.addEventListener("wheel", handleWheel, { passive: false });
  // 初始加载样式预设
  await refreshStylePreset();
});

onUnmounted(() => {
  unlistenReady?.();
  unlistenError?.();
  unlistenLoading?.();
  unlistenStream?.();
  window.removeEventListener("keydown", handleKeydown);
  shellRef.value?.removeEventListener("wheel", handleWheel);
  if (switchTimer !== null) {
    window.clearTimeout(switchTimer);
  }
});
</script>

<template>
  <div ref="shellRef" class="float-shell" :class="[`preset-${stylePreset}`, { typing: isTyping }]">
    <!-- 顶部拖拽栏 -->
    <div class="float-drag-region" @mousedown="startDragging">
      <span class="float-title">择言</span>
      <div class="float-actions">
        <button
          class="icon-btn"
          :class="{ active: alwaysOnTop }"
          title="置顶开关"
          @mousedown.stop
          @click="doTogglePin"
        >
          {{ alwaysOnTop ? "📌" : "📍" }}
        </button>
        <button
          class="icon-btn"
          title="关闭 (ESC)"
          @mousedown.stop
          @click="doCancel"
        >
          ✕
        </button>
      </div>
    </div>

    <!-- 加载中（流式开启时渐进显示生成内容，首字延迟降到首 token 时间） -->
    <div v-if="state === 'loading'" class="float-body">
      <div v-if="streamText" class="stream-box">
        <div class="stream-label">生成中</div>
        <div class="stream-text">{{ streamText }}<span class="stream-cursor">▋</span></div>
      </div>
      <div v-else class="stream-empty">
        <span class="hint-text">正在生成回复…</span>
      </div>
    </div>

    <!-- 错误 -->
    <div v-else-if="state === 'error'" class="float-body center-state">
      <span class="error-text">{{ errorMsg }}</span>
      <button class="retry-btn" @click="doCancel">关闭</button>
    </div>

    <!-- 候选列表 -->
    <div v-else class="float-body">
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
          @dblclick="doConfirm"
        >
          <span class="candidate-index">{{ index + 1 }}</span>
          <span class="candidate-text">{{ item }}</span>
        </div>
      </div>
    </div>

    <!-- 底部操作栏：切换指示器 + 位置 + 确认按钮 -->
    <div class="float-footer">
      <template v-if="state === 'ready' && candidates.length > 0">
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
          <span class="confirm-icon">✓</span>
          <span>{{ isTyping ? "输入中" : "确认" }}</span>
        </button>
      </template>
      <span v-else class="hint-text">↑↓/滚轮 切换 · Tab/点击 确认 · ESC 关闭</span>
    </div>
  </div>
</template>

<style scoped>
.float-shell {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--st-bg);
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

.float-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--st-text-soft);
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
  font-size: 13px;
  color: var(--st-text);
  line-height: 1.4;
  white-space: pre-wrap;
  word-break: break-all;
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

.confirm-icon {
  font-size: 13px;
  line-height: 1;
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
