<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, nextTick, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { open as openExternal } from "@tauri-apps/plugin-shell";

const VIEWABLE_EXTS = ["version", "hash", "report", "json", "txt", "log", "xml", "yaml", "yml", "csv"];

function isViewable(fileName: string): boolean {
  const ext = fileName.split(".").pop()?.toLowerCase() || "";
  return VIEWABLE_EXTS.includes(ext);
}

async function viewFile(fileName: string) {
  try {
    await openExternal(buildResourceUrl(fileName));
  } catch (e) {
    console.error("Failed to open URL:", e);
  }
}
import { api, type ProjectConfig, type ProjectVersion, type VersionEntry, type LogEntry, type FileEntry, type FileManifestEntry } from "../api/remote";

interface LocalVersionEntry {
  version: string;
  modified_timestamp: number;
  file_count: number;
  total_size: number;
}

const BUNDLES_DIR_KEY = "tengine_remote_bundles_dirs"; // {server_url__project_id: bundles_dir}

interface SavedConnection {
  id: string;
  name: string;
  url: string;
  password: string; // empty if user chose not to save
  rememberPassword: boolean;
  lastUsed: number;
}

const STORAGE_KEY = "tengine_remote_connections";

const connected = ref(false);
const serverUrl = ref("");
const password = ref("");
const connectionName = ref("");
const rememberPassword = ref(true);
const loginError = ref("");
const loginLoading = ref(false);

const savedConnections = ref<SavedConnection[]>([]);
const selectedConnectionId = ref<string>("");
const showAddForm = ref(false);

function loadSavedConnections() {
  try {
    const data = localStorage.getItem(STORAGE_KEY);
    if (data) {
      savedConnections.value = JSON.parse(data);
      savedConnections.value.sort((a, b) => b.lastUsed - a.lastUsed);
    }
  } catch {}
}

function saveSavedConnections() {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(savedConnections.value));
  } catch {}
}

function selectConnection(conn: SavedConnection) {
  selectedConnectionId.value = conn.id;
  serverUrl.value = conn.url;
  password.value = conn.password;
  connectionName.value = conn.name;
  rememberPassword.value = conn.rememberPassword;
  loginError.value = "";
  showAddForm.value = false;
}

function startAddNew() {
  selectedConnectionId.value = "";
  serverUrl.value = "";
  password.value = "";
  connectionName.value = "";
  rememberPassword.value = true;
  loginError.value = "";
  showAddForm.value = true;
}

function deleteConnection(id: string, e: Event) {
  e.stopPropagation();
  if (!confirm("确定删除这个连接吗？")) return;
  savedConnections.value = savedConnections.value.filter((c) => c.id !== id);
  saveSavedConnections();
  if (selectedConnectionId.value === id) {
    selectedConnectionId.value = "";
    serverUrl.value = "";
    password.value = "";
    connectionName.value = "";
    showAddForm.value = savedConnections.value.length === 0;
  }
}

function persistCurrentConnection() {
  const name = connectionName.value.trim() || serverUrl.value;
  const existing = savedConnections.value.find((c) => c.id === selectedConnectionId.value);
  const entry: SavedConnection = {
    id: existing?.id || `conn_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`,
    name,
    url: serverUrl.value,
    password: rememberPassword.value ? password.value : "",
    rememberPassword: rememberPassword.value,
    lastUsed: Date.now(),
  };
  if (existing) {
    Object.assign(existing, entry);
  } else {
    savedConnections.value.unshift(entry);
    selectedConnectionId.value = entry.id;
  }
  saveSavedConnections();
}

const projects = ref<ProjectConfig[]>([]);
const activeProjectId = ref("");
const activeProjectVersionName = ref<string>("");
const newProjectVersionName = ref<string>("");
const showCreateProjectVersion = ref(false);
const projectSettingsExpanded = ref(false);
const versions = ref<VersionEntry[]>([]);
const logs = ref<LogEntry[]>([]);
const uploading = ref(false);
const selectedPlatform = ref("Android");
let ws: WebSocket | null = null;

// Per-project local bundles dir (key: serverUrl__projectId)
const bundlesDirMap = ref<Record<string, string>>({});

// File browser dialog state
const fileBrowser = ref<{
  show: boolean;
  version: string;
  isActive: boolean;
  loading: boolean;
  files: FileEntry[];
  error: string;
}>({
  show: false,
  version: "",
  isActive: false,
  loading: false,
  files: [],
  error: "",
});
const copiedUrl = ref<string>("");

// Sync dialog state
const syncDialog = ref<{
  show: boolean;
  loading: boolean;
  versions: LocalVersionEntry[];
  selectedVersion: string;
  error: string;
}>({
  show: false,
  loading: false,
  versions: [],
  selectedVersion: "",
  error: "",
});

// === Incremental upload state ===
interface DiffResult {
  baseVersion: string;
  modified: FileManifestEntry[];   // 已修改：本地和远端都有，MD5 不同
  added: FileManifestEntry[];      // 新增：仅本地有
  unchanged: FileManifestEntry[];  // 未变化：MD5 相同
  removed: FileManifestEntry[];    // 已移除：仅远端有（仅展示）
  totalLocalCount: number;
  totalLocalSize: number;
  uploadCount: number;
  uploadSize: number;
}

const incrementalEnabled = ref(true);
const diffDialogOpen = ref(false);
const diff = ref<DiffResult | null>(null);
const diffLoading = ref(false);
const diffError = ref("");
const diffGroups = ref({ modified: true, added: true, unchanged: false, removed: false });

const canIncremental = computed(() => versions.value.length > 0);

const baseVersionForIncremental = computed(() =>
  versions.value.length > 0 ? versions.value[0].version : ""
);

const savedBytes = computed(() =>
  diff.value ? diff.value.totalLocalSize - diff.value.uploadSize : 0
);
const savedPercent = computed(() => {
  if (!diff.value || diff.value.totalLocalSize === 0) return 0;
  return Math.round((savedBytes.value / diff.value.totalLocalSize) * 100);
});

let diffGeneration = 0;

async function computeDiff() {
  const project = activeProject.value;
  if (!project || !syncDialog.value.selectedVersion) return;
  if (!canIncremental.value) return;

  const gen = ++diffGeneration;
  const baseVersion = baseVersionForIncremental.value;
  const localVersion = syncDialog.value.selectedVersion;

  diffLoading.value = true;
  diffError.value = "";
  diffDialogOpen.value = false;
  diff.value = null;

  try {
    const [local, remote] = await Promise.all([
      invoke<FileManifestEntry[]>("compute_local_manifest", {
        bundlesDir: currentBundlesDir.value,
        packageName: project.package_name,
        platform: selectedPlatform.value,
        version: localVersion,
      }),
      api.getVersionManifest(project.id, activeProjectVersionName.value, selectedPlatform.value, baseVersion),
    ]);
    if (gen !== diffGeneration) return;  // stale, discard

    const remoteMap = new Map(remote.map((f) => [f.name, f]));
    const localMap = new Map(local.map((f) => [f.name, f]));

    const modified: FileManifestEntry[] = [];
    const added: FileManifestEntry[] = [];
    const unchanged: FileManifestEntry[] = [];
    for (const f of local) {
      const r = remoteMap.get(f.name);
      if (!r) {
        added.push(f);
      } else if (r.md5 !== f.md5 || r.size !== f.size) {
        modified.push(f);
      } else {
        unchanged.push(f);
      }
    }
    const removed: FileManifestEntry[] = [];
    for (const f of remote) {
      if (!localMap.has(f.name)) removed.push(f);
    }

    const totalLocalSize = local.reduce((s, f) => s + f.size, 0);
    const uploadList = [...modified, ...added];
    const uploadSize = uploadList.reduce((s, f) => s + f.size, 0);

    diff.value = {
      baseVersion,
      modified,
      added,
      unchanged,
      removed,
      totalLocalCount: local.length,
      totalLocalSize,
      uploadCount: uploadList.length,
      uploadSize,
    };
  } catch (e: any) {
    if (gen !== diffGeneration) return;  // stale, discard
    diffError.value = `计算差异失败: ${e?.message || e}`;
  } finally {
    if (gen === diffGeneration) {
      diffLoading.value = false;
    }
  }
}

function selectSyncVersion(version: string) {
  syncDialog.value.selectedVersion = version;
  if (incrementalEnabled.value && canIncremental.value) {
    computeDiff();
  }
}

watch(incrementalEnabled, (val) => {
  if (val && canIncremental.value && syncDialog.value.selectedVersion && !diff.value && !diffLoading.value) {
    computeDiff();
  }
});

const AVAILABLE_PLATFORMS = ["Android", "iOS", "Windows", "MacOS", "Linux", "WebGL"];

const activeProject = computed(() =>
  projects.value.find((p) => p.id === activeProjectId.value)
);

const activeProjectVersion = computed<ProjectVersion | undefined>(() => {
  const proj = activeProject.value;
  if (!proj) return undefined;
  return proj.project_versions.find((v) => v.name === activeProjectVersionName.value);
});

const filteredLogs = computed(() => {
  if (!activeProjectId.value) return logs.value;
  return logs.value.filter((l) =>
    l.project_id === activeProject.value?.project_name || l.project_id === activeProjectId.value
  );
});

async function handleLogin() {
  loginError.value = "";
  loginLoading.value = true;
  try {
    api.setBaseUrl(serverUrl.value);
    const ok = await api.login(password.value);
    if (ok) {
      persistCurrentConnection();
      connected.value = true;
      await loadProjects();
      connectWebSocket();
    } else {
      loginError.value = "密码错误";
    }
  } catch (e: any) {
    console.error("[Login error]", e);
    const detail = e?.message || e?.toString?.() || (typeof e === "string" ? e : JSON.stringify(e));
    loginError.value = `连接失败: ${detail}`;
  } finally {
    loginLoading.value = false;
  }
}

onMounted(() => {
  loadSavedConnections();
  loadBundlesDirMap();
  if (savedConnections.value.length > 0) {
    selectConnection(savedConnections.value[0]);
  } else {
    showAddForm.value = true;
  }
});

function disconnect() {
  ws?.close();
  api.logout();
  connected.value = false;
  projects.value = [];
  logs.value = [];
}

function connectWebSocket() {
  ws = api.connectLogs(
    (log) => {
      logs.value.push(log);
      if (logs.value.length > 2000) logs.value = logs.value.slice(-1500);
      nextTick(() => {
        const el = document.querySelector(".log-body");
        if (el) el.scrollTop = el.scrollHeight;
      });
    },
    () => setTimeout(connectWebSocket, 3000),
  );
}

async function loadProjects() {
  try {
    const list = await api.listProjects();
    projects.value = list.map((p) => ({
      ...p,
      project_versions: Array.isArray(p.project_versions) ? p.project_versions : [],
    }));
    if (projects.value.length > 0 && !activeProjectId.value) {
      activeProjectId.value = projects.value[0].id;
    }
    syncActiveProjectVersion();
    if (activeProjectVersion.value) await loadVersions();
  } catch (e) {
    console.error("[loadProjects] error", e);
  }
}

function syncActiveProjectVersion() {
  const proj = activeProject.value;
  if (!proj) {
    activeProjectVersionName.value = "";
    return;
  }
  const list = proj.project_versions ?? [];
  if (
    !activeProjectVersionName.value ||
    !list.some((v) => v.name === activeProjectVersionName.value)
  ) {
    activeProjectVersionName.value = list[0]?.name || "";
  }
}

watch(() => activeProjectId.value, () => {
  showCreateProjectVersion.value = false;
  newProjectVersionName.value = "";
  syncActiveProjectVersion();
});

watch(() => activeProjectVersionName.value, () => {
  if (activeProjectVersion.value) loadVersions();
});

async function addProject() {
  const name = `Project_${projects.value.length + 1}`;
  try {
    const project = await api.createProject(name);
    if (!project || !project.id) {
      console.error("[addProject] response missing id", project);
      await loadProjects();
      return;
    }
    if (!Array.isArray(project.project_versions)) {
      project.project_versions = [];
    }
    projects.value.push(project);
    activeProjectId.value = project.id;
  } catch (e: any) {
    console.error("[addProject] error", e);
    alert("添加项目失败: " + (e?.message || e));
  }
}

async function removeProject(id: string) {
  if (projects.value.length <= 1) return;
  try {
    await api.deleteProject(id);
    projects.value = projects.value.filter((p) => p.id !== id);
    if (activeProjectId.value === id) activeProjectId.value = projects.value[0]?.id || "";
  } catch (e: any) { alert("删除项目失败: " + (e?.message || e)); }
}

async function saveProject() {
  const project = activeProject.value;
  if (!project) return;
  try { await api.updateProject(project); } catch {}
}

let saveTimer: ReturnType<typeof setTimeout> | null = null;
watch(
  () => {
    const p = activeProject.value;
    if (!p) return null;
    return {
      project_name: p.project_name,
      package_name: p.package_name,
      platforms: [...p.platforms],
    };
  },
  () => {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => saveProject(), 500);
  },
  { deep: true }
);

async function loadVersions() {
  const project = activeProject.value;
  const pvName = activeProjectVersionName.value;
  if (!project || !pvName) {
    versions.value = [];
    return;
  }
  try {
    versions.value = await api.listVersions(project.id, pvName, selectedPlatform.value);
  } catch {
    versions.value = [];
  }
}

async function openFileBrowser(version: string) {
  const project = activeProject.value;
  const pvName = activeProjectVersionName.value;
  if (!project || !pvName) return;
  const isActive =
    activeProjectVersion.value?.platform_settings[selectedPlatform.value]?.active_bundle === version;
  fileBrowser.value = { show: true, version, isActive, loading: true, files: [], error: "" };
  try {
    const files = await api.listFiles(project.id, pvName, selectedPlatform.value, version);
    fileBrowser.value.files = files;
  } catch (e: any) {
    fileBrowser.value.error = `加载失败: ${e?.message || e}`;
  } finally {
    fileBrowser.value.loading = false;
  }
}

function buildResourceUrl(fileName: string): string {
  const project = activeProject.value;
  const pvName = activeProjectVersionName.value;
  if (!project || !pvName) return "";
  return `${serverUrl.value.replace(/\/$/, "")}/res/${encodeURIComponent(pvName)}/${encodeURIComponent(project.project_name)}/${encodeURIComponent(selectedPlatform.value)}/${encodeURIComponent(fileName)}`;
}

async function copyToClipboard(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    copiedUrl.value = text;
    setTimeout(() => {
      if (copiedUrl.value === text) copiedUrl.value = "";
    }, 1500);
  } catch {}
}

function copyAllBrowserUrls() {
  if (!fileBrowser.value.isActive) return;
  const urls = fileBrowser.value.files.map((f) => buildResourceUrl(f.name)).join("\n");
  copyToClipboard(urls);
}

// === Bundles dir persistence ===
function loadBundlesDirMap() {
  try {
    const data = localStorage.getItem(BUNDLES_DIR_KEY);
    if (data) bundlesDirMap.value = JSON.parse(data);
  } catch {}
}
function saveBundlesDirMap() {
  try {
    localStorage.setItem(BUNDLES_DIR_KEY, JSON.stringify(bundlesDirMap.value));
  } catch {}
}
function bundlesDirKey(): string {
  const project = activeProject.value;
  if (!project) return "";
  return `${serverUrl.value}__${project.id}`;
}
const currentBundlesDir = computed({
  get: () => bundlesDirMap.value[bundlesDirKey()] || "",
  set: (val: string) => {
    const k = bundlesDirKey();
    if (!k) return;
    bundlesDirMap.value[k] = val;
    saveBundlesDirMap();
  },
});

async function selectBundlesDir() {
  const selected = await open({
    directory: true,
    multiple: false,
    title: "选择 Bundles 目录",
  });
  if (selected) {
    currentBundlesDir.value = selected as string;
  }
}

// === Sync workflow ===
async function startSync() {
  const project = activeProject.value;
  if (!project) return;
  if (!currentBundlesDir.value) {
    alert("请先选择本地 Bundles 目录");
    return;
  }
  if (!project.platforms.includes(selectedPlatform.value)) {
    alert(`项目未启用平台 ${selectedPlatform.value}`);
    return;
  }

  syncDialog.value = {
    show: true,
    loading: true,
    versions: [],
    selectedVersion: "",
    error: "",
  };
  // Reset incremental state
  diff.value = null;
  diffError.value = "";
  diffDialogOpen.value = false;
  // 服务器无版本时禁用增量上传
  incrementalEnabled.value = canIncremental.value;

  try {
    const list = await invoke<LocalVersionEntry[]>("list_local_bundle_versions", {
      bundlesDir: currentBundlesDir.value,
      packageName: project.package_name,
      platform: selectedPlatform.value,
    });
    syncDialog.value.versions = list;
    if (list.length > 0) {
      syncDialog.value.selectedVersion = list[0].version;
    }
    if (list.length === 0) {
      syncDialog.value.error = `在 ${selectedPlatform.value}/${project.package_name}/ 下未找到任何版本`;
    }
  } catch (e: any) {
    syncDialog.value.error = `读取版本失败: ${e}`;
  } finally {
    syncDialog.value.loading = false;
  }

  // 自动触发差异计算
  if (incrementalEnabled.value && syncDialog.value.selectedVersion) {
    computeDiff();
  }
}

async function confirmSync() {
  const project = activeProject.value;
  if (!project || !syncDialog.value.selectedVersion) return;

  const version = syncDialog.value.selectedVersion;
  const useIncremental =
    incrementalEnabled.value && canIncremental.value && diff.value !== null;
  syncDialog.value.show = false;
  uploading.value = true;

  try {
    const token = api.getToken();
    if (useIncremental && diff.value) {
      const uploadFiles = [...diff.value.modified, ...diff.value.added].map((f) => f.name);
      const copyFiles = diff.value.unchanged.map((f) => f.name);
      await invoke("incremental_upload_to_remote", {
        bundlesDir: currentBundlesDir.value,
        packageName: project.package_name,
        platform: selectedPlatform.value,
        version,
        projectId: project.id,
        projectVersion: activeProjectVersionName.value,
        serverUrl: serverUrl.value,
        token,
        baseVersion: diff.value.baseVersion,
        copyFiles,
        uploadFiles,
      });
    } else {
      await invoke("upload_version_to_remote", {
        bundlesDir: currentBundlesDir.value,
        packageName: project.package_name,
        platform: selectedPlatform.value,
        version,
        projectId: project.id,
        projectVersion: activeProjectVersionName.value,
        serverUrl: serverUrl.value,
        token,
      });
    }
    await loadVersions();
    // Auto-activate the just-uploaded version
    await activateVersion(version);
  } catch (e: any) {
    alert(`上传失败: ${e}`);
  } finally {
    uploading.value = false;
  }
}

async function activateVersion(version: string) {
  const project = activeProject.value;
  const pvName = activeProjectVersionName.value;
  if (!project || !pvName) return;
  try {
    await api.activateVersion(project.id, pvName, version, selectedPlatform.value);
    const pv = activeProjectVersion.value;
    if (pv) {
      const settings = pv.platform_settings[selectedPlatform.value] ?? { access_enabled: true, active_bundle: null };
      settings.active_bundle = version;
      pv.platform_settings[selectedPlatform.value] = settings;
    }
  } catch (e: any) { alert("激活失败: " + (e?.message || e)); }
}

async function deleteVersion(version: string) {
  const project = activeProject.value;
  const pvName = activeProjectVersionName.value;
  if (!project || !pvName) return;
  try {
    await api.deleteVersion(project.id, pvName, version, selectedPlatform.value);
    await loadVersions();
  } catch (e: any) { alert("删除版本失败: " + (e?.message || e)); }
}

async function createProjectVersion() {
  const proj = activeProject.value;
  const name = newProjectVersionName.value.trim();
  if (!proj || !name) return;
  try {
    const pv = await api.createProjectVersion(proj.id, name);
    proj.project_versions.push(pv);
    activeProjectVersionName.value = pv.name;
    newProjectVersionName.value = "";
    showCreateProjectVersion.value = false;
  } catch (e: any) {
    alert(`创建失败: ${e?.message || e}`);
  }
}

async function removeProjectVersion(name: string) {
  const proj = activeProject.value;
  if (!proj) return;
  if (!confirm(`确认删除项目版本 "${name}"？该版本下所有 bundle 资源会一并删除。`)) return;
  try {
    await api.deleteProjectVersion(proj.id, name);
    proj.project_versions = proj.project_versions.filter((v) => v.name !== name);
    if (activeProjectVersionName.value === name) {
      activeProjectVersionName.value = proj.project_versions[0]?.name || "";
    }
  } catch (e: any) {
    alert(`删除失败: ${e?.message || e}`);
  }
}

async function togglePlatformAccess(platform: string) {
  const proj = activeProject.value;
  const pv = activeProjectVersion.value;
  if (!proj || !pv) return;
  const current = pv.platform_settings[platform]?.access_enabled ?? false;
  const next = !current;
  try {
    await api.setPlatformAccess(proj.id, pv.name, platform, next);
    if (!pv.platform_settings[platform]) {
      pv.platform_settings[platform] = { access_enabled: next, active_bundle: null };
    } else {
      pv.platform_settings[platform].access_enabled = next;
    }
  } catch (e: any) {
    alert(`切换失败: ${e?.message || e}`);
  }
}

function togglePlatform(platform: string) {
  const project = activeProject.value;
  if (!project) return;
  const idx = project.platforms.indexOf(platform);
  if (idx >= 0) {
    if (project.platforms.length > 1) project.platforms.splice(idx, 1);
  } else {
    project.platforms.push(platform);
  }
  saveProject();
}

function formatSize(bytes: number): string {
  if (bytes >= 1073741824) return (bytes / 1073741824).toFixed(1) + " GB";
  if (bytes >= 1048576) return (bytes / 1048576).toFixed(1) + " MB";
  if (bytes >= 1024) return (bytes / 1024).toFixed(1) + " KB";
  return bytes + " B";
}

function formatTime(timestamp: number): string {
  if (!timestamp) return "";
  const d = new Date(timestamp * 1000);
  const pad = (n: number) => n.toString().padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function getStatusClass(status: number): string {
  if (status >= 200 && status < 300) return "s200";
  if (status >= 300 && status < 400) return "s301";
  if (status >= 400 && status < 500) return "s404";
  return "s500";
}

function clearLogs() { logs.value = []; }

const logPanelOpen = ref(true);
const logPanelHeight = ref(220);

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
  logPanelHeight.value = Math.max(100, Math.min(500, startHeight + (startY - e.clientY)));
}
function onResizeEnd() {
  isResizing = false;
  document.removeEventListener("mousemove", onResizeMove);
  document.removeEventListener("mouseup", onResizeEnd);
}

onUnmounted(() => { ws?.close(); });
</script>

<template>
  <!-- Login -->
  <div v-if="!connected" class="rm-login-wrap">
    <div class="rm-login-shell">
      <!-- Sidebar: saved servers -->
      <aside class="rm-sidebar">
        <div class="rm-sidebar-header">
          <span>已保存的服务器</span>
          <button class="rm-icon-btn" @click="startAddNew" title="新增连接">+</button>
        </div>
        <div class="rm-conn-list">
          <div v-if="savedConnections.length === 0" class="rm-conn-empty">
            还没有保存的连接<br/>点击右上角 + 添加
          </div>
          <div
            v-for="conn in savedConnections"
            :key="conn.id"
            class="rm-conn-item"
            :class="{ active: selectedConnectionId === conn.id && !showAddForm }"
            @click="selectConnection(conn)"
          >
            <div class="rm-conn-info">
              <div class="rm-conn-name">{{ conn.name }}</div>
              <div class="rm-conn-url">{{ conn.url }}</div>
            </div>
            <button class="rm-conn-delete" @click="deleteConnection(conn.id, $event)" title="删除">×</button>
          </div>
        </div>
      </aside>

      <!-- Main: login form -->
      <div class="rm-form">
        <div class="rm-form-title">
          <span>{{ showAddForm ? "新增远程连接" : "连接远程服务器" }}</span>
        </div>

        <div class="rm-field">
          <label>连接名称</label>
          <input
            v-model="connectionName"
            class="rm-input"
            placeholder="例如：群晖测试服 / 公司服务器"
            @keyup.enter="handleLogin"
          />
        </div>

        <div class="rm-field">
          <label>服务器地址</label>
          <input
            v-model="serverUrl"
            class="rm-input"
            placeholder="http://192.168.1.100:8082"
            @keyup.enter="handleLogin"
          />
        </div>

        <div class="rm-field">
          <label>管理密码</label>
          <input
            v-model="password"
            class="rm-input"
            type="password"
            placeholder="输入管理密码"
            @keyup.enter="handleLogin"
          />
        </div>

        <label class="rm-checkbox">
          <input type="checkbox" v-model="rememberPassword" />
          <span>记住密码（明文保存在本机，仅推荐内网使用）</span>
        </label>

        <div v-if="loginError" class="rm-error">{{ loginError }}</div>

        <button
          class="rm-submit"
          @click="handleLogin"
          :disabled="loginLoading || !serverUrl || !password"
        >
          {{ loginLoading ? "连接中..." : "连接" }}
        </button>
      </div>
    </div>
  </div>

  <!-- Connected -->
  <template v-else>
    <!-- Connection status bar -->
    <div style="display:flex;align-items:center;gap:8px;padding:4px 16px;background:var(--bg-secondary);border-bottom:1px solid var(--border);font-size:12px;">
      <span style="width:8px;height:8px;border-radius:50%;background:#4ade80;"></span>
      <span style="color:var(--text-secondary);">{{ serverUrl }}</span>
      <button class="btn btn-secondary" @click="disconnect" style="margin-left:auto;font-size:11px;padding:2px 8px;">断开</button>
    </div>

    <!-- L1 Tabs: projects -->
    <div class="tab-bar">
      <div v-for="project in projects" :key="project.id"
        class="tab" :class="{ active: activeProjectId === project.id }"
        @click="activeProjectId = project.id">
        <span>{{ project.project_name }}</span>
        <button v-if="projects.length > 1" class="close-btn" @click.stop="removeProject(project.id)">&times;</button>
      </div>
      <button class="add-tab" @click="addProject" title="添加项目">+</button>
    </div>

    <!-- L2 Tabs: project versions -->
    <div v-if="activeProject" class="tab-bar tab-bar-l2">
      <div
        v-for="pv in (activeProject.project_versions ?? [])"
        :key="pv.name"
        class="tab tab-l2"
        :class="{ active: activeProjectVersionName === pv.name }"
        @click="activeProjectVersionName = pv.name"
      >
        <span>{{ pv.name }}</span>
        <button class="close-btn" @click.stop="removeProjectVersion(pv.name)">&times;</button>
      </div>
      <template v-if="!showCreateProjectVersion">
        <button class="add-tab" @click="showCreateProjectVersion = true" title="添加项目版本">+</button>
      </template>
      <template v-else>
        <input
          class="pv-name-input"
          v-model="newProjectVersionName"
          placeholder="v1, v2, prod..."
          @keyup.enter="createProjectVersion"
          @keyup.escape="showCreateProjectVersion = false; newProjectVersionName = ''"
        />
        <button class="add-tab" @click="createProjectVersion">✓</button>
        <button class="add-tab" @click="showCreateProjectVersion = false; newProjectVersionName = ''">✕</button>
      </template>
    </div>

    <!-- Main Content -->
    <div class="main-content" v-if="activeProject && activeProjectVersion">
      <div class="project-panel">

        <!-- Foldable project settings -->
        <div class="rm-foldable">
          <button
            type="button"
            class="rm-fold-head"
            @click.stop="projectSettingsExpanded = !projectSettingsExpanded"
          >
            <span class="rm-fold-arrow" :class="{ open: projectSettingsExpanded }">▶</span>
            <span class="rm-fold-title">项目设置</span>
            <span class="rm-fold-meta">{{ activeProject.project_name }} · {{ activeProject.platforms.join(', ') }}</span>
          </button>
          <div v-if="projectSettingsExpanded" class="rm-fold-body">
            <div class="rm-inline-row">
              <label class="rm-inline-label">项目名</label>
              <input v-model="activeProject.project_name" class="rm-inline-input" />
              <label class="rm-inline-label">包名</label>
              <input v-model="activeProject.package_name" class="rm-inline-input" />
            </div>
            <div class="rm-inline-row">
              <label class="rm-inline-label">平台</label>
              <div class="platform-tags" style="flex:1">
                <span v-for="p in AVAILABLE_PLATFORMS" :key="p" class="platform-tag"
                  :class="{ selected: activeProject.platforms.includes(p) }"
                  @click="togglePlatform(p)">{{ p }}</span>
              </div>
            </div>
          </div>
        </div>

        <!-- Platform access toggle row -->
        <div class="rm-access-row">
          <span class="rm-access-label">平台访问</span>
          <span
            v-for="p in activeProject.platforms"
            :key="p"
            class="rm-access-chip"
            :class="{ on: activeProjectVersion.platform_settings[p]?.access_enabled }"
            @click="togglePlatformAccess(p)"
            :title="`${p} 访问 ${activeProjectVersion.platform_settings[p]?.access_enabled ? '已开启' : '已关闭'}`"
          >
            <span class="rm-access-dot"></span>
            {{ p }}
          </span>
        </div>

        <!-- Bundles dir (compact inline) -->
        <div class="rm-inline-row">
          <label class="rm-inline-label">Bundles</label>
          <input :value="currentBundlesDir" readonly placeholder="选择本地 Bundles 目录..." class="rm-inline-input" style="flex:1;cursor:pointer" @click="selectBundlesDir" />
          <button class="btn btn-secondary rm-inline-btn" @click="selectBundlesDir">浏览</button>
        </div>

        <!-- Sync controls (compact inline) -->
        <div class="rm-inline-row">
          <label class="rm-inline-label">同步</label>
          <select v-model="selectedPlatform" @change="loadVersions" class="rm-inline-select">
            <option v-for="p in activeProject.platforms" :key="p" :value="p">{{ p }}</option>
          </select>
          <button class="btn btn-primary rm-inline-btn" @click="startSync" :disabled="uploading || !currentBundlesDir">
            {{ uploading ? "上传中..." : "▶ 同步资源" }}
          </button>
          <span
            v-if="activeProjectVersion.platform_settings[selectedPlatform]?.active_bundle"
            class="rm-active-pill"
          >
            激活 <strong>{{ activeProjectVersion.platform_settings[selectedPlatform]?.active_bundle }}</strong>
          </span>
        </div>

        <!-- Versions -->
        <div class="rm-versions-section">
          <div class="rm-section-label">所有 Bundle 版本</div>
          <div v-if="versions.length > 0" class="rm-versions-list">
            <div v-for="entry in versions" :key="entry.version" class="rm-version-block">
              <div
                class="rm-version-row"
                :class="{ current: activeProjectVersion.platform_settings[selectedPlatform]?.active_bundle === entry.version }"
              >
                <div class="rm-version-info">
                  <div class="rm-version-name">
                    {{ entry.version }}
                    <span
                      v-if="activeProjectVersion.platform_settings[selectedPlatform]?.active_bundle === entry.version"
                      class="rm-active-badge"
                    >当前</span>
                  </div>
                  <div class="rm-version-meta">
                    {{ entry.file_count }} 个文件 · {{ formatSize(entry.total_size) }} · {{ formatTime(entry.modified_timestamp) }}
                  </div>
                </div>
                <button class="btn btn-secondary" style="font-size:11px;padding:2px 10px;" @click="openFileBrowser(entry.version)">浏览文件</button>
                <button class="btn btn-primary" style="font-size:11px;padding:2px 10px;" @click="activateVersion(entry.version)">激活</button>
                <button class="btn btn-danger" style="font-size:11px;padding:2px 10px;" @click="deleteVersion(entry.version)">删除</button>
              </div>
            </div>
          </div>
          <div v-else style="color:var(--text-muted);font-size:13px;padding:12px 0;">该项目版本下暂无 bundle，请上传资源</div>
        </div>
      </div>
    </div>

    <!-- Empty: no project versions yet -->
    <div v-else-if="activeProject" class="empty-state">
      <div class="icon">🏷️</div>
      <p>该项目还没有项目版本，点击上方 + 创建</p>
    </div>

    <!-- Sync Version Dialog -->
    <div v-if="syncDialog.show" class="rm-dialog-mask" @click.self="syncDialog.show = false">
      <div class="rm-dialog">
        <h3>选择要同步到远程服务器的版本</h3>
        <div v-if="syncDialog.loading" class="rm-dialog-loading">加载中...</div>
        <div v-else-if="syncDialog.error" class="rm-dialog-error">{{ syncDialog.error }}</div>
        <div v-else class="rm-dialog-versions">
          <div
            v-for="v in syncDialog.versions"
            :key="v.version"
            class="rm-dialog-version"
            :class="{ active: syncDialog.selectedVersion === v.version }"
            @click="selectSyncVersion(v.version)"
          >
            <input type="radio" :value="v.version" :checked="syncDialog.selectedVersion === v.version" @change="selectSyncVersion(v.version)" />
            <div class="rm-dialog-version-info">
              <div class="rm-dialog-version-name">{{ v.version }}</div>
              <div class="rm-dialog-version-meta">
                {{ v.file_count }} 个文件 · {{ formatSize(v.total_size) }} · {{ formatTime(v.modified_timestamp) }}
              </div>
            </div>
          </div>
        </div>

        <!-- Incremental upload bar -->
        <div v-if="!syncDialog.loading && !syncDialog.error && syncDialog.versions.length > 0" class="rm-incremental-bar">
          <label class="rm-incremental-toggle" :class="{ disabled: !canIncremental }" :title="!canIncremental ? '服务器还没有可对比的历史版本' : ''">
            <input type="checkbox" v-model="incrementalEnabled" :disabled="!canIncremental" />
            <span class="rm-toggle-label">增量上传</span>
          </label>

          <span v-if="!canIncremental" class="rm-hint-muted">服务器无可对比版本</span>
          <template v-else-if="incrementalEnabled">
            <span v-if="diffLoading" class="rm-hint-muted">
              <span class="rm-spinner"></span>
              计算差异中…
            </span>
            <span v-else-if="diffError" class="rm-hint-error">{{ diffError }}</span>
            <span v-else-if="diff" class="rm-savings-pill">
              ↓ 节省 {{ formatSize(savedBytes) }} ({{ savedPercent }}%)
            </span>
          </template>

          <button
            v-if="diff && !diffLoading"
            class="rm-link-btn"
            @click="diffDialogOpen = true"
          >显示更多 →</button>
        </div>

        <div class="rm-dialog-actions">
          <button class="btn btn-secondary" @click="syncDialog.show = false">取消</button>
          <button
            class="btn btn-primary"
            @click="confirmSync"
            :disabled="!syncDialog.selectedVersion || syncDialog.loading || (incrementalEnabled && diffLoading)"
          >{{ incrementalEnabled && canIncremental && diff ? '增量上传并激活' : '上传并激活' }}</button>
        </div>
      </div>
    </div>

    <!-- Diff Details Dialog (popup) -->
    <div v-if="diffDialogOpen && diff" class="rm-dialog-mask" @click.self="diffDialogOpen = false" style="z-index:1100">
      <div class="rm-dialog rm-diff-dialog">
        <div class="rm-dialog-head">
          <h3>
            上传差异
            <span class="rm-active-version" style="margin-left:8px">{{ syncDialog.selectedVersion }}</span>
            <span class="rm-diff-base-pill" style="margin-left:6px">基于 {{ diff.baseVersion }}</span>
          </h3>
          <button class="rm-mini-btn" @click="diffDialogOpen = false">关闭</button>
        </div>

        <!-- Comparison cards + savings -->
        <div class="rm-diff-summary">
          <div class="rm-diff-compare">
            <div class="rm-diff-card">
              <div class="rm-diff-card-label">全量上传</div>
              <div class="rm-diff-card-value">{{ formatSize(diff.totalLocalSize) }}</div>
              <div class="rm-diff-card-meta">{{ diff.totalLocalCount }} 个文件</div>
            </div>
            <div class="rm-diff-arrow">→</div>
            <div class="rm-diff-card rm-diff-card-incremental">
              <div class="rm-diff-card-label">增量上传</div>
              <div class="rm-diff-card-value">{{ formatSize(diff.uploadSize) }}</div>
              <div class="rm-diff-card-meta">{{ diff.uploadCount }} 个文件</div>
            </div>
          </div>
          <div class="rm-savings-banner">
            <span class="rm-savings-pill rm-savings-pill-large">↓ 节省 {{ formatSize(savedBytes) }} ({{ savedPercent }}%)</span>
          </div>
        </div>

        <!-- Category counts -->
        <div class="rm-diff-cats">
          <span class="rm-diff-cat rm-cat-modified">
            <span class="rm-cat-icon">✏️</span> 已修改 <strong>{{ diff.modified.length }}</strong>
          </span>
          <span class="rm-diff-cat rm-cat-added">
            <span class="rm-cat-icon">➕</span> 新增 <strong>{{ diff.added.length }}</strong>
          </span>
          <span class="rm-diff-cat rm-cat-unchanged">
            <span class="rm-cat-icon">✓</span> 未变化 <strong>{{ diff.unchanged.length }}</strong>
          </span>
          <span v-if="diff.removed.length > 0" class="rm-diff-cat rm-cat-removed">
            <span class="rm-cat-icon">🗑️</span> 已移除 <strong>{{ diff.removed.length }}</strong>
          </span>
        </div>

        <!-- File groups (modified > added > unchanged > removed) -->
        <div class="rm-diff-groups">
          <div v-if="diff.modified.length > 0" class="rm-diff-group">
            <div class="rm-diff-group-head" @click="diffGroups.modified = !diffGroups.modified">
              <span class="rm-diff-arrow-tiny">{{ diffGroups.modified ? '▼' : '▶' }}</span>
              <span class="rm-cat-icon">✏️</span>
              <span class="rm-diff-group-title">已修改</span>
              <span class="rm-diff-group-count">{{ diff.modified.length }}</span>
            </div>
            <div v-if="diffGroups.modified" class="rm-diff-files">
              <div v-for="f in diff.modified" :key="'m-'+f.name" class="rm-diff-file" :title="f.name">
                <span class="rm-diff-file-dot rm-dot-modified"></span>
                <span class="rm-diff-file-name">{{ f.name }}</span>
                <span class="rm-diff-file-size">{{ formatSize(f.size) }}</span>
              </div>
            </div>
          </div>

          <div v-if="diff.added.length > 0" class="rm-diff-group">
            <div class="rm-diff-group-head" @click="diffGroups.added = !diffGroups.added">
              <span class="rm-diff-arrow-tiny">{{ diffGroups.added ? '▼' : '▶' }}</span>
              <span class="rm-cat-icon">➕</span>
              <span class="rm-diff-group-title">新增</span>
              <span class="rm-diff-group-count">{{ diff.added.length }}</span>
            </div>
            <div v-if="diffGroups.added" class="rm-diff-files">
              <div v-for="f in diff.added" :key="'a-'+f.name" class="rm-diff-file" :title="f.name">
                <span class="rm-diff-file-dot rm-dot-added"></span>
                <span class="rm-diff-file-name">{{ f.name }}</span>
                <span class="rm-diff-file-size">{{ formatSize(f.size) }}</span>
              </div>
            </div>
          </div>

          <div v-if="diff.unchanged.length > 0" class="rm-diff-group">
            <div class="rm-diff-group-head" @click="diffGroups.unchanged = !diffGroups.unchanged">
              <span class="rm-diff-arrow-tiny">{{ diffGroups.unchanged ? '▼' : '▶' }}</span>
              <span class="rm-cat-icon">✓</span>
              <span class="rm-diff-group-title">未变化（服务器复用）</span>
              <span class="rm-diff-group-count">{{ diff.unchanged.length }}</span>
            </div>
            <div v-if="diffGroups.unchanged" class="rm-diff-files">
              <div v-for="f in diff.unchanged" :key="'u-'+f.name" class="rm-diff-file rm-diff-file-muted" :title="f.name">
                <span class="rm-diff-file-dot rm-dot-unchanged"></span>
                <span class="rm-diff-file-name">{{ f.name }}</span>
                <span class="rm-diff-file-size">{{ formatSize(f.size) }}</span>
              </div>
            </div>
          </div>

          <div v-if="diff.removed.length > 0" class="rm-diff-group">
            <div class="rm-diff-group-head" @click="diffGroups.removed = !diffGroups.removed">
              <span class="rm-diff-arrow-tiny">{{ diffGroups.removed ? '▼' : '▶' }}</span>
              <span class="rm-cat-icon">🗑️</span>
              <span class="rm-diff-group-title">服务器旧版有但本地无</span>
              <span class="rm-diff-group-count">{{ diff.removed.length }}</span>
            </div>
            <div v-if="diffGroups.removed" class="rm-diff-files">
              <div v-for="f in diff.removed" :key="'r-'+f.name" class="rm-diff-file rm-diff-file-muted" :title="f.name">
                <span class="rm-diff-file-dot rm-dot-removed"></span>
                <span class="rm-diff-file-name">{{ f.name }}</span>
                <span class="rm-diff-file-size">{{ formatSize(f.size) }}</span>
              </div>
            </div>
          </div>
        </div>

        <div class="rm-dialog-actions">
          <button class="btn btn-primary" @click="diffDialogOpen = false">确定</button>
        </div>
      </div>
    </div>

    <!-- File Browser Dialog -->
    <div v-if="fileBrowser.show" class="rm-dialog-mask" @click.self="fileBrowser.show = false">
      <div class="rm-dialog rm-dialog-wide">
        <div class="rm-dialog-head">
          <h3>
            浏览文件
            <span class="rm-active-version" style="margin-left:8px">{{ fileBrowser.version }}</span>
            <span v-if="fileBrowser.isActive" class="rm-active-badge" style="margin-left:6px">当前激活</span>
          </h3>
          <button class="rm-mini-btn" @click="fileBrowser.show = false">关闭</button>
        </div>

        <div v-if="!fileBrowser.isActive" class="rm-dialog-hint">
          ⓘ 该版本未激活，URL 暂不可访问。激活后才能通过 /res/ 路径下载。
        </div>

        <div v-if="fileBrowser.loading" class="rm-dialog-loading">加载中...</div>
        <div v-else-if="fileBrowser.error" class="rm-dialog-error">{{ fileBrowser.error }}</div>
        <div v-else-if="fileBrowser.files.length === 0" class="rm-dialog-loading">空目录</div>
        <div v-else class="rm-dialog-toolbar">
          <span class="rm-dialog-count">{{ fileBrowser.files.length }} 个文件</span>
          <button
            v-if="fileBrowser.isActive"
            class="rm-mini-btn"
            @click="copyAllBrowserUrls"
          >复制全部 URL</button>
        </div>
        <div v-if="!fileBrowser.loading && !fileBrowser.error && fileBrowser.files.length > 0" class="rm-file-list rm-file-list-dialog">
          <div v-for="f in fileBrowser.files" :key="f.name" class="rm-file-row-v2">
            <span class="rm-file-name-v2" :title="f.name">{{ f.name }}</span>
            <span class="rm-file-size-v2">{{ formatSize(f.size) }}</span>
            <div class="rm-file-actions">
              <button
                v-if="fileBrowser.isActive && isViewable(f.name)"
                class="rm-mini-btn"
                @click="viewFile(f.name)"
                title="在浏览器中打开预览"
              >显示内容</button>
              <button
                v-if="fileBrowser.isActive"
                class="rm-mini-btn"
                @click="copyToClipboard(buildResourceUrl(f.name))"
              >
                {{ copiedUrl === buildResourceUrl(f.name) ? "✓ 已复制" : "复制 URL" }}
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Log Panel -->
    <div v-if="logPanelOpen" class="resize-handle" @mousedown="onResizeStart"></div>
    <div class="log-panel" :class="{ collapsed: !logPanelOpen }"
      :style="{ height: logPanelOpen ? logPanelHeight + 'px' : '36px' }">
      <div class="log-header" @click="logPanelOpen = !logPanelOpen">
        <h3>
          <span class="toggle-icon" :class="{ expanded: logPanelOpen }">&#9650;</span>
          日志 <span class="log-count">{{ filteredLogs.length }}</span>
        </h3>
        <div class="log-actions" @click.stop>
          <button @click="clearLogs">清空</button>
        </div>
      </div>
      <div v-if="logPanelOpen" class="log-body">
        <div v-if="filteredLogs.length === 0" class="empty-state" style="height:100%">
          <p style="font-size:12px;color:var(--text-muted)">暂无日志</p>
        </div>
        <div v-for="(log, idx) in filteredLogs" :key="idx" class="log-entry">
          <span class="time">{{ log.timestamp }}</span>
          <span class="status" :class="getStatusClass(log.status)">
            {{ log.type === "request" ? log.status : log.type?.toUpperCase() }}
          </span>
          <span class="path">{{ log.message || log.path }}</span>
        </div>
      </div>
    </div>
  </template>
</template>

<style scoped>
.rm-login-wrap {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: radial-gradient(circle at 30% 20%, rgba(34, 211, 238, 0.06), transparent 50%),
              radial-gradient(circle at 70% 80%, rgba(74, 222, 128, 0.05), transparent 50%);
}

.rm-login-shell {
  display: flex;
  width: 100%;
  max-width: 820px;
  height: 480px;
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 16px;
  overflow: hidden;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.3);
}

.rm-sidebar {
  width: 240px;
  background: var(--bg-primary);
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
}

.rm-sidebar-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px 16px 12px;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.rm-icon-btn {
  width: 24px;
  height: 24px;
  border-radius: 6px;
  border: none;
  background: var(--accent);
  color: var(--bg-primary);
  font-size: 18px;
  font-weight: 700;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  line-height: 1;
  transition: transform 0.15s, box-shadow 0.15s;
}
.rm-icon-btn:hover {
  transform: scale(1.08);
  box-shadow: 0 0 12px rgba(34, 211, 238, 0.4);
}

.rm-conn-list {
  flex: 1;
  overflow-y: auto;
  padding: 4px 8px 12px;
}

.rm-conn-empty {
  color: var(--text-muted);
  font-size: 12px;
  text-align: center;
  padding: 32px 16px;
  line-height: 1.6;
}

.rm-conn-item {
  position: relative;
  padding: 10px 12px;
  border-radius: 8px;
  cursor: pointer;
  margin-bottom: 4px;
  border: 1px solid transparent;
  transition: background 0.15s, border-color 0.15s;
  display: flex;
  align-items: center;
  gap: 8px;
}
.rm-conn-item:hover {
  background: var(--bg-tertiary);
}
.rm-conn-item.active {
  background: var(--bg-tertiary);
  border-color: var(--accent);
}

.rm-conn-info {
  flex: 1;
  min-width: 0;
}
.rm-conn-name {
  font-size: 13px;
  color: var(--text-primary);
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.rm-conn-url {
  font-size: 11px;
  color: var(--text-muted);
  margin-top: 2px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.rm-conn-delete {
  width: 22px;
  height: 22px;
  border-radius: 50%;
  border: none;
  background: transparent;
  color: var(--text-muted);
  font-size: 18px;
  line-height: 1;
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.15s, background 0.15s, color 0.15s;
  display: flex;
  align-items: center;
  justify-content: center;
}
.rm-conn-item:hover .rm-conn-delete {
  opacity: 1;
}
.rm-conn-delete:hover {
  background: rgba(255, 107, 107, 0.15);
  color: #ff6b6b;
}

.rm-form {
  flex: 1;
  padding: 32px 36px;
  display: flex;
  flex-direction: column;
  overflow-y: auto;
}

.rm-form-title {
  font-size: 18px;
  font-weight: 600;
  color: var(--accent);
  margin-bottom: 24px;
  letter-spacing: 0.5px;
}

.rm-field {
  margin-bottom: 16px;
}

.rm-field label {
  display: block;
  color: var(--text-secondary);
  font-size: 12px;
  margin-bottom: 6px;
  font-weight: 500;
}

.rm-input {
  width: 100%;
  height: 38px;
  padding: 0 12px;
  background: var(--bg-tertiary);
  border: 1px solid var(--border);
  border-radius: 8px;
  color: var(--text-primary);
  font-size: 13px;
  outline: none;
  transition: border-color 0.15s, box-shadow 0.15s, background 0.15s;
  font-family: inherit;
}
.rm-input::placeholder {
  color: var(--text-muted);
}
.rm-input:hover {
  background: var(--bg-secondary);
}
.rm-input:focus {
  border-color: var(--accent);
  background: var(--bg-secondary);
  box-shadow: 0 0 0 3px rgba(34, 211, 238, 0.15);
}

.rm-checkbox {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text-secondary);
  cursor: pointer;
  margin: 4px 0 16px;
  user-select: none;
}
.rm-checkbox input {
  width: 14px;
  height: 14px;
  cursor: pointer;
  accent-color: var(--accent);
}

.rm-error {
  color: #ff6b6b;
  font-size: 12px;
  background: rgba(255, 107, 107, 0.08);
  border: 1px solid rgba(255, 107, 107, 0.25);
  padding: 8px 12px;
  border-radius: 6px;
  margin-bottom: 12px;
}

.rm-submit {
  width: 100%;
  height: 42px;
  border: none;
  border-radius: 8px;
  background: var(--accent);
  color: var(--bg-primary);
  font-size: 14px;
  font-weight: 600;
  cursor: pointer;
  transition: transform 0.1s, box-shadow 0.15s, opacity 0.15s;
  margin-top: auto;
}
.rm-submit:hover:not(:disabled) {
  transform: translateY(-1px);
  box-shadow: 0 6px 16px rgba(34, 211, 238, 0.3);
}
.rm-submit:active:not(:disabled) {
  transform: translateY(0);
}
.rm-submit:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* Sync dialog */
.rm-dialog-mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.6);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
  backdrop-filter: blur(2px);
}
.rm-dialog {
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 24px;
  width: 480px;
  max-width: 90vw;
  max-height: 80vh;
  display: flex;
  flex-direction: column;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.4);
}
.rm-dialog h3 {
  color: var(--accent);
  font-size: 15px;
  font-weight: 600;
  margin: 0 0 16px;
}
.rm-dialog-loading,
.rm-dialog-error {
  padding: 24px;
  text-align: center;
  color: var(--text-secondary);
  font-size: 13px;
}
.rm-dialog-error {
  color: #ff6b6b;
}
.rm-dialog-versions {
  flex: 1;
  overflow-y: auto;
  margin-bottom: 16px;
  max-height: 360px;
}
.rm-dialog-version {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 12px;
  border-radius: 8px;
  cursor: pointer;
  margin-bottom: 4px;
  border: 1px solid transparent;
  transition: background 0.15s, border-color 0.15s;
}
.rm-dialog-version:hover {
  background: var(--bg-tertiary);
}
.rm-dialog-version.active {
  background: var(--bg-tertiary);
  border-color: var(--accent);
}
.rm-dialog-version input[type="radio"] {
  accent-color: var(--accent);
}
.rm-dialog-version-info {
  flex: 1;
}
.rm-dialog-version-name {
  font-size: 13px;
  color: var(--text-primary);
  font-weight: 500;
}
.rm-dialog-version-meta {
  font-size: 11px;
  color: var(--text-muted);
  margin-top: 2px;
}
.rm-dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

.rm-active-version {
  font-size: 12px;
  color: var(--accent);
  background: rgba(34, 211, 238, 0.1);
  padding: 2px 8px;
  border-radius: 4px;
  font-weight: 500;
}
/* === File list === */
.rm-file-list {
  max-height: 240px;
  overflow-y: auto;
}
.rm-file-list-nested {
  max-height: 200px;
  background: rgba(0, 0, 0, 0.15);
}
.rm-file-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 14px;
  font-size: 12px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.03);
  font-family: ui-monospace, "SF Mono", Menlo, monospace;
}
.rm-file-row:last-child { border-bottom: none; }
.rm-file-row:hover { background: rgba(255, 255, 255, 0.02); }
.rm-file-name {
  flex: 1;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.rm-file-size {
  color: var(--text-secondary);
  font-size: 11px;
  width: 70px;
  text-align: right;
  flex-shrink: 0;
}
.rm-file-time {
  color: var(--text-muted);
  font-size: 11px;
  width: 130px;
  text-align: right;
  flex-shrink: 0;
}

/* === Mini button === */
.rm-mini-btn {
  height: 24px;
  padding: 0 10px;
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 4px;
  color: var(--text-secondary);
  font-size: 11px;
  cursor: pointer;
  transition: all 0.15s;
  white-space: nowrap;
}
.rm-mini-btn:hover:not(:disabled) {
  border-color: var(--accent);
  color: var(--accent);
}
.rm-mini-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* === Versions section === */
.rm-versions-section {
  margin-top: 4px;
}
.rm-section-label {
  font-size: 11px;
  color: var(--text-muted);
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.5px;
  margin: 12px 0 8px;
  padding: 0 2px;
}
.rm-versions-list {
  max-height: 280px;
  overflow-y: auto;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--bg-tertiary);
}
.rm-version-block {
  border-bottom: 1px solid rgba(255, 255, 255, 0.04);
}
.rm-version-block:last-child {
  border-bottom: none;
}
.rm-version-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 12px;
  transition: background 0.15s;
}
.rm-version-row:hover {
  background: rgba(255, 255, 255, 0.02);
}
.rm-version-row.current {
  background: rgba(34, 211, 238, 0.04);
}
.rm-version-info {
  flex: 1;
  min-width: 0;
}
.rm-version-name {
  font-size: 13px;
  color: var(--text-primary);
  font-weight: 500;
  display: flex;
  align-items: center;
  gap: 8px;
}
.rm-version-meta {
  font-size: 11px;
  color: var(--text-muted);
  margin-top: 2px;
}
.rm-active-badge {
  font-size: 10px;
  padding: 1px 6px;
  background: var(--accent);
  color: var(--bg-primary);
  border-radius: 3px;
  font-weight: 600;
  letter-spacing: 0.3px;
}
/* === File browser dialog === */
.rm-dialog-wide {
  width: 720px;
}
.rm-dialog-head {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 16px;
}
.rm-dialog-head h3 {
  margin: 0;
  flex: 1;
  display: flex;
  align-items: center;
}
.rm-dialog-hint {
  background: rgba(251, 191, 36, 0.08);
  border: 1px solid rgba(251, 191, 36, 0.25);
  color: #fbbf24;
  padding: 8px 12px;
  border-radius: 6px;
  font-size: 12px;
  margin-bottom: 12px;
}
.rm-dialog-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 8px;
  padding: 0 2px;
}
.rm-dialog-count {
  font-size: 12px;
  color: var(--text-muted);
}
.rm-file-list-dialog {
  max-height: 55vh;
  overflow-y: auto;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--bg-tertiary);
}

/* === Improved file row (full name visible) === */
.rm-file-row-v2 {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 12px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.04);
  font-size: 12px;
  font-family: ui-monospace, "SF Mono", Menlo, monospace;
}
.rm-file-row-v2:last-child {
  border-bottom: none;
}
.rm-file-row-v2:hover {
  background: rgba(255, 255, 255, 0.03);
}
.rm-file-name-v2 {
  flex: 1;
  min-width: 0;
  color: var(--text-primary);
  word-break: break-all;
  line-height: 1.5;
}
.rm-file-size-v2 {
  width: 70px;
  flex-shrink: 0;
  text-align: right;
  color: var(--text-secondary);
  font-size: 11px;
}
.rm-file-actions {
  display: flex;
  gap: 6px;
  flex-shrink: 0;
}

/* === Incremental upload === */
.rm-diff-dialog {
  width: 760px;
  max-width: 92vw;
  max-height: 88vh;
}

.rm-diff-dialog .rm-dialog-head {
  margin-bottom: 14px;
}

.rm-diff-base-pill {
  font-size: 11px;
  color: var(--text-muted);
  background: var(--bg-tertiary);
  border: 1px solid var(--border);
  padding: 2px 8px;
  border-radius: 999px;
  font-weight: normal;
}

.rm-diff-summary {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 12px;
  background: var(--bg-tertiary);
  border: 1px solid var(--border);
  border-radius: 10px;
  margin-bottom: 12px;
}
.rm-diff-summary .rm-diff-compare {
  flex: 1;
}
.rm-savings-banner {
  flex-shrink: 0;
}
.rm-savings-pill-large {
  font-size: 13px;
  padding: 6px 14px;
}

.rm-incremental-bar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 12px;
  margin: 4px 0 12px;
  background: var(--bg-tertiary);
  border: 1px solid var(--border);
  border-radius: 8px;
  font-size: 12px;
  flex-wrap: wrap;
}

.rm-incremental-toggle {
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
  user-select: none;
  color: var(--text-primary);
  font-weight: 500;
}
.rm-incremental-toggle input {
  width: 14px;
  height: 14px;
  cursor: pointer;
  accent-color: var(--accent);
}
.rm-incremental-toggle.disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.rm-incremental-toggle.disabled input {
  cursor: not-allowed;
}

.rm-toggle-label {
  font-size: 12px;
}

.rm-hint-muted {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--text-muted);
  font-size: 11px;
}

.rm-hint-error {
  color: #ff6b6b;
  font-size: 11px;
}

.rm-savings-pill {
  display: inline-flex;
  align-items: center;
  padding: 2px 10px;
  background: rgba(74, 222, 128, 0.12);
  border: 1px solid rgba(74, 222, 128, 0.35);
  color: #4ade80;
  border-radius: 999px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.2px;
}

.rm-link-btn {
  margin-left: auto;
  background: transparent;
  border: none;
  color: var(--accent);
  font-size: 12px;
  cursor: pointer;
  padding: 2px 4px;
  text-decoration: underline;
  text-underline-offset: 2px;
  font-family: inherit;
}
.rm-link-btn:hover {
  opacity: 0.8;
}

.rm-spinner {
  display: inline-block;
  width: 10px;
  height: 10px;
  border: 1.5px solid rgba(34, 211, 238, 0.25);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: rm-spin 0.7s linear infinite;
}
@keyframes rm-spin {
  to { transform: rotate(360deg); }
}

/* Diff panel */
.rm-diff-panel {
  margin: 0 0 12px;
  padding: 14px;
  background: var(--bg-tertiary);
  border: 1px solid var(--border);
  border-radius: 10px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  max-height: 380px;
  overflow-y: auto;
  animation: rm-fade-in 0.18s ease-out;
}
@keyframes rm-fade-in {
  from { opacity: 0; transform: translateY(-4px); }
  to { opacity: 1; transform: translateY(0); }
}

.rm-diff-header {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 11px;
  color: var(--text-muted);
}
.rm-diff-base strong {
  color: var(--text-primary);
  font-weight: 600;
}

.rm-diff-compare {
  display: flex;
  align-items: stretch;
  gap: 8px;
}
.rm-diff-card {
  flex: 1;
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 10px 12px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.rm-diff-card-incremental {
  border-color: rgba(74, 222, 128, 0.35);
  background: rgba(74, 222, 128, 0.05);
}
.rm-diff-card-label {
  font-size: 10px;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.5px;
  font-weight: 600;
}
.rm-diff-card-value {
  font-size: 18px;
  color: var(--text-primary);
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}
.rm-diff-card-incremental .rm-diff-card-value {
  color: #4ade80;
}
.rm-diff-card-meta {
  font-size: 11px;
  color: var(--text-muted);
}
.rm-diff-arrow {
  display: flex;
  align-items: center;
  color: var(--text-muted);
  font-size: 16px;
  font-weight: 600;
  padding: 0 2px;
}

.rm-diff-cats {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.rm-diff-cat {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 3px 10px;
  border-radius: 999px;
  font-size: 11px;
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  color: var(--text-secondary);
}
.rm-diff-cat strong {
  color: var(--text-primary);
  font-weight: 600;
  margin-left: 2px;
}
.rm-cat-icon {
  font-size: 11px;
  line-height: 1;
}
.rm-cat-modified {
  border-color: rgba(251, 191, 36, 0.35);
  background: rgba(251, 191, 36, 0.08);
  color: #fbbf24;
}
.rm-cat-modified strong { color: #fbbf24; }
.rm-cat-added {
  border-color: rgba(74, 222, 128, 0.35);
  background: rgba(74, 222, 128, 0.08);
  color: #4ade80;
}
.rm-cat-added strong { color: #4ade80; }
.rm-cat-unchanged {
  border-color: var(--border);
  color: var(--text-muted);
}
.rm-cat-unchanged strong { color: var(--text-secondary); }
.rm-cat-removed {
  border-color: rgba(255, 107, 107, 0.35);
  background: rgba(255, 107, 107, 0.06);
  color: #ff6b6b;
}
.rm-cat-removed strong { color: #ff6b6b; }

/* File groups */
.rm-diff-groups {
  display: flex;
  flex-direction: column;
  gap: 4px;
  border-top: 1px solid var(--border);
  padding-top: 10px;
}
.rm-diff-group {
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 6px;
  overflow: hidden;
}
.rm-diff-group-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  cursor: pointer;
  font-size: 12px;
  color: var(--text-primary);
  user-select: none;
  transition: background 0.12s;
}
.rm-diff-group-head:hover {
  background: rgba(255, 255, 255, 0.02);
}
.rm-diff-arrow-tiny {
  width: 10px;
  font-size: 9px;
  color: var(--text-muted);
}
.rm-diff-group-title {
  flex: 1;
  font-weight: 500;
}
.rm-diff-group-count {
  font-size: 11px;
  color: var(--text-muted);
  background: var(--bg-tertiary);
  padding: 1px 8px;
  border-radius: 999px;
  font-variant-numeric: tabular-nums;
}

.rm-diff-files {
  border-top: 1px solid var(--border);
  max-height: 180px;
  overflow-y: auto;
  font-family: ui-monospace, "SF Mono", Menlo, monospace;
}
.rm-diff-file {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 10px 5px 26px;
  font-size: 11.5px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.03);
  color: var(--text-primary);
}
.rm-diff-file:last-child {
  border-bottom: none;
}
.rm-diff-file:hover {
  background: rgba(255, 255, 255, 0.02);
}
.rm-diff-file-muted {
  color: var(--text-muted);
}
.rm-diff-file-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  flex-shrink: 0;
}
.rm-dot-modified { background: #fbbf24; }
.rm-dot-added { background: #4ade80; }
.rm-dot-unchanged { background: rgba(255, 255, 255, 0.2); }
.rm-dot-removed { background: #ff6b6b; }

.rm-diff-file-name {
  flex: 1;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.rm-diff-file-size {
  flex-shrink: 0;
  color: var(--text-muted);
  font-size: 10.5px;
  font-variant-numeric: tabular-nums;
}

.tab-bar-l2 {
  background: var(--bg-primary);
  padding: 2px 12px;
}
.tab.tab-l2 {
  font-size: 12px;
  padding: 4px 12px;
  height: 28px;
}
.pv-name-input {
  height: 24px;
  padding: 0 8px;
  background: var(--bg-tertiary);
  border: 1px solid var(--border);
  border-radius: 4px;
  color: var(--text-primary);
  font-size: 12px;
  width: 140px;
  margin: 0 4px;
  outline: none;
}
.pv-name-input:focus { border-color: var(--accent); }

.rm-foldable {
  border: 1px solid var(--border);
  border-radius: 8px;
  margin: 8px 0;
  overflow: hidden;
}
.rm-fold-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 12px;
  cursor: pointer;
  background: var(--bg-tertiary);
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
  width: 100%;
  border: none;
  text-align: left;
  font-family: inherit;
  outline: none;
}
.rm-fold-head:hover { background: var(--bg-secondary); }
.rm-fold-head > * { pointer-events: none; }
.rm-fold-title { flex-shrink: 0; }
.rm-fold-arrow {
  display: inline-block;
  font-size: 9px;
  transition: transform 0.15s;
  color: var(--text-muted);
}
.rm-fold-arrow.open { transform: rotate(90deg); }
.rm-fold-meta {
  margin-left: auto;
  font-weight: normal;
  color: var(--text-muted);
  font-size: 11px;
}
.rm-fold-body { padding: 8px 12px; }

/* Compact inline rows */
.rm-inline-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 6px 0;
  flex-wrap: nowrap;
}
.rm-inline-label {
  font-size: 11px;
  color: var(--text-muted);
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.4px;
  flex-shrink: 0;
  min-width: 50px;
}
.rm-inline-input {
  flex: 1;
  min-width: 0;
  height: 28px;
  padding: 0 10px;
  background: var(--bg-tertiary);
  border: 1px solid var(--border);
  border-radius: 4px;
  color: var(--text-primary);
  font-size: 12px;
  outline: none;
}
.rm-inline-input:focus { border-color: var(--accent); }
.rm-inline-select {
  height: 28px;
  padding: 0 10px;
  background: var(--bg-tertiary);
  border: 1px solid var(--border);
  border-radius: 4px;
  color: var(--text-primary);
  font-size: 12px;
  flex-shrink: 0;
  min-width: 100px;
}
.rm-inline-btn {
  height: 28px;
  padding: 0 14px !important;
  font-size: 12px !important;
  flex-shrink: 0;
}
.rm-active-pill {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 28px;
  padding: 0 10px;
  background: rgba(34, 211, 238, 0.08);
  border: 1px solid rgba(34, 211, 238, 0.3);
  border-radius: 4px;
  color: var(--text-secondary);
  font-size: 11px;
  flex-shrink: 0;
  margin-left: auto;
}
.rm-active-pill strong {
  color: var(--accent);
  font-weight: 600;
  font-size: 12px;
}

.rm-access-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 10px;
  background: var(--bg-tertiary);
  border: 1px solid var(--border);
  border-radius: 6px;
  margin: 6px 0;
  flex-wrap: wrap;
}
.rm-access-label {
  font-size: 11px;
  color: var(--text-muted);
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}
.rm-access-chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 3px 10px;
  border-radius: 999px;
  font-size: 11px;
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  color: var(--text-muted);
  cursor: pointer;
  user-select: none;
  transition: all 0.15s;
}
.rm-access-chip:hover { border-color: var(--accent); }
.rm-access-chip.on {
  background: rgba(74, 222, 128, 0.10);
  border-color: rgba(74, 222, 128, 0.45);
  color: #4ade80;
}
.rm-access-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--text-muted);
}
.rm-access-chip.on .rm-access-dot { background: #4ade80; }
</style>
