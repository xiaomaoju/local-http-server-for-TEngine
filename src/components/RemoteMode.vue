<script setup lang="ts">
import { ref, computed, onUnmounted, nextTick, watch } from "vue";
import { api, type ProjectConfig, type VersionEntry, type LogEntry } from "../api/remote";

const connected = ref(false);
const serverUrl = ref("");
const password = ref("");
const loginError = ref("");
const loginLoading = ref(false);

const projects = ref<ProjectConfig[]>([]);
const activeProjectId = ref("");
const versions = ref<VersionEntry[]>([]);
const logs = ref<LogEntry[]>([]);
const uploading = ref(false);
const selectedPlatform = ref("Android");
const uploadVersion = ref("");
let ws: WebSocket | null = null;

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

async function handleUpload(event: Event) {
  const input = event.target as HTMLInputElement;
  const files = Array.from(input.files || []);
  if (!files.length || !activeProject.value) return;
  uploading.value = true;
  try {
    await api.uploadResources(activeProject.value.id, selectedPlatform.value, uploadVersion.value, files);
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
  <div v-if="!connected" class="login-container" style="flex:1;display:flex;align-items:center;justify-content:center;">
    <div class="login-card" style="background:var(--bg-secondary);border:1px solid var(--border);border-radius:12px;padding:32px;width:360px;">
      <h2 style="color:var(--accent);text-align:center;margin-bottom:24px;font-size:18px;">连接远程服务器</h2>
      <div style="margin-bottom:16px;">
        <label style="display:block;color:var(--text-secondary);font-size:12px;margin-bottom:4px;">服务器地址</label>
        <input v-model="serverUrl" placeholder="http://192.168.1.100:8080" @keyup.enter="handleLogin" />
      </div>
      <div style="margin-bottom:16px;">
        <label style="display:block;color:var(--text-secondary);font-size:12px;margin-bottom:4px;">管理密码</label>
        <input v-model="password" type="password" placeholder="输入管理密码" @keyup.enter="handleLogin" />
      </div>
      <div v-if="loginError" style="color:#ff6b6b;font-size:12px;margin-bottom:12px;">{{ loginError }}</div>
      <button class="btn btn-primary" @click="handleLogin" :disabled="loginLoading || !serverUrl || !password" style="width:100%;padding:10px;">
        {{ loginLoading ? "连接中..." : "连接" }}
      </button>
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

        <!-- Upload -->
        <div class="control-bar">
          <div class="config-field" style="width:120px">
            <label>上传平台</label>
            <select v-model="selectedPlatform" @change="loadVersions" style="width:100%;padding:4px 8px;background:var(--bg-tertiary);border:1px solid var(--border);color:var(--text-primary);border-radius:4px;">
              <option v-for="p in activeProject.platforms" :key="p" :value="p">{{ p }}</option>
            </select>
          </div>
          <div class="config-field" style="width:120px">
            <label>版本号</label>
            <input v-model="uploadVersion" placeholder="自动生成" />
          </div>
          <div style="display:flex;align-items:flex-end;">
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

        <!-- Versions -->
        <div v-if="versions.length > 0" style="margin:8px 0;max-height:200px;overflow-y:auto;">
          <div v-for="entry in versions" :key="entry.version"
            class="version-select-item"
            :class="{ current: activeProject.active_versions[selectedPlatform] === entry.version }">
            <div class="vsi-info" style="flex:1;">
              <div class="vsi-version">
                {{ entry.version }}
                <span v-if="activeProject.active_versions[selectedPlatform] === entry.version" class="vsi-badge current">当前</span>
              </div>
              <div class="vsi-meta">
                {{ entry.file_count }} 个文件 &middot; {{ formatSize(entry.total_size) }} &middot; {{ formatTime(entry.modified_timestamp) }}
              </div>
            </div>
            <button class="btn btn-primary" style="font-size:11px;padding:2px 8px;" @click="activateVersion(entry.version)">激活</button>
            <button class="btn btn-danger" style="font-size:11px;padding:2px 8px;" @click="deleteVersion(entry.version)">删除</button>
          </div>
        </div>
        <div v-else style="color:var(--text-muted);font-size:13px;padding:12px 0;">暂无版本，请上传资源</div>
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
