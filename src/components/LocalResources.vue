<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { open as openExternal } from "@tauri-apps/plugin-shell";
import Dialog from "../../shared/Dialog.vue";
import ApkDirectory from "../../shared/ApkDirectory.vue";
import Attachments from "../../shared/Attachments.vue";
import SegmentedControl from "../../shared/SegmentedControl.vue";
import { readApkDirectory, importApkRelease } from "../api/apk";
import ToggleSwitch from "../../shared/ToggleSwitch.vue";
import ActionButton from "../../shared/ActionButton.vue";
import { publicResourcePath, sizeText, timeText, type NativeFile, type VersionEntry, type FileEntry, type ApkRelease } from "../../shared/api";
import type { LocalProjectConfig } from "../api/local";
import { attachmentPath, type AttachmentClient, type AttachmentResult } from "../../shared/api";

const props = defineProps<{project: LocalProjectConfig; running?: boolean}>();
const emit = defineEmits<{updated: [LocalProjectConfig]; busy: [boolean]}>();
const versions = ref<VersionEntry[]>([]);
const chosen = ref<NativeFile[]>([]);
const version = ref("");
const importing = ref(false);
const showingFiles = ref(false);
const fileVersion = ref("");
const files = ref<FileEntry[]>([]);
const busy = ref(false);
const error = ref("");
const notice = ref("");
const attachmentsOpen = ref(false);
const attachmentClient = computed<AttachmentClient>(() => {
  const target = props.project;
  const accept = (result: AttachmentResult<LocalProjectConfig>) => { emit("updated", result.project); return result; };
  return {
    list: async () => accept(await invoke<AttachmentResult<LocalProjectConfig>>("list_local_attachments", {projectId: target.id})),
    upload: async (files, current) => accept(await invoke<AttachmentResult<LocalProjectConfig>>("upload_local_attachments", {projectId: target.id, projectVersion: target.project_version, expectedSnapshot: current.snapshot, paths: files.map(file => (file as NativeFile).path)})),
    remove: async (name, current) => accept(await invoke<AttachmentResult<LocalProjectConfig>>("delete_local_attachment", {projectId: target.id, projectVersion: target.project_version, expectedSnapshot: current.snapshot, name})),
    chooseFiles: async () => {
      const paths = await open({multiple: true, directory: false, title: "选择教程附件"});
      return paths ? invoke<NativeFile[]>("describe_upload_files", {paths: Array.isArray(paths) ? paths : [paths]}) : [];
    },
    chooseDirectory: async () => await open({directory: true, multiple: false, title: "选择教程附件目录"}) as string | null,
    readDirectory: directory => invoke<NativeFile[]>("read_attachment_directory", {directory}),
    directoryKey: "tengine_local_attachment_directory_" + JSON.stringify([target.id, target.project_version]),
    url: name => `http://127.0.0.1:${target.port}${attachmentPath(target, target.project_version, name)}`,
    open: openExternal,
  };
});
const directory = ref("");
const directorySource = ref(false);
const apkReleases = ref<ApkRelease[]>([]);
const confirmation = ref<{title: string; message: string; action: () => Promise<void>} | null>(null);
const apk = computed(() => props.project.project_type === "apk");
const platform = computed(() => apk.value ? "Android" : "Any");
const active = computed(() => props.project.active_releases?.[platform.value] || "");
const selectedApkRelease = computed(() => apkReleases.value.find(r => r.version === version.value));
const alreadyImported = computed(() => apk.value && directorySource.value && !!version.value && (versions.value.some(v => v.version === version.value) || !!props.project.published_releases?.includes(version.value)));
function directoryKey() { return "tengine_local_apk_directory_" + JSON.stringify([props.project.id, props.project.project_version]); }
watch(busy, value => emit("busy", value));
let generation = 0;
async function reload() {
  const current = ++generation;
  const result = await invoke<VersionEntry[]>("list_local_releases", {projectId: props.project.id});
  if (current === generation) versions.value = result;
}
async function run(action: () => Promise<void>) {
  if (busy.value) return;
  busy.value = true;
  error.value = "";
  notice.value = "";
  try { await action(); } catch (e) { error.value = String(e); } finally { busy.value = false; }
}
watch(() => [props.project.id, props.project.project_version], () => {
  importing.value = false; showingFiles.value = false; chosen.value = []; version.value = ""; versions.value = []; apkReleases.value = []; confirmation.value = null;
  try { directory.value = JSON.parse(localStorage.getItem(directoryKey()) || '""'); } catch { directory.value = ""; }
  void run(reload);
}, {immediate: true});
function selectApkVersion(selected: string) { version.value = selected; chosen.value = apkReleases.value.find(r => r.version === selected)?.files || []; }
function setApkSource(source: string) { directorySource.value = source === "directory"; chosen.value = []; version.value = ""; if (directorySource.value) selectApkVersion(apkReleases.value[0]?.version || ""); }
async function loadApkVersions() { apkReleases.value = []; selectApkVersion(""); apkReleases.value = await readApkDirectory(directory.value); selectApkVersion(apkReleases.value[0]?.version || ""); }
async function chooseDirectory() {
  await run(async () => {
    const selected = await open({directory: true, multiple: false, title: "选择 APK 构建目录"});
    if (!selected || Array.isArray(selected)) return;
    directory.value = selected;
    localStorage.setItem(directoryKey(), JSON.stringify(selected));
    await loadApkVersions();
  });
}
async function rereadDirectory() { await run(async () => { await Promise.all([reload(), loadApkVersions()]); }); }
async function openImport() {
  chosen.value = []; version.value = ""; directorySource.value = apk.value; importing.value = true;
  await run(async () => { await Promise.all([reload(), apk.value && directory.value ? loadApkVersions() : Promise.resolve()]); });
}
async function importLatest() {
  await openImport();
  if (error.value || !selectedApkRelease.value) return;
  if (alreadyImported.value) { error.value = "最新 APK 编号已导入或曾发布，请查看现有版本，或构建新编号后重新读取。"; return; }
  await importFiles();
}
async function choose(role: "apk" | "catalog" | "files") {
  await run(async () => {
    const selection = await open({multiple: role === "files", directory: false, title: role === "apk" ? "选择 APK" : role === "catalog" ? "选择 catalog.json" : "选择下载文件", filters: role === "files" ? undefined : [{name: role === "apk" ? "APK" : "JSON", extensions: [role === "apk" ? "apk" : "json"]}]});
    if (!selection) return;
    const picked = await invoke<NativeFile[]>("describe_upload_files", {paths: Array.isArray(selection) ? selection : [selection]});
    if (role === "catalog" && picked.some(f => f.name !== "catalog.json")) throw new Error("目录文件必须命名为 catalog.json");
    if (role === "apk" && picked.some(f => !f.name.toLowerCase().endsWith(".apk"))) throw new Error("请选择 .apk 文件");
    chosen.value = role === "files" ? picked : [...chosen.value.filter(f => role === "apk" ? !f.name.toLowerCase().endsWith(".apk") : f.name !== "catalog.json"), ...picked];
  });
}
function closeImport() {
  if (busy.value) return;
  if (chosen.value.length || version.value) confirmation.value = {title: "放弃未导入的文件？", message: "当前文件选择和发布编号尚未导入。", action: async () => { importing.value = false; chosen.value = []; version.value = ""; }};
  else importing.value = false;
}
async function importFiles() {
  await run(async () => {
    if (apk.value && directorySource.value && (!selectedApkRelease.value || alreadyImported.value)) throw new Error("请选择完整且尚未导入的 APK 版本");
    const uploaded = apk.value && directorySource.value ? await importApkRelease(props.project.id, directory.value, selectedApkRelease.value!) : await invoke<string>("import_local_release", {projectId: props.project.id, paths: chosen.value.map(f => f.path), version: version.value});
    importing.value = false;
    chosen.value = [];
    version.value = "";
    await reload();
    notice.value = `${uploaded} 已导入，尚未发布。`;
  });
}
function release(selected: string, unpublish = false) {
  confirmation.value = {title: unpublish ? "取消当前发布？" : "发布此版本？", message: unpublish && apk.value ? "停止提供 catalog，历史 APK 仍可下载。" : unpublish ? "取消后文件将不可下载。" : `当前发布 ${active.value || '—'} → ${selected}。`, action: async () => {
    const updated = await invoke<LocalProjectConfig>("activate_local_release", {projectId: props.project.id, version: selected, unpublish});
    emit("updated", updated);
    notice.value = unpublish ? "已取消发布" : `发布成功，当前版本已更新为 ${selected}。`;
  }};
}
function remove(selected: string) {
  confirmation.value = {title: "删除资源版本？", message: `永久删除 ${selected} 的全部文件。${apk.value ? '旧 catalog 的 APK 链接会失效；曾发布的编号不能复用。' : ''}`, action: async () => { await invoke("delete_local_release", {projectId: props.project.id, version: selected}); await reload(); }};
}
async function accept() { await run(async () => { await confirmation.value!.action(); confirmation.value = null; }); }
async function toggleDownload(enabled: boolean) {
  await run(async () => { emit("updated", await invoke<LocalProjectConfig>("set_local_download_access", {projectId: props.project.id, enabled})); });
}
async function showFiles(selected: string) {
  await run(async () => { files.value = await invoke<FileEntry[]>("list_local_release_files", {projectId: props.project.id, version: selected}); fileVersion.value = selected; showingFiles.value = true; });
}
function downloadable(name: string) {
  return apk.value && name.toLowerCase().endsWith(".apk") ? !!props.project.published_releases?.includes(fileVersion.value) : fileVersion.value === active.value;
}
function fileUrl(name: string) { return `http://127.0.0.1:${props.project.port}${publicResourcePath(props.project, props.project.project_version, platform.value, name, apk.value && name.toLowerCase().endsWith('.apk') ? fileVersion.value : undefined)}`; }
async function copy(name: string) { await run(async () => { await navigator.clipboard.writeText(fileUrl(name)); notice.value = "已复制下载地址"; }); }
</script>

<template>
  <Attachments :key="project.id + '/' + project.project_version" :open="attachmentsOpen" :target="project.project_name + ' / ' + project.project_version" :client="attachmentClient" :access-enabled="project.platform_access.Android !== false" :service-available="running" @close="attachmentsOpen = false" @busy="busy = $event" />
  <div class="ui-form local-resources">
    <div class="ui-toolbar"><span>当前发布 <strong class="ui-mono">{{ active || '—' }}</strong></span><ToggleSwitch :model-value="project.platform_access[platform] !== false" label="允许客户端下载" :disabled="busy" @update:model-value="toggleDownload" /><button v-if="apk" :disabled="busy" @click="attachmentsOpen = true">教程附件</button><span class="ui-spacer" /><button v-if="active" :disabled="busy" @click="release(active, true)">取消发布</button><button v-if="apk && directory" class="primary" :disabled="busy" @click="importLatest">导入最新版本</button><button :class="{primary: !(apk && directory)}" :disabled="busy" @click="openImport">导入文件</button></div>
    <div v-if="busy" role="status">正在处理文件…</div><p v-if="notice" role="status" class="ui-note">{{ notice }}</p><div v-if="error" role="alert" class="ui-error">{{ error }}</div>
    <div class="ui-table-wrap"><table class="ui-table"><thead><tr><th>{{ apk ? '发布编号' : '文件版本' }}</th><th class="num">文件</th><th class="num">大小</th><th>状态</th><th>操作</th></tr></thead><tbody><tr v-for="v in versions" :key="v.version"><td class="ui-mono" :title="timeText(v.modified_timestamp)">{{ v.version }}</td><td class="num">{{ v.file_count }}</td><td class="num">{{ sizeText(v.total_size) }}</td><td>{{ v.version === active ? '当前发布' : project.published_releases?.includes(v.version) ? '历史发布' : '待发布' }}</td><td><div class="ui-inline"><button :disabled="busy" @click="showFiles(v.version)">文件</button><button :disabled="busy || v.version === active" @click="release(v.version)">发布</button><ActionButton label="删除" :accessible-label="'删除资源版本 ' + v.version" :disabled="busy || v.version === active" reason="当前版本已发布，请先切换或取消发布。" danger @click="remove(v.version)" /></div></td></tr><tr v-if="!versions.length"><td colspan="5" class="ui-muted">先导入文件，再发布一个版本。</td></tr></tbody></table></div>
  </div>
  <Dialog :open="importing" title="导入本地文件" :busy="busy" @close="closeImport">
    <form id="local-import-form" class="ui-form" @submit.prevent="importFiles">
      <SegmentedControl v-if="apk" :model-value="directorySource ? 'directory' : 'files'" label="导入来源" :options="[{value:'files',label:'文件导入'},{value:'directory',label:'本地构建目录'}]" :disabled="busy" @update:model-value="setApkSource" />
      <template v-if="apk && directorySource"><ApkDirectory :directory="directory" :releases="apkReleases" :version="version" :busy="busy" @choose="chooseDirectory" @reread="rereadDirectory" @select="selectApkVersion" /><p v-if="alreadyImported" class="ui-warning-note">此编号已导入或曾发布，请使用现有版本或构建新编号。</p></template>
      <template v-else>
        <div class="ui-toolbar"><template v-if="apk"><button type="button" :disabled="busy" @click="choose('apk')">选择 APK</button><button type="button" :disabled="busy" @click="choose('catalog')">选择 catalog.json</button></template><button v-else type="button" :disabled="busy" @click="choose('files')">选择文件</button></div>
        <p class="ui-muted ui-apk-directory">{{ chosen.map(f => f.name).join(' · ') || '尚未选择文件' }} · {{ sizeText(chosen.reduce((sum, f) => sum + f.size, 0)) }}</p>
        <div class="ui-field"><label for="local-release-number">{{ apk ? '发布编号（versionCode）' : '文件版本号（可选）' }}</label><input id="local-release-number" v-model="version" :required="apk" :disabled="busy" :placeholder="apk ? '例如：69' : '留空自动生成'" /><small class="ui-muted">{{ apk ? 'catalog 原样托管；APK 路径为 releases/发布编号/APK文件名。' : '平铺文件，整批导入后发布，不保留子目录。' }} 单次最多 512MB。</small></div>
      </template>
      <div v-if="busy" class="ui-progress" role="status">{{ apk && directorySource ? '正在准备 APK、校验 SHA-256 并导入…' : '正在处理…' }}</div><div v-if="error" class="ui-error" role="alert">{{ error }}</div>
    </form>
    <template #footer><button :disabled="busy" @click="closeImport">取消</button><button class="primary" type="submit" form="local-import-form" :disabled="busy || !chosen.length || alreadyImported">{{ busy ? '正在处理…' : '确认导入' }}</button></template>
  </Dialog>
  <Dialog :open="showingFiles" :title="'资源文件 · ' + fileVersion" :busy="busy" wide @close="showingFiles = false"><div class="ui-table-wrap"><table class="ui-table ui-file-table"><thead><tr><th>文件名</th><th class="num">大小</th><th>操作</th></tr></thead><tbody><tr v-for="file in files" :key="file.name"><td class="ui-filename ui-mono">{{ file.name }}</td><td class="num">{{ sizeText(file.size) }}</td><td><div class="ui-inline"><button :disabled="busy || !downloadable(file.name)" @click="copy(file.name)">复制地址</button><button :disabled="busy || !downloadable(file.name)" @click="run(() => openExternal(fileUrl(file.name)))">下载</button></div></td></tr></tbody></table></div><div v-if="error" class="ui-error" role="alert">{{ error }}</div></Dialog>
  <Dialog :open="!!confirmation" :title="confirmation?.title || ''" :busy="busy" @close="confirmation = null"><p>{{ confirmation?.message }}</p><div v-if="error" class="ui-error" role="alert">{{ error }}</div><template #footer><button :disabled="busy" @click="confirmation = null">取消</button><button :disabled="busy" class="primary" @click="accept">确认</button></template></Dialog>
</template>
