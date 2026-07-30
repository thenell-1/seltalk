<script setup lang="ts">
// NOTE 仪表盘页面：系统状态概览 + 使用说明
// PRD US-4：双击托盘图标打开管理面板，配置完成后关闭面板不影响后台监听
import { ref, onMounted } from "vue";
import * as api from "@/api";
import type { SystemStatus } from "@/api";

const status = ref<SystemStatus | null>(null);
const loading = ref(true);
const errorMsg = ref("");

onMounted(async () => {
  await loadStatus();
});

async function loadStatus(): Promise<void> {
  loading.value = true;
  try {
    status.value = await api.getSystemStatus();
  } catch (err) {
    errorMsg.value = formatError(err);
  } finally {
    loading.value = false;
  }
}

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
  <div class="dashboard">
    <h2 class="page-title">仪表盘</h2>

    <!-- 错误提示 -->
    <div v-if="errorMsg" class="alert alert-error">
      <svg viewBox="0 0 24 24" width="16" height="16" class="alert-icon">
        <path
          fill="currentColor"
          d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm1 15h-2v-2h2v2zm0-4h-2V7h2v6z"
        />
      </svg>
      <span class="alert-text">{{ errorMsg }}</span>
    </div>

    <!-- 状态卡片 -->
    <div class="stat-cards">
      <div class="stat-card">
        <div class="stat-card-icon stat-card-icon--primary">
          <svg viewBox="0 0 24 24" width="20" height="20">
            <path
              fill="currentColor"
              d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-2 15l-5-5 1.41-1.41L10 14.17l7.59-7.59L19 8l-9 9z"
            />
          </svg>
        </div>
        <div class="stat-card-content">
          <div class="stat-card-label">运行状态</div>
          <div class="stat-card-value">
            <span v-if="loading">加载中...</span>
            <span v-else-if="status?.running" class="status-running">运行中</span>
            <span v-else class="status-stopped">已停止</span>
          </div>
        </div>
      </div>

      <div class="stat-card">
        <div class="stat-card-icon stat-card-icon--success">
          <svg viewBox="0 0 24 24" width="20" height="20">
            <path fill="currentColor" d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z" />
          </svg>
        </div>
        <div class="stat-card-content">
          <div class="stat-card-label">已采纳回复</div>
          <div class="stat-card-value">{{ loading ? "-" : status?.total_adopted ?? 0 }}</div>
        </div>
      </div>

      <div class="stat-card">
        <div class="stat-card-icon stat-card-icon--info">
          <svg viewBox="0 0 24 24" width="20" height="20">
            <path
              fill="currentColor"
              d="M19.35 10.04C18.67 6.59 15.64 4 12 4 9.11 4 6.6 5.64 5.35 8.04 2.34 8.36 0 10.91 0 14c0 3.31 2.69 6 6 6h13c2.76 0 5-2.24 5-5 0-2.64-2.05-4.78-4.65-4.96z"
            />
          </svg>
        </div>
        <div class="stat-card-content">
          <div class="stat-card-label">LLM 模式</div>
          <div class="stat-card-value">{{ loading ? "-" : status?.llm_mode ?? "cloud" }}</div>
        </div>
      </div>
    </div>

    <!-- 使用说明 -->
    <div class="guide-section">
      <h3 class="section-title">使用说明</h3>
      <div class="guide-steps">
        <div class="guide-step">
          <div class="guide-step-num">1</div>
          <div class="guide-step-content">
            <div class="guide-step-title">打开微信或 QQ 聊天窗口</div>
            <div class="guide-step-desc">确保聊天窗口处于前台激活状态</div>
          </div>
        </div>
        <div class="guide-step">
          <div class="guide-step-num">2</div>
          <div class="guide-step-content">
            <div class="guide-step-title">选中对方发来的消息文本</div>
            <div class="guide-step-desc">用鼠标选中需要回复的对方消息</div>
          </div>
        </div>
        <div class="guide-step">
          <div class="guide-step-num">3</div>
          <div class="guide-step-content">
            <div class="guide-step-title">按 F8 键触发</div>
            <div class="guide-step-desc">悬浮窗将显示"AI 思考中..."，1-3 秒后填充候选回复</div>
          </div>
        </div>
        <div class="guide-step">
          <div class="guide-step-num">4</div>
          <div class="guide-step-content">
            <div class="guide-step-title">方向键切换，Tab 确认</div>
            <div class="guide-step-desc">↑↓ 切换候选，Tab 确认后自动逐字输入到聊天框，Esc 关闭</div>
          </div>
        </div>
      </div>
    </div>

    <!-- 配置提示 -->
    <div v-if="status?.llm_mode === 'cloud' && status?.total_adopted === 0" class="config-tip">
      <svg viewBox="0 0 24 24" width="16" height="16" class="config-tip-icon">
        <path
          fill="currentColor"
          d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm1 15h-2v-2h2v2zm0-4h-2V7h2v6z"
        />
      </svg>
      <span class="config-tip-text">首次使用请先到"设置"页面配置 LLM API 密钥</span>
    </div>
  </div>
</template>

<style scoped>
.dashboard {
  padding: 24px;
  max-width: 900px;
}

.page-title {
  margin: 0 0 24px;
  font-size: 20px;
  font-weight: 600;
  color: #111827;
}

.alert {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 16px;
  margin-bottom: 16px;
  border-radius: 8px;
  font-size: 13px;
}

.alert-error {
  background: #fef2f2;
  border: 1px solid #fecaca;
  color: #dc2626;
}

.alert-icon {
  flex-shrink: 0;
}

.stat-cards {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
  gap: 16px;
  margin-bottom: 32px;
}

.stat-card {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 20px;
  background: #fff;
  border: 1px solid #e5e7eb;
  border-radius: 8px;
}

.stat-card-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  border-radius: 8px;
  flex-shrink: 0;
}

.stat-card-icon--primary { background: #eef2ff; color: #6366f1; }
.stat-card-icon--success { background: #ecfdf5; color: #10b981; }
.stat-card-icon--info { background: #eff6ff; color: #3b82f6; }

.stat-card-label {
  font-size: 13px;
  color: #6b7280;
  margin-bottom: 4px;
}

.stat-card-value {
  font-size: 18px;
  font-weight: 600;
  color: #111827;
}

.status-running { color: #10b981; }
.status-stopped { color: #ef4444; }

.guide-section {
  background: #fff;
  border: 1px solid #e5e7eb;
  border-radius: 8px;
  padding: 20px;
}

.section-title {
  margin: 0 0 16px;
  font-size: 15px;
  font-weight: 600;
  color: #374151;
}

.guide-steps {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.guide-step {
  display: flex;
  align-items: flex-start;
  gap: 12px;
}

.guide-step-num {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border-radius: 50%;
  background: #eef2ff;
  color: #6366f1;
  font-size: 13px;
  font-weight: 600;
  flex-shrink: 0;
}

.guide-step-title {
  font-size: 14px;
  font-weight: 500;
  color: #111827;
  margin-bottom: 2px;
}

.guide-step-desc {
  font-size: 13px;
  color: #9ca3af;
}

.config-tip {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 16px;
  padding: 12px 16px;
  background: #fffbeb;
  border: 1px solid #fde68a;
  border-radius: 8px;
  font-size: 13px;
  color: #92400e;
}

.config-tip-icon {
  flex-shrink: 0;
}
</style>
