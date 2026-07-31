<script setup lang="ts">
// TODO 人工审查点：1.模板CRUD操作 2.默认模板切换 3.变量渲染预览 4.删除确认 5.标签录入与回显
// NOTE Prompt 模板管理页：列表/新建/编辑/删除/设默认 + 标签分类 + 变量提取与渲染预览
import { ref, computed, onMounted, watch } from "vue";
import {
  NCard, NButton, NSpace, NInput, NTag, NModal, NForm, NFormItem,
  NPopconfirm, NCollapse, NCollapseItem, NDynamicTags, useMessage,
} from "naive-ui";
import {
  promptList, promptCreate, promptUpdate, promptDelete, promptSetDefault,
  promptExtractVariables, promptRenderPreview, promptAllTags,
  type PromptTemplate,
} from "@/lib/api";

const message = useMessage();

// 数据
const templates = ref<PromptTemplate[]>([]);
const loading = ref(false);

// 编辑弹窗
const showModal = ref(false);
const editingId = ref<number | null>(null);
const editingName = ref("");
const editingTemplate = ref("");
// 标签数组（编辑时用 NDynamicTags 录入，保存时 join 成逗号串传后端）
const editingTags = ref<string[]>([]);
// 全库去重标签列表（供 NDynamicTags 自动补全建议）
const allTags = ref<string[]>([]);

const isEditing = computed(() => editingId.value !== null);
const modalTitle = computed(() => isEditing.value ? "编辑模板" : "新建模板");

// 加载列表
async function loadList(): Promise<void> {
  loading.value = true;
  try {
    templates.value = await promptList();
  } catch (e) {
    message.error(`加载失败: ${e}`);
  } finally {
    loading.value = false;
  }
}

/// 加载全库去重标签（供编辑弹窗自动补全）
async function loadAllTags(): Promise<void> {
  try {
    allTags.value = await promptAllTags();
  } catch (e) {
    console.warn("加载标签列表失败:", e);
  }
}

/// 解析标签字符串为数组（逗号分隔）
function parseTags(tags: string): string[] {
  return tags
    .split(",")
    .map((t) => t.trim())
    .filter((t) => t.length > 0);
}

onMounted(() => {
  void loadList();
  void loadAllTags();
});

// 打开新建弹窗
function handleCreate(): void {
  editingId.value = null;
  editingName.value = "";
  editingTemplate.value = "你是一个聊天回复助手。请根据下面的对话上下文，生成 {{n}} 条简短、自然、口语化的回复候选。每条回复独占一行，用 --- 分隔。\n\n对话上下文：\n{{origin}}";
  editingTags.value = [];
  showModal.value = true;
  resetPreview();
  void refreshVariables();
}

// 打开编辑弹窗
function handleEdit(item: PromptTemplate): void {
  editingId.value = item.id;
  editingName.value = item.name;
  editingTemplate.value = item.template;
  // 回显标签：逗号串 → 数组
  editingTags.value = parseTags(item.tags);
  showModal.value = true;
  resetPreview();
  void refreshVariables();
}

// 保存（新建/编辑）
async function handleSave(): Promise<void> {
  if (!editingName.value.trim()) {
    message.warning("请输入模板名称");
    return;
  }
  if (!editingTemplate.value.trim()) {
    message.warning("请输入模板内容");
    return;
  }

  try {
    // 标签数组 → 逗号串（后端存储格式）
    const tagsStr = editingTags.value.join(",");
    if (isEditing.value && editingId.value !== null) {
      await promptUpdate(editingId.value, editingName.value, editingTemplate.value, tagsStr);
      message.success("模板已更新");
    } else {
      await promptCreate(editingName.value, editingTemplate.value, tagsStr);
      message.success("模板已创建");
    }
    showModal.value = false;
    await loadList();
    // 刷新全库标签（新标签可能加入）
    void loadAllTags();
  } catch (e) {
    message.error(`保存失败: ${e}`);
  }
}

// 删除
async function handleDelete(id: number): Promise<void> {
  try {
    await promptDelete(id);
    message.success("已删除");
    await loadList();
  } catch (e) {
    message.error(`删除失败: ${e}`);
  }
}

// 设为默认
async function handleSetDefault(id: number): Promise<void> {
  try {
    await promptSetDefault(id);
    message.success("已设为默认");
    await loadList();
  } catch (e) {
    message.error(`设置失败: ${e}`);
  }
}

// ===== 变量渲染预览 =====

/// 内置变量默认示例值（用户可在预览面板覆盖）
const DEFAULT_VAR_SAMPLES: Record<string, string> = {
  origin: "对方说：在吗？最近怎么样",
  n: "3",
  words: "你好、在的、最近挺好的",
};

const extractedVars = ref<string[]>([]);
const previewVars = ref<Record<string, string>>({});
const previewResult = ref("");
const previewLoading = ref(false);

/// 提取模板变量并补齐预览值（保留用户已填写的值）
async function refreshVariables(): Promise<void> {
  const tpl = editingTemplate.value;
  if (!tpl.trim()) {
    extractedVars.value = [];
    return;
  }
  try {
    const vars = await promptExtractVariables(tpl);
    extractedVars.value = vars;
    // 补齐默认值，保留用户已输入的值
    const next: Record<string, string> = {};
    for (const v of vars) {
      next[v] = previewVars.value[v] ?? DEFAULT_VAR_SAMPLES[v] ?? "";
    }
    previewVars.value = next;
  } catch {
    // 静默失败，不影响编辑
  }
}

// 模板内容变化时防抖提取变量
let varTimer: ReturnType<typeof setTimeout> | null = null;
watch(editingTemplate, () => {
  if (varTimer) clearTimeout(varTimer);
  varTimer = setTimeout(() => refreshVariables(), 400);
});

/// 渲染预览
async function doPreview(): Promise<void> {
  if (!editingTemplate.value.trim()) {
    message.warning("请先输入模板内容");
    return;
  }
  previewLoading.value = true;
  try {
    previewResult.value = await promptRenderPreview(
      editingTemplate.value,
      previewVars.value,
    );
  } catch (e) {
    message.error(`渲染失败: ${e}`);
  } finally {
    previewLoading.value = false;
  }
}

/// 打开弹窗时重置预览状态
function resetPreview(): void {
  extractedVars.value = [];
  previewVars.value = {};
  previewResult.value = "";
}
</script>

<template>
  <div>
    <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 16px">
      <h2>Prompt 模板</h2>
      <NButton type="primary" @click="handleCreate">新建模板</NButton>
    </div>

    <!-- 变量说明 -->
    <NCard :bordered="false" size="small" style="margin-bottom: 16px">
      <div v-pre style="font-size: 13px; color: var(--st-text-soft); line-height: 1.6">
        <strong>可用变量：</strong>
        <code>{{origin}}</code> = 原始对话文本 ·
        <code>{{n}}</code> = 候选条数 ·
        <code>{{words}}</code> = 参考词库（阶段二）
      </div>
    </NCard>

    <!-- 模板列表 -->
    <NSpace vertical :size="12">
      <NCard
        v-for="(item, index) in templates"
        :key="item.id ?? index"
        :bordered="true"
        size="small"
      >
        <div style="display: flex; justify-content: space-between; align-items: center">
          <NSpace align="center">
            <NTag v-if="item.is_default" type="success" size="small">默认</NTag>
            <strong>{{ item.name }}</strong>
            <!-- 标签展示 -->
            <NTag
              v-for="tag in parseTags(item.tags)"
              :key="tag"
              size="small"
              :bordered="false"
              type="info"
            >
              {{ tag }}
            </NTag>
          </NSpace>
          <NSpace>
            <NButton v-if="!item.is_default" size="small" @click="handleSetDefault(item.id!)">
              设为默认
            </NButton>
            <NButton size="small" @click="handleEdit(item)">编辑</NButton>
            <NPopconfirm @positive-click="handleDelete(item.id!)">
              <template #trigger>
                <NButton size="small" type="error" ghost>删除</NButton>
              </template>
              确认删除「{{ item.name }}」？
            </NPopconfirm>
          </NSpace>
        </div>
        <!-- 模板预览（前2行） -->
        <div
          style="margin-top: 8px; font-size: 12px; color: var(--st-text-soft);
                 white-space: pre-wrap; max-height: 60px; overflow: hidden"
        >
          {{ item.template.slice(0, 200) }}{{ item.template.length > 200 ? "…" : "" }}
        </div>
      </NCard>
    </NSpace>

    <!-- 编辑弹窗 -->
    <NModal
      v-model:show="showModal"
      :title="modalTitle"
      preset="card"
      style="width: 640px; max-width: 90vw"
    >
      <NForm label-placement="top">
        <NFormItem label="模板名称">
          <NInput v-model:value="editingName" placeholder="如：简短回复模板" />
        </NFormItem>
        <NFormItem label="标签">
          <NDynamicTags
            v-model:value="editingTags"
            :max="10"
            placeholder="输入标签后回车（如：简短、正式、委婉、幽默）"
          />
        </NFormItem>
        <NFormItem label="模板内容">
          <NInput
            v-model:value="editingTemplate"
            type="textarea"
            :autosize="{ minRows: 8, maxRows: 20 }"
            placeholder="输入 Prompt 模板…"
          />
        </NFormItem>
      </NForm>

      <!-- 变量渲染预览 -->
      <NCollapse :default-expanded-names="['preview']" style="margin-top: 8px">
        <NCollapseItem name="preview" title="变量渲染预览">
          <NSpace vertical :size="10">
            <div v-if="extractedVars.length === 0" style="font-size: 12px; color: var(--st-text-soft)">
              模板中暂无 <code v-pre>{{var}}</code> 变量。
            </div>
            <NFormItem
              v-for="v in extractedVars"
              :key="v"
              :label="v"
              label-placement="left"
              :label-width="80"
            >
              <NInput
                v-model:value="previewVars[v]"
                :placeholder="`输入 ${v} 的示例值`"
              />
            </NFormItem>
            <div>
              <NButton size="small" :loading="previewLoading" @click="doPreview">
                渲染预览
              </NButton>
            </div>
            <NInput
              v-if="previewResult"
              :value="previewResult"
              type="textarea"
              :autosize="{ minRows: 4, maxRows: 16 }"
              readonly
              placeholder="渲染结果将显示在此处"
            />
          </NSpace>
        </NCollapseItem>
      </NCollapse>
      <template #footer>
        <NSpace justify="end">
          <NButton @click="showModal = false">取消</NButton>
          <NButton type="primary" @click="handleSave">保存</NButton>
        </NSpace>
      </template>
    </NModal>
  </div>
</template>
