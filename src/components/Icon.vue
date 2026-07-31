<script setup lang="ts">
// TODO 人工审查点：1.图标名称合法性 2.size/color 默认值 3.SVG viewBox 一致性 4.currentColor 继承
// NOTE iconfont 风格矢量图标组件：inline SVG，无网络/字体依赖，currentColor 继承父级颜色
//       所需图标由 name prop 索引 ICON_PATHS 常量表；缺失 name 渲染空占位避免崩溃
//       图标 path 采用 iconfont.cn 常见图标路径（Material Design 风格，iconfont 上均收录）
//       后续可替换为精确 iconfont 项目 symbol（<symbol id="xxx"> + <use xlink:href>）

/** 支持的图标名称 */
export type IconName =
  | "pin"
  | "pin-off"
  | "pin-clock"
  | "close"
  | "edit"
  | "check"
  | "save"
  | "opacity"
  | "chevron-down"
  | "warn"
  | "refresh"
  | "cursor";

/** 图标 SVG 内部 path（viewBox=24x24，fill=currentColor 统一着色） */
const ICON_PATHS: Record<IconName, string> = {
  // 实心图钉：普通置顶（持久）
  pin:
    "M16,9V4l1,0c0.55,0,1-0.45,1-1v0c0-0.55-0.45-1-1-1H7C6.45,2,6,2.45,6,3v0c0,0.55,0.45,1,1,1l1,0v5c0,1.66-1.34,3-3,3h0v2h5.97v7l1,1l1-1v-7H19v-2h0C17.34,12,16,10.66,16,9z",
  // 空心图钉：未置顶
  "pin-off":
    "M14,4v5c0,1.12,0.37,2.16,1,3v2h-1V9V4l1,0c0.55,0,1-0.45,1-1V3c0-0.55-0.45-1-1-1H7C6.45,2,6,2.45,6,3v0c0,0.55,0.45,1,1,1l1,0 M9,4 M9,13v2 M3.41,2.59L2,4l5,5v9c0,0.55,0.45,1,1,1h0c0.55,0,1-0.45,1-1l0-3l2,2v1c0,0.55,0.45,1,1,1h0c0.32,0,0.59-0.15,0.77-0.38L18.59,20.59L20,19.2L3.41,2.59z",
  // 时钟：临时置顶（悬浮窗关闭后失效）
  "pin-clock":
    "M11.99,2C6.47,2,2,6.48,2,12s4.47,10,9.99,10C17.52,22,22,17.52,22,12S17.52,2,11.99,2z M12,20c-4.42,0-8-3.58-8-8s3.58-8,8-8s8,3.58,8,8S16.42,20,12,20z M12.5,7H11v6l5.25,3.15l0.75-1.23l-4.5-2.67V7z",
  // 关闭
  close:
    "M19,6.41L17.59,5L12,10.59L6.41,5L5,6.41L10.59,12L5,17.59L6.41,19L12,13.41L17.59,19L19,17.59L13.41,12L19,6.41z",
  // 编辑
  edit:
    "M3,17.25V21h3.75L17.81,9.94l-3.75-3.75L3,17.25z M20.71,7.04c0.39-0.39,0.39-1.02,0-1.41l-2.34-2.34c-0.39-0.39-1.02-0.39-1.41,0l-1.83,1.83l3.75,3.75L20.71,7.04z",
  // 确认（勾选）
  check: "M9,16.17L4.83,12l-1.42,1.41L9,19L21,7l-1.41-1.41L9,16.17z",
  // 存入词库（书签）
  save:
    "M17,3H7C5.9,3,5,3.9,5,5v16l7-3l7,3V5C19,3.9,18.1,3,17,3z",
  // 透明度（水滴）
  opacity:
    "M17.66,8L12,2.35L6.34,8C4.78,9.56,4,11.64,4,13.64C4,15.64,4.78,17.75,6.34,19.31C7.9,20.87,9.95,21.66,12,21.66c2.05,0,4.1-0.79,5.66-2.35C19.22,17.75,20,15.64,20,13.64C20,11.64,19.22,9.56,17.66,8z M7,14c0-1.48,0.51-2.87,1.42-3.95L12,5.64l3.58,4.41C16.49,11.13,17,12.52,17,14H7z",
  // 下拉箭头
  "chevron-down": "M7.41,8.59L12,13.17l4.59-4.58L18,10l-6,6l-6-6L7.41,8.59z",
  // 警告
  warn:
    "M1,21h22L12,2L1,21z M13,18h-2v-2h2V18z M13,14h-2v-4h2V14z",
  // 重新生成（刷新）
  refresh:
    "M17.65,6.35C16.2,4.9,14.21,4,12,4c-4.42,0-7.99,3.58-7.99,8s3.57,8,7.99,8c3.73,0,6.84-2.55,7.73-6h-2.08c-0.82,2.33-3.04,4-5.65,4c-3.31,0-6-2.69-6-6s2.69-6,6-6c1.66,0,3.14,0.69,4.22,1.78L13,11h7V4L17.65,6.35z",
  // 流式光标（竖条，配合 blink 动画）
  cursor: "M3,2h2v20H3z",
};

const props = withDefaults(
  defineProps<{
    /** 图标名称（见 IconName） */
    name: IconName;
    /** 图标尺寸（px），默认 16 */
    size?: number;
    /** 图标颜色（默认继承 currentColor） */
    color?: string;
  }>(),
  {
    size: 16,
    color: "currentColor",
  },
);
</script>

<template>
  <svg
    class="st-icon"
    :width="props.size"
    :height="props.size"
    viewBox="0 0 24 24"
    :style="{ color: props.color }"
    aria-hidden="true"
    focusable="false"
  >
    <path :d="ICON_PATHS[props.name]" fill="currentColor" />
  </svg>
</template>

<style scoped>
.st-icon {
  display: inline-block;
  vertical-align: middle;
  flex-shrink: 0;
  line-height: 1;
}
</style>
