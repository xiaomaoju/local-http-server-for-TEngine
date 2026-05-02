<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { api, type ProjectConfig, type VersionEntry, type LogEntry, type FileEntry } from "../api/remote";
import LogPanel from "./LogPanel.vue";
import SettingsDialog from "./SettingsDialog.vue";
import HelpModal from "./HelpModal.vue";

const emit = defineEmits<{ (e: "logout"): void }>();

const APP_VERSION = "v1.0.1";
const VIEWABLE_EXTS = ["version", "hash", "report", "json", "txt", "log", "xml", "yaml", "yml", "csv"];

const projects = ref<ProjectConfig[]>([]);
const activeProjectId = ref("");
const versions = ref<VersionEntry[]>([]);
const logs = ref<LogEntry[]>([]);
const uploading = ref(false);
const selectedPlatform = ref("Android");
const uploadVersion = ref("");
let ws: WebSocket | null = null;

const showSettings = ref(false);
const showHelp = ref(false);
const copiedUrl = ref("");

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

const AVAILABLE_PLATFORMS = ["Android", "iOS", "Windows", "MacOS", "Linux", "WebGL"];

const activeProject = computed(() =>
  projects.value.find((p) => p.id === activeProjectId.value)
);

onMounted(async () => {
  await loadProjects();
  connectWebSocket();
});

onUnmounted(() => {
  ws?.close();
});

function connectWebSocket() {
  ws = api.connectLogs(
    (log) => {
      logs.value.push(log);
      if (logs.value.length > 2000) {
        logs.value = logs.value.slice(-1500);
      }
    },
    () => {
      setTimeout(connectWebSocket, 3000);
    }
  );
}

async function loadProjects() {
  try {
    projects.value = await api.listProjects();
    if (projects.value.length > 0 && !activeProjectId.value) {
      activeProjectId.value = projects.value[0].id;
      await loadVersions();
    }
  } catch (e: any) {
    if (e.message === "Unauthorized") emit("logout");
  }
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
  if (!confirm("确定删除该项目吗？项目下所有资源会一同删除。")) return;
  try {
    await api.deleteProject(id);
    projects.value = projects.value.filter((p) => p.id !== id);
    if (activeProjectId.value === id) {
      activeProjectId.value = projects.value[0]?.id || "";
    }
  } catch {}
}

async function saveProject() {
  const project = activeProject.value;
  if (!project) return;
  try {
    await api.updateProject(project);
  } catch {}
}

async function loadVersions() {
  const project = activeProject.value;
  if (!project) return;
  try {
    versions.value = await api.listVersions(project.id, selectedPlatform.value);
  } catch {
    versions.value = [];
  }
}

async function handleUpload(event: Event) {
  const input = event.target as HTMLInputElement;
  const files = Array.from(input.files || []);
  if (!files.length || !activeProject.value) return;

  uploading.value = true;
  try {
    await api.uploadResources(
      activeProject.value.id,
      selectedPlatform.value,
      uploadVersion.value,
      files,
    );
    uploadVersion.value = "";
    await loadVersions();
  } catch {} finally {
    uploading.value = false;
    input.value = "";
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
  if (!confirm(`确定删除版本 ${version} 吗？`)) return;
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

function logout() {
  api.logout();
  emit("logout");
}

const filteredLogs = computed(() => {
  if (!activeProjectId.value) return logs.value;
  return logs.value.filter((l) => l.project_id === activeProject.value?.project_name || l.project_id === activeProjectId.value);
});

// === File browser ===
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
  const origin = window.location.origin;
  return `${origin}/res/${encodeURIComponent(project.project_name)}/${encodeURIComponent(selectedPlatform.value)}/${encodeURIComponent(fileName)}`;
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

function isViewable(fileName: string): boolean {
  const ext = fileName.split(".").pop()?.toLowerCase() || "";
  return VIEWABLE_EXTS.includes(ext);
}

function viewFile(fileName: string) {
  window.open(buildResourceUrl(fileName), "_blank");
}
</script>

<template>
  <div class="app-container">
    <div class="title-bar">
      <h1>TEngine Server Admin</h1>
      <div class="title-actions">
        <button class="icon-btn" @click="showSettings = true" title="设置">
          <span class="gear">⚙</span>
        </button>
        <button class="btn btn-secondary" @click="logout" style="font-size:12px;">退出登录</button>
      </div>
    </div>

    <div class="tab-bar">
      <div v-for="project in projects" :key="project.id"
        class="tab" :class="{ active: activeProjectId === project.id }"
        @click="activeProjectId = project.id; loadVersions()">
        <span>{{ project.project_name }}</span>
        <button v-if="projects.length > 1" class="close-btn" @click.stop="removeProject(project.id)">&times;</button>
      </div>
      <button class="add-tab" @click="addProject" title="添加项目">+</button>
    </div>

    <div class="main-content" v-if="activeProject">
      <div class="project-panel">
        <div class="config-compact">
          <div class="config-row">
            <div class="config-field">
              <label>项目名称</label>
              <input v-model="activeProject.project_name" @change="saveProject" />
            </div>
            <div class="config-field">
              <label>包名</label>
              <input v-model="activeProject.package_name" @change="saveProject" />
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

        <div class="control-bar">
          <div class="config-field" style="width:120px">
            <label>上传平台</label>
            <select v-model="selectedPlatform" @change="loadVersions" class="rm-select">
              <option v-for="p in activeProject.platforms" :key="p" :value="p">{{ p }}</option>
            </select>
          </div>
          <div class="config-field" style="width:140px">
            <label>版本号（可选）</label>
            <input v-model="uploadVersion" placeholder="留空自动生成" />
          </div>
          <div style="display:flex;align-items:flex-end;gap:8px;">
            <label class="btn btn-primary" style="cursor:pointer;margin-bottom:0;">
              选择文件上传
              <input type="file" multiple style="display:none" @change="handleUpload" :disabled="uploading" />
            </label>
          </div>
          <div v-if="uploading" style="color:var(--accent);font-size:12px;align-self:flex-end;">上传中...</div>
          <div v-if="activeProject.active_versions[selectedPlatform]" class="server-url" style="margin-left:auto;">
            当前激活: <strong>{{ activeProject.active_versions[selectedPlatform] }}</strong>
          </div>
        </div>

        <div class="rm-versions-section">
          <div class="rm-section-label">所有版本</div>
          <div v-if="versions.length > 0" class="rm-versions-list">
            <div v-for="entry in versions" :key="entry.version" class="rm-version-block">
              <div class="rm-version-row" :class="{ current: activeProject.active_versions[selectedPlatform] === entry.version }">
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

    <!-- File browser dialog -->
    <div v-if="fileBrowser.show" class="rm-dialog-mask" @click.self="fileBrowser.show = false">
      <div class="rm-dialog rm-dialog-wide">
        <div class="rm-dialog-head">
          <h3>
            浏览文件
            <span class="rm-active-version">{{ fileBrowser.version }}</span>
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
        <template v-else>
          <div class="rm-dialog-toolbar">
            <span class="rm-dialog-count">{{ fileBrowser.files.length }} 个文件</span>
            <button v-if="fileBrowser.isActive" class="rm-mini-btn" @click="copyAllBrowserUrls">复制全部 URL</button>
          </div>
          <div class="rm-file-list-dialog">
            <div v-for="f in fileBrowser.files" :key="f.name" class="rm-file-row-v2">
              <span class="rm-file-name-v2" :title="f.name">{{ f.name }}</span>
              <span class="rm-file-size-v2">{{ formatSize(f.size) }}</span>
              <div class="rm-file-actions">
                <button v-if="fileBrowser.isActive && isViewable(f.name)" class="rm-mini-btn" @click="viewFile(f.name)" title="在新窗口预览">显示内容</button>
                <button v-if="fileBrowser.isActive" class="rm-mini-btn" @click="copyToClipboard(buildResourceUrl(f.name))">
                  {{ copiedUrl === buildResourceUrl(f.name) ? "✓ 已复制" : "复制 URL" }}
                </button>
              </div>
            </div>
          </div>
        </template>
      </div>
    </div>

    <SettingsDialog
      :show="showSettings"
      :version="APP_VERSION"
      @close="showSettings = false"
      @open-help="showHelp = true"
      @logout="logout"
    />
    <HelpModal :show="showHelp" @close="showHelp = false" />

    <LogPanel :logs="filteredLogs" @clear="logs = []" />
  </div>
</template>

<style scoped>
.title-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}
.icon-btn {
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
  transition: background 0.15s, color 0.15s;
}
.icon-btn:hover {
  background: var(--bg-tertiary);
  color: var(--accent);
}
.gear {
  display: inline-block;
  transition: transform 0.4s;
  line-height: 1;
}
.icon-btn:hover .gear {
  transform: rotate(45deg);
}

.rm-select {
  width: 100%;
  padding: 4px 8px;
  background: var(--bg-tertiary);
  border: 1px solid var(--border);
  color: var(--text-primary);
  border-radius: 4px;
}

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
.rm-version-block:last-child { border-bottom: none; }
.rm-version-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 12px;
}
.rm-version-row:hover { background: rgba(255, 255, 255, 0.02); }
.rm-version-row.current { background: rgba(34, 211, 238, 0.04); }
.rm-version-info { flex: 1; min-width: 0; }
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
.rm-active-version {
  font-size: 12px;
  color: var(--accent);
  background: rgba(34, 211, 238, 0.1);
  padding: 2px 8px;
  border-radius: 4px;
  font-weight: 500;
  margin-left: 8px;
}

/* Dialog */
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
  max-width: 90vw;
  max-height: 80vh;
  display: flex;
  flex-direction: column;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.4);
}
.rm-dialog-wide { width: 720px; }
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
  font-size: 15px;
  font-weight: 600;
  color: var(--accent);
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
.rm-dialog-loading,
.rm-dialog-error {
  padding: 24px;
  text-align: center;
  color: var(--text-secondary);
  font-size: 13px;
}
.rm-dialog-error { color: #ff6b6b; }
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
.rm-file-row-v2 {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 12px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.04);
  font-size: 12px;
  font-family: ui-monospace, "SF Mono", Menlo, monospace;
}
.rm-file-row-v2:last-child { border-bottom: none; }
.rm-file-row-v2:hover { background: rgba(255, 255, 255, 0.03); }
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
</style>
