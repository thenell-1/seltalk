<script setup lang="ts">
// NOTE 历史记录页面：展示捕获与回复历史
import { ref, onMounted } from "vue";
import * as api from "@/api";
import type { HistoryRecord } from "@/api";

const records = ref<HistoryRecord[]>([]);
const loading = ref(true);
const page = ref(0);
const pageSize = ref(20);

onMounted(async () => {
  await loadHistory();
});

async function loadHistory(): Promise<void> {
  loading.value = true;
  try {
    records.value = await api.listHistory(page.value, pageSize.value);
  } catch (err) {
    console.error("加载历史记录失败:", err);
  } finally {
    loading.value = false;
  }
}

function formatTime(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleString("zh-CN");
  } catch {
    return iso;
  }
}
</script>

<template>
  <div class="history-page">
    <div class="page-header">
      <h2 class="page-title">历史记录</h2>
      <button class="refresh-btn" @click="loadHistory">
        <svg viewBox="0 0 24 24" width="14" height="14">
          <path
            fill="currentColor"
            d="M17.65 6.35C16.2 4.9 14.21 4 12 4c-4.42 0-7.99 3.58-7.99 8s3.57 8 7.99 8c3.73 0 6.84-2.55 7.73-6h-2.08c-.82 2.33-3.04 4-5.65 4-3.31 0-6-2.69-6-6s2.69-6 6-6c1.66 0 3.14.69 4.22 1.78L13 11h7V4l-2.35 2.35z"
          />
        </svg>
        <span>刷新</span>
      </button>
    </div>

    <div v-if="loading" class="loading-state">加载中...</div>

    <div v-else-if="records.length === 0" class="empty-state">
      <svg viewBox="0 0 24 24" width="48" height="48">
        <path
          fill="currentColor"
          d="M14 2H6c-1.1 0-1.99.9-1.99 2L4 20c0 1.1.89 2 1.99 2H18c1.1 0 2-.9 2-2V8l-6-6zm2 16H8v-2h8v2zm0-4H8v-2h8v2zm-3-5V3.5L18.5 9H13z"
        />
      </svg>
      <p>暂无历史记录</p>
    </div>

    <div v-else class="history-list">
      <div v-for="(record, idx) in records" :key="record.id ?? idx" class="history-item">
        <div class="history-item-header">
          <span :class="['history-badge', record.adopted ? 'history-badge--adopted' : '']">
            {{ record.adopted ? "已采纳" : "未采纳" }}
          </span>
          <span class="history-time">{{ formatTime(record.created_at) }}</span>
          <span class="history-mode">{{ record.llm_mode }}</span>
        </div>
        <div class="history-content">
          <div class="history-captured">
            <span class="history-label">捕获：</span>
            <span class="history-text">{{ record.captured_text }}</span>
          </div>
          <div class="history-reply">
            <span class="history-label">回复：</span>
            <span class="history-text">{{ record.reply_text }}</span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.history-page {
  padding: 24px;
}

.page-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 16px;
}

.page-title {
  margin: 0;
  font-size: 20px;
  font-weight: 600;
  color: #111827;
}

.refresh-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  background: #fff;
  border: 1px solid #e5e7eb;
  border-radius: 6px;
  font-size: 13px;
  color: #4b5563;
  cursor: pointer;
  transition: all 0.15s;
}

.refresh-btn:hover {
  border-color: #6366f1;
  color: #6366f1;
}

.loading-state,
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 64px 0;
  color: #9ca3af;
  gap: 12px;
}

.empty-state svg {
  color: #d1d5db;
}

.history-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.history-item {
  padding: 16px;
  background: #fff;
  border: 1px solid #e5e7eb;
  border-radius: 8px;
}

.history-item-header {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 12px;
}

.history-badge {
  padding: 2px 8px;
  border-radius: 4px;
  font-size: 11px;
  background: #f3f4f6;
  color: #6b7280;
}

.history-badge--adopted {
  background: #ecfdf5;
  color: #10b981;
}

.history-time {
  font-size: 12px;
  color: #9ca3af;
}

.history-mode {
  margin-left: auto;
  padding: 2px 8px;
  background: #eff6ff;
  color: #3b82f6;
  border-radius: 4px;
  font-size: 11px;
}

.history-content {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.history-label {
  font-size: 12px;
  color: #9ca3af;
  margin-right: 4px;
}

.history-text {
  font-size: 13px;
  color: #374151;
}
</style>
