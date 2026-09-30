<script setup lang="ts">
import { ref } from "vue";
import { Sun, Moon, Package } from "@lucide/vue";
import LocalMode from "./components/LocalMode.vue";
import RemoteMode from "./components/RemoteMode.vue";
import { theme, toggleTheme } from "../shared/theme";
import SegmentedControl from "../shared/SegmentedControl.vue";
const mode = ref<"local" | "remote">("local");
</script>
<template>
  <div class="app-container">
    <header class="desktop-header">
      <span class="desktop-brand"
        ><Package :size="16" />TEngine 资源工作台</span
      >
      <SegmentedControl :model-value="mode" label="工作模式" hide-label :options="[{value:'local',label:'本地服务'},{value:'remote',label:'远程协作'}]" @update:model-value="mode = $event as 'local' | 'remote'" />
      <button
        class="theme-toggle"
        :aria-label="theme === 'dark' ? '切换浅色主题' : '切换深色主题'"
        @click="toggleTheme"
      >
        <Sun v-if="theme === 'dark'" :size="16" /><Moon v-else :size="16" />
      </button>
    </header>
    <KeepAlive
      ><LocalMode v-if="mode === 'local'" /><RemoteMode v-else
    /></KeepAlive>
  </div>
</template>
<style scoped>
.desktop-header {
  height: 44px;
  min-height: 44px;
  padding: 0 12px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-bottom: 1px solid var(--line);
  background: var(--surface-muted);
}
.desktop-brand {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  font-weight: 600;
}
.theme-toggle {
  border: 0;
  height: 32px;
  border-radius: 4px;
  padding: 0 8px;
  background: transparent;
  color: var(--ink-secondary);
  display: flex;
  align-items: center;
  gap: 4px;
  cursor: pointer;
  font-size: 13px;
}
.theme-toggle {
  width: 32px;
}
.app-container > :last-child {
  flex: 1;
  min-height: 0;
}
</style>
