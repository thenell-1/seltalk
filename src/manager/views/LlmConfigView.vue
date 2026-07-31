<script setup lang="ts">
// TODO 人工审查点：1.下拉切换即时生效 2.密钥安全显示 3.保存/另存为/删除分支 4.连通性测试 5.空配置兜底
// NOTE LLM 配置页：多份命名配置（llm_profiles）+ 下拉快速切换（选中即设为 active）+ 连通性测试
//       下拉点击历史配置 → llmProfileSetActive → 重新加载表单，主链路立即使用该配置
import { ref, computed, onMounted } from "vue";
import {
  NCard, NForm, NFormItem, NInput, NInputNumber, NButton, NSpace, NTag,
  NSelect, NSwitch, NPopconfirm, useMessage,
} from "naive-ui";
import {
  llmProfileList, getActiveLlmProfile, llmProfileCreate, llmProfileUpdate,
  llmProfileDelete, llmProfileSetActive, testLlmConnection,
  type LlmProfile, type LlmProfileInput,
} from "@/lib/api";

const message = useMessage();

// 模型类型常用项（filterable + tag 允许自定义输入）
const MODEL_TYPE_OPTIONS = [
  { label: "OpenAI", value: "openai" },
  { label: "Anthropic", value: "anthropic" },
  { label: "Azure OpenAI", value: "azure" },
  { label: "DeepSeek", value: "deepseek" },
  { label: "通义千问", value: "qwen" },
  { label: "本地模型 (Ollama 等)", value: "local" },
];

// 列表与当前生效
const profiles = ref<LlmProfile[]>([]);
const activeId = ref<number | null>(null);
const editingId = ref<number | null>(null);

// 表单
const form = ref<LlmProfileInput>(emptyForm());

// 状态
const loading = ref(false);
const testing = ref(false);
const testResult = ref<{ ok: boolean; msg: string } | null>(null);

function emptyForm(): LlmProfileInput {
  return {
    name: "",
    base_url: "",
    api_key: "",
    model: "",
    model_type: "",
    temperature: 0.6,
    max_tokens: 1024,
    max_context_length: 0,
    stream_enabled: true,
  };
}

// 历史配置下拉选项：名称（模型），过滤无 id 的异常记录
const profileOptions = computed(() =>
  profiles.value
    .filter((p) => p.id != null)
    .map((p) => ({
      label: `${p.name}${p.model ? `（${p.model}）` : "（未设置模型）"}`,
      value: p.id as number,
    })),
);

// 模型类型下拉选项：当前值不在预设列表时补一项，保证可显示
const modelTypeOptions = computed(() => {
  const opts = [...MODEL_TYPE_OPTIONS];
  if (form.value.model_type && !opts.some((o) => o.value === form.value.model_type)) {
    opts.unshift({ label: form.value.model_type, value: form.value.model_type });
  }
  return opts;
});

function fillFromProfile(p: LlmProfile): void {
  form.value = {
    name: p.name,
    base_url: p.base_url,
    api_key: p.api_key,
    model: p.model,
    model_type: p.model_type,
    temperature: p.temperature,
    max_tokens: p.max_tokens,
    max_context_length: p.max_context_length,
    stream_enabled: p.stream_enabled,
  };
  editingId.value = p.id;
}

async function reloadAll(): Promise<void> {
  profiles.value = await llmProfileList();
  const active = await getActiveLlmProfile();
  if (active) {
    activeId.value = active.id;
    fillFromProfile(active);
  } else {
    activeId.value = null;
    editingId.value = null;
    form.value = emptyForm();
  }
}

onMounted(async () => {
  loading.value = true;
  try {
    await reloadAll();
  } catch (e) {
    message.error(`加载配置失败: ${e}`);
  } finally {
    loading.value = false;
  }
});

// 下拉切换 → 立即设为 active → 重新加载表单
async function handleSelectProfile(value: string | number | null): Promise<void> {
  if (value == null) return;
  const id = Number(value);
  loading.value = true;
  try {
    await llmProfileSetActive(id);
    await reloadAll();
    message.success("已切换为该配置");
  } catch (e) {
    message.error(`切换失败: ${e}`);
    // 回退下拉显示到当前 editing
    activeId.value = editingId.value;
  } finally {
    loading.value = false;
  }
}

// 保存：有 editingId 则更新，否则新建（新建即设为 active）
async function handleSave(): Promise<void> {
  loading.value = true;
  try {
    if (editingId.value != null) {
      await llmProfileUpdate(editingId.value, form.value);
      message.success("配置已保存");
    } else {
      const id = await llmProfileCreate(form.value);
      editingId.value = id;
      message.success("已创建并设为当前配置");
    }
    await reloadAll();
  } catch (e) {
    message.error(`保存失败: ${e}`);
  } finally {
    loading.value = false;
  }
}

// 另存为新配置（始终新建，设为 active）
async function handleSaveAs(): Promise<void> {
  loading.value = true;
  try {
    const id = await llmProfileCreate(form.value);
    editingId.value = id;
    await reloadAll();
    message.success("已另存为新配置并设为当前");
  } catch (e) {
    message.error(`另存失败: ${e}`);
  } finally {
    loading.value = false;
  }
}

// 删除当前编辑的配置（删除 active 时后端自动提升剩余首条）
async function handleDelete(): Promise<void> {
  if (editingId.value == null) return;
  loading.value = true;
  try {
    await llmProfileDelete(editingId.value);
    await reloadAll();
    message.success("已删除");
  } catch (e) {
    message.error(`删除失败: ${e}`);
  } finally {
    loading.value = false;
  }
}

// 测试连通性（先保存使 active 配置生效，再测）
async function handleTest(): Promise<void> {
  testing.value = true;
  testResult.value = null;
  try {
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
      <!-- 历史配置下拉：选中即切换为当前生效配置 -->
      <NFormItem label="历史配置" :label-width="100" style="margin-bottom: 20px">
        <NSelect
          :value="activeId"
          :options="profileOptions"
          placeholder="暂无已保存配置，填写下方表单后点击「保存」即可创建"
          :disabled="loading || profiles.length === 0"
          @update:value="handleSelectProfile"
        />
      </NFormItem>

      <NForm label-placement="left" :label-width="100">
        <NFormItem label="配置名称">
          <NInput
            v-model:value="form.name"
            placeholder="如：工作用 GPT-4o"
            :disabled="loading"
          />
        </NFormItem>

        <NFormItem label="接口地址">
          <NInput
            v-model:value="form.base_url"
            placeholder="https://api.openai.com"
            :disabled="loading"
          />
        </NFormItem>

        <NFormItem label="API Key">
          <NInput
            v-model:value="form.api_key"
            type="password"
            show-password-on="click"
            placeholder="sk-..."
            :disabled="loading"
          />
        </NFormItem>

        <NFormItem label="模型名称">
          <NInput
            v-model:value="form.model"
            placeholder="gpt-4o-mini"
            :disabled="loading"
          />
        </NFormItem>

        <NFormItem label="模型类型">
          <NSelect
            v-model:value="form.model_type"
            :options="modelTypeOptions"
            filterable
            tag
            placeholder="选择或输入模型类型"
            :disabled="loading"
          />
        </NFormItem>

        <NFormItem label="温度">
          <NInputNumber
            v-model:value="form.temperature"
            :min="0"
            :max="2"
            :step="0.1"
            :disabled="loading"
            style="width: 100%"
          />
        </NFormItem>

        <NFormItem label="最大输出长度">
          <NInputNumber
            v-model:value="form.max_tokens"
            :min="1"
            :max="8192"
            :step="128"
            :disabled="loading"
            style="width: 100%"
          />
        </NFormItem>

        <NFormItem label="最大上下文长度">
          <NInputNumber
            v-model:value="form.max_context_length"
            :min="0"
            :step="1024"
            :disabled="loading"
            style="width: 100%"
          >
            <template #suffix>0 = 不限</template>
          </NInputNumber>
        </NFormItem>

        <NFormItem label="流式输出">
          <NSwitch v-model:value="form.stream_enabled" :disabled="loading" />
        </NFormItem>

        <NFormItem :label="' '">
          <NSpace>
            <NButton type="primary" :loading="loading" @click="handleSave">
              保存
            </NButton>
            <NButton :loading="loading" @click="handleSaveAs">
              另存为新配置
            </NButton>
            <NPopconfirm @positive-click="handleDelete">
              <template #trigger>
                <NButton
                  :loading="loading"
                  :disabled="editingId == null"
                  type="error"
                  ghost
                >
                  删除
                </NButton>
              </template>
              确认删除当前配置？删除后不可恢复。
            </NPopconfirm>
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
