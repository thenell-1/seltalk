<script setup lang="ts">
// TODO 人工审查点：1.热键录入交互 2.参数边界校验 3.保存反馈 4.配置键名一致 5.黑名单正则合法性
// NOTE 设置页：全局热键 / 打字速度 / 悬浮窗默认尺寸 / 默认置顶 / 样式预设 / 透明度 / 剪贴板处理模式 / 黑名单
import { ref, onMounted } from "vue";
import {
  NCard, NForm, NFormItem, NInput, NInputNumber, NButton, NSpace, NSwitch, NSelect,
  NSlider, NRadioGroup, NRadio, useMessage,
} from "naive-ui";
import {
  getAppConfig, updateHotkey, setSetting, getAllSettings,
  blacklistGet, blacklistSet, autostartGet, autostartSet,
  getFloatOpacity, setFloatOpacity,
  getClipboardMode, setClipboardMode,
} from "@/lib/api";
import type { AppConfig } from "@/lib/api";

const message = useMessage();

// 配置键名
const KEY_TYPE_MIN_MS = "type_min_ms";
const KEY_TYPE_MAX_MS = "type_max_ms";
const KEY_FLOAT_W = "float_w";
const KEY_FLOAT_H = "float_h";
const KEY_FLOAT_ALWAYS_ON_TOP = "float_always_on_top";
const KEY_CANDIDATE_COUNT = "candidate_count";
const KEY_FLOAT_STYLE_PRESET = "float_style_preset";
const KEY_FLOAT_FOLLOW_CURSOR = "float_follow_cursor";

/// 默认黑名单正则（与 Rust 端 DEFAULT_BLACKLIST 保持一致）
const DEFAULT_BLACKLIST_PATTERNS: string[] = [
  "1[3-9]\\d{9}",
  "\\d{17}[\\dXx]",
  "[\\w.+-]+@[\\w-]+\\.[\\w.-]+",
];

/// 样式预设选项
const PRESET_OPTIONS = [
  { label: "紧凑", value: "compact" },
  { label: "标准", value: "standard" },
  { label: "宽松", value: "loose" },
];

// 表单数据
const hotkey = ref("");
const candidateCount = ref(3);
const typeMinMs = ref(30);
const typeMaxMs = ref(120);
const floatW = ref(420);
const floatH = ref(360);
const floatAlwaysOnTop = ref(true);
const floatStylePreset = ref("standard");
const floatFollowCursor = ref(true);
// 悬浮窗透明度（0.30~1.0，默认 1.0）
const floatOpacity = ref(1.0);
// 剪贴板处理模式（"A"=兼容复原 / "B"=纯净只读，默认 "B"）
const clipboardMode = ref("B");

// 黑名单
const blacklistPatterns = ref<string[]>([]);
const blacklistSaving = ref(false);

// 开机自启
const autostartEnabled = ref(false);
const autostartSaving = ref(false);

// 状态
const loading = ref(false);
const saving = ref(false);
const recordingHotkey = ref(false);

// 加载配置
onMounted(async () => {
  loading.value = true;
  try {
    const cfg: AppConfig = await getAppConfig();
    hotkey.value = cfg.hotkey;
    candidateCount.value = cfg.candidate_count;
    typeMinMs.value = cfg.type_min_ms;
    typeMaxMs.value = cfg.type_max_ms;
    floatW.value = cfg.float_w;
    floatH.value = cfg.float_h;
    floatAlwaysOnTop.value = cfg.float_always_on_top;

    // 加载样式预设 + 跟随鼠标 + 黑名单
    try {
      const all = await getAllSettings();
      floatStylePreset.value = all[KEY_FLOAT_STYLE_PRESET] || "standard";
      // 跟随鼠标未配置时默认 true（与后端 DEFAULT_FLOAT_FOLLOW_CURSOR 一致）
      const raw = all[KEY_FLOAT_FOLLOW_CURSOR];
      floatFollowCursor.value = raw === undefined ? true : (raw === "1" || raw.toLowerCase() === "true");
    } catch {
      // 忽略，使用默认
    }
    // 加载透明度 + 剪贴板模式（独立加载，失败不影响其他配置）
    try {
      floatOpacity.value = await getFloatOpacity();
    } catch (e) {
      console.warn("加载透明度失败:", e);
    }
    try {
      clipboardMode.value = await getClipboardMode();
    } catch (e) {
      console.warn("加载剪贴板模式失败:", e);
    }
    await loadBlacklist();
    await loadAutostart();
  } catch (e) {
    message.error(`加载配置失败: ${e}`);
  } finally {
    loading.value = false;
  }
});

/// 判断热键是否为系统保留键（与后端 hotkey::is_reserved 逻辑保持一致）
/// 禁止 Ctrl+单字母（冲突复制/粘贴等）、Alt+F4、Alt+Tab
function isReservedHotkey(combo: string): boolean {
  const normalized = combo.toLowerCase().replace(/\s+/g, "");
  if (normalized === "alt+f4" || normalized === "alt+tab") return true;
  if (normalized.startsWith("ctrl+")) {
    const rest = normalized.slice(5);
    if (rest.length === 1 && /^[a-z]$/.test(rest)) return true;
  }
  return false;
}

// 热键录入：点击后监听键盘，组合键格式化为 "Ctrl+Shift+Key"
function startRecordHotkey(): void {
  recordingHotkey.value = true;
  const originalHotkey = hotkey.value;
  hotkey.value = "请按下组合键…";

  const handler = (event: KeyboardEvent): void => {
    event.preventDefault();
    event.stopPropagation();

    // Esc 取消录入，恢复原值
    if (event.key === "Escape") {
      recordingHotkey.value = false;
      hotkey.value = originalHotkey;
      window.removeEventListener("keydown", handler, true);
      return;
    }

    // 忽略单独的修饰键
    if (["Control", "Shift", "Alt", "Meta"].includes(event.key)) {
      return;
    }

    const parts: string[] = [];
    if (event.ctrlKey) parts.push("Ctrl");
    if (event.shiftKey) parts.push("Shift");
    if (event.altKey) parts.push("Alt");
    if (event.metaKey) parts.push("Super");

    // 主键
    let key = event.key;
    if (key === " ") key = "Space";
    else if (key.length === 1) key = key.toUpperCase();
    parts.push(key);

    if (parts.length >= 2) {
      const combo = parts.join("+");
      // 系统保留键即时拦截，避免录入后注册失败
      if (isReservedHotkey(combo)) {
        message.error(`热键 ${combo} 与系统快捷键冲突，请更换（如 Alt+X）`);
        hotkey.value = originalHotkey;
      } else {
        hotkey.value = combo;
      }
    } else {
      message.warning("请使用组合键（至少含一个修饰键）");
    }

    recordingHotkey.value = false;
    window.removeEventListener("keydown", handler, true);
  };

  window.addEventListener("keydown", handler, true);
}

// 保存热键
async function handleSaveHotkey(): Promise<void> {
  if (!hotkey.value || hotkey.value === "请按下组合键…") {
    message.warning("请先录入热键");
    return;
  }
  saving.value = true;
  try {
    await updateHotkey(hotkey.value);
    message.success("热键已保存并注册");
  } catch (e) {
    message.error(`热键保存失败: ${e}`);
  } finally {
    saving.value = false;
  }
}

// 保存其他设置
async function handleSaveSettings(): Promise<void> {
  saving.value = true;
  try {
    await setSetting(KEY_CANDIDATE_COUNT, candidateCount.value.toString());
    await setSetting(KEY_TYPE_MIN_MS, typeMinMs.value.toString());
    await setSetting(KEY_TYPE_MAX_MS, typeMaxMs.value.toString());
    await setSetting(KEY_FLOAT_W, floatW.value.toString());
    await setSetting(KEY_FLOAT_H, floatH.value.toString());
    await setSetting(KEY_FLOAT_ALWAYS_ON_TOP, floatAlwaysOnTop.value ? "true" : "false");
    await setSetting(KEY_FLOAT_FOLLOW_CURSOR, floatFollowCursor.value ? "true" : "false");
    message.success("设置已保存");
  } catch (e) {
    message.error(`保存失败: ${e}`);
  } finally {
    saving.value = false;
  }
}

// ===== 黑名单管理 =====

async function loadBlacklist(): Promise<void> {
  try {
    blacklistPatterns.value = await blacklistGet();
  } catch (e) {
    message.error(`黑名单加载失败: ${e}`);
  }
}

function addBlacklistPattern(): void {
  blacklistPatterns.value.push("");
}

function removeBlacklistPattern(index: number): void {
  blacklistPatterns.value.splice(index, 1);
}

function resetDefaultBlacklist(): void {
  blacklistPatterns.value = [...DEFAULT_BLACKLIST_PATTERNS];
}

async function saveBlacklist(): Promise<void> {
  blacklistSaving.value = true;
  try {
    const cleaned = blacklistPatterns.value
      .map((p) => p.trim())
      .filter((p) => p.length > 0);
    await blacklistSet(cleaned);
    blacklistPatterns.value = cleaned;
    message.success("黑名单已保存");
  } catch (e) {
    message.error(`保存失败: ${e}`);
  } finally {
    blacklistSaving.value = false;
  }
}

// ===== 开机自启 =====

async function loadAutostart(): Promise<void> {
  try {
    autostartEnabled.value = await autostartGet();
  } catch (e) {
    // 查询失败时默认关闭，不影响页面加载
    autostartEnabled.value = false;
    console.warn("查询自启状态失败:", e);
  }
}

async function handleAutostartToggle(enabled: boolean): Promise<void> {
  autostartSaving.value = true;
  try {
    await autostartSet(enabled);
    autostartEnabled.value = enabled;
    message.success(enabled ? "已启用开机自启" : "已关闭开机自启");
  } catch (e) {
    message.error(`设置失败: ${e}`);
    // 恢复原值
    autostartEnabled.value = !enabled;
  } finally {
    autostartSaving.value = false;
  }
}

// ===== 样式预设（切换即时保存生效） =====

async function onPresetChange(value: string): Promise<void> {
  floatStylePreset.value = value;
  try {
    await setSetting(KEY_FLOAT_STYLE_PRESET, value);
    message.success("样式预设已保存");
  } catch (e) {
    message.error(`保存失败: ${e}`);
  }
}

// ===== 透明度（即时保存，悬浮窗下次呼出生效） =====

async function onOpacityChange(value: number): Promise<void> {
  floatOpacity.value = value;
  try {
    await setFloatOpacity(value);
  } catch (e) {
    message.error(`透明度保存失败: ${e}`);
  }
}

// ===== 剪贴板处理模式（切换即时保存生效） =====

async function onClipboardModeChange(value: string): Promise<void> {
  clipboardMode.value = value;
  try {
    await setClipboardMode(value);
    message.success(value === "A" ? "已切换为兼容复原模式" : "已切换为纯净只读模式");
  } catch (e) {
    message.error(`切换失败: ${e}`);
  }
}
</script>

<template>
  <div>
    <h2 style="margin-bottom: 16px">设置</h2>

    <!-- 热键设置 -->
    <NCard title="全局热键" :bordered="false" style="max-width: 640px; margin-bottom: 16px">
      <NForm label-placement="left" :label-width="100">
        <NFormItem label="触发热键">
          <NSpace align="center">
            <NInput
              v-model:value="hotkey"
              placeholder="Ctrl+Shift+Space"
              :disabled="recordingHotkey"
              style="width: 200px"
              readonly
            />
            <NButton :disabled="recordingHotkey" @click="startRecordHotkey">
              {{ recordingHotkey ? "录入中…" : "录入" }}
            </NButton>
            <NButton type="primary" :loading="saving" @click="handleSaveHotkey">
              保存并注册
            </NButton>
          </NSpace>
        </NFormItem>
      </NForm>
    </NCard>

    <!-- 输入行为设置 -->
    <NCard title="输入行为" :bordered="false" style="max-width: 640px; margin-bottom: 16px">
      <NForm label-placement="left" :label-width="100">
        <NFormItem label="候选条数">
          <NInputNumber
            v-model:value="candidateCount"
            :min="1"
            :max="10"
            :disabled="loading"
            style="width: 100%"
          />
        </NFormItem>

        <NFormItem label="最小延迟(ms)">
          <NInputNumber
            v-model:value="typeMinMs"
            :min="0"
            :max="1000"
            :step="10"
            :disabled="loading"
            style="width: 100%"
          />
        </NFormItem>

        <NFormItem label="最大延迟(ms)">
          <NInputNumber
            v-model:value="typeMaxMs"
            :min="0"
            :max="2000"
            :step="10"
            :disabled="loading"
            style="width: 100%"
          />
        </NFormItem>
      </NForm>
    </NCard>

    <!-- 悬浮窗设置 -->
    <NCard title="悬浮窗默认样式" :bordered="false" style="max-width: 640px; margin-bottom: 16px">
      <NForm label-placement="left" :label-width="100">
        <NFormItem label="默认宽度">
          <NInputNumber
            v-model:value="floatW"
            :min="320"
            :max="800"
            :step="20"
            :disabled="loading"
            style="width: 100%"
          />
        </NFormItem>

        <NFormItem label="默认高度">
          <NInputNumber
            v-model:value="floatH"
            :min="240"
            :max="600"
            :step="20"
            :disabled="loading"
            style="width: 100%"
          />
        </NFormItem>

        <NFormItem label="默认置顶">
          <NSwitch v-model:value="floatAlwaysOnTop" :disabled="loading" />
        </NFormItem>

        <NFormItem label="样式预设">
          <NSelect
            :value="floatStylePreset"
            :options="PRESET_OPTIONS"
            :disabled="loading"
            @update:value="onPresetChange"
          />
        </NFormItem>

        <NFormItem label="跟随鼠标">
          <NSpace align="center">
            <NSwitch v-model:value="floatFollowCursor" :disabled="loading" />
            <span style="font-size: 13px; color: var(--st-text-soft)">
              开启后悬浮窗在鼠标光标附近弹出（关闭后沿用上次位置）
            </span>
          </NSpace>
        </NFormItem>

        <NFormItem label="默认透明度">
          <NSpace align="center" style="width: 100%">
            <NSlider
              :value="floatOpacity"
              :min="0.3"
              :max="1.0"
              :step="0.05"
              :disabled="loading"
              style="width: 200px"
              @update:value="onOpacityChange"
            />
            <span style="font-size: 13px; color: var(--st-text-soft); min-width: 40px">
              {{ Math.round(floatOpacity * 100) }}%
            </span>
          </NSpace>
        </NFormItem>
      </NForm>
    </NCard>

    <!-- 剪贴板处理模式 -->
    <NCard title="剪贴板处理模式" :bordered="false" style="max-width: 640px; margin-bottom: 16px">
      <div
        style="font-size: 13px; color: var(--st-text-soft); margin-bottom: 12px; line-height: 1.6"
      >
        控制热键触发时剪贴板的读取方式。模式 B（默认）不修改剪贴板，彻底解决 Win+V 历史杂乱问题。
      </div>
      <NRadioGroup :value="clipboardMode" @update:value="onClipboardModeChange">
        <NSpace vertical :size="12">
          <NRadio value="B">
            <strong>纯净只读模式（推荐）</strong>
            <div style="font-size: 12px; color: var(--st-text-soft); margin-top: 2px">
              仅读取剪贴板文本，不执行任何写入操作。Win+V 历史无新增条目，稳定性最高。
            </div>
          </NRadio>
          <NRadio value="A">
            <strong>兼容复原模式</strong>
            <div style="font-size: 12px; color: var(--st-text-soft); margin-top: 2px">
              快照→读文本→复原剪贴板原内容。复原时会新增一条 Win+V 历史记录，适合需要恢复原内容的场景。
            </div>
          </NRadio>
        </NSpace>
      </NRadioGroup>
    </NCard>

    <!-- 文本过滤黑名单 -->
    <NCard title="文本过滤黑名单" :bordered="false" style="max-width: 640px; margin-bottom: 16px">
      <div
        style="font-size: 13px; color: var(--st-text-soft); margin-bottom: 12px; line-height: 1.6"
      >
        命中正则的文本在送入 LLM 前会被替换为 <code>***</code>，用于保护手机号、身份证、邮箱等隐私信息。每行一条正则表达式。
      </div>
      <NSpace vertical :size="8">
        <NSpace v-for="(pattern, i) in blacklistPatterns" :key="i" align="center">
          <NInput
            :value="pattern"
            placeholder="如：1[3-9]\d{9}"
            style="width: 460px"
            @update:value="(val: string) => (blacklistPatterns[i] = val)"
          />
          <NButton size="small" type="error" ghost @click="removeBlacklistPattern(i)">
            删除
          </NButton>
        </NSpace>
        <NSpace>
          <NButton size="small" @click="addBlacklistPattern">添加规则</NButton>
          <NButton size="small" @click="resetDefaultBlacklist">恢复默认</NButton>
          <NButton
            size="small"
            type="primary"
            :loading="blacklistSaving"
            @click="saveBlacklist"
          >
            保存黑名单
          </NButton>
        </NSpace>
      </NSpace>
    </NCard>

    <!-- 系统设置 -->
    <NCard title="系统设置" :bordered="false" style="max-width: 640px; margin-bottom: 16px">
      <NForm label-placement="left" :label-width="100">
        <NFormItem label="开机自启">
          <NSpace align="center">
            <NSwitch
              :value="autostartEnabled"
              :loading="autostartSaving"
              @update:value="handleAutostartToggle"
            />
            <span style="font-size: 13px; color: var(--st-text-soft)">
              系统启动时自动运行择言 SelTalk
            </span>
          </NSpace>
        </NFormItem>
      </NForm>
    </NCard>

    <!-- 保存按钮 -->
    <div style="max-width: 640px; text-align: right">
      <NButton type="primary" size="large" :loading="saving" @click="handleSaveSettings">
        保存全部设置
      </NButton>
    </div>
  </div>
</template>
