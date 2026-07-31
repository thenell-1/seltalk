<script setup lang="ts">
import { computed } from "vue";
import { useRoute, useRouter } from "vue-router";
import { NLayout, NLayoutSider, NLayoutContent, NMenu, type MenuOption } from "naive-ui";

const route = useRoute();
const router = useRouter();

// NOTE 侧边栏菜单：阶段一仅 LLM/设置可用，其余项占位（阶段二/三激活）
const menuOptions = computed<MenuOption[]>(() => [
  { label: "LLM 配置", key: "llm" },
  { label: "词库管理", key: "words" },
  { label: "Prompt 模板", key: "prompts" },
  { label: "高频词云", key: "wordcloud" },
  { label: "历史记录", key: "history" },
  { label: "设置", key: "settings" },
]);

const activeKey = computed(() => (route.name as string) ?? "llm");

function handleSelect(key: string): void {
  router.push({ name: key });
}
</script>

<template>
  <NLayout has-sider style="height: 100vh">
    <NLayoutSider bordered :width="200" content-style="padding: 12px 0;">
      <div
        style="height: 48px; display: flex; align-items: center; justify-content: center; font-weight: 600; font-size: 16px;"
      >
        择言 SelTalk
      </div>
      <NMenu
        :value="activeKey"
        :options="menuOptions"
        @update:value="handleSelect"
      />
    </NLayoutSider>
    <NLayoutContent content-style="padding: 20px;">
      <RouterView />
    </NLayoutContent>
  </NLayout>
</template>
