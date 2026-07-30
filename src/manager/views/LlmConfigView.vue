<script setup lang="ts">
// TODO 人工审查点：1.表单校验 2.密钥安全显示 3.连通性测试状态 4.保存反馈
// NOTE LLM 配置页：接口地址/密钥/模型/温度/max_tokens + 连通性测试
import { ref, onMounted } from "vue";
import {
  NCard, NForm, NFormItem, NInput, NInputNumber, NButton, NSpace, NTag, useMessage,
} from "naive-ui";
import { getAllSettings, setSetting, testLlmConnection } from "@/lib/api";

const message = useMessage();

// 配置键名
const KEY_BASE_URL = "llm_base_url";
const KEY_API_KEY = "llm_api_key";
const KEY_MODEL = "llm_model";
const KEY_TEMPERATURE = "llm_temperature";
const KEY_MAX_TOKENS = "llm_max_tokens";

// 表单数据
const baseUrl = ref("");
const apiKey = ref("");
const model = ref("");
const temperature = ref(0.8);
const maxTokens = ref(1024);

// 状态
const loading = ref(false);
const testing = ref(false);
const testResult = ref<{ ok: boolean; msg: string } | null>(null);

// 加载配置
onMounted(async () => {
  loading.value = true;
  try {
    const settings = await getAllSettings();
    baseUrl.value = settings[KEY_BASE_URL] ?? "";
    apiKey.value = settings[KEY_API_KEY] ?? "";
    model.value = settings[KEY_MODEL] ?? "";
    temperature.value = settings[KEY_TEMPERATURE] ? parseFloat(settings[KEY_TEMPERATURE]) : 0.8;
    maxTokens.value = settings[KEY_MAX_TOKENS] ? parseInt(settings[KEY_MAX_TOKENS]) : 1024;
  } catch (e) {
    message.error(`加载配置失败: ${e}`);
  } finally {
    loading.value = false;
  }
});

// 保存配置
async function handleSave(): Promise<void> {
  loading.value = true;
  try {
    await setSetting(KEY_BASE_URL, baseUrl.value);
    await setSetting(KEY_API_KEY, apiKey.value);
    await setSetting(KEY_MODEL, model.value);
    await setSetting(KEY_TEMPERATURE, temperature.value.toString());
    await setSetting(KEY_MAX_TOKENS, maxTokens.value.toString());
    message.success("配置已保存");
  } catch (e) {
    message.error(`保存失败: ${e}`);
  } finally {
    loading.value = false;
  }
}

// 测试连通性
async function handleTest(): Promise<void> {
  testing.value = true;
  testResult.value = null;
  try {
    // 先保存再测试
    await handleSave();
    const result = await testLlmConnection();
    testResult.value = {
      ok: result.ok,
      msg: `${result.message}（${result.latency_ms}ms）`,
    };
    if (result.ok) {
      message.success("连接成功");
    } else {
      message.error(`连接失败: ${result.message}`);
    }
  } catch (e) {
    testResult.value = { ok: false, msg: String(e) };
    message.error(`连接失败: ${e}`);
  } finally {
    testing.value = false;
  }
}
</script>

<template>
  <div>
    <h2 style="margin-bottom: 16px">LLM 配置</h2>

    <NCard :bordered="false" style="max-width: 640px">
      <NForm label-placement="left" :label-width="100">
        <NFormItem label="接口地址">
          <NInput
            v-model:value="baseUrl"
            placeholder="https://api.openai.com"
            :disabled="loading"
          />
        </NFormItem>

        <NFormItem label="API Key">
          <NInput
            v-model:value="apiKey"
            type="password"
            show-password-on="click"
            placeholder="sk-..."
            :disabled="loading"
          />
        </NFormItem>

        <NFormItem label="模型名称">
          <NInput
            v-model:value="model"
            placeholder="gpt-4o-mini"
            :disabled="loading"
          />
        </NFormItem>

        <NFormItem label="温度">
          <NInputNumber
            v-model:value="temperature"
            :min="0"
            :max="2"
            :step="0.1"
            :disabled="loading"
            style="width: 100%"
          />
        </NFormItem>

        <NFormItem label="最大长度">
          <NInputNumber
            v-model:value="maxTokens"
            :min="1"
            :max="8192"
            :step="128"
            :disabled="loading"
            style="width: 100%"
          />
        </NFormItem>

        <NFormItem :label="' '">
          <NSpace>
            <NButton type="primary" :loading="loading" @click="handleSave">
              保存
            </NButton>
            <NButton :loading="testing" @click="handleTest">
              测试连通性
            </NButton>
          </NSpace>
        </NFormItem>

        <!-- 测试结果 -->
        <NFormItem v-if="testResult" :label="' '">
          <NSpace align="center">
            <NTag :type="testResult.ok ? 'success' : 'error'">
              {{ testResult.ok ? "成功" : "失败" }}
            </NTag>
            <span style="font-size: 13px; color: var(--st-text-soft)">
              {{ testResult.msg }}
            </span>
          </NSpace>
        </NFormItem>
      </NForm>
    </NCard>
  </div>
</template>
