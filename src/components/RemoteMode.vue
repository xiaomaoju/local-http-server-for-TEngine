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
import { api, type ProjectConfig, type VersionEntry, type LogEntry, type FileEntry } from "../api/remote";

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

const AVAILABLE_PLATFORMS = ["Android", "iOS", "Windows", "MacOS", "Linux", "WebGL"];

const activeProject = computed(() =>
  projects.value.find((p) => p.id === activeProjectId.value)
);

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
    loginError.value = `连接失败: ${e.message}`;
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
    projects.value = await api.listProjects();
    if (projects.value.length > 0 && !activeProjectId.value) {
      activeProjectId.value = projects.value[0].id;
      await loadVersions();
    }
  } catch {}
}

async function addProject() {
  const name = `Project_${projects.value.length + 1}`;
  try {
    const project = await api.createProject(name);
    projects.value.push(project);
    activeProjectId.value = project.id;
  } catch {}
}

async function removeProject(id: string) {
  if (projects.value.length <= 1) return;
  try {
    await api.deleteProject(id);
    projects.value = projects.value.filter((p) => p.id !== id);
    if (activeProjectId.value === id) activeProjectId.value = projects.value[0]?.id || "";
  } catch {}
}

async function saveProject() {
  const project = activeProject.value;
  if (!project) return;
  try { await api.updateProject(project); } catch {}
}

let saveTimer: ReturnType<typeof setTimeout> | null = null;
watch(() => activeProject.value, () => {
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = setTimeout(() => saveProject(), 500);
}, { deep: true });

async function loadVersions() {
  const project = activeProject.value;
  if (!project) return;
  try {
    versions.value = await api.listVersions(project.id, selectedPlatform.value);
  } catch { versions.value = []; }
}

async function openFileBrowser(version: string) {
  const project = activeProject.value;
  if (!project) return;

  const isActive = project.active_versions[selectedPlatform.value] === version;
  fileBrowser.value = {
    show: true,
    version,
    isActive,
    loading: true,
    files: [],
    error: "",
  };

  try {
    // 激活版本传 undefined 走平台根目录；非激活版本传 version 走 _versions
    const files = await api.listFiles(
      project.id,
      selectedPlatform.value,
      isActive ? undefined : version,
    );
    fileBrowser.value.files = files;
  } catch (e: any) {
    fileBrowser.value.error = `加载失败: ${e?.message || e}`;
  } finally {
    fileBrowser.value.loading = false;
  }
}

function buildResourceUrl(fileName: string): string {
  const project = activeProject.value;
  if (!project) return "";
  return `${serverUrl.value.replace(/\/$/, "")}/res/${encodeURIComponent(project.project_name)}/${encodeURIComponent(selectedPlatform.value)}/${encodeURIComponent(fileName)}`;
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

  try {
    const list = await invoke<LocalVersionEntry[]>("list_local_bundle_versions", {
      bundlesDir: currentBundlesDir.value,
      packageName: project.package_name,
      platform: selectedPlatform.value,
    });
    syncDialog.value.versions = list;
    syncDialog.value.selectedVersion = list.length > 0 ? list[0].version : "";
    if (list.length === 0) {
      syncDialog.value.error = `在 ${selectedPlatform.value}/${project.package_name}/ 下未找到任何版本`;
    }
  } catch (e: any) {
    syncDialog.value.error = `读取版本失败: ${e}`;
  } finally {
    syncDialog.value.loading = false;
  }
}

async function confirmSync() {
  const project = activeProject.value;
  if (!project || !syncDialog.value.selectedVersion) return;

  const version = syncDialog.value.selectedVersion;
  syncDialog.value.show = false;
  uploading.value = true;

  try {
    const token = api.getToken();
    await invoke("upload_version_to_remote", {
      bundlesDir: currentBundlesDir.value,
      packageName: project.package_name,
      platform: selectedPlatform.value,
      version,
      projectId: project.id,
      serverUrl: serverUrl.value,
      token,
    });
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
  if (!project) return;
  try {
    await api.activateVersion(project.id, version, selectedPlatform.value);
    project.active_versions[selectedPlatform.value] = version;
  } catch {}
}

async function deleteVersion(version: string) {
  const project = activeProject.value;
  if (!project) return;
  try {
    await api.deleteVersion(project.id, version, selectedPlatform.value);
    await loadVersions();
  } catch {}
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

    <!-- Tab Bar -->
    <div class="tab-bar">
      <div v-for="project in projects" :key="project.id"
        class="tab" :class="{ active: activeProjectId === project.id }"
        @click="activeProjectId = project.id; loadVersions()">
        <span>{{ project.project_name }}</span>
        <button v-if="projects.length > 1" class="close-btn" @click.stop="removeProject(project.id)">&times;</button>
      </div>
      <button class="add-tab" @click="addProject" title="添加项目">+</button>
    </div>

    <!-- Main Content -->
    <div class="main-content" v-if="activeProject">
      <div class="project-panel">
        <div class="config-compact">
          <div class="config-row">
            <div class="config-field">
              <label>项目名称</label>
              <input v-model="activeProject.project_name" />
            </div>
            <div class="config-field">
              <label>包名</label>
              <input v-model="activeProject.package_name" />
            </div>
          </div>
          <div class="config-row">
            <div class="config-field config-platforms-field">
              <label>平台</label>
              <div class="platform-tags">
                <span v-for="p in AVAILABLE_PLATFORMS" :key="p" class="platform-tag"
                  :class="{ selected: activeProject.platforms.includes(p) }"
                  @click="togglePlatform(p)">{{ p }}</span>
              </div>
            </div>
          </div>
        </div>

        <!-- Bundles dir + Sync -->
        <div class="config-row">
          <div class="config-field" style="flex:1">
            <label>BUNDLES 目录</label>
            <div style="display:flex;gap:8px;">
              <input :value="currentBundlesDir" readonly placeholder="选择本地 Bundles 目录..." style="flex:1;cursor:pointer" @click="selectBundlesDir" />
              <button class="btn btn-secondary" @click="selectBundlesDir">浏览</button>
            </div>
          </div>
        </div>

        <div class="control-bar">
          <div class="config-field" style="width:140px">
            <label>同步平台</label>
            <select v-model="selectedPlatform" @change="loadVersions" style="width:100%;padding:4px 8px;background:var(--bg-tertiary);border:1px solid var(--border);color:var(--text-primary);border-radius:4px;">
              <option v-for="p in activeProject.platforms" :key="p" :value="p">{{ p }}</option>
            </select>
          </div>
          <div style="display:flex;align-items:flex-end;">
            <button class="btn btn-primary" @click="startSync" :disabled="uploading || !currentBundlesDir">
              {{ uploading ? "上传中..." : "▶ 同步资源" }}
            </button>
          </div>
          <div v-if="activeProject.active_versions[selectedPlatform]" class="server-url" style="margin-left:auto;">
            当前激活: <strong>{{ activeProject.active_versions[selectedPlatform] }}</strong>
          </div>
        </div>

        <!-- Versions -->
        <div class="rm-versions-section">
          <div class="rm-section-label">所有版本</div>
          <div v-if="versions.length > 0" class="rm-versions-list">
            <div v-for="entry in versions" :key="entry.version" class="rm-version-block">
              <div
                class="rm-version-row"
                :class="{ current: activeProject.active_versions[selectedPlatform] === entry.version }"
              >
                <div class="rm-version-info">
                  <div class="rm-version-name">
                    {{ entry.version }}
                    <span v-if="activeProject.active_versions[selectedPlatform] === entry.version" class="rm-active-badge">当前</span>
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
          <div v-else style="color:var(--text-muted);font-size:13px;padding:12px 0;">暂无版本，请上传资源</div>
        </div>
      </div>
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
            @click="syncDialog.selectedVersion = v.version"
          >
            <input type="radio" :value="v.version" v-model="syncDialog.selectedVersion" />
            <div class="rm-dialog-version-info">
              <div class="rm-dialog-version-name">{{ v.version }}</div>
              <div class="rm-dialog-version-meta">
                {{ v.file_count }} 个文件 · {{ formatSize(v.total_size) }} · {{ formatTime(v.modified_timestamp) }}
              </div>
            </div>
          </div>
        </div>
        <div class="rm-dialog-actions">
          <button class="btn btn-secondary" @click="syncDialog.show = false">取消</button>
          <button
            class="btn btn-primary"
            @click="confirmSync"
            :disabled="!syncDialog.selectedVersion || syncDialog.loading"
          >上传并激活</button>
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
</style>
