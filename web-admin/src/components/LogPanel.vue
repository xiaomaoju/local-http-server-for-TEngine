<script setup lang="ts">
import { nextTick, watch, ref } from "vue";
import type { LogEntry } from "../api/remote";

const props = defineProps<{ logs: LogEntry[] }>();
const emit = defineEmits<{ (e: "clear"): void }>();

const logPanelOpen = ref(true);
const logPanelHeight = ref(220);

watch(() => props.logs.length, () => {
  nextTick(() => {
    const el = document.querySelector(".log-body");
    if (el) el.scrollTop = el.scrollHeight;
  });
});

function getStatusClass(status: number): string {
  if (status >= 200 && status < 300) return "s200";
  if (status >= 300 && status < 400) return "s301";
  if (status >= 400 && status < 500) return "s404";
  return "s500";
}

let isResizing = false;
let startY = 0;
let startHeight = 0;

function onResizeStart(e: MouseEvent) {
  isResizing = true;
  startY = e.clientY;
  startHeight = logPanelHeight.value;
  document.addEventListener("mousemove", onResizeMove);
  document.addEventListener("mouseup", onResizeEnd);
}

function onResizeMove(e: MouseEvent) {
  if (!isResizing) return;
  const delta = startY - e.clientY;
  logPanelHeight.value = Math.max(100, Math.min(500, startHeight + delta));
}

function onResizeEnd() {
  isResizing = false;
  document.removeEventListener("mousemove", onResizeMove);
  document.removeEventListener("mouseup", onResizeEnd);
}
</script>

<template>
  <div v-if="logPanelOpen" class="resize-handle" @mousedown="onResizeStart"></div>
  <div class="log-panel" :class="{ collapsed: !logPanelOpen }"
    :style="{ height: logPanelOpen ? logPanelHeight + 'px' : '36px' }">
    <div class="log-header" @click="logPanelOpen = !logPanelOpen">
      <h3>
        <span class="toggle-icon" :class="{ expanded: logPanelOpen }">&#9650;</span>
        日志 <span class="log-count">{{ logs.length }}</span>
      </h3>
      <div class="log-actions" @click.stop>
        <button @click="emit('clear')">清空</button>
      </div>
    </div>
    <div v-if="logPanelOpen" class="log-body">
      <div v-if="logs.length === 0" class="empty-state" style="height:100%">
        <p style="font-size:12px;color:var(--text-muted)">暂无日志</p>
      </div>
      <div v-for="(log, idx) in logs" :key="idx" class="log-entry">
        <span class="time">{{ log.timestamp }}</span>
        <span class="status" :class="getStatusClass(log.status)">
          {{ log.type === "request" ? log.status : log.type?.toUpperCase() }}
        </span>
        <span class="path">{{ log.message || log.path }}</span>
      </div>
    </div>
  </div>
</template>
