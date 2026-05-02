<script setup lang="ts">
import { ref, computed, shallowRef } from "vue";
import LocalMode from "./components/LocalMode.vue";
import RemoteMode from "./components/RemoteMode.vue";
import SettingsDialog from "./components/SettingsDialog.vue";
import HelpModal from "./components/HelpModal.vue";

const mode = ref<"local" | "remote">("local");
const LocalModeRef = shallowRef(LocalMode);
const RemoteModeRef = shallowRef(RemoteMode);
const currentMode = computed(() => mode.value === "local" ? LocalModeRef.value : RemoteModeRef.value);
const showSettings = ref(false);
const showHelp = ref(false);
const APP_VERSION = "v1.0.1";
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
      <button class="settings-btn" @click="showSettings = true" title="设置">
        <span class="gear">⚙</span>
      </button>
    </div>

    <KeepAlive>
      <component :is="currentMode" />
    </KeepAlive>

    <SettingsDialog
      :show="showSettings"
      :version="APP_VERSION"
      @close="showSettings = false"
      @open-help="showHelp = true"
    />
    <HelpModal :show="showHelp" @close="showHelp = false" />
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

.settings-btn {
  width: 32px;
  height: 32px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  border-radius: 6px;
  cursor: pointer;
  font-size: 18px;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.15s, color 0.15s, transform 0.3s;
}

.settings-btn:hover {
  background: var(--bg-tertiary);
  color: var(--accent);
}

.settings-btn:active .gear {
  transform: rotate(90deg);
}

.gear {
  display: inline-block;
  transition: transform 0.4s;
  line-height: 1;
}

.settings-btn:hover .gear {
  transform: rotate(45deg);
}
</style>
