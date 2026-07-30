<script setup lang="ts">
// NOTE F9 管理面板根组件：左侧固定侧边栏 + 主内容区
// 导航项：仪表盘 / 历史记录 / 设置
import { ref, shallowRef, onMounted } from "vue";
import * as api from "@/api";
import Dashboard from "./views/Dashboard.vue";
import History from "./views/History.vue";
import Settings from "./views/Settings.vue";

type ViewName = "dashboard" | "history" | "settings";

const currentView = ref<ViewName>("dashboard");
const currentComponent = shallowRef(Dashboard);
const llmMode = ref<string>("");

const navItems: Array<{ name: ViewName; label: string; icon: string }> = [
  {
    name: "dashboard",
    label: "仪表盘",
    icon: "M3 13h8V3H3v10zm0 8h8v-6H3v6zm10 0h8V11h-8v10zm0-18v6h8V3h-8z",
  },
  {
    name: "history",
    label: "历史记录",
    icon: "M13 3a9 9 0 0 0-9 9H1l3.89 3.89.07.14L9 12H6c0-3.87 3.13-7 7-7s7 3.13 7 7-3.13 7-7 7c-1.93 0-3.68-.79-4.94-2.06l-1.42 1.42A8.954 8.954 0 0 0 13 21a9 9 0 0 0 0-18zm-1 5v5l4.28 2.54.72-1.21-3.5-2.08V8H12z",
  },
  {
    name: "settings",
    label: "设置",
    icon: "M19.14 12.94c.04-.3.06-.61.06-.94 0-.32-.02-.64-.07-.94l2.03-1.58a.49.49 0 0 0 .12-.61l-1.92-3.32a.488.488 0 0 0-.59-.22l-2.39.96c-.5-.38-1.03-.7-1.62-.94l-.36-2.54a.484.484 0 0 0-.48-.41h-3.84c-.24 0-.43.17-.47.41l-.36 2.54c-.59.24-1.13.57-1.62.94l-2.39-.96c-.22-.08-.47 0-.59.22L2.74 8.87c-.12.21-.08.47.12.61l2.03 1.58c-.05.3-.09.63-.09.94s.02.64.07.94l-2.03 1.58a.49.49 0 0 0-.12.61l1.92 3.32c.12.22.37.29.59.22l2.39-.96c.5.38 1.03.7 1.62.94l.36 2.54c.05.24.24.41.48.41h3.84c.24 0 .44-.17.47-.41l.36-2.54c.59-.24 1.13-.56 1.62-.94l2.39.96c.22.08.47 0 .59-.22l1.92-3.32c.12-.22.07-.47-.12-.61l-2.01-1.58zM12 15.6c-1.98 0-3.6-1.62-3.6-3.6s1.62-3.6 3.6-3.6 3.6 1.62 3.6 3.6-1.62 3.6-3.6 3.6z",
  },
];

function switchView(view: ViewName): void {
  currentView.value = view;
  switch (view) {
    case "dashboard":
      currentComponent.value = Dashboard;
      break;
    case "history":
      currentComponent.value = History;
      break;
    case "settings":
      currentComponent.value = Settings;
      break;
  }
}

onMounted(async () => {
  try {
    const status = await api.getSystemStatus();
    llmMode.value = status.llm_mode;
  } catch (err) {
    console.error("加载状态失败:", err);
  }
});
</script>

<template>
  <div class="panel-layout">
    <!-- 侧边栏 -->
    <aside class="sidebar">
      <div class="sidebar-header">
        <svg viewBox="0 0 24 24" width="24" height="24" class="sidebar-logo">
          <path
            fill="currentColor"
            d="M12 2L2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5"
          />
        </svg>
        <span class="sidebar-title">创意输入法</span>
      </div>

      <nav class="sidebar-nav">
        <button
          v-for="item in navItems"
          :key="item.name"
          :class="['nav-item', { 'is-active': currentView === item.name }]"
          @click="switchView(item.name)"
        >
          <svg viewBox="0 0 24 24" width="16" height="16" class="nav-icon">
            <path fill="currentColor" :d="item.icon" />
          </svg>
          <span class="nav-label">{{ item.label }}</span>
        </button>
      </nav>

      <div class="sidebar-footer">
        <div class="status-indicator">
          <span class="status-dot"></span>
          <span class="status-text">运行中 · {{ llmMode || "cloud" }}</span>
        </div>
      </div>
    </aside>

    <!-- 主内容区 -->
    <main class="main-content">
      <header class="main-header">
        <span class="header-title">{{ navItems.find((n) => n.name === currentView)?.label }}</span>
      </header>
      <div class="main-body">
        <component :is="currentComponent" />
      </div>
    </main>
  </div>
</template>

<style scoped>
.panel-layout {
  display: flex;
  width: 100vw;
  height: 100vh;
  overflow: hidden;
  background: #f9fafb;
}

.sidebar {
  display: flex;
  flex-direction: column;
  width: 220px;
  background: #fff;
  border-right: 1px solid #e5e7eb;
  flex-shrink: 0;
}

.sidebar-header {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 56px;
  padding: 0 20px;
  border-bottom: 1px solid #f3f4f6;
}

.sidebar-logo {
  color: #6366f1;
  flex-shrink: 0;
}

.sidebar-title {
  font-size: 15px;
  font-weight: 600;
  color: #111827;
}

.sidebar-nav {
  flex: 1;
  padding: 12px 8px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.nav-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  border: none;
  background: transparent;
  border-radius: 6px;
  font-size: 13px;
  color: #6b7280;
  cursor: pointer;
  transition: all 0.15s;
  text-align: left;
  width: 100%;
}

.nav-item:hover {
  background: #f9fafb;
  color: #111827;
}

.nav-item.is-active {
  background: #eef2ff;
  color: #6366f1;
  font-weight: 500;
}

.nav-icon {
  flex-shrink: 0;
}

.nav-label {
  flex: 1;
}

.sidebar-footer {
  padding: 12px 20px;
  border-top: 1px solid #f3f4f6;
}

.status-indicator {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  color: #9ca3af;
}

.status-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: #10b981;
}

.main-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.main-header {
  height: 56px;
  display: flex;
  align-items: center;
  padding: 0 24px;
  background: #fff;
  border-bottom: 1px solid #e5e7eb;
}

.header-title {
  font-size: 15px;
  font-weight: 500;
  color: #111827;
}

.main-body {
  flex: 1;
  overflow: auto;
}
</style>
