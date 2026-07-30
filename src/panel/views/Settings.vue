<script setup lang="ts">
// NOTE 设置页面：通用配置 + LLM 配置
import { ref, onMounted } from "vue";
import * as api from "@/api";
import type { AppConfig, LlmConfig } from "@/api";

const appConfig = ref<AppConfig | null>(null);
const llmConfig = ref<LlmConfig | null>(null);
const loading = ref(true);
const saving = ref(false);
const testing = ref(false);
const testResult = ref("");
const testResultType = ref<"success" | "error">("success");

onMounted(async () => {
  try {
    appConfig.value = await api.getConfig();
    llmConfig.value = await api.getLlmConfig();
  } catch (err) {
    console.error("加载配置失败:", err);
  } finally {
    loading.value = false;
  }
});

async function saveAppConfigData(): Promise<void> {
  if (!appConfig.value) return;
  saving.value = true;
  testResult.value = "";
  try {
    await api.saveConfig(appConfig.value);
    testResult.value = "应用配置已保存";
    testResultType.value = "success";
  } catch (err) {
    testResult.value = formatError(err);
    testResultType.value = "error";
  } finally {
    saving.value = false;
  }
}

async function saveLlmConfigData(): Promise<void> {
  if (!llmConfig.value) return;
  saving.value = true;
  testResult.value = "";
  try {
    await api.saveLlmConfig(llmConfig.value);
    testResult.value = "LLM 配置已保存";
    testResultType.value = "success";
  } catch (err) {
    testResult.value = formatError(err);
    testResultType.value = "error";
  } finally {
    saving.value = false;
  }
}

async function testLlmConnection(): Promise<void> {
  testing.value = true;
  testResult.value = "";
  try {
    const msg = await api.testLlm();
    testResult.value = msg;
    testResultType.value = "success";
  } catch (err) {
    testResult.value = formatError(err);
    testResultType.value = "error";
  } finally {
    testing.value = false;
  }
}

/** 格式化错误信息 */
function formatError(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  if (err && typeof err === "object" && "message" in err) {
    return String((err as { message: unknown }).message);
  }
  return "未知错误";
}
</script>

<template>
  <div class="settings-page">
    <h2 class="page-title">设置</h2>

    <div v-if="loading" class="loading-state">加载中...</div>

    <template v-else>
      <!-- 通用配置 -->
      <section v-if="appConfig" class="settings-section">
        <h3 class="section-title">通用配置</h3>
        <div class="form-grid">
          <div class="form-item">
            <label class="form-label">触发快捷键</label>
            <input v-model="appConfig.trigger_key" class="form-input" placeholder="F8" />
          </div>
          <div class="form-item">
            <label class="form-label">候选回复数量</label>
            <input
              v-model.number="appConfig.candidate_count"
              type="number"
              min="1"
              max="10"
              class="form-input"
            />
          </div>
          <div class="form-item">
            <label class="form-label">打字延迟下限 (ms)</label>
            <input
              v-model.number="appConfig.typing_delay_min"
              type="number"
              min="0"
              class="form-input"
            />
          </div>
          <div class="form-item">
            <label class="form-label">打字延迟上限 (ms)</label>
            <input
              v-model.number="appConfig.typing_delay_max"
              type="number"
              min="0"
              class="form-input"
            />
          </div>
          <div class="form-item">
            <label class="form-label">LLM 模式</label>
            <select v-model="appConfig.llm_mode" class="form-input">
              <option value="cloud">云端</option>
              <option value="local">本地</option>
            </select>
          </div>
        </div>
        <button class="save-btn" :disabled="saving" @click="saveAppConfigData">
          {{ saving ? "保存中..." : "保存通用配置" }}
        </button>
      </section>

      <!-- LLM 配置 -->
      <section v-if="llmConfig" class="settings-section">
        <h3 class="section-title">LLM 配置</h3>

        <div v-if="appConfig?.llm_mode === 'cloud'" class="form-grid">
          <div class="form-item form-item--full">
            <label class="form-label">API 密钥</label>
            <input
              v-model="llmConfig.cloud_api_key"
              type="password"
              class="form-input"
              placeholder="sk-..."
            />
          </div>
          <div class="form-item form-item--full">
            <label class="form-label">API 端点</label>
            <input
              v-model="llmConfig.cloud_endpoint"
              class="form-input"
              placeholder="https://api.deepseek.com/v1"
            />
          </div>
          <div class="form-item">
            <label class="form-label">模型名称</label>
            <input
              v-model="llmConfig.cloud_model"
              class="form-input"
              placeholder="deepseek-chat"
            />
          </div>
        </div>

        <div v-if="appConfig?.llm_mode === 'local'" class="form-grid">
          <div class="form-item form-item--full">
            <label class="form-label">Ollama 端点</label>
            <input
              v-model="llmConfig.local_endpoint"
              class="form-input"
              placeholder="http://localhost:11434"
            />
          </div>
          <div class="form-item">
            <label class="form-label">模型名称</label>
            <input
              v-model="llmConfig.local_model"
              class="form-input"
              placeholder="qwen2.5:7b"
            />
          </div>
        </div>

        <div class="settings-actions">
          <button class="save-btn" :disabled="saving" @click="saveLlmConfigData">
            {{ saving ? "保存中..." : "保存 LLM 配置" }}
          </button>
          <button class="test-btn" :disabled="testing" @click="testLlmConnection">
            {{ testing ? "测试中..." : "测试连接" }}
          </button>
        </div>

        <div
          v-if="testResult"
          :class="['test-result', testResultType === 'error' ? 'test-result--error' : 'test-result--success']"
        >
          {{ testResult }}
        </div>
      </section>
    </template>
  </div>
</template>

<style scoped>
.settings-page {
  padding: 24px;
  max-width: 800px;
}

.page-title {
  margin: 0 0 24px;
  font-size: 20px;
  font-weight: 600;
  color: #111827;
}

.loading-state {
  padding: 64px 0;
  text-align: center;
  color: #9ca3af;
}

.settings-section {
  margin-bottom: 32px;
  padding: 20px;
  background: #fff;
  border: 1px solid #e5e7eb;
  border-radius: 8px;
}

.section-title {
  margin: 0 0 16px;
  font-size: 15px;
  font-weight: 600;
  color: #374151;
}

.form-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 16px;
  margin-bottom: 16px;
}

.form-item {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.form-item--full {
  grid-column: 1 / -1;
}

.form-label {
  font-size: 13px;
  color: #4b5563;
}

.form-input {
  padding: 8px 12px;
  border: 1px solid #d1d5db;
  border-radius: 6px;
  font-size: 13px;
  color: #111827;
  background: #fff;
  transition: border-color 0.15s;
}

.form-input:focus {
  outline: none;
  border-color: #6366f1;
  box-shadow: 0 0 0 3px rgba(99, 102, 241, 0.1);
}

.settings-actions {
  display: flex;
  gap: 12px;
}

.save-btn,
.test-btn {
  padding: 8px 20px;
  border-radius: 6px;
  font-size: 13px;
  cursor: pointer;
  transition: all 0.15s;
  border: 1px solid transparent;
}

.save-btn {
  background: #6366f1;
  color: #fff;
}

.save-btn:hover:not(:disabled) {
  background: #4f46e5;
}

.save-btn:disabled {
  background: #c7d2fe;
  cursor: not-allowed;
}

.test-btn {
  background: #fff;
  border-color: #d1d5db;
  color: #4b5563;
}

.test-btn:hover:not(:disabled) {
  border-color: #6366f1;
  color: #6366f1;
}

.test-btn:disabled {
  cursor: not-allowed;
  opacity: 0.6;
}

.test-result {
  margin-top: 12px;
  padding: 10px 14px;
  border-radius: 6px;
  font-size: 13px;
}

.test-result--success {
  background: #ecfdf5;
  border: 1px solid #d1fae5;
  color: #059669;
}

.test-result--error {
  background: #fef2f2;
  border: 1px solid #fecaca;
  color: #dc2626;
}
</style>
