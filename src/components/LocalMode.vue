<script setup lang="ts">
import { ref, onMounted, computed, nextTick, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

// ==================== Types ====================
interface ProjectConfig {
  id: string;
  project_name: string;
  bundles_dir: string;
  port: number;
  platforms: string[];
  cors_enabled: boolean;
  package_name: string;
}

interface LogEntry {
  timestamp: string;
  status: number;
  method: string;
  path: string;
  project_id: string;
  type?: "http" | "sync";
  message?: string;
  success?: boolean;
}

interface PlatformVersionInfo {
  platform: string;
  latest: string | null;
  synced: string | null;
}

interface VersionEntry {
  version: string;
  modified_timestamp: number;
  file_count: number;
  total_size: number;
}

interface SyncConfirmData {
  show: boolean;
  versions: PlatformVersionInfo[];
  selectedVersion: string;
}

// ==================== State ====================
const projects = ref<ProjectConfig[]>([]);
const activeProjectId = ref<string>("");
const runningServers = ref<Set<string>>(new Set());
const serverUrls = ref<Map<string, string>>(new Map());
const logs = ref<LogEntry[]>([]);
const logPanelOpen = ref(true);
const logPanelHeight = ref(220);
const syncing = ref<Set<string>>(new Set());
const syncVersions = ref<Map<string, string>>(new Map());
const resourceVersions = ref<Map<string, PlatformVersionInfo[]>>(new Map());
const syncConfirm = ref<SyncConfirmData>({ show: false, versions: [], selectedVersion: "" });
const toast = ref<{ show: boolean; message: string }>({ show: false, message: "" });
const versionList = ref<VersionEntry[]>([]);

const localIps = ref<string[]>([]);

const AVAILABLE_PLATFORMS = ["Android", "iOS", "Windows", "MacOS", "Linux", "WebGL"];

// ==================== Computed ====================
const activeProject = computed(() =>
  projects.value.find((p) => p.id === activeProjectId.value)
);

const filteredLogs = computed(() => {
  if (!activeProjectId.value) return logs.value;
  return logs.value.filter(
    (l) => l.project_id === activeProjectId.value
  );
});

// ==================== Lifecycle ====================
onMounted(async () => {
  await loadProjects();

  await listen<LogEntry>("server-log", (event) => {
    const entry = { ...event.payload, type: "http" as const };
    logs.value.push(entry);
    if (logs.value.length > 2000) {
      logs.value = logs.value.slice(-1500);
    }
    nextTick(() => scrollLogToBottom());
  });

  await listen<{ project_id: string; message: string; success: boolean }>(
    "sync-log",
    (event) => {
      const entry: LogEntry = {
        timestamp: new Date().toLocaleTimeString("zh-CN", { hour12: false }),
        status: event.payload.success ? 200 : 500,
        method: "SYNC",
        path: event.payload.message,
        project_id: event.payload.project_id,
        type: "sync",
        message: event.payload.message,
        success: event.payload.success,
      };
      logs.value.push(entry);
      nextTick(() => scrollLogToBottom());
    }
  );
});

// ==================== Methods ====================
async function loadProjects() {
  try {
    projects.value = await invoke<ProjectConfig[]>("get_projects");
    if (projects.value.length > 0 && !activeProjectId.value) {
      activeProjectId.value = projects.value[0].id;
    }
    const running = await invoke<string[]>("get_running_servers");
    runningServers.value = new Set(running);
    localIps.value = await invoke<string[]>("get_local_ips");
    // 加载所有项目的版本号
    for (const p of projects.value) {
      loadResourceVersion(p.id);
    }
  } catch (e) {
    console.error("加载项目失败:", e);
  }
}

async function addProject() {
  try {
    const project = await invoke<ProjectConfig>("add_project");
    projects.value.push(project);
    activeProjectId.value = project.id;
  } catch (e) {
    console.error("添加项目失败:", e);
  }
}

async function removeProject(id: string) {
  if (projects.value.length <= 1) return;
  try {
    await invoke("remove_project", { projectId: id });
    projects.value = projects.value.filter((p) => p.id !== id);
    runningServers.value.delete(id);
    if (activeProjectId.value === id) {
      activeProjectId.value = projects.value[0]?.id || "";
    }
  } catch (e) {
    console.error("删除项目失败:", e);
  }
}

async function saveProject() {
  const project = activeProject.value;
  if (!project) return;
  try {
    await invoke("update_project", { project });
  } catch (e) {
    console.error("保存配置失败:", e);
  }
}

let saveTimer: ReturnType<typeof setTimeout> | null = null;
watch(
  () => activeProject.value,
  () => {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => saveProject(), 500);
  },
  { deep: true }
);

async function startServer() {
  const project = activeProject.value;
  if (!project) return;
  try {
    const url = await invoke<string>("start_server", {
      projectId: project.id,
    });
    runningServers.value.add(project.id);
    serverUrls.value.set(project.id, url);
    addSysLog(project.id, `服务器已启动: ${url}`);
  } catch (e: any) {
    addSysLog(project.id, `启动失败: ${e}`, false);
  }
}

async function stopServer() {
  const project = activeProject.value;
  if (!project) return;
  try {
    await invoke("stop_server", { projectId: project.id });
    runningServers.value.delete(project.id);
    serverUrls.value.delete(project.id);
    addSysLog(project.id, "服务器已停止");
  } catch (e: any) {
    addSysLog(project.id, `停止失败: ${e}`, false);
  }
}

// 点击同步按钮 -> 加载版本列表并弹窗
async function checkAndSync() {
  const project = activeProject.value;
  if (!project || !project.platforms.length) return;

  // 加载版本信息
  await loadResourceVersion(project.id);
  const infos = resourceVersions.value.get(project.id) || [];

  // 加载版本列表（用第一个平台）
  try {
    versionList.value = await invoke<VersionEntry[]>("list_versions", {
      projectId: project.id,
      platform: project.platforms[0],
    });
  } catch (e) {
    versionList.value = [];
  }

  if (versionList.value.length === 0) {
    showToast("未找到任何可用的资源版本");
    return;
  }

  // 默认选中最新版本
  const latestVersion = versionList.value[0].version;
  // const currentSynced = infos.length > 0 ? infos[0].synced : null;

  syncConfirm.value = {
    show: true,
    versions: infos,
    selectedVersion: latestVersion,
  };
}

// 确认同步后执行
async function doSync() {
  const selectedVer = syncConfirm.value.selectedVersion;
  syncConfirm.value.show = false;
  const project = activeProject.value;
  if (!project) return;
  syncing.value.add(project.id);
  try {
    const result = await invoke<{
      success: boolean;
      message: string;
      version: string | null;
      synced_files: string[];
    }>("sync_specific_version", {
      projectId: project.id,
      version: selectedVer,
    });

    if (result.version) {
      syncVersions.value.set(project.id, result.version);
    }
    addSysLog(
      project.id,
      `同步完成: ${result.message} (${result.synced_files.length} 个文件)`
    );
  } catch (e: any) {
    addSysLog(project.id, `同步失败: ${e}`, false);
  } finally {
    syncing.value.delete(project.id);
    loadResourceVersion(project.id);
  }
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

async function selectBundlesDir() {
  const project = activeProject.value;
  if (!project) return;
  const selected = await open({
    directory: true,
    multiple: false,
    title: "选择 Bundles 目录",
  });
  if (selected) {
    project.bundles_dir = selected as string;
  }
}

function togglePlatform(platform: string) {
  const project = activeProject.value;
  if (!project) return;
  const idx = project.platforms.indexOf(platform);
  if (idx >= 0) {
    if (project.platforms.length > 1) {
      project.platforms.splice(idx, 1);
    }
  } else {
    project.platforms.push(platform);
  }
}

function isRunning(id: string): boolean {
  return runningServers.value.has(id);
}

function isSyncing(id: string): boolean {
  return syncing.value.has(id);
}


const copiedUrl = ref("");

async function loadResourceVersion(projectId?: string) {
  const id = projectId || activeProjectId.value;
  if (!id) return;
  try {
    const infos = await invoke<PlatformVersionInfo[]>("get_resource_version", {
      projectId: id,
    });
    resourceVersions.value.set(id, infos);
  } catch (e) {
    console.error("读取版本失败:", e);
  }
}

function showToast(message: string, duration = 2000) {
  toast.value = { show: true, message };
  setTimeout(() => { toast.value.show = false; }, duration);
}

async function copyUrl(url: string) {
  await navigator.clipboard.writeText(url);
  copiedUrl.value = url;
  setTimeout(() => (copiedUrl.value = ""), 1500);
}

function clearLogs() {
  logs.value = [];
}

function addSysLog(projectId: string, message: string, success = true) {
  logs.value.push({
    timestamp: new Date().toLocaleTimeString("zh-CN", { hour12: false }),
    status: success ? 200 : 500,
    method: "SYS",
    path: message,
    project_id: projectId,
    type: "sync",
    success,
  });
  nextTick(() => scrollLogToBottom());
}

function scrollLogToBottom() {
  const el = document.querySelector(".log-body");
  if (el) el.scrollTop = el.scrollHeight;
}

function getStatusClass(status: number): string {
  if (status >= 200 && status < 300) return "s200";
  if (status >= 300 && status < 400) return "s301";
  if (status >= 400 && status < 500) return "s404";
  return "s500";
}

// ========== Log panel resize ==========
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
  <div class="local-mode">
    <!-- Tab Bar -->
    <div class="tab-bar">
      <div
        v-for="project in projects"
        :key="project.id"
        class="tab"
        :class="{ active: activeProjectId === project.id }"
        @click="activeProjectId = project.id"
      >
        <span class="status-dot" :class="{ running: isRunning(project.id) }"></span>
        <span>{{ project.project_name }}</span>
        <button
          v-if="projects.length > 1"
          class="close-btn"
          @click.stop="removeProject(project.id)"
        >
          &times;
        </button>
      </div>
      <button class="add-tab" @click="addProject" title="添加项目">+</button>
    </div>

    <!-- Main Content -->
    <div class="main-content">
      <div v-if="activeProject" class="project-panel">
        <!-- Config Section - Compact -->
        <div class="config-compact">
          <!-- Row 1: Name, Port, Package, CORS -->
          <div class="config-row">
            <div class="config-field">
              <label>项目名称</label>
              <input v-model="activeProject.project_name" placeholder="TEngine" :disabled="isRunning(activeProject.id)" />
            </div>
            <div class="config-field config-port">
              <label>端口</label>
              <input v-model.number="activeProject.port" type="number" min="1024" max="65535" :disabled="isRunning(activeProject.id)" />
            </div>
            <div class="config-field">
              <label>包名</label>
              <input v-model="activeProject.package_name" placeholder="DefaultPackage" :disabled="isRunning(activeProject.id)" />
            </div>
            <div class="config-field checkbox-field tooltip-wrap">
              <input type="checkbox" v-model="activeProject.cors_enabled" :disabled="isRunning(activeProject.id)" />
              <label>CORS</label>
              <div class="tooltip">
                <strong>跨域资源共享 (CORS)</strong><br/>
                允许不同域名/端口的客户端访问资源。<br/><br/>
                <span class="tooltip-item">&#9679; WebGL 构建 — <em>必须开启</em></span><br/>
                <span class="tooltip-item">&#9679; Android/iOS 真机 — 无需开启</span><br/>
                <span class="tooltip-item">&#9679; Unity Editor — 一般无需开启</span><br/><br/>
                <span class="tooltip-hint">建议保持勾选，没有副作用。</span>
              </div>
            </div>
          </div>
          <!-- Row 2: Bundles path + Platforms -->
          <div class="config-row">
            <div class="config-field config-path-field">
              <label>Bundles</label>
              <div class="path-input">
                <input v-model="activeProject.bundles_dir" placeholder="选择 Bundles 目录..." :disabled="isRunning(activeProject.id)" />
                <button @click="selectBundlesDir" :disabled="isRunning(activeProject.id)">浏览</button>
              </div>
            </div>
            <div class="config-field config-platforms-field">
              <label>平台</label>
              <div class="platform-tags">
                <span v-for="p in AVAILABLE_PLATFORMS" :key="p" class="platform-tag" :class="{ selected: activeProject.platforms.includes(p) }" @click="togglePlatform(p)">{{ p }}</span>
              </div>
            </div>
          </div>
        </div>

        <!-- Control Bar -->
        <div class="control-bar">
          <button
            v-if="!isRunning(activeProject.id)"
            class="btn btn-primary"
            @click="startServer"
            :disabled="!activeProject.bundles_dir"
          >
            &#9654; 启动服务
          </button>
          <button
            v-else
            class="btn btn-danger"
            @click="stopServer"
          >
            &#9632; 停止服务
          </button>

          <button
            class="btn btn-secondary"
            @click="checkAndSync"
            :disabled="!activeProject.bundles_dir || isSyncing(activeProject.id)"
          >
            <span v-if="isSyncing(activeProject.id)" class="spinner"></span>
            <span v-else>&#8635;</span>
            同步资源
          </button>

          <div class="server-urls" v-if="isRunning(activeProject.id)">
            <div class="url-row">
              <span class="url-label">本机</span>
              <span class="url-text">http://127.0.0.1:{{ activeProject.port }}/{{ activeProject.project_name }}/</span>
              <button class="url-copy" @click="copyUrl(`http://127.0.0.1:${activeProject.port}/${activeProject.project_name}/`)">
                {{ copiedUrl === `http://127.0.0.1:${activeProject.port}/${activeProject.project_name}/` ? '&#10003;' : '&#128203;' }}
              </button>
            </div>
            <div class="url-row" v-for="ip in localIps" :key="ip">
              <span class="url-label">局域网</span>
              <span class="url-text">http://{{ ip }}:{{ activeProject.port }}/{{ activeProject.project_name }}/</span>
              <button class="url-copy" @click="copyUrl(`http://${ip}:${activeProject.port}/${activeProject.project_name}/`)">
                {{ copiedUrl === `http://${ip}:${activeProject.port}/${activeProject.project_name}/` ? '&#10003;' : '&#128203;' }}
              </button>
            </div>
          </div>
          <div v-else class="server-url">服务未启动</div>
        </div>

        <!-- Resource Version -->
        <div v-if="resourceVersions.get(activeProject.id)?.length" class="version-bar">
          <span class="version-bar-label">资源版本</span>
          <div class="version-tags">
            <span
              v-for="info in resourceVersions.get(activeProject.id)"
              :key="info.platform"
              class="version-tag"
            >
              <span class="version-tag-platform">{{ info.platform }}</span>
              <span class="version-tag-value">{{ info.synced || info.latest || '未构建' }}</span>
            </span>
          </div>
        </div>
      </div>

      <!-- Empty State -->
      <div v-else class="empty-state">
        <div class="icon">&#128268;</div>
        <p>点击 + 添加一个项目</p>
      </div>
    </div>

    <!-- Resize Handle -->
    <div
      v-if="logPanelOpen"
      class="resize-handle"
      @mousedown="onResizeStart"
    ></div>

    <!-- Log Panel -->
    <div
      class="log-panel"
      :class="{ collapsed: !logPanelOpen }"
      :style="{ height: logPanelOpen ? logPanelHeight + 'px' : '36px' }"
    >
      <div class="log-header" @click="logPanelOpen = !logPanelOpen">
        <h3>
          <span
            class="toggle-icon"
            :class="{ expanded: logPanelOpen }"
          >&#9650;</span>
          日志
          <span class="log-count">{{ filteredLogs.length }}</span>
        </h3>
        <div class="log-actions" @click.stop>
          <button @click="clearLogs">清空</button>
        </div>
      </div>
      <div v-if="logPanelOpen" class="log-body">
        <div v-if="filteredLogs.length === 0" class="empty-state" style="height: 100%">
          <p style="font-size: 12px; color: var(--text-muted)">暂无日志</p>
        </div>
        <div
          v-for="(log, idx) in filteredLogs"
          :key="idx"
          class="log-entry"
          :class="{
            'sync-entry': log.type === 'sync',
            error: log.success === false,
          }"
        >
          <span class="time">{{ log.timestamp }}</span>
          <span class="status" :class="getStatusClass(log.status)">
            {{ log.method === 'SYS' ? 'SYS' : log.method === 'SYNC' ? 'SYNC' : log.status }}
          </span>
          <span class="path">{{ log.path }}</span>
        </div>
      </div>
    </div>

    <!-- ==================== Sync Confirm Dialog ==================== -->
    <Teleport to="body">
      <div v-if="syncConfirm.show" class="help-overlay" @click.self="syncConfirm.show = false">
        <div class="sync-dialog">
          <div class="sync-dialog-header">
            <h3>资源同步</h3>
            <button class="help-close" @click="syncConfirm.show = false">&times;</button>
          </div>
          <div class="sync-dialog-body">

            <!-- 当前同步状态 -->
            <div class="sync-current" v-if="syncConfirm.versions.some(v => v.synced)">
              <span class="sync-current-label">当前版本</span>
              <span class="sync-current-version">{{ syncConfirm.versions.find(v => v.synced)?.synced }}</span>
            </div>
            <div class="sync-current" v-else>
              <span class="sync-current-label">当前版本</span>
              <span class="sync-current-version none">未同步</span>
            </div>

            <!-- 版本选择列表 -->
            <div class="version-select-label">选择要同步的版本</div>
            <div class="version-select-list">
              <div
                v-for="(entry, idx) in versionList"
                :key="entry.version"
                class="version-select-item"
                :class="{
                  selected: syncConfirm.selectedVersion === entry.version,
                  current: syncConfirm.versions.some(v => v.synced === entry.version),
                }"
                @click="syncConfirm.selectedVersion = entry.version"
              >
                <div class="vsi-radio">
                  <div class="vsi-radio-dot" v-if="syncConfirm.selectedVersion === entry.version"></div>
                </div>
                <div class="vsi-info">
                  <div class="vsi-version">
                    {{ entry.version }}
                    <span v-if="idx === 0" class="vsi-badge latest">最新</span>
                    <span v-if="syncConfirm.versions.some(v => v.synced === entry.version)" class="vsi-badge current">当前</span>
                  </div>
                  <div class="vsi-meta">
                    {{ entry.file_count }} 个文件 &middot; {{ formatSize(entry.total_size) }} &middot; {{ formatTime(entry.modified_timestamp) }}
                  </div>
                </div>
              </div>
            </div>

          </div>
          <div class="sync-dialog-footer">
            <button class="btn btn-secondary" @click="syncConfirm.show = false">取消</button>
            <button
              class="btn btn-primary"
              @click="doSync"
              :disabled="!syncConfirm.selectedVersion"
            >
              同步至 {{ syncConfirm.selectedVersion }}
            </button>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- ==================== Toast ==================== -->
    <Teleport to="body">
      <Transition name="toast">
        <div v-if="toast.show" class="toast">
          {{ toast.message }}
        </div>
      </Transition>
    </Teleport>

  </div>
</template>
