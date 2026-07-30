<script setup lang="ts">
// TODO 人工审查点：1.输入校验 2.批量导入解析健壮性 3.删除确认 4.分类缓存刷新
// NOTE 词库管理页：列表/搜索/分类筛选/启禁用/CRUD/批量导入/导出 JSON
import { ref, computed, h, onMounted } from "vue";
import {
  NButton, NSpace, NInput, NSelect, NSwitch, NModal, NForm, NFormItem,
  NPopconfirm, NTag, NDataTable, NCard, useMessage,
} from "naive-ui";
import type { DataTableColumns } from "naive-ui";
import {
  wordList, wordCreate, wordUpdate, wordDelete, wordToggleEnable,
  wordBatchImport, wordExportJson, wordCategories,
  type WordEntry, type BatchImportEntry, type BatchResult,
} from "@/lib/api";

const message = useMessage();

// 列表数据
const list = ref<WordEntry[]>([]);
const categories = ref<string[]>([]);
const loading = ref(false);

// 筛选条件
const searchText = ref("");
const filterCategory = ref<string>("");
const enabledOnly = ref(false);

// 编辑弹窗
const showEdit = ref(false);
const editingId = ref<number | null>(null);
const editWord = ref("");
const editCategory = ref("");
const saving = ref(false);

// 批量导入弹窗
const showImport = ref(false);
const importText = ref("");
const importCategory = ref("通用");
const importing = ref(false);

// 导出预览弹窗
const showExport = ref(false);
const exportText = ref("");

const isEditing = computed(() => editingId.value !== null);

// 分类下拉选项
const categoryOptions = computed(() => {
  const opts = categories.value.map((c) => ({ label: c, value: c }));
  return [{ label: "全部分类", value: "" }, ...opts];
});

// 加载列表
async function loadList(): Promise<void> {
  loading.value = true;
  try {
    list.value = await wordList(
      searchText.value || undefined,
      filterCategory.value || undefined,
      enabledOnly.value,
    );
  } catch (e) {
    message.error(`加载失败: ${e}`);
  } finally {
    loading.value = false;
  }
}

// 加载分类
async function loadCategories(): Promise<void> {
  try {
    categories.value = await wordCategories();
  } catch (e) {
    message.error(`分类加载失败: ${e}`);
  }
}

onMounted(async () => {
  await Promise.all([loadList(), loadCategories()]);
});

// 搜索防抖
let searchTimer: ReturnType<typeof setTimeout> | null = null;
function onSearchInput(): void {
  if (searchTimer) clearTimeout(searchTimer);
  searchTimer = setTimeout(() => loadList(), 300);
}

// 新增
function openCreate(): void {
  editingId.value = null;
  editWord.value = "";
  editCategory.value = categories.value[0] ?? "";
  showEdit.value = true;
}

// 编辑
function openEdit(row: WordEntry): void {
  editingId.value = row.id;
  editWord.value = row.word;
  editCategory.value = row.category;
  showEdit.value = true;
}

// 保存（新增/编辑）
async function handleSave(): Promise<void> {
  const word = editWord.value.trim();
  if (!word) {
    message.warning("请输入词条内容");
    return;
  }
  saving.value = true;
  try {
    if (isEditing.value && editingId.value !== null) {
      await wordUpdate(editingId.value, word, editCategory.value.trim());
      message.success("已更新");
    } else {
      await wordCreate(word, editCategory.value.trim());
      message.success("已新增");
    }
    showEdit.value = false;
    await Promise.all([loadList(), loadCategories()]);
  } catch (e) {
    message.error(`保存失败: ${e}`);
  } finally {
    saving.value = false;
  }
}

// 删除
async function handleDelete(id: number): Promise<void> {
  try {
    await wordDelete(id);
    message.success("已删除");
    await Promise.all([loadList(), loadCategories()]);
  } catch (e) {
    message.error(`删除失败: ${e}`);
  }
}

// 启禁用切换
async function handleToggle(row: WordEntry, enabled: boolean): Promise<void> {
  try {
    await wordToggleEnable(row.id!, enabled);
    row.enabled = enabled;
  } catch (e) {
    message.error(`切换失败: ${e}`);
  }
}

// ===== 批量导入 =====

function openImport(): void {
  importText.value = "";
  importCategory.value = "通用";
  showImport.value = true;
}

/// 解析导入文本：优先 JSON 数组，否则按 CSV/纯文本逐行解析
function parseImportText(): BatchImportEntry[] {
  const text = importText.value.trim();
  if (!text) return [];
  // JSON 数组格式
  if (text.startsWith("[")) {
    let arr: Array<{ word?: unknown; category?: unknown }>;
    try {
      arr = JSON.parse(text);
    } catch {
      throw new Error("JSON 格式错误，请检查");
    }
    return arr
      .filter((it) => it && typeof it.word === "string" && it.word.trim())
      .map((it) => ({
        word: String(it.word).trim(),
        category:
          typeof it.category === "string" && it.category.trim()
            ? it.category.trim()
            : importCategory.value,
      }));
  }
  // CSV / 纯文本：每行一个词，可用 , 或 ， 分隔分类
  return text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((line) => {
      const parts = line.split(/[,，]/).map((s) => s.trim());
      const w = parts[0] ?? "";
      const c = parts[1] || importCategory.value;
      return { word: w, category: c };
    })
    .filter((it) => it.word.length > 0);
}

async function handleImport(): Promise<void> {
  let entries: BatchImportEntry[];
  try {
    entries = parseImportText();
  } catch (e) {
    message.error(`解析失败: ${e}`);
    return;
  }
  if (entries.length === 0) {
    message.warning("未解析到任何词条");
    return;
  }
  importing.value = true;
  try {
    const result: BatchResult = await wordBatchImport(entries);
    message.success(
      `导入完成：成功 ${result.imported}，跳过 ${result.skipped}，错误 ${result.errors.length}`,
    );
    if (result.errors.length > 0) {
      console.warn("导入错误详情:", result.errors);
    }
    showImport.value = false;
    await Promise.all([loadList(), loadCategories()]);
  } catch (e) {
    message.error(`导入失败: ${e}`);
  } finally {
    importing.value = false;
  }
}

// ===== 导出 =====

async function handleExport(): Promise<void> {
  try {
    const json = await wordExportJson();
    // 尝试触发浏览器下载
    try {
      const blob = new Blob([json], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "seltalk_words.json";
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
      message.success("已导出下载");
    } catch {
      // 下载不可用时展示文本供复制
      exportText.value = json;
      showExport.value = true;
    }
  } catch (e) {
    message.error(`导出失败: ${e}`);
  }
}

// 表格列定义
const columns = computed<DataTableColumns<WordEntry>>(() => [
  {
    title: "词条",
    key: "word",
    render: (row) => h("span", { style: "word-break: break-all" }, row.word),
  },
  {
    title: "分类",
    key: "category",
    width: 140,
    render: (row) =>
      row.category
        ? h(NTag, { size: "small", type: "info" }, { default: () => row.category })
        : h("span", { style: "color: var(--st-text-soft)" }, "—"),
  },
  {
    title: "启用",
    key: "enabled",
    width: 80,
    render: (row) =>
      h(NSwitch, {
        value: row.enabled,
        size: "small",
        onUpdateValue: (v: boolean) => handleToggle(row, v),
      }),
  },
  {
    title: "操作",
    key: "actions",
    width: 140,
    render: (row) =>
      h(NSpace, { size: 8 }, {
        default: () => [
          h(NButton, { size: "small", onClick: () => openEdit(row) }, { default: () => "编辑" }),
          h(NPopconfirm, { onPositiveClick: () => handleDelete(row.id!) }, {
            trigger: () =>
              h(NButton, { size: "small", type: "error", ghost: true }, { default: () => "删除" }),
            default: () => `确认删除「${row.word}」？`,
          }),
        ],
      }),
  },
]);
</script>

<template>
  <div>
    <div
      style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 16px"
    >
      <h2>词库管理</h2>
      <NSpace>
        <NButton @click="openImport">批量导入</NButton>
        <NButton @click="handleExport">导出 JSON</NButton>
        <NButton type="primary" @click="openCreate">新增词条</NButton>
      </NSpace>
    </div>

    <!-- 筛选栏 -->
    <NCard :bordered="false" size="small" style="margin-bottom: 12px">
      <NSpace align="center">
        <NInput
          v-model:value="searchText"
          placeholder="搜索词条…"
          clearable
          style="width: 220px"
          @update:value="onSearchInput"
        />
        <NSelect
          v-model:value="filterCategory"
          :options="categoryOptions"
          style="width: 180px"
          @update:value="loadList"
        />
        <NSpace align="center">
          <NSwitch v-model:value="enabledOnly" size="small" @update:value="loadList" />
          <span style="font-size: 13px; color: var(--st-text-soft)">仅启用</span>
        </NSpace>
        <span style="font-size: 13px; color: var(--st-text-soft)">共 {{ list.length }} 条</span>
      </NSpace>
    </NCard>

    <!-- 列表 -->
    <NDataTable
      :columns="columns"
      :data="list"
      :loading="loading"
      :bordered="true"
      :single-line="false"
      size="small"
      :pagination="{ pageSize: 20 }"
    />

    <!-- 编辑弹窗 -->
    <NModal
      v-model:show="showEdit"
      :title="isEditing ? '编辑词条' : '新增词条'"
      preset="card"
      style="width: 480px; max-width: 90vw"
    >
      <NForm label-placement="top">
        <NFormItem label="词条内容">
          <NInput
            v-model:value="editWord"
            type="textarea"
            :autosize="{ minRows: 2, maxRows: 6 }"
            placeholder="输入回复词或短语…"
          />
        </NFormItem>
        <NFormItem label="分类">
          <NInput v-model:value="editCategory" placeholder="如：问候 / 告别（可留空）" />
        </NFormItem>
      </NForm>
      <template #footer>
        <NSpace justify="end">
          <NButton @click="showEdit = false">取消</NButton>
          <NButton type="primary" :loading="saving" @click="handleSave">保存</NButton>
        </NSpace>
      </template>
    </NModal>

    <!-- 批量导入弹窗 -->
    <NModal
      v-model:show="showImport"
      title="批量导入"
      preset="card"
      style="width: 640px; max-width: 90vw"
    >
      <NSpace vertical :size="12">
        <div style="font-size: 13px; color: var(--st-text-soft); line-height: 1.7">
          支持两种格式：<br />
          1. JSON 数组：<code>[{"word":"你好","category":"问候"}]</code><br />
          2. 纯文本 / CSV：每行一个词，可用逗号附加分类，如 <code>你好,问候</code>。未带分类则使用下方默认分类。
        </div>
        <NInput
          v-model:value="importCategory"
          placeholder="默认分类"
          style="width: 200px"
        />
        <NInput
          v-model:value="importText"
          type="textarea"
          :autosize="{ minRows: 8, maxRows: 20 }"
          placeholder="粘贴 JSON 或每行一个词条…"
        />
      </NSpace>
      <template #footer>
        <NSpace justify="end">
          <NButton @click="showImport = false">取消</NButton>
          <NButton type="primary" :loading="importing" @click="handleImport">导入</NButton>
        </NSpace>
      </template>
    </NModal>

    <!-- 导出预览弹窗 -->
    <NModal
      v-model:show="showExport"
      title="导出 JSON"
      preset="card"
      style="width: 640px; max-width: 90vw"
    >
      <NInput
        :value="exportText"
        type="textarea"
        :autosize="{ minRows: 10, maxRows: 24 }"
        readonly
      />
      <template #footer>
        <NSpace justify="end">
          <NButton @click="showExport = false">关闭</NButton>
        </NSpace>
      </template>
    </NModal>
  </div>
</template>
