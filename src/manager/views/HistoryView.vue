<script setup lang="ts">
// TODO 人工审查点：1.分页参数一致性 2.删除确认交互 3.搜索防抖 4.空状态处理
// NOTE 历史记录页：按时间倒序展示用户选中的候选回复，支持搜索/分页/删除/清空
//       场景：用户在悬浮窗选中候选 → orchestrator 异步写入 → 此页按时间倒序查询
import { ref, computed, h, onMounted } from "vue";
import {
  NButton, NSpace, NInput, NTag, NDataTable, NCard, NEmpty,
  NPopconfirm, NTooltip, useMessage, useDialog,
} from "naive-ui";
import type { DataTableColumns } from "naive-ui";
import {
  historyList, historyDelete, historyClear,
  type HistoryEntry,
} from "@/lib/api";

const message = useMessage();
const dialog = useDialog();

// ===== 响应式状态 =====

const list = ref<HistoryEntry[]>([]);
const total = ref(0);
const loading = ref(false);
const clearing = ref(false);

// 搜索 + 分页参数
const searchText = ref("");
const page = ref(1);              // 1-based 页码
const pageSize = ref(20);          // 每页条数

// ===== 工具函数 =====

/** 格式化 RFC3339 时间为本地可读格式（YYYY-MM-DD HH:mm:ss） */
function formatTime(rfc3339: string): string {
  try {
    const d = new Date(rfc3339);
    if (Number.isNaN(d.getTime())) return rfc3339;
    const pad = (n: number): string => n.toString().padStart(2, "0");
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} `
         + `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
  } catch {
    return rfc3339;
  }
}

/** 文本截断（超过 maxLen 字符则截断并加省略号） */
function truncate(text: string, maxLen: number): string {
  if (text.length <= maxLen) return text;
  return text.slice(0, maxLen) + "…";
}

// ===== 数据加载 =====

/** 加载历史记录列表（按当前搜索 + 分页参数） */
async function loadData(): Promise<void> {
  loading.value = true;
  try {
    const offset = (page.value - 1) * pageSize.value;
    const result = await historyList(
      searchText.value || undefined,
      pageSize.value,
      offset,
    );
    list.value = result.items;
    total.value = result.total;
  } catch (e) {
    message.error(`加载历史记录失败: ${e}`);
  } finally {
    loading.value = false;
  }
}

// ===== 搜索防抖 =====

let searchTimer: ReturnType<typeof setTimeout> | null = null;
function onSearchInput(): void {
  if (searchTimer) clearTimeout(searchTimer);
  searchTimer = setTimeout(() => {
    // 搜索时重置到第一页
    page.value = 1;
    void loadData();
  }, 300);
}

// ===== 交互处理 =====

/** 删除单条历史记录 */
async function handleDelete(row: HistoryEntry): Promise<void> {
  if (row.id === null) return;
  try {
    await historyDelete(row.id);
    message.success("已删除");
    // 若当前页删除后空了，回退到上一页
    if (list.value.length === 1 && page.value > 1) {
      page.value -= 1;
    }
    await loadData();
  } catch (e) {
    message.error(`删除失败: ${e}`);
  }
}

/** 清空全部历史记录（二次确认） */
function handleClear(): void {
  dialog.warning({
    title: "清空历史记录",
    content: "确定清空全部历史记录吗？此操作不可撤销。",
    positiveText: "确定清空",
    negativeText: "取消",
    onPositiveClick: async (): Promise<void> => {
      clearing.value = true;
      try {
        await historyClear();
        message.success("历史记录已清空");
        page.value = 1;
        await loadData();
      } catch (e) {
        message.error(`清空失败: ${e}`);
      } finally {
        clearing.value = false;
      }
    },
  });
}

// ===== 分页变更处理 =====

function onPageChange(p: number): void {
  page.value = p;
  void loadData();
}

function onPageSizeChange(ps: number): void {
  pageSize.value = ps;
  page.value = 1;
  void loadData();
}

// ===== 表格列定义 =====

const columns = computed<DataTableColumns<HistoryEntry>>(() => [
  {
    title: "时间",
    key: "created_at",
    width: 170,
    render: (row) => formatTime(row.created_at),
  },
  {
    title: "原始文本",
    key: "origin",
    render: (row) => h(
      NTooltip,
      { style: { maxWidth: "500px" } },
      {
        trigger: () => h("span", truncate(row.origin, 40)),
        default: () => row.origin,
      },
    ),
  },
  {
    title: "选中候选",
    key: "selected",
    render: (row) => h(
      NTooltip,
      { style: { maxWidth: "500px" } },
      {
        trigger: () => h("span", truncate(row.selected, 40)),
        default: () => row.selected,
      },
    ),
  },
  {
    title: "Prompt 模板",
    key: "prompt_name",
    width: 140,
    render: (row) => row.prompt_name
      ? h(NTag, { size: "small", type: "info", round: true }, () => truncate(row.prompt_name, 12))
      : h("span", { style: "color: var(--st-text-soft)" }, "—"),
  },
  {
    title: "模型",
    key: "model",
    width: 140,
    render: (row) => row.model
      ? h(NTag, { size: "small", type: "default", round: true }, () => truncate(row.model, 16))
      : h("span", { style: "color: var(--st-text-soft)" }, "—"),
  },
  {
    title: "操作",
    key: "actions",
    width: 80,
    render: (row) => h(
      NPopconfirm,
      {
        onPositiveClick: () => handleDelete(row),
      },
      {
        trigger: () => h(
          NButton,
          { size: "tiny", type: "error", ghost: true },
          () => "删除",
        ),
        default: () => "确定删除该条记录？",
      },
    ),
  },
]);

// ===== 生命周期 =====

onMounted(async () => {
  await loadData();
});
</script>

<template>
  <div>
    <h2 style="margin-bottom: 16px">历史记录</h2>

    <!-- 操作栏：搜索 + 刷新 + 清空 -->
    <NSpace align="center" justify="space-between" style="margin-bottom: 12px">
      <NSpace align="center">
        <NInput
          v-model:value="searchText"
          placeholder="搜索原文或选中候选..."
          clearable
          style="width: 320px"
          @update:value="onSearchInput"
        />
        <span style="font-size: 13px; color: var(--st-text-soft)">
          共 {{ total }} 条记录
        </span>
      </NSpace>
      <NSpace>
        <NButton size="small" :loading="loading" @click="loadData">刷新</NButton>
        <NButton
          size="small"
          type="error"
          ghost
          :loading="clearing"
          :disabled="total === 0"
          @click="handleClear"
        >
          清空全部
        </NButton>
      </NSpace>
    </NSpace>

    <!-- 历史记录表格 -->
    <NCard :bordered="false">
      <NEmpty
        v-if="!loading && list.length === 0"
        description="暂无历史记录 · 在悬浮窗选中候选回复后将自动记录"
        style="padding: 80px 0"
      >
        <template #extra>
          <NButton size="small" @click="loadData">刷新</NButton>
        </template>
      </NEmpty>
      <NDataTable
        v-else
        :columns="columns"
        :data="list"
        :loading="loading"
        :row-key="(row: HistoryEntry) => row.id ?? 0"
        :bordered="false"
        :single-line="false"
        size="small"
        :pagination="{
          page: page,
          pageSize: pageSize,
          itemCount: total,
          showSizePicker: true,
          pageSizes: [10, 20, 50, 100],
          onChange: onPageChange,
          onUpdatePageSize: onPageSizeChange,
        }"
      />
    </NCard>
  </div>
</template>
