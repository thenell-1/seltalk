<script setup lang="ts">
// TODO 人工审查点：1.ECharts 生命周期管理 2.词云点击交互 3.空状态处理 4.响应式 resize
// NOTE 高频词云页：ECharts + echarts-wordcloud 渲染，支持重置 + 点击词语加入词库
import { ref, onMounted, onUnmounted, nextTick, shallowRef } from "vue";
import {
  NCard, NButton, NSpace, NEmpty, NStatistic, NGrid, NGi, NTag, useMessage, useDialog,
} from "naive-ui";
import * as echarts from "echarts";
import "echarts-wordcloud";
import {
  wordFreqList, wordFreqReset, wordFreqOverview, wordCreate,
  type WordFreqEntry, type WordFreqOverview,
} from "@/lib/api";

const message = useMessage();
const dialog = useDialog();

// ===== 响应式状态 =====
const entries = ref<WordFreqEntry[]>([]);
const overview = ref<WordFreqOverview>({ total_words: 0, total_usage: 0 });
const loading = ref(false);
const resetting = ref(false);

// ECharts 实例（shallowRef 避免 Vue 深层代理，提升性能）
const chartRef = ref<HTMLDivElement | null>(null);
const chartInstance = shallowRef<echarts.ECharts | null>(null);

// ===== 数据加载 =====

/** 加载词频列表 + 概览统计 */
async function loadData(): Promise<void> {
  loading.value = true;
  try {
    const [list, ov] = await Promise.all([
      wordFreqList(200),
      wordFreqOverview(),
    ]);
    entries.value = list;
    overview.value = ov;
    renderChart();
  } catch (e) {
    message.error(`加载词频数据失败: ${e}`);
  } finally {
    loading.value = false;
  }
}

/** 渲染词云图表 */
function renderChart(): void {
  if (!chartRef.value || entries.value.length === 0) return;

  // 初始化或复用 ECharts 实例
  if (!chartInstance.value) {
    chartInstance.value = echarts.init(chartRef.value);
    // 点击词语 → 确认加入词库
    chartInstance.value.on("click", (params: { name?: string }) => {
      const word = params.name;
      if (word) {
        handleWordClick(word);
      }
    });
  }

  // 转换为 echarts-wordcloud 需要的数据格式
  const data = entries.value.map((e) => ({
    name: e.word,
    value: e.count,
  }));

  chartInstance.value.setOption({
    tooltip: {
      show: true,
      formatter: (params: { name: string; value: number }) =>
        `${params.name}: ${params.value} 次`,
    },
    series: [
      {
        type: "wordCloud",
        shape: "circle",
        left: "center",
        top: "center",
        width: "100%",
        height: "100%",
        sizeRange: [14, 60],
        rotationRange: [-30, 30],
        rotationStep: 30,
        gridSize: 8,
        drawOutOfBound: false,
        layoutAnimation: true,
        textStyle: {
          fontFamily: "sans-serif",
          fontWeight: "bold",
          color: (): string => {
            // 随机色板（蓝/青/绿/橙/紫系，柔和色调）
            const palette = [
              "#5470c6", "#91cc75", "#fac858", "#ee6666",
              "#73c0de", "#3ba272", "#fc8452", "#9a60b4",
            ];
            return palette[Math.floor(Math.random() * palette.length)];
          },
        },
        emphasis: {
          focus: "self",
          textStyle: {
            textShadowBlur: 10,
            textShadowColor: "rgba(0, 0, 0, 0.25)",
          },
        },
        data,
      },
    ],
  });
}

// ===== 交互处理 =====

/** 点击词语：确认后加入词库 */
function handleWordClick(word: string): void {
  dialog.warning({
    title: "加入词库",
    content: `确定将「${word}」加入词库吗？`,
    positiveText: "加入",
    negativeText: "取消",
    onPositiveClick: async (): Promise<void> => {
      try {
        await wordCreate(word, "");
        message.success(`「${word}」已加入词库`);
      } catch (e) {
        message.error(`加入词库失败: ${e}`);
      }
    },
  });
}

/** 重置词频 */
function handleReset(): void {
  dialog.warning({
    title: "重置词频",
    content: "确定清空全部词频记录吗？此操作不可撤销。",
    positiveText: "确定重置",
    negativeText: "取消",
    onPositiveClick: async (): Promise<void> => {
      resetting.value = true;
      try {
        await wordFreqReset();
        message.success("词频已重置");
        await loadData();
      } catch (e) {
        message.error(`重置失败: ${e}`);
      } finally {
        resetting.value = false;
      }
    },
  });
}

// ===== 生命周期 =====

/** 窗口 resize 时重绘图表 */
function handleResize(): void {
  chartInstance.value?.resize();
}

onMounted(async () => {
  await nextTick();
  await loadData();
  window.addEventListener("resize", handleResize);
});

onUnmounted(() => {
  window.removeEventListener("resize", handleResize);
  // 销毁 ECharts 实例，防止内存泄漏
  chartInstance.value?.dispose();
  chartInstance.value = null;
});
</script>

<template>
  <div>
    <h2 style="margin-bottom: 16px">高频词云</h2>

    <!-- 统计概览 -->
    <NGrid x-gap="16" y-gap="16" cols="1 s:2 m:4" responsive="screen" style="margin-bottom: 16px">
      <NGi>
        <NCard :bordered="false" size="small">
          <NStatistic label="不同词语" :value="overview.total_words" />
        </NCard>
      </NGi>
      <NGi>
        <NCard :bordered="false" size="small">
          <NStatistic label="累计使用" :value="overview.total_usage" />
        </NCard>
      </NGi>
      <NGi>
        <NCard :bordered="false" size="small">
          <NStatistic label="最高频次" :value="entries.length > 0 ? entries[0].count : 0" />
        </NCard>
      </NGi>
      <NGi>
        <NCard :bordered="false" size="small">
          <NStatistic label="最热词语" :value="entries.length > 0 ? entries[0].word : '—'" />
        </NCard>
      </NGi>
    </NGrid>

    <!-- 操作栏 -->
    <NSpace align="center" justify="space-between" style="margin-bottom: 12px">
      <span style="font-size: 13px; color: var(--st-text-soft)">
        点击词语可快速加入词库 · 词频来自选中候选的分词统计
      </span>
      <NSpace>
        <NButton size="small" :loading="loading" @click="loadData">刷新</NButton>
        <NButton
          size="small"
          type="error"
          ghost
          :loading="resetting"
          :disabled="entries.length === 0"
          @click="handleReset"
        >
          重置词频
        </NButton>
      </NSpace>
    </NSpace>

    <!-- 词云图表 -->
    <NCard :bordered="false" style="margin-bottom: 16px">
      <div v-if="entries.length > 0" ref="chartRef" style="width: 100%; height: 420px" />
      <NEmpty v-else description="暂无词频数据 · 选中候选回复后将自动统计" style="padding: 80px 0">
        <template #extra>
          <NButton size="small" @click="loadData">刷新</NButton>
        </template>
      </NEmpty>
    </NCard>

    <!-- 高频词列表（补充词云的表格视角） -->
    <NCard v-if="entries.length > 0" title="高频词列表" :bordered="false">
      <NSpace :size="8" wrap>
        <NTag
          v-for="entry in entries.slice(0, 50)"
          :key="entry.word"
          type="info"
          size="small"
          round
          checkable
          @click="handleWordClick(entry.word)"
        >
          {{ entry.word }} · {{ entry.count }}
        </NTag>
      </NSpace>
    </NCard>
  </div>
</template>
