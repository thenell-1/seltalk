// NOTE 全局应用状态管理（Pinia）
import { defineStore } from "pinia";
import { ref } from "vue";
import type { AppConfig, LlmConfig, SystemStatus } from "@/api";
import * as api from "@/api";

export const useAppStore = defineStore("app", () => {
  // 状态
  const config = ref<AppConfig | null>(null);
  const llmConfig = ref<LlmConfig | null>(null);
  const systemStatus = ref<SystemStatus | null>(null);
  const loading = ref(false);

  // 加载配置
  async function loadConfig(): Promise<void> {
    loading.value = true;
    try {
      config.value = await api.getConfig();
      llmConfig.value = await api.getLlmConfig();
      systemStatus.value = await api.getSystemStatus();
    } catch (err) {
      console.error("加载配置失败:", err);
    } finally {
      loading.value = false;
    }
  }

  // 保存配置
  async function saveAppConfig(newConfig: AppConfig): Promise<void> {
    await api.saveConfig(newConfig);
    config.value = newConfig;
  }

  // 保存 LLM 配置
  async function saveLlmConfigData(newConfig: LlmConfig): Promise<void> {
    await api.saveLlmConfig(newConfig);
    llmConfig.value = newConfig;
  }

  return {
    config,
    llmConfig,
    systemStatus,
    loading,
    loadConfig,
    saveAppConfig,
    saveLlmConfigData,
  };
});
