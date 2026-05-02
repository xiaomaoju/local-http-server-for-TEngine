<script setup lang="ts">
defineProps<{ show: boolean; version: string }>();
const emit = defineEmits<{
  (e: "close"): void;
  (e: "open-help"): void;
}>();

function clearAllCache() {
  if (!confirm("确定清空所有本地缓存吗？\n\n会清除：\n- 已保存的远程服务器列表\n- 已记录的 Bundles 目录路径\n- 其他界面状态\n\n本地模式的项目配置不会受影响。")) {
    return;
  }
  try {
    localStorage.clear();
    alert("缓存已清空。建议重启应用以确保完全生效。");
  } catch (e: any) {
    alert(`清空失败: ${e?.message || e}`);
  }
}
</script>

<template>
  <Teleport to="body">
    <div v-if="show" class="settings-mask" @click.self="emit('close')">
      <div class="settings-dialog">
        <div class="settings-header">
          <h3>
            <span class="settings-gear">⚙</span>
            设置
          </h3>
          <button class="settings-close" @click="emit('close')">&times;</button>
        </div>
        <div class="settings-body">
          <!-- 版本信息 -->
          <div class="settings-row">
            <div class="settings-row-info">
              <div class="settings-row-title">版本信息</div>
              <div class="settings-row-desc">当前应用版本号</div>
            </div>
            <div class="settings-row-value">{{ version }}</div>
          </div>

          <!-- 帮助 -->
          <div class="settings-row clickable" @click="emit('open-help'); emit('close')">
            <div class="settings-row-info">
              <div class="settings-row-title">帮助</div>
              <div class="settings-row-desc">查看使用指南、Unity 接入示例</div>
            </div>
            <div class="settings-row-action">→</div>
          </div>

          <!-- 清理缓存 -->
          <div class="settings-row clickable danger" @click="clearAllCache">
            <div class="settings-row-info">
              <div class="settings-row-title">清理所有缓存</div>
              <div class="settings-row-desc">清除已保存的远程连接、Bundles 目录等记录</div>
            </div>
            <div class="settings-row-action">→</div>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.settings-mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.6);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 2000;
  backdrop-filter: blur(2px);
}
.settings-dialog {
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 12px;
  width: 440px;
  max-width: 90vw;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.4);
  overflow: hidden;
}
.settings-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px 20px;
  border-bottom: 1px solid var(--border);
}
.settings-header h3 {
  margin: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 15px;
  font-weight: 600;
  color: var(--accent);
}
.settings-gear {
  font-size: 16px;
}
.settings-close {
  background: transparent;
  border: none;
  color: var(--text-muted);
  font-size: 22px;
  cursor: pointer;
  width: 28px;
  height: 28px;
  border-radius: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  line-height: 1;
}
.settings-close:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}
.settings-body {
  padding: 8px 0;
}
.settings-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 14px 20px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.04);
  transition: background 0.15s;
}
.settings-row:last-child {
  border-bottom: none;
}
.settings-row.clickable {
  cursor: pointer;
}
.settings-row.clickable:hover {
  background: var(--bg-tertiary);
}
.settings-row.danger:hover {
  background: rgba(255, 107, 107, 0.06);
}
.settings-row-info {
  flex: 1;
  min-width: 0;
}
.settings-row-title {
  font-size: 13px;
  color: var(--text-primary);
  font-weight: 500;
}
.settings-row-desc {
  font-size: 11px;
  color: var(--text-muted);
  margin-top: 3px;
}
.settings-row-value {
  font-size: 13px;
  color: var(--accent);
  font-weight: 500;
}
.settings-row-action {
  color: var(--text-muted);
  font-size: 14px;
}
.settings-row.danger .settings-row-title {
  color: #ff6b6b;
}
</style>
