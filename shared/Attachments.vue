<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { Copy, ExternalLink, RefreshCw, Trash2, Upload } from "@lucide/vue";
import Dialog from "./Dialog.vue";
import SegmentedControl from "./SegmentedControl.vue";
import { sizeText, timeText, type AttachmentClient, type AttachmentInput, type AttachmentListing } from "./api";

const props = withDefaults(defineProps<{open: boolean; target: string; client: AttachmentClient; accessEnabled: boolean; serviceAvailable?: boolean}>(), {serviceAvailable: true});
const emit = defineEmits<{close: []; busy: [boolean]}>();
const current = ref<AttachmentListing>({files: [], snapshot: null});
const selected = ref<AttachmentInput[]>([]);
const busy = ref(false);
const loaded = ref(false);
const error = ref("");
const notice = ref("");
const deleting = ref<string | null>(null);
const discarding = ref(false);
const input = ref<HTMLInputElement>();
const directoryInput = ref<HTMLInputElement>();
const mode = ref("files");
const directory = ref("");
const overwrites = computed(() => selected.value.filter(file => current.value.files.some(old => old.name === file.name)).map(file => file.name));
const selectedSize = computed(() => selected.value.reduce((total, file) => total + file.size, 0));
const available = computed(() => props.accessEnabled && props.serviceAvailable !== false);
watch(busy, value => emit("busy", value));
async function run(action: () => Promise<void>) {
  if (busy.value) return;
  busy.value = true; error.value = ""; notice.value = "";
  try { await action(); } catch (e) { error.value = String(e instanceof Error ? e.message : e); } finally { busy.value = false; }
}
async function readDirectory() {
  selected.value = [];
  select(await props.client.readDirectory!(directory.value));
}
async function reload() {
  await run(async () => {
    loaded.value = false;
    current.value = await props.client.list(); loaded.value = true;
    if (mode.value === "directory" && directory.value && props.client.readDirectory) await readDirectory();
  });
}
watch(() => props.open, open => {
  if (!open) return;
  selected.value = []; loaded.value = false; deleting.value = null; discarding.value = false;
  if (input.value) input.value.value = "";
  if (directoryInput.value) directoryInput.value.value = "";
  current.value = {files: [], snapshot: null};
  mode.value = props.client.readDirectory ? "directory" : "files";
  try { directory.value = props.client.directoryKey ? JSON.parse(localStorage.getItem(props.client.directoryKey) || '""') : ""; } catch { directory.value = ""; }
  if (typeof directory.value !== "string") directory.value = "";
  void reload();
}, {immediate: true});
function select(files: AttachmentInput[]) {
  if (!files.length) return;
  error.value = ""; notice.value = "";
  const names = new Set<string>();
  for (const file of files) {
    const name = file.name.toLowerCase();
    if (name.endsWith(".apk") || name === "catalog.json") { error.value = "教程附件不能包含 APK 或 catalog.json"; return; }
    if (file.name.includes("/") || file.name.includes("\\") || names.has(name)) { error.value = "只支持平铺文件，文件名不能重复（不区分大小写）"; return; }
    if (current.value.files.some(old => old.name !== file.name && old.name.toLowerCase() === name)) { error.value = "文件名与已有附件仅大小写不同，请先删除旧文件"; return; }
    names.add(name);
  }
  const total = files.reduce((sum, file) => sum + file.size, 0) + current.value.files.filter(old => !files.some(file => file.name === old.name)).reduce((sum, file) => sum + file.size, 0);
  if (total > 512 * 1024 * 1024) { error.value = "每套教程附件最多 512MB"; return; }
  selected.value = files;
}
function webSelect(event: Event, folder: boolean) {
  const files = Array.from((event.target as HTMLInputElement).files || []);
  if (!files.length) return;
  if (folder) {
    selected.value = [];
    if (files.some(file => file.webkitRelativePath.split('/').length !== 2)) { selected.value = []; error.value = "目前只支持同目录文件，请选择不含子目录的教程文件夹"; return; }
    const visible = files.filter(file => !file.name.startsWith('.'));
    if (!visible.length) { error.value = "目录中没有可上传附件"; return; }
    select(visible);
  } else select(files);
}
async function chooseDirectory() {
  await run(async () => {
    const chosen = await props.client.chooseDirectory!();
    if (!chosen) return;
    directory.value = chosen;
    try { if (props.client.directoryKey) localStorage.setItem(props.client.directoryKey, JSON.stringify(chosen)); } catch { /* Directory selection still works without persistence. */ }
    await readDirectory();
  });
}
function setMode(value: string) {
  mode.value = value; selected.value = []; error.value = "";
  if (value === "directory" && directory.value && props.client.readDirectory) void run(readDirectory);
}
async function choose() { await run(async () => select(await props.client.chooseFiles!())); }
async function upload() {
  await run(async () => {
    if (mode.value === "directory" && props.client.readDirectory) {
      const latest = await props.client.readDirectory(directory.value);
      const signature = (files: AttachmentInput[]) => JSON.stringify(files.map(file => [file.name, file.size]));
      if (signature(latest) !== signature(selected.value)) { selected.value = []; select(latest); if (!error.value) notice.value = "目录文件列表已变化，请检查后再次确认上传。"; return; }
    }
    current.value = await props.client.upload(selected.value, current.value);
    selected.value = []; if (input.value) input.value.value = ""; if (directoryInput.value) directoryInput.value.value = "";
    notice.value = "教程附件已更新并立即生效。";
  });
}
async function remove() {
  const name = deleting.value!;
  await run(async () => { current.value = await props.client.remove(name, current.value); deleting.value = null; notice.value = `${name} 已删除。`; });
}
function close() { if (busy.value) return; if (selected.value.length) discarding.value = true; else emit("close"); }
function canOpen(name: string) { return /\.(html?|svg)$/i.test(name); }
async function copy(name: string) { await run(async () => { await navigator.clipboard.writeText(props.client.url(name)); notice.value = "已复制附件地址。"; }); }
</script>

<template>
  <Dialog :open="open" title="教程附件" wide :busy="busy" @close="close">
    <div class="ui-form">
      <p><strong>{{ target }}</strong> · 同一项目版本共用，独立于 APK 发布编号。</p>
      <p class="ui-muted">同目录多文件；确认上传后立即生效，同名替换，其他文件保留。HTML 可直接打开，交互教程不保存浏览器本地进度。</p>
      <SegmentedControl :model-value="mode" label="附件上传来源" :options="[{value:'directory',label:'选择目录'},{value:'files',label:'选择文件'}]" :disabled="busy" @update:model-value="setMode" />
      <div class="ui-toolbar">
        <template v-if="mode === 'directory'"><button v-if="client.chooseDirectory" :disabled="busy || !loaded" @click="chooseDirectory"><Upload :size="14" />{{ directory ? '更换附件目录' : '选择附件目录' }}</button><div v-else class="ui-field"><label for="tutorial-directory">选择教程目录</label><input id="tutorial-directory" ref="directoryInput" type="file" webkitdirectory multiple :disabled="busy || !loaded" @change="webSelect($event, true)" /><small class="ui-muted">浏览器每次打开都需重新选择目录。</small></div></template>
        <template v-else><button v-if="client.chooseFiles" :disabled="busy || !loaded" @click="choose"><Upload :size="14" />选择附件</button>
        <div v-else class="ui-field"><label for="tutorial-attachments">选择教程附件</label><input id="tutorial-attachments" ref="input" type="file" multiple :disabled="busy || !loaded" @change="webSelect($event, false)" /></div></template>
        <span class="ui-spacer" /><button :disabled="busy" @click="reload"><RefreshCw :size="14" />重新读取</button>
      </div>
      <p v-if="mode === 'directory' && client.readDirectory" class="ui-muted ui-attachment-names">{{ directory || '选择 HTML 所在目录；自动记住目录并在下次打开时重新读取。' }}</p>
      <div v-if="selected.length" class="ui-form">
        <p class="ui-attachment-names">待上传：{{ selected.map(file => file.name).join(' · ') }} · {{ sizeText(selectedSize) }}</p>
        <p v-if="overwrites.length" class="ui-warning-note ui-attachment-names">将覆盖：{{ overwrites.join(' · ') }}</p>
        <div class="ui-inline"><button class="primary" :disabled="busy || !loaded" @click="upload">{{ busy ? '正在处理…' : '确认上传并生效' }}</button><button :disabled="busy" @click="selected = []; if (input) input.value = ''">清除选择</button></div>
      </div>
      <p v-if="!available" class="ui-note">{{ !accessEnabled ? '客户端下载已关闭，附件暂不可公开访问。' : '请先启动本地服务，再打开附件地址。' }}</p>
      <p v-if="busy" role="status" class="ui-muted">正在处理附件…</p>
      <p v-if="error" role="alert" class="ui-error">{{ error }}</p>
      <p v-if="notice" role="status" class="ui-note">{{ notice }}</p>
      <div class="ui-table-wrap">
        <table class="ui-table"><thead><tr><th>文件名</th><th class="num">大小</th><th class="ui-secondary-column">更新时间</th><th>操作</th></tr></thead>
          <tbody><tr v-for="file in current.files" :key="file.name"><td class="ui-attachment-name" :title="file.name">{{ file.name }}</td><td class="num">{{ sizeText(file.size) }}</td><td class="ui-secondary-column ui-mono">{{ timeText(file.modified_timestamp) }}</td><td><div class="ui-inline">
            <button v-if="canOpen(file.name)" :disabled="busy || !available" :title="client.url(file.name)" @click="run(() => client.open(client.url(file.name)))"><ExternalLink :size="14" />打开网页</button>
            <button :disabled="busy" :title="client.url(file.name)" @click="copy(file.name)"><Copy :size="14" />复制地址</button>
            <button class="danger" :disabled="busy || !!selected.length" @click="deleting = file.name"><Trash2 :size="14" />删除</button>
          </div></td></tr><tr v-if="loaded && !current.files.length"><td colspan="4" class="ui-muted">还没有教程附件，可单独上传 HTML 及其关联文件。</td></tr></tbody>
        </table>
      </div>
    </div>
    <template #footer><button :disabled="busy" @click="close">关闭</button></template>
  </Dialog>
  <Dialog :open="deleting !== null" title="删除教程附件？" :busy="busy" @close="deleting = null"><p class="ui-attachment-names">删除 {{ deleting }} 后固定地址立即失效，引用它的页面可能无法完整显示。</p><p v-if="error" class="ui-error" role="alert">{{ error }}</p><template #footer><button :disabled="busy" @click="deleting = null">取消</button><button class="danger" :disabled="busy" @click="remove">确认删除</button></template></Dialog>
  <Dialog :open="discarding" title="放弃未上传的附件？" @close="discarding = false"><p>已选择的文件尚未上传。</p><template #footer><button @click="discarding = false">继续编辑</button><button @click="discarding = false; emit('close')">放弃并关闭</button></template></Dialog>
</template>

<style scoped>
.ui-attachment-names, .ui-attachment-name { overflow-wrap: anywhere; }
.ui-attachment-name { min-width: 100px; max-width: 280px; }
</style>
