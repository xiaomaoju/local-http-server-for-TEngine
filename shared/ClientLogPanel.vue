<script lang="ts">
export interface ClientLogRow {
  id: string;
  timestamp: number;
  source: "客户端" | "操作" | "资源";
  httpStatus?: number;
  level: "info" | "success" | "warning" | "error";
  message: string;
}
</script>
<script setup lang="ts">
import { computed, nextTick, ref, watch, useId } from "vue";
import { ChevronUp, ChevronDown, History } from "@lucide/vue";
const props = defineProps<{ entries: ClientLogRow[]; resourceEntries: ClientLogRow[]; connectionState: string; projectName?: string; resourceLogsAvailable: boolean | null }>();
defineEmits<{ history: [] }>();
const expanded = ref(false);
type Channel = "operations" | "resources";
const channel = ref<Channel>("operations");
const id = useId();
const follows = ref({operations: true, resources: true});
const follow = computed({get: () => follows.value[channel.value], set: (value: boolean) => { follows.value[channel.value] = value; }});
const positions = {operations: 0, resources: 0};
const body = ref<HTMLElement>();
const cleared = ref({operations: new Set<string>(), resources: new Set<string>()});
const operations = computed(() => props.entries.filter(row => !cleared.value.operations.has(row.id)));
const resources = computed(() => props.resourceEntries.filter(row => !cleared.value.resources.has(row.id)));
const rows = computed(() => channel.value === "operations" ? operations.value : resources.value);
const errors = computed(() => rows.value.filter(row => row.level === "error").length);
const levelText = { info: "信息", success: "成功", warning: "提醒", error: "错误" };
function clearView() {
  // Only clear the current view. Server audit history is never deleted.
  cleared.value[channel.value] = new Set([...cleared.value[channel.value], ...rows.value.map(row => row.id)].slice(-3000));
}
function selectChannel(next: Channel) { channel.value = next; expanded.value = true; }
async function focusChannel(next: Channel) {
  selectChannel(next);
  await nextTick();
  document.getElementById(id + '-' + next)?.focus();
}
function rememberScroll() { if (body.value) positions[channel.value] = body.value.scrollTop; }
watch([rows, expanded, follow, channel], async () => {
  if (!expanded.value) return;
  await nextTick();
  if (body.value) body.value.scrollTop = follow.value ? body.value.scrollHeight : positions[channel.value];
});
</script>
<template>
  <section class="ui-client-logs" aria-label="客户端日志">
    <div class="ui-client-log-toolbar">
      <button class="quiet ui-log-toggle" :aria-expanded="expanded" :aria-controls="id + '-body'" @click="expanded = !expanded">
        <ChevronDown v-if="expanded" :size="14" /><ChevronUp v-else :size="14" />日志 <span v-if="errors" class="danger">{{ errors }} 条错误</span>
      </button>
      <div class="ui-log-channels" role="tablist" aria-label="日志分类">
        <button :id="id + '-operations'" role="tab" :aria-selected="channel === 'operations'" :aria-controls="id + '-body'" :tabindex="channel === 'operations' ? 0 : -1" @click="selectChannel('operations')" @keydown.right.prevent="focusChannel('resources')" @keydown.left.prevent="focusChannel('resources')">操作日志 <span>{{ operations.length }}</span></button>
        <button :id="id + '-resources'" role="tab" :aria-selected="channel === 'resources'" :aria-controls="id + '-body'" :tabindex="channel === 'resources' ? 0 : -1" @click="selectChannel('resources')" @keydown.left.prevent="focusChannel('operations')" @keydown.right.prevent="focusChannel('operations')">资源请求 <span>{{ resources.length }}</span></button>
      </div>
      <span class="ui-muted ui-log-context" :title="projectName || '全部有权限的项目'">{{ projectName || '全部项目' }}</span>
      <span class="ui-spacer" />
      <span class="ui-muted ui-log-connection">{{ connectionState === 'live' ? '实时连接' : connectionState === 'retrying' ? '重连中 · 定时同步' : '正在连接' }}</span>
      <button class="ui-action" @click="$emit('history')"><History :size="13" />查看操作记录</button>
    </div>
    <template v-if="expanded">
      <div class="ui-client-log-options">
        <span class="ui-muted">{{ channel === 'operations' ? '操作记录与客户端事件，独立保留最近 500 条。' : resourceLogsAvailable === false ? '当前服务器不支持资源请求日志，请更新服务端。' : resourceLogsAvailable ? '资源请求与 HTTP 状态，独立保留最近 500 条。' : '正在连接资源请求日志…' }}</span>
        <span class="ui-spacer" />
        <label class="ui-inline"><input v-model="follow" type="checkbox" />跟随最新</label>
        <button class="ui-action" :disabled="!rows.length" title="只清空当前分类视图，不影响另一类日志或服务端历史" @click="clearView">清空视图</button>
      </div>
      <div :id="id + '-body'" ref="body" class="ui-client-log-body" tabindex="0" role="tabpanel" :aria-labelledby="id + '-' + channel" @scroll="rememberScroll">
        <p v-if="!rows.length" class="ui-muted">暂无日志，连接或操作产生的新记录会显示在这里。</p>
        <div v-for="row in rows" :key="row.id" class="ui-client-log-row">
          <time class="ui-mono ui-muted" :title="new Date(row.timestamp).toLocaleString()">{{ new Date(row.timestamp).toLocaleTimeString('zh-CN', {hour12: false}) }}</time>
          <span class="ui-muted">{{ row.source }}</span>
          <span :title="row.httpStatus ? 'HTTP ' + row.httpStatus + ' · ' + levelText[row.level] : undefined" :class="row.level === 'error' ? 'danger' : row.level === 'warning' ? 'warning' : row.level === 'success' ? 'success' : 'ui-muted'">{{ row.httpStatus || levelText[row.level] }}</span>
          <span class="ui-log-message">{{ row.message }}</span>
        </div>
      </div>
    </template>
  </section>
</template>
