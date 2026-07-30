<script setup lang="ts">
// NOTE F5 悬浮候选窗根组件
// 严格遵循 PRD 5.3：
// - 无边框、透明背景、置顶、不抢焦点（WS_EX_NOACTIVATE，由后端设置）
// - 交互：↑↓ 切换候选 · Tab 确认输入 · ESC 关闭 · 刷新按钮重新生成
// - 事件：capture.triggered → llm.generating → llm.done → typing.done
import { ref, onMounted, onUnmounted, computed } from "vue";
import * as api from "@/api";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type { LlmDonePayload } from "@/api";

// NOTE getCurrentWindow 用于 hideWindow() 调用 window.hide()

const candidates = ref<string[]>([]);
const selectedIndex = ref(0);
const loading = ref(false);
const errorMessage = ref("");
const requestId = ref("");
const windowTitle = ref("");
const typing = ref(false);

let unlistenCapture: UnlistenFn | null = null;
let unlistenGenerating: UnlistenFn | null = null;
let unlistenDone: UnlistenFn | null = null;
let unlistenTypingStarted: UnlistenFn | null = null;
let unlistenTypingDone: UnlistenFn | null = null;
let unlistenTypingInterrupted: UnlistenFn | null = null;
let unlistenError: UnlistenFn | null = null;

const hasCandidates = computed(() => candidates.value.length > 0);

onMounted(async () => {
  console.log("[Overlay] 悬浮窗 Vue 组件已挂载，开始注册事件监听器");

  // PRD 4.3.2 事件监听
  unlistenCapture = await api.onCaptureTriggered((event) => {
    console.log("[Overlay] 收到 capture.triggered 事件:", event.payload);
    loading.value = true;
    errorMessage.value = "";
    candidates.value = [];
    windowTitle.value = event.payload.window_title;
    requestId.value = event.payload.request_id;
  });

  unlistenGenerating = await api.onLlmGenerating(() => {
    console.log("[Overlay] 收到 llm.generating 事件");
    loading.value = true;
  });

  unlistenDone = await api.onLlmDone((event) => {
    const payload = event.payload as LlmDonePayload;
    console.log("[Overlay] 收到 llm.done 事件, 候选数:", payload.candidates.length, payload.candidates);
    loading.value = false;
    requestId.value = payload.request_id;
    candidates.value = payload.candidates;
    selectedIndex.value = 0;
  });

  unlistenTypingStarted = await api.onTypingStarted(() => {
    console.log("[Overlay] 收到 typing.started 事件");
    typing.value = true;
  });

  unlistenTypingDone = await api.onTypingDone(() => {
    console.log("[Overlay] 收到 typing.done 事件");
    typing.value = false;
    void hideWindow();
  });

  // NOTE PRD US-3：ESC 中断输入后，浮窗隐藏
  unlistenTypingInterrupted = await api.onTypingInterrupted(() => {
    console.log("[Overlay] 收到 typing.interrupted 事件");
    typing.value = false;
    void hideWindow();
  });

  unlistenError = await api.onError((event) => {
    const err = event.payload as { message: string };
    console.log("[Overlay] 收到 error 事件:", err);
    loading.value = false;
    errorMessage.value = err.message;
  });

  console.log("[Overlay] 所有事件监听器注册完成");

  // NOTE 悬浮窗使用 WS_EX_NOACTIVATE（永不获取焦点），不能用 onFocusChanged 监听失焦
  // PRD 5.3.2 切窗关闭：改由后端在捕获时记录前台窗口，若用户切走则下次 F8 触发时自然隐藏旧浮窗

  window.addEventListener("keydown", handleKeyDown);
});

onUnmounted(() => {
  unlistenCapture?.();
  unlistenGenerating?.();
  unlistenDone?.();
  unlistenTypingStarted?.();
  unlistenTypingDone?.();
  unlistenTypingInterrupted?.();
  unlistenError?.();
  window.removeEventListener("keydown", handleKeyDown);
});

/** 键盘事件处理（PRD 5.3.3：↑↓切换 / Tab确认 / ESC关闭） */
function handleKeyDown(e: KeyboardEvent): void {
  switch (e.key) {
    case "ArrowDown":
      e.preventDefault();
      selectNext();
      break;
    case "ArrowUp":
      e.preventDefault();
      selectPrev();
      break;
    case "Tab":
      e.preventDefault();
      void confirmSelection();
      break;
    case "Escape":
      e.preventDefault();
      void hideWindow();
      break;
  }
}

function selectNext(): void {
  if (selectedIndex.value < candidates.value.length - 1) {
    selectedIndex.value++;
  }
}

function selectPrev(): void {
  if (selectedIndex.value > 0) {
    selectedIndex.value--;
  }
}

/** Tab 确认并模拟输入 */
async function confirmSelection(): Promise<void> {
  if (!hasCandidates.value || !requestId.value || typing.value) return;
  const replyText = candidates.value[selectedIndex.value];
  try {
    await api.adoptReply(requestId.value, replyText);
  } catch (err) {
    errorMessage.value = err instanceof Error ? err.message : "输入失败";
  }
}

function selectCandidate(index: number): void {
  selectedIndex.value = index;
}

/** 刷新候选（PRD US-2） */
async function refreshCandidates(): Promise<void> {
  if (!requestId.value) return;
  loading.value = true;
  errorMessage.value = "";
  candidates.value = [];
  try {
    await api.generateReply();
  } catch (err) {
    loading.value = false;
    errorMessage.value = err instanceof Error ? err.message : "刷新失败";
  }
}

/** 隐藏悬浮窗（PRD 5.3.2：ESC/切窗/点空白关闭） */
async function hideWindow(): Promise<void> {
  candidates.value = [];
  errorMessage.value = "";
  loading.value = false;
  try {
    await getCurrentWindow().hide();
  } catch (err) {
    console.error("隐藏窗口失败:", err);
  }
}
</script>

<template>
  <div class="overlay-container">
    <!-- 顶部：来源窗口信息 -->
    <div v-if="windowTitle && !loading" class="overlay-header">
      <svg class="overlay-header-icon" viewBox="0 0 24 24" width="12" height="12">
        <path
          fill="currentColor"
          d="M20 2H4c-1.1 0-1.99.9-1.99 2L2 22l4-4h14c1.1 0 2-.9 2-2V4c0-1.1-.9-2-2-2zm-2 12H6v-2h12v2zm0-3H6V9h12v2zm0-3H6V6h12v2z"
        />
      </svg>
      <span class="overlay-header-title">{{ windowTitle }}</span>
      <button v-if="hasCandidates" class="overlay-refresh-btn" @click="refreshCandidates" title="刷新候选">
        <svg viewBox="0 0 24 24" width="12" height="12">
          <path
            fill="currentColor"
            d="M17.65 6.35C16.2 4.9 14.21 4 12 4c-4.42 0-7.99 3.58-7.99 8s3.57 8 7.99 8c3.73 0 6.84-2.55 7.73-6h-2.08c-.82 2.33-3.04 4-5.65 4-3.31 0-6-2.69-6-6s2.69-6 6-6c1.66 0 3.14.69 4.22 1.78L13 11h7V4l-2.35 2.35z"
          />
        </svg>
      </button>
    </div>

    <!-- 加载中状态 -->
    <div v-if="loading" class="overlay-status">
      <svg class="overlay-spinner" viewBox="0 0 24 24" width="20" height="20">
        <path
          fill="currentColor"
          d="M12 4V1L8 5l4 4V6c3.31 0 6 2.69 6 6 0 1.01-.25 1.97-.7 2.8l1.46 1.46C19.54 15.03 20 13.57 20 12c0-4.42-3.58-8-8-8zm0 14c-3.31 0-6-2.69-6-6 0-1.01.25-1.97.7-2.8L5.24 7.74C4.46 8.97 4 10.43 4 12c0 4.42 3.58 8 8 8v3l4-4-4-4v3z"
        />
      </svg>
      <span class="overlay-status-text">AI 思考中...</span>
    </div>

    <!-- 输入中状态 -->
    <div v-else-if="typing" class="overlay-status overlay-typing">
      <svg class="overlay-spinner" viewBox="0 0 24 24" width="16" height="16">
        <path
          fill="currentColor"
          d="M20 5H4c-1.1 0-1.99.9-1.99 2L2 17c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V7c0-1.1-.9-2-2-2zm-2 12H6v-2h12v2zm0-3H6v-2h12v2zm0-3H6V8h12v2z"
        />
      </svg>
      <span class="overlay-status-text">正在输入...</span>
    </div>

    <!-- 错误状态 -->
    <div v-else-if="errorMessage" class="overlay-status overlay-error">
      <svg viewBox="0 0 24 24" width="16" height="16">
        <path
          fill="currentColor"
          d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm1 15h-2v-2h2v2zm0-4h-2V7h2v6z"
        />
      </svg>
      <span class="overlay-status-text">{{ errorMessage }}</span>
    </div>

    <!-- 候选列表 -->
    <div v-else-if="hasCandidates" class="overlay-list">
      <div
        v-for="(item, idx) in candidates"
        :key="idx"
        :class="['overlay-item', { 'is-selected': idx === selectedIndex }]"
        @click="selectCandidate(idx)"
      >
        <svg
          v-if="idx === selectedIndex"
          class="overlay-mark"
          viewBox="0 0 24 24"
          width="12"
          height="12"
        >
          <path fill="currentColor" d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z" />
        </svg>
        <span class="overlay-text">{{ item }}</span>
      </div>
    </div>

    <!-- 空状态 -->
    <div v-else class="overlay-status">
      <span class="overlay-status-text">等待捕获...</span>
    </div>

    <!-- 底部操作提示（PRD 5.3.3） -->
    <div v-if="hasCandidates && !loading && !typing" class="overlay-hint">
      <span class="overlay-hint-key">↑↓</span>
      <span class="overlay-hint-text">切换</span>
      <span class="overlay-hint-divider">·</span>
      <span class="overlay-hint-key">Tab</span>
      <span class="overlay-hint-text">确认</span>
      <span class="overlay-hint-divider">·</span>
      <span class="overlay-hint-key">Esc</span>
      <span class="overlay-hint-text">关闭</span>
    </div>
  </div>
</template>

<style scoped>
.overlay-container {
  width: 360px;
  background: rgba(255, 255, 255, 0.96);
  border-radius: 8px;
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.12);
  padding: 8px 0;
  font-size: 14px;
  color: #111827;
  backdrop-filter: blur(8px);
  -webkit-backdrop-filter: blur(8px);
}

.overlay-header {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 16px 8px;
  border-bottom: 1px solid #f3f4f6;
  margin-bottom: 4px;
}

.overlay-header-icon {
  color: #9ca3af;
  flex-shrink: 0;
}

.overlay-header-title {
  flex: 1;
  font-size: 11px;
  color: #9ca3af;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.overlay-refresh-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border: none;
  background: transparent;
  color: #9ca3af;
  cursor: pointer;
  border-radius: 4px;
  transition: all 0.15s;
}

.overlay-refresh-btn:hover {
  background: #f3f4f6;
  color: #6366f1;
}

.overlay-status {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 16px;
  color: #9ca3af;
}

.overlay-error {
  color: #ef4444;
}

.overlay-typing {
  color: #6366f1;
}

.overlay-spinner {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

.overlay-status-text {
  font-size: 13px;
}

.overlay-list {
  display: flex;
  flex-direction: column;
}

.overlay-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 16px;
  cursor: pointer;
  transition: background-color 0.15s;
}

.overlay-item:hover {
  background: #f9fafb;
}

.overlay-item.is-selected {
  background: #eef2ff;
  color: #6366f1;
}

.overlay-mark {
  flex-shrink: 0;
  color: #6366f1;
}

.overlay-text {
  flex: 1;
  line-height: 1.4;
}

.overlay-hint {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-top: 4px;
  padding: 6px 16px;
  border-top: 1px solid #f3f4f6;
  font-size: 11px;
  color: #9ca3af;
}

.overlay-hint-key {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 18px;
  height: 18px;
  padding: 0 4px;
  background: #f3f4f6;
  border-radius: 3px;
  font-size: 10px;
  color: #6b7280;
}

.overlay-hint-text {
  color: #9ca3af;
}

.overlay-hint-divider {
  color: #d1d5db;
  margin: 0 2px;
}
</style>
