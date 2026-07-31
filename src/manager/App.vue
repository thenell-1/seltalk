<script setup lang="ts">
import {
  NConfigProvider,
  NMessageProvider,
  NDialogProvider,
  zhCN,
  dateZhCN,
  darkTheme,
} from "naive-ui";
import { ref, computed, onMounted, onUnmounted } from "vue";
import ManagerLayout from "@/manager/layouts/ManagerLayout.vue";

// NOTE 管理面板根组件：Naive UI 主题/消息/对话框 Provider + 布局
// dark mode：跟随系统 prefers-color-scheme 自动切换，无需用户手动设置

const isDark = ref(false);
let mediaQuery: MediaQueryList | null = null;

const handleThemeChange = (e: MediaQueryListEvent): void => {
  isDark.value = e.matches;
};

onMounted(() => {
  mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
  isDark.value = mediaQuery.matches;
  mediaQuery.addEventListener("change", handleThemeChange);
});

onUnmounted(() => {
  mediaQuery?.removeEventListener("change", handleThemeChange);
});

const theme = computed(() => (isDark.value ? darkTheme : null));
</script>

<template>
  <NConfigProvider :locale="zhCN" :date-locale="dateZhCN" :theme="theme">
    <NMessageProvider>
      <NDialogProvider>
        <ManagerLayout />
      </NDialogProvider>
    </NMessageProvider>
  </NConfigProvider>
</template>
