<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount, computed, nextTick } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import Dialog from "../../shared/Dialog.vue";
import ToggleSwitch from "../../shared/ToggleSwitch.vue";
import StatusBadge from "../../shared/StatusBadge.vue";
import { Settings, Folder, Play, Square, RefreshCw } from "@lucide/vue";

// ==================== Types ====================
interface ProjectConfig {
  id: string;
  project_name: string;
  bundles_dir: string;
  port: number;
  platforms: string[];
  cors_enabled: boolean;
  package_name: string;
  project_version: string;
  platform_access: Record<string, boolean>;
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
  projectId: string;
  versions: PlatformVersionInfo[];
  selectedVersion: string;
}

// ==================== State ====================
const projects = ref<ProjectConfig[]>([]);
const activeProjectId = ref<string>("");
const runningServers = ref<Set<string>>(new Set());
const serverUrls = ref<Map<string, string>>(new Map());
const logs = ref<LogEntry[]>([]);
const logPanelOpen = ref(false);
const settingsOpen = ref(false);
const deleteOpen = ref(false);
const localError = ref("");
const saving = ref(false);
const deleting = ref(false);
const syncLoading = ref(false);
const discardOpen = ref(false);
let settingsSnapshot = "";
const unlisteners: (() => void)[] = [];
function openSettings() {
  settingsSnapshot = JSON.stringify(activeProject.value);
  settingsOpen.value = true;
  localError.value = "";
}
function cancelSettings() {
  if (saving.value || deleting.value) return;
  if (JSON.stringify(activeProject.value) !== settingsSnapshot) {
    discardOpen.value = true;
    return;
  }
  discardSettings();
}
function discardSettings() {
  if (activeProject.value && settingsSnapshot)
    Object.assign(activeProject.value, JSON.parse(settingsSnapshot));
  settingsOpen.value = false;
  discardOpen.value = false;
}
async function applySettings() {
  if (await saveProject()) {
    settingsOpen.value = false;
    loadResourceVersion();
  }
}
const logPanelHeight = ref(220);
const syncing = ref<Set<string>>(new Set());
const syncVersions = ref<Map<string, string>>(new Map());
const resourceVersions = ref<Map<string, PlatformVersionInfo[]>>(new Map());
const syncConfirm = ref<SyncConfirmData>({
  show: false,
  projectId: "",
  versions: [],
  selectedVersion: "",
});
const toast = ref<{ show: boolean; message: string }>({
  show: false,
  message: "",
});
const versionList = ref<VersionEntry[]>([]);

const localIps = ref<string[]>([]);

const AVAILABLE_PLATFORMS = [
  "Android",
  "iOS",
  "Windows",
  "MacOS",
  "Linux",
  "WebGL",
];

// ==================== Computed ====================
const activeProject = computed(() =>
  projects.value.find((p) => p.id === activeProjectId.value),
);

const filteredLogs = computed(() => {
  if (!activeProjectId.value) return logs.value;
  return logs.value.filter((l) => l.project_id === activeProjectId.value);
});

// ==================== Lifecycle ====================
onMounted(async () => {
  await loadProjects();

  unlisteners.push(
    await listen<LogEntry>("server-log", (event) => {
      const entry = { ...event.payload, type: "http" as const };
      logs.value.push(entry);
      if (logs.value.length > 2000) {
        logs.value = logs.value.slice(-1500);
      }
      nextTick(() => scrollLogToBottom());
    }),
  );

  unlisteners.push(
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
      },
    ),
  );
});
onBeforeUnmount(() => {
  unlisteners.forEach((stop) => stop());
  onResizeEnd();
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
    localError.value = `加载项目失败: ${e}`;
  }
}

async function addProject() {
  try {
    const project = await invoke<ProjectConfig>("add_project");
    projects.value.push(project);
    activeProjectId.value = project.id;
    openSettings();
  } catch (e) {
    localError.value = `添加项目失败: ${e}`;
  }
}

async function removeProject(id: string) {
  if (deleting.value || saving.value) return;
  deleting.value = true;
  try {
    await invoke("remove_project", { projectId: id });
    projects.value = projects.value.filter((p) => p.id !== id);
    runningServers.value.delete(id);
    deleteOpen.value = false;
    settingsOpen.value = false;
    if (activeProjectId.value === id) {
      activeProjectId.value = projects.value[0]?.id || "";
    }
  } catch (e) {
    localError.value = `删除失败: ${e}`;
  } finally {
    deleting.value = false;
  }
}

async function saveProject() {
  if (saving.value || deleting.value) return false;
  const project = activeProject.value;
  if (!project) return false;
  if (isRunning(project.id)) {
    localError.value = "请先停止服务，再修改并保存配置。";
    return false;
  }
  if (
    !project.project_name.trim() ||
    !project.project_version.trim() ||
    !project.package_name.trim() ||
    project.port < 1024 ||
    project.port > 65535
  ) {
    localError.value = "请填写项目名称、版本和包名，端口需在 1024–65535 之间";
    return false;
  }
  saving.value = true;
  try {
    await invoke("update_project", { project });
    return true;
  } catch (e) {
    localError.value = `保存失败: ${e}`;
    return false;
  } finally {
    saving.value = false;
  }
}

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
  if (
    !project ||
    !project.platforms.length ||
    syncLoading.value ||
    isSyncing(project.id)
  )
    return;
  syncLoading.value = true;
  try {
    // 加载版本信息
    await loadResourceVersion(project.id);
    const infos = resourceVersions.value.get(project.id) || [];

    // 加载版本列表（用第一个平台）
    try {
      const result = await invoke<VersionEntry[]>("list_versions", {
        projectId: project.id,
        platform: project.platforms[0],
      });
      if (activeProjectId.value !== project.id) return;
      versionList.value = result;
    } catch (e) {
      localError.value = `读取资源版本失败: ${e}`;
      return;
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
      projectId: project.id,
      versions: infos,
      selectedVersion: latestVersion,
    };
  } finally {
    syncLoading.value = false;
  }
}

// 确认同步后执行
async function doSync() {
  const selectedVer = syncConfirm.value.selectedVersion;
  syncConfirm.value.show = false;
  const project = projects.value.find(
    (p) => p.id === syncConfirm.value.projectId,
  );
  if (!project) return;
  if (isSyncing(project.id)) return;
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

    if (!result.success) {
      addSysLog(project.id, `同步失败: ${result.message}`, false);
      return;
    }
    if (result.version) {
      syncVersions.value.set(project.id, result.version);
    }
    addSysLog(
      project.id,
      `同步完成: ${result.message} (${result.synced_files.length} 个文件)`,
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
    if (!settingsOpen.value) await saveProject();
  }
}

function togglePlatform(platform: string) {
  const project = activeProject.value;
  if (!project || isRunning(project.id) || saving.value) return;
  const idx = project.platforms.indexOf(platform);
  if (idx >= 0) {
    if (project.platforms.length > 1) {
      project.platforms.splice(idx, 1);
      delete project.platform_access[platform];
    }
  } else {
    project.platforms.push(platform);
    if (!project.platform_access) project.platform_access = {};
    project.platform_access[platform] = true;
  }
}

function toggleLocalAccess(platform: string) {
  const project = activeProject.value;
  if (!project || isRunning(project.id) || saving.value) return;
  if (!project.platform_access) project.platform_access = {};
  project.platform_access[platform] = !(
    project.platform_access[platform] ?? true
  );
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
    localError.value = `读取版本失败: ${e}`;
  }
}

function showToast(message: string, duration = 2000) {
  toast.value = { show: true, message };
  setTimeout(() => {
    toast.value.show = false;
  }, duration);
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
  if (!success) localError.value = message;
  if (logs.value.length >= 2000) logs.value = logs.value.slice(-1500);
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
    <Dialog
      :open="settingsOpen"
      title="本地项目设置"
      drawer
      :busy="saving || deleting"
      @close="cancelSettings"
    >
      <form
        v-if="activeProject"
        id="local-settings-form"
        class="ui-form"
        @submit.prevent="applySettings"
      >
        <div class="ui-field">
          <label for="local-name">项目名称</label
          ><input
            id="local-name"
            v-model="activeProject.project_name"
            :disabled="isRunning(activeProject.id)"
          />
        </div>
        <div class="ui-two">
          <div class="ui-field">
            <label for="local-port">服务端口</label
            ><input
              id="local-port"
              v-model.number="activeProject.port"
              type="number"
              min="1024"
              max="65535"
              :disabled="isRunning(activeProject.id)"
            />
          </div>
          <div class="ui-field">
            <label for="local-version">项目版本</label
            ><input
              id="local-version"
              v-model="activeProject.project_version"
              :disabled="isRunning(activeProject.id)"
            />
          </div>
        </div>
        <div class="ui-field">
          <label for="local-package">资源包名</label
          ><input
            id="local-package"
            v-model="activeProject.package_name"
            :disabled="isRunning(activeProject.id)"
          /><small class="ui-muted">与 Unity / YooAsset 的配置保持一致。</small>
        </div>
        <div class="ui-field">
          <label for="local-directory">资源目录</label>
          <div class="ui-inline">
            <input
              id="local-directory"
              v-model="activeProject.bundles_dir"
              :disabled="isRunning(activeProject.id)"
            /><button
              type="button"
              @click="selectBundlesDir"
              :disabled="isRunning(activeProject.id)"
            >
              浏览
            </button>
          </div>
        </div>
        <label class="ui-inline"
          ><input
            v-model="activeProject.cors_enabled"
            type="checkbox"
            :disabled="isRunning(activeProject.id)"
          />允许跨域下载（WebGL 需要）</label
        >
        <div class="ui-field">
          <label>目标平台（可多选）</label>
          <div class="ui-checks">
            <label v-for="p in AVAILABLE_PLATFORMS" :key="p"
              ><input
                type="checkbox"
                :checked="activeProject.platforms.includes(p)"
                :disabled="isRunning(activeProject.id)"
                @change="togglePlatform(p)"
              />{{ p }}</label
            >
          </div>
        </div>
        <div class="ui-field">
          <label>客户端下载</label>
          <div v-for="p in activeProject.platforms" :key="p" class="ui-inline"><ToggleSwitch :model-value="activeProject.platform_access?.[p] !== false" :label="p + '客户端下载'" :disabled="isRunning(activeProject.id)" @update:model-value="toggleLocalAccess(p)" /></div>
        </div>
        <p v-if="isRunning(activeProject.id)" class="ui-muted">
          服务运行期间配置只读。请先停止服务，再修改并保存配置。
        </p>
        <div v-if="localError" class="ui-error" role="alert">
          {{ localError }}
        </div>
        <button type="button" class="danger" @click="deleteOpen = true">
          删除本地项目
        </button>
      </form>
      <template #footer
        ><button :disabled="saving || deleting" @click="cancelSettings">
          取消</button
        ><button
          class="primary"
          type="submit"
          form="local-settings-form"
          :disabled="
            saving || deleting || (activeProject && isRunning(activeProject.id))
          "
        >
          保存修改
        </button></template
      >
    </Dialog>
    <Dialog
      :open="deleteOpen"
      title="删除本地项目？"
      :busy="deleting"
      @close="deleteOpen = false"
      ><p>
        将删除项目配置并停止本地服务。请确认这是要删除的项目：{{
          activeProject?.project_name
        }}。
      </p>
      <template #footer
        ><button :disabled="deleting" @click="deleteOpen = false">取消</button
        ><button
          :disabled="deleting"
          class="danger"
          @click="removeProject(activeProjectId)"
        >
          删除项目
        </button></template
      ></Dialog
    >
    <Dialog
      :open="discardOpen"
      title="放弃未保存的修改？"
      @close="discardOpen = false"
    >
      <p>关闭后将恢复上次保存的本地项目配置。</p>
      <template #footer
        ><button @click="discardOpen = false">继续编辑</button
        ><button class="danger" @click="discardSettings">
          放弃修改
        </button></template
      >
    </Dialog>
    <!-- Tab Bar -->
    <div class="tab-bar">
      <button
        v-for="project in projects"
        :key="project.id"
        class="tab"
        :class="{ active: activeProjectId === project.id }"
        @click="activeProjectId = project.id"
      >
        <span
          class="status-dot"
          :class="{ running: isRunning(project.id) }"
        ></span>
        <span>{{ project.project_name }}</span>
      </button>
      <button class="add-tab" @click="addProject" title="添加项目">+</button>
    </div>

    <!-- Main Content -->
    <div class="main-content">
      <div v-if="activeProject" class="project-panel">
        <div class="ui-toolbar">
          <strong>{{ activeProject.project_name }}</strong
          ><span class="ui-muted"
            >项目版本 {{ activeProject.project_version }}</span><StatusBadge :tone="isRunning(activeProject.id) ? 'success' : 'neutral'">{{ isRunning(activeProject.id) ? '服务运行中' : '服务未启动' }}</StatusBadge><span class="ui-spacer" /><button
            class="ui-button"
            @click="openSettings"
          >
            <Settings :size="14" />项目设置
          </button>
        </div>
        <div v-if="localError" class="ui-error" role="alert">
          {{ localError
          }}<button class="ui-button" @click="localError = ''">关闭</button>
        </div>
        <div class="ui-toolbar">
          <button
            class="ui-button"
            @click="selectBundlesDir"
            :disabled="isRunning(activeProject.id)"
          >
            <Folder :size="14" />选择资源目录</button
          ><span
            class="ui-muted"
            style="
              overflow: hidden;
              text-overflow: ellipsis;
              white-space: nowrap;
            "
            :title="activeProject.bundles_dir"
            >{{
              activeProject.bundles_dir ||
              "先选择 Bundles 目录，再同步资源并启动服务。"
            }}</span
          >
        </div>
        <!-- Control Bar -->
        <div class="control-bar">
          <button
            v-if="!isRunning(activeProject.id)"
            class="btn btn-primary"
            @click="startServer"
            :disabled="!activeProject.bundles_dir"
          >
            <Play :size="14" /> 启动服务
          </button>
          <button v-else class="btn btn-danger" @click="stopServer">
            <Square :size="14" /> 停止服务
          </button>

          <button
            class="btn btn-secondary"
            @click="checkAndSync"
            :disabled="
              !activeProject.bundles_dir ||
              syncLoading ||
              isSyncing(activeProject.id)
            "
          >
            <span v-if="isSyncing(activeProject.id)" class="spinner"></span>
            <RefreshCw v-else :size="14" />
            同步资源
          </button>

          <div class="server-urls" v-if="isRunning(activeProject.id)">
            <div class="url-row">
              <span class="url-label">本机</span>
              <span class="url-text"
                >http://127.0.0.1:{{ activeProject.port }}/{{
                  activeProject.project_version
                }}/{{ activeProject.project_name }}/</span
              >
              <button
                class="url-copy"
                @click="
                  copyUrl(
                    `http://127.0.0.1:${activeProject.port}/${activeProject.project_version}/${activeProject.project_name}/`,
                  )
                "
              >
                {{
                  copiedUrl ===
                  `http://127.0.0.1:${activeProject.port}/${activeProject.project_version}/${activeProject.project_name}/`
                    ? "已复制"
                    : "复制"
                }}
              </button>
            </div>
            <div class="url-row" v-for="ip in localIps" :key="ip">
              <span class="url-label">局域网</span>
              <span class="url-text"
                >http://{{ ip }}:{{ activeProject.port }}/{{
                  activeProject.project_version
                }}/{{ activeProject.project_name }}/</span
              >
              <button
                class="url-copy"
                @click="
                  copyUrl(
                    `http://${ip}:${activeProject.port}/${activeProject.project_version}/${activeProject.project_name}/`,
                  )
                "
              >
                {{
                  copiedUrl ===
                  `http://${ip}:${activeProject.port}/${activeProject.project_version}/${activeProject.project_name}/`
                    ? "已复制"
                    : "复制"
                }}
              </button>
            </div>
          </div>
          <div v-else class="server-url">服务未启动</div>
        </div>

        <!-- Resource Version -->
        <div
          v-if="resourceVersions.get(activeProject.id)?.length"
          class="version-bar"
        >
          <span class="version-bar-label">资源版本</span>
          <div class="version-tags">
            <span
              v-for="info in resourceVersions.get(activeProject.id)"
              :key="info.platform"
              class="version-tag"
            >
              <span class="version-tag-platform">{{ info.platform }}</span>
              <span class="version-tag-value">{{
                info.synced || info.latest || "未构建"
              }}</span>
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
      <div class="log-header">
        <button
          class="local-log-toggle"
          :aria-expanded="logPanelOpen"
          aria-controls="local-log-body"
          @click="logPanelOpen = !logPanelOpen"
        >
          <span class="toggle-icon" :class="{ expanded: logPanelOpen }"
            >&#9650;</span
          >
          日志
          <span class="log-count">{{ filteredLogs.length }}</span>
        </button>
        <div class="log-actions" @click.stop>
          <button @click="clearLogs">清空</button>
        </div>
      </div>
      <div v-if="logPanelOpen" id="local-log-body" class="log-body">
        <div
          v-if="filteredLogs.length === 0"
          class="empty-state"
          style="height: 100%"
        >
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
            {{
              log.method === "SYS"
                ? "SYS"
                : log.method === "SYNC"
                  ? "SYNC"
                  : log.status
            }}
          </span>
          <span class="path">{{ log.path }}</span>
        </div>
      </div>
    </div>

    <Dialog
      :open="syncConfirm.show"
      title="同步本地资源"
      @close="syncConfirm.show = false"
    >
      <div class="ui-form">
        <p class="ui-muted">选择资源版本，同步后本地客户端将读取该版本。</p>
        <label
          v-for="entry in versionList"
          :key="entry.version"
          class="ui-inline"
          ><input
            v-model="syncConfirm.selectedVersion"
            type="radio"
            name="sync-version"
            :value="entry.version"
          /><span class="ui-mono">{{ entry.version }}</span
          ><span class="ui-muted"
            >{{ entry.file_count }} 文件 · {{ formatSize(entry.total_size) }} ·
            {{ formatTime(entry.modified_timestamp) }}</span
          ></label
        >
      </div>
      <template #footer
        ><button @click="syncConfirm.show = false">取消</button
        ><button
          class="primary"
          :disabled="!syncConfirm.selectedVersion"
          @click="doSync"
        >
          确认同步
        </button></template
      >
    </Dialog>

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
