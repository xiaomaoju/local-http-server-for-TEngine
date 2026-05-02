<script setup lang="ts">
import { ref } from "vue";
import LocalMode from "./components/LocalMode.vue";
import RemoteMode from "./components/RemoteMode.vue";

const mode = ref<"local" | "remote">("local");
</script>

<template>
  <div class="app-container">
    <div class="title-bar">
      <h1>TEngine Http Server</h1>
      <div class="mode-switcher">
        <button
          :class="{ active: mode === 'local' }"
          @click="mode = 'local'"
        >本地模式</button>
        <button
          :class="{ active: mode === 'remote' }"
          @click="mode = 'remote'"
        >远程模式</button>
      </div>
      <span class="version">v1.0.0</span>
    </div>

    <LocalMode v-if="mode === 'local'" />
    <RemoteMode v-else />
  </div>
</template>

<style scoped>
.mode-switcher {
  display: flex;
  gap: 2px;
  background: var(--bg-tertiary);
  border-radius: 6px;
  padding: 2px;
}

.mode-switcher button {
  padding: 4px 16px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  border-radius: 4px;
  cursor: pointer;
  font-size: 13px;
  transition: all 0.2s;
}

.mode-switcher button.active {
  background: var(--accent);
  color: var(--bg-primary);
}

.mode-switcher button:hover:not(.active) {
  color: var(--text-primary);
}
</style>
