import { createRouter, createWebHashHistory, type RouteRecordRaw } from "vue-router";

// NOTE 管理面板路由：阶段一仅 LLM 配置 + 设置；阶段二补词库/Prompt；阶段三补词云
const routes: RouteRecordRaw[] = [
  { path: "/", redirect: "/llm" },
  {
    path: "/llm",
    name: "llm",
    component: () => import("@/manager/views/LlmConfigView.vue"),
    meta: { title: "LLM 配置" },
  },
  {
    path: "/settings",
    name: "settings",
    component: () => import("@/manager/views/SettingsView.vue"),
    meta: { title: "设置" },
  },
  {
    path: "/words",
    name: "words",
    component: () => import("@/manager/views/WordLibraryView.vue"),
    meta: { title: "词库管理" },
  },
  {
    path: "/prompts",
    name: "prompts",
    component: () => import("@/manager/views/PromptTemplateView.vue"),
    meta: { title: "Prompt 模板" },
  },
  {
    path: "/wordcloud",
    name: "wordcloud",
    component: () => import("@/manager/views/WordCloudView.vue"),
    meta: { title: "高频词云" },
  },
  {
    path: "/history",
    name: "history",
    component: () => import("@/manager/views/HistoryView.vue"),
    meta: { title: "历史记录" },
  },
];

export const router = createRouter({
  history: createWebHashHistory(),
  routes,
});
