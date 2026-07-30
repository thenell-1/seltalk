<script setup lang="ts">
import { NConfigProvider, NMessageProvider, NDialogProvider, zhCN, dateZhCN } from "naive-ui";
import ManagerLayout from "@/manager/layouts/ManagerLayout.vue";

// NOTE 管理面板根组件：Naive UI 主题/消息/对话框 Provider + 布局
</script>

<template>
  <NConfigProvider :locale="zhCN" :date-locale="dateZhCN">
    <NMessageProvider>
      <NDialogProvider>
        <ManagerLayout />
      </NDialogProvider>
    </NMessageProvider>
  </NConfigProvider>
</template>
