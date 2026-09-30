<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount, watch } from "vue";
import {
  Package,
  Folder,
  History,
  Users,
  Settings,
  Sun,
  Moon,
  LogOut,
  Plus,
  Search,
  Upload,
  Copy,
  ArrowLeft,
  Menu,
  RefreshCw,
  CircleHelp,
  AlertCircle,
  Check,
  UserRound,
  Trash2,
  ChevronRight,
  Eye,
  GitCompareArrows,
} from "@lucide/vue";
import Dialog from "./Dialog.vue";
import SegmentedControl from "./SegmentedControl.vue";
import ToggleSwitch from "./ToggleSwitch.vue";
import StatusBadge from "./StatusBadge.vue";
import ActionButton from "./ActionButton.vue";
import ClientLogPanel, { type ClientLogRow } from "./ClientLogPanel.vue";
import DesktopConnections from "./DesktopConnections.vue";
import { loadConnections, storeConnections } from "./connectionProfiles";
import type { ConnectionLogin } from "./api";
import UserAdmin from "./UserAdmin.vue";
import { theme, toggleTheme } from "./theme";
import { compareManifests, type UploadDiff } from "./uploadDiff";
import {
  ApiError,
  type RemoteApi,
  type User,
  type ProjectConfig,
  type VersionEntry,
  type FileEntry,
  type LogEntry,
  type ResourceRequestLog,
  type LogConnection,
  type NativeUpload,
  type FileManifestEntry,
  resourcePath,
  sizeText,
  timeText,
} from "./api";
const props = defineProps<{ api: RemoteApi; native?: NativeUpload }>();
const api = props.api;
const user = ref<User | null>(null);
const booting = ref(true);
const username = ref(localStorage.getItem("tengine_username") || "");
const password = ref("");
const connectionId = ref("");
const serverUrl = ref(
  localStorage.getItem("tengine_server_url") || "http://localhost:8083",
);
const projects = ref<ProjectConfig[]>([]);
const selected = ref("");
const projectVersion = ref("");
const platform = ref("Android");
const page = ref("projects");
const scope = ref("mine");
const query = ref("");
const navOpen = ref(false);
const error = ref("");
const dialogError = ref("");
const notice = ref("");
const busy = ref(false);
const loading = ref(false);
const logState = ref<"connecting" | "live" | "retrying">("connecting");
const versions = ref<VersionEntry[]>([]);
const logs = ref<LogEntry[]>([]);
const resourceLogs = ref<ResourceRequestLog[]>([]);
const resourceLogsAvailable = ref<boolean | null>(null);
const clientLogs = ref<(ClientLogRow & { projectId: string | null })[]>([]);
let clientLogId = 0;
const clientLogRows = computed<ClientLogRow[]>(() => {
  const allowed = new Set(projects.value.map(p => p.id));
  const canShow = (id: string | null) => (!id || allowed.has(id)) && (!selected.value || !id || id === selected.value);
  const audit = logs.value.filter(log => canShow(log.project_id) && (!selected.value || log.project_id === selected.value) && (log.project_id || user.value?.role === "admin")).map(log => ({
    id: `audit-${log.id}`, timestamp: Date.parse(log.timestamp), source: "操作" as const, level: "info" as const,
    message: `${log.actor_name} · ${projects.value.find(p => p.id === log.project_id)?.project_name || '系统'} · ${log.message}${log.object ? ' · ' + log.object : ''}`,
  }));
  return [...audit, ...clientLogs.value.filter(log => canShow(log.projectId))].sort((a, b) => a.timestamp - b.timestamp || a.id.localeCompare(b.id)).slice(-500);
});
const resourceLogRows = computed<ClientLogRow[]>(() => {
  const allowed = new Set(projects.value.map(p => p.id));
  return resourceLogs.value.filter(log => (log.project_id ? allowed.has(log.project_id) : user.value?.role === "admin") && (!selected.value || log.project_id === selected.value)).map(log => ({
    id: `request-${log.id}`, timestamp: Date.parse(log.timestamp), source: "资源" as const, httpStatus: log.status,
    level: log.status >= 400 || log.outcome !== "completed" ? "error" as const : "success" as const,
    message: `${log.method} ${log.path} · ${log.message}`,
  })).sort((a, b) => a.timestamp - b.timestamp || a.id.localeCompare(b.id)).slice(-500);
});
function addClientLog(text: string, level: ClientLogRow["level"] = "info", global = false) {
  if (!props.native || !user.value || !text) return;
  const context = !global && project.value ? `${project.value.project_name} / ${projectVersion.value} / ${platform.value} · ` : "";
  clientLogs.value.push({ id: `client-${++clientLogId}`, timestamp: Date.now(), source: "客户端", level, projectId: global ? null : selected.value || null, message: (context + text).slice(0, 4000) });
  if (clientLogs.value.length > 500) clientLogs.value = clientLogs.value.slice(-500);
}
const tablePage = ref(1);
const panel = ref("");
const form = ref({
  project_name: "",
  project_version: "v1",
  package_name: "DefaultPackage",
  platforms: ["Android"],
});
const baseline = ref("");
const formRevision = ref(0);
const draftBaseline = ref("");
const settingsConflicts = ref<
  {
    key: "project_name" | "package_name" | "platforms";
    label: string;
    latest: string | string[];
  }[]
>([]);
const newVersion = ref("");
const newPlatform = ref("");
const renameVersion = ref("");
const memberName = ref("");
const members = ref<{
  owner: { id: string; username: string };
  members: { id: string; username: string }[];
}>();
const files = ref<FileEntry[]>([]);
const fileVersion = ref("");
const fileQuery = ref("");
const filePage = ref(1);
const uploadFiles = ref<File[]>([]);
const uploadVersion = ref("");
const uploadDone = ref("");
const directory = ref("");
const localVersions = ref<VersionEntry[]>([]);
const incremental = ref(true);
const uploadNative = ref(false);
const uploadPreparing = ref(false);
const autoPublish = ref(false);
const uploadPhase = ref("");
const uploadDiff = ref<UploadDiff | null>(null);
const diffLoading = ref(false);
const diffError = ref("");
const diffOpen = ref(false);
const diffBase = ref("");
let diffGeneration = 0;
const currentPassword = ref("");
const newPassword = ref("");
const repeatPassword = ref("");
const protocol = ref({
  tls_configured: false,
  http_enabled: true,
  https_enabled: true,
});
const confirm = ref<{
  title: string;
  message: string;
  label: string;
  action: () => Promise<void>;
  danger?: boolean;
}>();
const confirmOpen = ref(false);
let connection: LogConnection | null = null;
let reconnect: ReturnType<typeof setTimeout> | undefined;
let noticeTimer: ReturnType<typeof setTimeout> | undefined;
let backgroundTimer: ReturnType<typeof setTimeout> | undefined;
let pollTimer: ReturnType<typeof setInterval> | undefined;
let backgroundRunning = false;
let reconnectDelay = 2000;
let alive = true;
let logGeneration = 0;
let loadGeneration = 0;
const platforms = ["Android", "iOS", "Windows", "MacOS", "Linux", "WebGL"];
const project = computed(() =>
  projects.value.find((p) => p.id === selected.value),
);
const settings = computed(
  () =>
    project.value?.project_versions.find((v) => v.name === projectVersion.value)
      ?.platform_settings[platform.value],
);
const resourceUrl = computed(() =>
  project.value
    ? api.resourceUrl(project.value, projectVersion.value, platform.value)
    : "",
);
const filteredProjects = computed(() =>
  projects.value.filter(
    (p) =>
      (scope.value === "all" ||
        (scope.value === "mine"
          ? p.owner_id === user.value?.id
          : p.shared_with_me)) &&
      p.project_name.toLowerCase().includes(query.value.toLowerCase()),
  ),
);
const visibleVersions = computed(() =>
  versions.value.slice((tablePage.value - 1) * 50, tablePage.value * 50),
);
const filteredFiles = computed(() =>
  files.value.filter((f) =>
    f.name.toLowerCase().includes(fileQuery.value.toLowerCase()),
  ),
);
const visibleFiles = computed(() =>
  filteredFiles.value.slice((filePage.value - 1) * 100, filePage.value * 100),
);
const uploadSize = computed(() =>
  uploadFiles.value.reduce((n, f) => n + f.size, 0),
);
const nativeUploadExists = computed(() => uploadNative.value && !!uploadVersion.value && versions.value.some(v => v.version === uploadVersion.value));
const diffGroups = computed(() => uploadDiff.value ? [
  { key: "modified", label: "已修改", files: uploadDiff.value.modified, tone: "warning" },
  { key: "added", label: "新增", files: uploadDiff.value.added, tone: "success" },
  { key: "unchanged", label: "未变化", files: uploadDiff.value.unchanged, tone: "ui-muted" },
  { key: "removed", label: "旧版有，本地已移除", files: uploadDiff.value.removed, tone: "danger" },
] : []);
const savedBytes = computed(() => uploadDiff.value ? uploadDiff.value.totalSize - uploadDiff.value.uploadSize : 0);
const savedPercent = computed(() => uploadDiff.value?.totalSize ? Math.round(savedBytes.value / uploadDiff.value.totalSize * 100) : 0);
function preferenceKey(kind: string, ...parts: string[]) {
  return "tengine_client_" + JSON.stringify([api.getBaseUrl() || location.origin, user.value?.id, kind, ...parts]);
}
function readPreference<T>(key: string, fallback: T): T {
  try { return JSON.parse(localStorage.getItem(key) || "null") ?? fallback; }
  catch { return fallback; }
}
function savePreference(key: string, value: unknown) {
  try { localStorage.setItem(key, JSON.stringify(value)); }
  catch { error.value = "客户端配置保存失败，请检查可用空间；当前操作仍可继续。"; }
}
function setPublishMode(automatic: boolean) {
  autoPublish.value = automatic;
  savePreference(preferenceKey("autoPublish", selected.value, projectVersion.value, platform.value), automatic);
}
function createProjectVersion() {
  newVersion.value = "";
  renameVersion.value = "";
  panel.value = "version";
  dialogError.value = "";
}
function selectProjectVersion(name: string) {
  if (busy.value || projectVersion.value === name) return;
  projectVersion.value = name;
  void changeContext();
}
function toggleFormPlatform(name: string) {
  form.value.platforms = form.value.platforms.includes(name)
    ? form.value.platforms.filter((value) => value !== name)
    : [...form.value.platforms, name];
}
const pageTitle = computed(() =>
  project.value && page.value === "projects"
    ? project.value.project_name
    : {
        projects: "项目",
        logs: "操作记录",
        users: "用户管理",
        system: "系统设置",
      }[page.value] || "项目",
);
function message(e: unknown) {
  return e instanceof Error ? e.message : String(e);
}
function feedback(text: string) {
  addClientLog(text, "success");
  notice.value = text;
  clearTimeout(noticeTimer);
  noticeTimer = setTimeout(() => (notice.value = ""), 6000);
}
function writeHash() {
  const q = new URLSearchParams({ page: page.value });
  if (selected.value) {
    q.set("project", selected.value);
    q.set("version", projectVersion.value);
    q.set("platform", platform.value);
    if (user.value) savePreference(preferenceKey("context", selected.value), { version: projectVersion.value, platform: platform.value });
  }
  history.replaceState(null, "", "#" + q);
}
function restoreHash() {
  const q = new URLSearchParams(location.hash.slice(1));
  page.value = ["projects", "logs", "users", "system"].includes(
    q.get("page") || "",
  )
    ? q.get("page")!
    : "projects";
  selected.value = q.get("project") || "";
  projectVersion.value = q.get("version") || "";
  platform.value = q.get("platform") || "Android";
}
function selectContext() {
  if (!project.value) {
    selected.value = "";
    versions.value = [];
    return;
  }
  if (
    !project.value.project_versions.some((v) => v.name === projectVersion.value)
  )
    projectVersion.value = project.value.project_versions[0]?.name || "";
  if (!project.value.platforms.includes(platform.value))
    platform.value = project.value.platforms[0] || "";
}
async function loadProjects() {
  applyProjects(await api.listProjects());
}
function applyProjects(latest: ProjectConfig[]) {
  projects.value = latest;
  clientLogs.value = clientLogs.value.filter(log => !log.projectId || latest.some(p => p.id === log.projectId));
  if (selected.value && !projects.value.some((p) => p.id === selected.value))
    error.value =
      "项目已被删除，或共享权限已被移除。请联系项目所有者，或选择其他项目。";
  selectContext();
}
async function loadVersions(silent = false) {
  const p = project.value;
  const version = projectVersion.value;
  const plat = platform.value;
  const generation = ++loadGeneration;
  if (!p || !version) {
    versions.value = [];
    return;
  }
  if (!silent) loading.value = true;
  try {
    const result = await api.request<VersionEntry[]>(
      `${resourcePath(p, version)}/versions?platform=${encodeURIComponent(plat)}`,
    );
    if (generation === loadGeneration) {
      versions.value = result;
      tablePage.value = Math.min(
        tablePage.value,
        Math.max(1, Math.ceil(result.length / 50)),
      );
    }
  } finally {
    if (generation === loadGeneration) loading.value = false;
  }
}
async function refresh(silent = false) {
  await loadProjects();
  if (project.value) await loadVersions(silent);
  writeHash();
}
async function refreshAfterWrite() {
  try {
    await refresh(true);
  } catch (e) {
    error.value = `操作已完成，但最新数据读取失败。请重试刷新。${message(e)}`;
  }
}
async function expireSession() {
  await disconnect();
  user.value = null;
  api.clear();
  panel.value = "";
  confirmOpen.value = false;
  projects.value = [];
  logs.value = [];
  error.value = "登录已失效，请重新登录。";
}
async function run(action: () => Promise<void>, inDialog = false) {
  if (busy.value) return;
  busy.value = true;
  if (inDialog) dialogError.value = "";
  else error.value = "";
  try {
    await action();
  } catch (e) {
    if (e instanceof ApiError && e.status === 401 && user.value) {
      await expireSession();
    } else if (inDialog && (panel.value || confirmOpen.value))
      dialogError.value = message(e);
    else error.value = message(e);
  } finally {
    busy.value = false;
  }
}
async function selectProject(id: string) {
  if (busy.value) return;
  uploadDone.value = "";
  versions.value = [];
  selected.value = id;
  const saved = readPreference(preferenceKey("context", id), { version: "", platform: "Android" });
  projectVersion.value = saved.version;
  platform.value = saved.platform;
  page.value = "projects";
  selectContext();
  writeHash();
  tablePage.value = 1;
  error.value = "";
  try {
    await loadVersions();
  } catch (e) {
    error.value = message(e);
  }
}
async function changeContext() {
  tablePage.value = 1;
  uploadDone.value = "";
  versions.value = [];
  writeHash();
  try {
    await loadVersions();
  } catch (e) {
    error.value = message(e);
  }
}
async function navigate(next: string) {
  if (busy.value) return;
  ++loadGeneration;
  loading.value = false;
  uploadDone.value = "";
  versions.value = [];
  page.value = next;
  selected.value = "";
  navOpen.value = false;
  query.value = "";
  writeHash();
  await run(async () => {
    if (next === "logs")
      logs.value = await api.request<LogEntry[]>("/api/audit");
    if (next === "system")
      protocol.value = await api.request("/api/settings/protocol-access");
  });
}
async function login() {
  await run(async () => {
    if (api.client === "desktop") {
      const url = new URL(serverUrl.value);
      if (
        !["http:", "https:"].includes(url.protocol) ||
        url.username ||
        url.password
      )
        throw new Error("请输入有效的 HTTP 或 HTTPS 服务地址");
      api.setBaseUrl(serverUrl.value);
    }
    user.value = await api.login(username.value, password.value);
    localStorage.setItem("tengine_username", username.value);
    if (api.client === "desktop")
      localStorage.setItem("tengine_server_url", serverUrl.value);
    currentPassword.value = password.value;
    password.value = "";
    if (!user.value.must_change_password) {
      currentPassword.value = "";
      await start();
    }
  });
}
async function desktopLogin(details: ConnectionLogin) {
  serverUrl.value = details.url;
  username.value = details.username;
  password.value = details.password;
  connectionId.value = details.connectionId;
  await login();
}
async function logout() {
  await run(async () => {
    await api.logout();
    await disconnect();
    user.value = null;
    projects.value = [];
    logs.value = [];
    selected.value = "";
    password.value = "";
    panel.value = "";
    currentPassword.value = "";
    newPassword.value = "";
    repeatPassword.value = "";
  });
}
async function start() {
  await loadProjects();
  if (page.value === "users" || page.value === "system") {
    if (!user.value?.role || user.value.role !== "admin" || props.native)
      page.value = "projects";
  }
  if (project.value) await loadVersions();
  if (page.value === "logs") logs.value = await api.request("/api/audit");
  if (page.value === "system")
    protocol.value = await api.request("/api/settings/protocol-access");
  void connectLogs();
}
async function disconnect() {
  logGeneration++;
  loadGeneration++;
  loading.value = false;
  clearTimeout(reconnect);
  clearTimeout(backgroundTimer);
  if (connection) {
    const previous = connection;
    connection = null;
    try {
      await previous.close();
    } catch {
      /* The connection may already be gone. */
    }
  }
}
function queueBackgroundRefresh() {
  clearTimeout(backgroundTimer);
  backgroundTimer = setTimeout(refreshInBackground, 150);
}
async function refreshInBackground() {
  if (!alive || !user.value || user.value.must_change_password) return;
  if (busy.value || backgroundRunning) {
    queueBackgroundRefresh();
    return;
  }
  backgroundRunning = true;
  const userId = user.value.id;
  try {
    const current = await api.me();
    if (!alive || user.value?.id !== userId) return;
    const latestProjects = await api.listProjects();
    if (!alive || user.value?.id !== userId) return;
    if (busy.value) {
      queueBackgroundRefresh();
      return;
    }
    user.value = current;
    applyProjects(latestProjects);
    if (project.value) await loadVersions(true);
    writeHash();
    if (
      !project.value &&
      [
        "settings",
        "members",
        "files",
        "version",
        "upload",
      ].includes(panel.value)
    ) {
      panel.value = "";
      confirmOpen.value = false;
    }
    const recent = await api.request<LogEntry[]>("/api/audit");
    if (alive && user.value?.id === userId) {
      // Keep loaded history only for projects the current account can still access.
      const allowed = new Set(projects.value.map((p) => p.id));
      const retained = logs.value.filter(
        (log) =>
          current.role === "admin" ||
          (log.project_id && allowed.has(log.project_id)),
      );
      logs.value = Array.from(
        new Map([...retained, ...recent].map((log) => [log.id, log])).values(),
      )
        .sort((a, b) => b.id - a.id)
        .slice(0, 2000);
      resourceLogs.value = resourceLogs.value.filter(log => current.role === 'admin' || !!log.project_id && allowed.has(log.project_id));
      if (resourceLogsAvailable.value !== false) {
        try {
          const recentResources = await api.request<ResourceRequestLog[]>('/api/resource-logs');
          if (alive && user.value?.id === userId) {
            resourceLogsAvailable.value = true;
            resourceLogs.value = Array.from(new Map([...resourceLogs.value, ...recentResources].map(log => [log.id, log])).values()).sort((a, b) => Date.parse(a.timestamp) - Date.parse(b.timestamp)).slice(-500);
          }
        } catch (e) {
          if (e instanceof ApiError && e.status === 404) resourceLogsAvailable.value = false;
          else throw e;
        }
      }
    }
  } catch (e) {
    if (e instanceof ApiError && e.status === 401 && user.value?.id === userId)
      await expireSession();
    // Background failures do not clear inputs or replace foreground errors.
  } finally {
    backgroundRunning = false;
  }
}
function scheduleReconnect(generation: number) {
  if (!alive || !user.value || generation !== logGeneration) return;
  logState.value = "retrying";
  clearTimeout(reconnect);
  reconnect = setTimeout(() => {
    void connectLogs();
  }, reconnectDelay);
  reconnectDelay = Math.min(reconnectDelay * 2, 30000);
}
async function connectLogs() {
  if (!alive || !user.value || user.value.must_change_password) return;
  const generation = ++logGeneration;
  try {
    const opened = await api.connectLogs(
      (log) => {
        if (!alive || generation !== logGeneration) return;
        if ("type" in log) {
          if (log.type === "resource_request") {
            resourceLogsAvailable.value = true;
            resourceLogs.value = [...resourceLogs.value.filter(entry => entry.id !== log.id), log].slice(-500);
            return;
          }
          logs.value = [];
          clientLogs.value = [];
          resourceLogs.value = [];
          queueBackgroundRefresh();
          return;
        }
        logs.value = [
          log,
          ...logs.value.filter((entry) => entry.id !== log.id),
        ].slice(0, 2000);
        queueBackgroundRefresh();
      },
      () => {
        if (generation !== logGeneration) return;
        connection = null;
        scheduleReconnect(generation);
      },
    );
    if (alive && generation === logGeneration) {
      connection = opened;
      logState.value = "live";
      reconnectDelay = 2000;
      queueBackgroundRefresh();
    } else await opened.close();
  } catch (e) {
    if (!alive || generation !== logGeneration) return;
    if (e instanceof ApiError && e.status === 401) await expireSession();
    else scheduleReconnect(generation);
  }
}
function panelDraft() {
  if (["create", "settings"].includes(panel.value))
    return JSON.stringify(form.value);
  if (panel.value === "version") return newVersion.value;
  if (panel.value === "platform") return newPlatform.value;
  if (panel.value === "members") return memberName.value;
  if (panel.value === "password")
    return JSON.stringify([
      currentPassword.value,
      newPassword.value,
      repeatPassword.value,
    ]);
  if (panel.value === "upload")
    return JSON.stringify([
      uploadFiles.value.map((file) => [
        file.name,
        file.size,
        file.lastModified,
      ]),
      uploadVersion.value,
      directory.value,
    ]);
  return "";
}
watch(
  panel,
  () => {
    draftBaseline.value = panelDraft();
  },
  { flush: "post" },
);
function leavePanel(action: () => void) {
  if (busy.value) return;
  if (panelDraft() !== draftBaseline.value) {
    ask(
      "放弃未保存的修改？",
      "当前输入尚未保存，继续后将丢弃这些输入。",
      "放弃修改",
      async () => {
        action();
      },
      true,
    );
    return;
  }
  action();
  dialogError.value = "";
}
function closePanel() {
  leavePanel(() => {
    panel.value = "";
  });
}
function editProjectVersion() {
  leavePanel(() => {
    renameVersion.value = projectVersion.value;
    newVersion.value = projectVersion.value;
    panel.value = "version";
  });
}
function createProject() {
  form.value = {
    project_name: "",
    project_version: "v1",
    package_name: "DefaultPackage",
    platforms: ["Android"],
  };
  panel.value = "create";
  dialogError.value = "";
}
function createPlatform() {
  newPlatform.value = platforms.find(p => !project.value?.platforms.includes(p)) || "";
  dialogError.value = "";
  panel.value = "platform";
}
async function selectPlatform(name: string) {
  if (busy.value || name === platform.value) return;
  platform.value = name;
  await changeContext();
}
async function addPlatform() {
  const id = project.value!.id;
  await run(async () => {
    const name = newPlatform.value.trim();
    if (!name) throw new Error("请输入平台名称。");
    const latest = await api.project(id);
    if (latest.platforms.includes(name)) throw new Error("此平台已存在，请直接选择对应标签。");
    await api.request(`/api/projects/${id}`, {
      method: "PUT",
      headers: { "If-Match": String(latest.revision) },
      body: JSON.stringify({project_name: latest.project_name, package_name: latest.package_name, project_version: projectVersion.value, platforms: [...latest.platforms, name]}),
    });
    platform.value = name;
    panel.value = "";
    await refreshAfterWrite();
    writeHash();
    feedback(`已添加 ${name} 平台，可以上传资源。`);
  }, true);
}
function openSettings() {
  if (!project.value) return;
  form.value = {
    project_name: project.value.project_name,
    project_version: projectVersion.value,
    package_name: project.value.package_name,
    platforms: [...project.value.platforms],
  };
  baseline.value = JSON.stringify(form.value);
  formRevision.value = project.value.revision;
  settingsConflicts.value = [];
  panel.value = "settings";
  dialogError.value = "";
}
async function saveProject() {
  await run(async () => {
    if (settingsConflicts.value.length && panel.value === "settings")
      throw new Error("请先逐项确认冲突字段，再保存。");
    if (!form.value.project_name.trim()) throw new Error("请输入项目名称");
    if (!form.value.platforms.length) throw new Error("请至少选择一个平台");
    const p = await api.request<ProjectConfig>(
      panel.value === "create"
        ? "/api/projects"
        : `/api/projects/${project.value!.id}`,
      {
        method: panel.value === "create" ? "POST" : "PUT",
        body: JSON.stringify(form.value),
        headers:
          panel.value === "settings"
            ? { "If-Match": String(formRevision.value) }
            : {},
      },
    );
    panel.value = "";
    selected.value = p.id;
    page.value = "projects";
    projectVersion.value = form.value.project_version;
    await refreshAfterWrite();
    feedback("项目已保存，可以上传资源。");
  }, true);
}
async function reloadSettings() {
  const latest = await api.project(project.value!.id);
  const original = JSON.parse(baseline.value);
  const latestForm = {
    project_name: latest.project_name,
    package_name: latest.package_name,
    platforms: [...latest.platforms],
    project_version: form.value.project_version,
  };
  settingsConflicts.value = [];
  for (const [key, label] of [
    ["project_name", "项目名称"],
    ["package_name", "资源包名"],
    ["platforms", "目标平台"],
  ] as const) {
    const mine = JSON.stringify(form.value[key]);
    const before = JSON.stringify(original[key]);
    const remote = JSON.stringify(latestForm[key]);
    if (mine === before) form.value = { ...form.value, [key]: latestForm[key] };
    else if (remote !== before && remote !== mine)
      settingsConflicts.value.push({ key, label, latest: latestForm[key] });
  }
  baseline.value = JSON.stringify(latestForm);
  draftBaseline.value = baseline.value;
  formRevision.value = latest.revision;
  dialogError.value = settingsConflicts.value.length
    ? "存在同字段修改，请逐项选择要保留的内容。"
    : "已合并最新配置，保留你的修改。请核对后保存。";
}
function resolveSetting(
  key: "project_name" | "package_name" | "platforms",
  useLatest: boolean,
) {
  const conflict = settingsConflicts.value.find((item) => item.key === key);
  if (conflict && useLatest)
    form.value = { ...form.value, [key]: conflict.latest };
  settingsConflicts.value = settingsConflicts.value.filter(
    (item) => item.key !== key,
  );
}
function ask(
  title: string,
  text: string,
  label: string,
  action: () => Promise<void>,
  danger = false,
) {
  confirm.value = { title, message: text, label, action, danger };
  confirmOpen.value = true;
  dialogError.value = "";
}
async function accept() {
  await run(async () => {
    await confirm.value!.action();
    confirmOpen.value = false;
  }, true);
}
async function release(version: string, unpublish = false) {
  const p = project.value!;
  const pv = projectVersion.value;
  const plat = platform.value;
  ask(
    unpublish ? "取消当前发布？" : "发布资源版本",
    `${p.project_name} / ${pv} / ${plat}：${settings.value?.active_bundle || "尚未发布"} → ${unpublish ? "取消发布，客户端下载将不可用" : version}。`,
    unpublish ? "取消发布" : "确认发布",
    async () => {
      await api.mutate(
        `${resourcePath(p, pv)}/versions/${encodeURIComponent(version)}/activate?platform=${encodeURIComponent(plat)}`,
        "PUT",
        { unpublish },
        p.revision,
      );
      await refreshAfterWrite();
      uploadDone.value = "";
      feedback(unpublish ? "已取消发布" : "发布成功，客户端将读取此版本。");
    },
    unpublish,
  );
}
async function toggleAccess() {
  const p = project.value!;
  const pv = projectVersion.value;
  const plat = platform.value;
  const enabled = !settings.value?.access_enabled;
  ask(
    enabled ? "允许客户端下载？" : "关闭客户端下载？",
    `${p.project_name} / ${projectVersion.value} / ${platform.value}。${enabled ? "将恢复该平台已发布资源的下载。" : "该平台的客户端将无法下载资源。"}`,
    enabled ? "开启下载" : "关闭下载",
    async () => {
      await api.mutate(
        `${resourcePath(p, pv)}/platforms/${encodeURIComponent(plat)}/access`,
        "PUT",
        { enabled },
        p.revision,
      );
      await refreshAfterWrite();
    },
    !enabled,
  );
}
function deleteProject() {
  const p = project.value!;
  ask(
    "删除整个项目？",
    `将永久删除“${p.project_name}”的全部版本及资源，所有共享成员都会失去此项目。`,
    "永久删除项目",
    async () => {
      await api.mutate(
        `/api/projects/${p.id}`,
        "DELETE",
        undefined,
        p.revision,
      );
      panel.value = "";
      selected.value = "";
      await refreshAfterWrite();
    },
    true,
  );
}
function deleteBundle(version: string) {
  const p = project.value!;
  const pv = projectVersion.value;
  const plat = platform.value;
  ask(
    "删除资源版本？",
    `将永久删除 ${projectVersion.value} / ${platform.value} / ${version} 的全部文件。`,
    "删除资源版本",
    async () => {
      await api.mutate(
        `${resourcePath(p, pv)}/versions/${encodeURIComponent(version)}?platform=${encodeURIComponent(plat)}`,
        "DELETE",
        undefined,
        p.revision,
      );
      await refreshAfterWrite();
    },
    true,
  );
}
async function copy(value: string) {
  try {
    await navigator.clipboard.writeText(value);
    feedback("已复制到剪贴板");
  } catch {
    error.value = `复制失败，请手动复制：${value}`;
  }
}
async function openFiles(version: string) {
  panel.value = "files";
  fileVersion.value = version;
  fileQuery.value = "";
  filePage.value = 1;
  files.value = [];
  dialogError.value = "";
  await run(async () => {
    files.value = await api.request(
      `${resourcePath(project.value!, projectVersion.value)}/files?platform=${encodeURIComponent(platform.value)}&version=${encodeURIComponent(version)}`,
    );
  }, true);
}
async function previewFile(name: string) {
  const url = api.resourceUrl(
    project.value!,
    projectVersion.value,
    platform.value,
    name,
  );
  if (props.native) await props.native.openUrl(url);
  else window.open(url, "_blank", "noopener,noreferrer");
}
async function openMembers() {
  panel.value = "members";
  memberName.value = "";
  dialogError.value = "";
  members.value = undefined;
  await run(async () => {
    members.value = await api.request(
      `/api/projects/${project.value!.id}/members`,
    );
  }, true);
}
async function memberAction(transfer = false) {
  await run(async () => {
    const p = project.value!;
    const target = await api.request<{ id: string; username: string }>(
      `/api/users/lookup?username=${encodeURIComponent(memberName.value)}`,
    );
    if (transfer) {
      ask(
        "转交项目？",
        `将“${project.value!.project_name}”交给 ${target.username}，你将成为共享成员。`,
        "转交项目",
        async () => {
          await api.mutate(
            `/api/projects/${p.id}/owner`,
            "PUT",
            {
              user_id: target.id,
            },
            p.revision,
          );
          panel.value = "";
          await refreshAfterWrite();
        },
      );
    } else {
      await api.mutate(`/api/projects/${project.value!.id}/members`, "POST", {
        user_id: target.id,
      });
      await refreshAfterWrite();
      members.value = await api.request(
        `/api/projects/${project.value!.id}/members`,
      );
      memberName.value = "";
    }
  }, true);
}
function removeMember(id: string, name: string) {
  const p = project.value!;
  ask(
    "移除共享成员？",
    `${name} 将立即失去此项目的管理权限。`,
    "移除成员",
    async () => {
      await api.mutate(
        `/api/projects/${p.id}/members/${id}`,
        "DELETE",
        undefined,
        p.revision,
      );
      await refreshAfterWrite();
      members.value = await api.request(
        `/api/projects/${project.value!.id}/members`,
      );
    },
    true,
  );
}
function validateName() {
  if (!newVersion.value.trim()) throw new Error("请输入项目版本名称");
}
async function versionSubmit() {
  await run(async () => {
    validateName();
    const path =
      `/api/projects/${project.value!.id}/project-versions` +
      (renameVersion.value
        ? `/${encodeURIComponent(renameVersion.value)}`
        : "");
    await api.mutate(path, renameVersion.value ? "PUT" : "POST", {
      name: newVersion.value.trim(),
    });
    projectVersion.value = newVersion.value.trim();
    panel.value = "";
    await refreshAfterWrite();
  }, true);
}
function deleteProjectVersion() {
  const p = project.value!;
  const name = projectVersion.value;
  ask(
    "删除项目版本？",
    `永久删除 ${name} 下的全部平台资源。请先取消所有平台的发布。`,
    "删除项目版本",
    async () => {
      await api.mutate(resourcePath(p, name), "DELETE", undefined, p.revision);
      panel.value = "";
      await refreshAfterWrite();
    },
    true,
  );
}
async function openUpload() {
  if (busy.value) return;
  uploadPreparing.value = true;
  panel.value = "upload";
  uploadFiles.value = [];
  uploadVersion.value = "";
  localVersions.value = [];
  directory.value = props.native ? readPreference(preferenceKey("bundlesDir", selected.value), "") : "";
  uploadNative.value = !!props.native;
  diffBase.value = "";
  incremental.value = false;
  diffOpen.value = false;
  dialogError.value = "";
  uploadPhase.value = "正在读取最新的本地与服务器版本…";
  await run(async () => {
    await Promise.all([loadVersions(true), directory.value ? loadLocalVersions() : Promise.resolve()]);
    diffBase.value = versions.value[0]?.version || "";
    incremental.value = !!diffBase.value;
  }, true);
  uploadPreparing.value = false;
  uploadPhase.value = "";
  draftBaseline.value = panelDraft();
}
async function loadLocalVersions() {
  const found = await props.native!.versions(directory.value, project.value!, platform.value);
  localVersions.value = found.sort((a, b) => b.modified_timestamp - a.modified_timestamp || b.version.localeCompare(a.version, undefined, {numeric: true}));
  uploadVersion.value = localVersions.value[0]?.version || "";
  if (!uploadVersion.value) throw new Error(`未找到 ${platform.value}/${project.value!.package_name} 下的资源版本，请更换目录或先构建资源。`);
}
async function chooseDirectory() {
  uploadPhase.value = "正在选择并读取本地构建目录…";
  await run(async () => {
    const dir = await props.native!.chooseDirectory();
    if (!dir) return;
    directory.value = dir;
    savePreference(preferenceKey("bundlesDir", selected.value), dir);
    await loadLocalVersions();
  }, true);
  uploadPhase.value = "";
}
async function rereadUpload() {
  uploadPreparing.value = true;
  uploadPhase.value = "正在重新读取本地与服务器版本…";
  await run(async () => {
    await Promise.all([loadVersions(true), loadLocalVersions()]);
    diffBase.value = versions.value[0]?.version || "";
    incremental.value = !!diffBase.value;
  }, true);
  uploadPreparing.value = false;
  uploadPhase.value = "";
}
async function computeUploadDiff() {
  const generation = ++diffGeneration;
  uploadDiff.value = null;
  diffError.value = "";
  diffLoading.value = false;
  if (uploadPreparing.value || panel.value !== "upload" || !uploadNative.value || !props.native || !directory.value || !uploadVersion.value || !project.value) return;
  const p = project.value;
  const pv = projectVersion.value;
  const plat = platform.value;
  const base = diffBase.value;
  diffLoading.value = true;
  try {
    const [local, remote] = await Promise.all([
      props.native.manifest(directory.value, p, plat, uploadVersion.value),
      base ? api.request<FileManifestEntry[]>(`${resourcePath(p, pv)}/manifest?platform=${encodeURIComponent(plat)}&version=${encodeURIComponent(base)}`) : Promise.resolve([]),
    ]);
    if (generation === diffGeneration) uploadDiff.value = compareManifests(local, remote, base || null);
  } catch (e) {
    if (generation === diffGeneration) diffError.value = `读取差异失败：${message(e)}。可重试或取消增量上传后完整上传。`;
  } finally {
    if (generation === diffGeneration) diffLoading.value = false;
  }
}
async function upload() {
  await run(async () => {
    const p = project.value!;
    const pv = projectVersion.value;
    const plat = platform.value;
    const automatic = autoPublish.value;
    const previousRelease = settings.value?.active_bundle || null;
    uploadPhase.value = "正在上传资源…";
    if (uploadNative.value && props.native) {
      if (nativeUploadExists.value) throw new Error("此资源版本已上传，请构建新版本或选择其他本地版本。");
      if (!uploadVersion.value || !directory.value)
        throw new Error("请选择本地资源目录和版本");
      await props.native.upload(
        directory.value,
        p,
        pv,
        plat,
        uploadVersion.value,
        diffBase.value || null,
        incremental.value,
      );
      uploadDone.value = uploadVersion.value;
    } else {
      if (!uploadFiles.value.length) throw new Error("请至少选择一个资源文件");
      const data = new FormData();
      data.append("platform", plat);
      data.append("version", uploadVersion.value);
      for (const f of uploadFiles.value) data.append("files", f);
      const result = await api.request<{ version: string }>(
        `${resourcePath(p, pv)}/upload`,
        { method: "POST", body: data },
      );
      uploadDone.value = result.version;
    }
    panel.value = "";
    if (automatic) {
      uploadPhase.value = "上传完成，正在发布…";
      try {
        const latest = await api.project(p.id);
        const currentRelease = latest.project_versions.find((version) => version.name === pv)?.platform_settings[plat]?.active_bundle || null;
        if (currentRelease !== previousRelease) throw new Error("其他成员已切换发布版本，请核对后手动发布。");
        await api.mutate(`${resourcePath(p, pv)}/versions/${encodeURIComponent(uploadDone.value)}/activate?platform=${encodeURIComponent(plat)}`, "PUT", {}, latest.revision);
        uploadDone.value = "";
        await refreshAfterWrite();
        feedback("上传完成并已自动发布，客户端将读取此版本。");
      } catch (e) {
        await refreshAfterWrite();
        error.value = `资源已上传，但自动发布失败：${message(e)}。资源已保留，可手动发布。`;
      }
      return;
    }
    await refreshAfterWrite();
    feedback("上传完成，尚未发布。确认无误后发布此版本。");
  }, true);
  uploadPhase.value = "";
}
async function changePassword() {
  await run(async () => {
    if (newPassword.value !== repeatPassword.value) {
      document.getElementById("repeat-password")?.focus();
      throw new Error("两次输入的新密码不一致");
    }
    await api.mutate("/api/auth/password", "PUT", {
      current_password: currentPassword.value,
      password: newPassword.value,
    });
    if (props.native?.credentials && connectionId.value) {
      try {
        await props.native.credentials.remove(connectionId.value);
        storeConnections(loadConnections().map(p => p.id === connectionId.value ? {...p, passwordStored: false} : p));
      } catch {
        error.value = "密码已修改，但本机旧密码未能清除；请在连接页重新输入新密码并保存。";
      }
    }
    await disconnect();
    user.value = null;
    api.clear();
    currentPassword.value = "";
    newPassword.value = "";
    repeatPassword.value = "";
    panel.value = "";
    feedback("密码已修改，请使用新密码重新登录。");
  }, true);
}
async function webAdmin() {
  await props.native?.openUrl(
    api.getBaseUrl() +
      "/#" +
      new URLSearchParams({ page: "projects", project: selected.value }),
  );
}
async function moreLogs() {
  const last = logs.value[logs.value.length - 1];
  if (!last) return;
  const older = await api.request<LogEntry[]>(`/api/audit?before=${last.id}`);
  logs.value.push(...older);
}
watch(fileQuery, () => (filePage.value = 1));
watch(() => user.value?.id, () => { clientLogs.value = []; resourceLogs.value = []; resourceLogsAvailable.value = null; }, { flush: "sync" });
watch(logState, state => addClientLog(state === "live" ? "实时连接已建立。" : state === "retrying" ? "实时连接中断，正在重连；操作记录将定时同步。" : "正在建立实时连接。", state === "retrying" ? "warning" : "info", true));
watch(uploadPhase, text => addClientLog(text));
watch([error, dialogError, diffError], (values, previous) => {
  values.forEach((text, i) => { if (text && text !== previous[i]) addClientLog(text, "error"); });
});
watch([selected, projectVersion, platform, () => user.value?.id], () => {
  autoPublish.value = readPreference(preferenceKey("autoPublish", selected.value, projectVersion.value, platform.value), false);
});
watch([panel, uploadNative, directory, uploadVersion, diffBase, uploadPreparing], () => {
  if (panel.value !== "upload") diffOpen.value = false;
  void computeUploadDiff();
});
onMounted(async () => {
  pollTimer = setInterval(() => {
    if (
      user.value &&
      !user.value.must_change_password &&
      logState.value !== "live"
    )
      queueBackgroundRefresh();
  }, 15000);
  localStorage.removeItem("tengine_web_login");
  restoreHash();
  if (api.client === "web") {
    try {
      user.value = await api.me();
      if (!user.value.must_change_password) await start();
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401))
        error.value = message(e);
    }
  }
  booting.value = false;
});
onBeforeUnmount(() => {
  alive = false;
  ++diffGeneration;
  void disconnect();
  clearTimeout(noticeTimer);
  clearInterval(pollTimer);
});
</script>
<template>
  <div class="ui-workspace">
    <div v-if="booting" class="ui-empty" role="status">正在连接工作台…</div>
    <DesktopConnections v-else-if="!user && native" :credentials="native.credentials" :busy="busy" :error="error" :notice="notice" @login="desktopLogin" />
    <form v-else-if="!user" class="ui-login" @submit.prevent="login">
      <div class="ui-brand"><Package :size="22" />TEngine</div>
      <p class="intro">登录资源工作台，继续上传与发布。</p>
      <div class="ui-form">
        <div v-if="native" class="ui-field">
          <label for="server-url">服务地址</label
          ><input
            id="server-url"
            v-model="serverUrl"
            type="url"
            required
            placeholder="https://resources.example.com"
          />
        </div>
        <div class="ui-field">
          <label for="login-name">用户名</label
          ><input
            id="login-name"
            v-model="username"
            name="username"
            autocomplete="username"
            required
            autofocus
          />
        </div>
        <div class="ui-field">
          <label for="login-password">密码</label
          ><input
            id="login-password"
            v-model="password"
            name="password"
            type="password"
            autocomplete="current-password"
            required
          />
        </div>
        <div v-if="error" class="ui-error" role="alert">
          <AlertCircle :size="16" />{{ error }}
        </div>
        <div v-if="notice" class="ui-notice" role="status">{{ notice }}</div>
        <button class="primary" :disabled="busy" type="submit">
          {{ busy ? "正在登录…" : "登录" }}
        </button>
        <p class="ui-muted">账号由管理员创建。忘记密码请联系管理员重置。</p>
        <button class="ui-action" type="button" @click="toggleTheme">
          <Sun v-if="theme === 'dark'" :size="16" /><Moon
            v-else
            :size="16"
          />切换{{ theme === "dark" ? "浅色" : "深色" }}
        </button>
      </div>
    </form>
    <template v-else>
      <nav
        class="ui-sidebar"
        :class="{ 'nav-open': navOpen }"
        id="workspace-nav"
        aria-label="主导航"
      >
        <div class="ui-brand"><Package :size="20" />TEngine</div>
        <button
          :class="{ active: page === 'projects' }"
          :aria-current="page === 'projects' ? 'page' : undefined"
          @click="navigate('projects')"
        >
          <Folder :size="16" />项目
        </button>
        <button :class="{ active: page === 'logs' }" @click="navigate('logs')">
          <History :size="16" />操作记录
        </button>
        <template v-if="user.role === 'admin' && !native"
          ><div class="nav-note">管理</div>
          <button
            :class="{ active: page === 'users' }"
            @click="navigate('users')"
          >
            <Users :size="16" />用户管理</button
          ><button
            :class="{ active: page === 'system' }"
            @click="navigate('system')"
          >
            <Settings :size="16" />系统设置
          </button></template
        >
        <button v-if="native" @click="webAdmin">
          <Users :size="16" />Web 管理后台
        </button>
        <div class="nav-bottom">
          <button @click="panel = 'help'">
            <CircleHelp :size="16" />使用说明
          </button>
          <div class="nav-note">
            {{ user.username }} ·
            {{ user.role === "admin" ? "管理员" : "普通用户" }}
          </div>
          <button
            @click="
              panel = 'password';
              dialogError = '';
            "
          >
            <UserRound :size="16" />修改密码</button
          ><button @click="toggleTheme">
            <Sun v-if="theme === 'dark'" :size="16" /><Moon
              v-else
              :size="16"
            />{{ theme === "dark" ? "浅色外观" : "深色外观" }}</button
          ><button :disabled="busy" @click="logout">
            <LogOut :size="16" />退出登录
          </button>
        </div>
      </nav>
      <main class="ui-main" id="main-content">
        <header class="ui-header">
          <button
            class="ui-icon ui-mobile-menu"
            aria-label="切换导航"
            :aria-expanded="navOpen"
            aria-controls="workspace-nav"
            @click="navOpen = !navOpen"
          >
            <Menu :size="18" /></button
          ><button
            v-if="project && page === 'projects'"
            class="ui-icon quiet"
            aria-label="返回项目列表"
            @click="navigate('projects')"
          >
            <ArrowLeft :size="16" />
          </button>
          <h1>{{ pageTitle }}</h1>
          <span v-if="project && page === 'projects'" class="context">{{
            project.owner_id === user.id
              ? "我的项目"
              : project.shared_with_me
                ? "共享协作"
                : "管理员访问"
          }}</span
          ><span class="ui-spacer" />
          <template v-if="project && page === 'projects'"
            ><button @click="native ? webAdmin() : openMembers()">
              <Users :size="14" /><span>共享成员</span></button
            ><button @click="openSettings">
              <Settings :size="14" /><span>项目设置</span>
            </button></template
          >
        </header>
        <div v-if="page === 'projects' && projects.length" class="ui-project-tabs" aria-label="项目标签">
          <button :class="{ active: !selected }" :aria-pressed="!selected" :disabled="busy" @click="navigate('projects')">项目列表</button>
          <button v-for="p in projects" :key="p.id" :class="{ active: selected === p.id }" :aria-pressed="selected === p.id" :title="p.project_name" :disabled="busy" @click="selectProject(p.id)">{{ p.project_name }}</button>
          <button class="ui-add-tab" :disabled="busy" @click="createProject"><Plus :size="14" />新建项目</button>
        </div>
        <div class="ui-content" :aria-busy="busy || loading">
          <div v-if="error" class="ui-error" role="alert">
            <AlertCircle :size="16" />{{ error
            }}<span class="ui-spacer" /><button
              :disabled="busy"
              @click="run(refresh)"
            >
              重试</button
            ><button class="ui-action" @click="error = ''">关闭</button>
          </div>
          <div v-if="notice" class="ui-notice" role="status">
            <Check :size="16" />{{ notice }}
          </div>
          <template v-if="page === 'projects' && !project">
            <div class="ui-toolbar">
              <SegmentedControl v-model="scope" label="项目范围" hide-label :options="[{value:'mine',label:'我的项目'},{value:'shared',label:'共享给我'},...(user.role === 'admin' ? [{value:'all',label:'全部项目'}] : [])]" />
              <span class="ui-spacer" /><button
                class="primary"
                @click="createProject"
              >
                <Plus :size="16" />创建项目
              </button>
            </div>
            <div class="ui-toolbar">
              <Search :size="16" /><input
                v-model="query"
                class="ui-search"
                aria-label="搜索项目"
                placeholder="搜索项目名称"
              /><span class="ui-muted"
                >{{ filteredProjects.length }} 个项目</span
              ><span class="ui-spacer" /><button
                class="ui-icon quiet"
                aria-label="刷新项目"
                :disabled="busy"
                @click="run(refresh)"
              >
                <RefreshCw :size="16" />
              </button>
            </div>
            <div v-if="filteredProjects.length" class="ui-table-wrap">
              <table class="ui-table">
                <thead>
                  <tr>
                    <th>项目名称</th>
                    <th>平台</th>
                    <th>项目版本</th>
                    <th class="ui-secondary-column">资源包</th>
                  </tr>
                </thead>
                <tbody>
                  <tr v-for="p in filteredProjects" :key="p.id" class="ui-project-row" tabindex="0" :aria-label="'打开项目 ' + p.project_name" @click="selectProject(p.id)" @keydown.enter.prevent="selectProject(p.id)" @keydown.space.prevent="selectProject(p.id)">
                    <td class="truncate ui-project-name" :title="p.project_name"><span>{{ p.project_name }}</span><ChevronRight :size="14" aria-hidden="true" /></td>
                    <td>{{ p.platforms.join(" · ") }}</td>
                    <td class="truncate ui-mono" :title="p.project_versions.map(v => v.name).join(' · ')">{{ p.project_versions.map(v => v.name).join(' · ') || '未创建' }}</td>
                    <td class="ui-secondary-column ui-mono">
                      {{ p.package_name }}
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
            <div v-else class="ui-empty">
              <Folder :size="24" />
              <p>
                {{
                  query
                    ? "没有匹配的项目，试试其他名称。"
                    : scope === "shared"
                      ? "尚未收到项目分享，请让所有者添加你的用户名。"
                      : "创建第一个项目，然后上传资源。"
                }}
              </p>
              <button
                v-if="!query && scope !== 'shared'"
                @click="createProject"
              >
                创建项目
              </button>
            </div>
          </template>
          <template v-else-if="page === 'projects' && project">
            <div class="ui-toolbar ui-context-toolbar">
            <div class="ui-version-tabs" aria-label="项目版本标签">
              <span class="ui-muted">项目版本</span>
              <button v-for="v in project.project_versions" :key="v.name" :class="{ active: projectVersion === v.name }" :aria-pressed="projectVersion === v.name" :disabled="busy" @click="selectProjectVersion(v.name)">{{ v.name }}</button>
              <button class="ui-add-tab" :disabled="busy" @click="createProjectVersion"><Plus :size="14" />新增版本</button>
            </div>
              <span class="ui-spacer" /><button
                class="primary"
                :disabled="busy || !projectVersion"
                @click="openUpload"
              >
                <Upload :size="16" />上传资源
              </button>
            </div>
            <div class="ui-release-bar">
              <div class="ui-platform-tabs" aria-label="平台标签">
                <span class="ui-muted">平台</span>
                <button v-for="p in project.platforms" :key="p" :class="{ active: platform === p }" :aria-pressed="platform === p" :disabled="busy" :title="p" @click="selectPlatform(p)">{{ p }}</button>
                <button class="ui-add-tab" :disabled="busy || project.platforms.length >= 16" @click="createPlatform"><Plus :size="14" />新增平台</button>
              </div>
              <div class="ui-release-controls">
              <span class="ui-muted ui-release-hint">{{ autoPublish ? '上传成功后自动发布' : '上传后待发布' }}</span>
              <SegmentedControl :model-value="autoPublish ? 'auto' : 'manual'" label="发布方式" :options="[{value:'manual',label:'手动发布'},{value:'auto',label:'自动发布'}]" :disabled="busy" @update:model-value="setPublishMode($event === 'auto')" title="仅当前客户端；按当前账号、项目版本和平台保存" />
              </div>
            </div>
            <div class="ui-status">
              <span class="ui-release-state"><StatusBadge :tone="settings?.active_bundle ? 'success' : 'warning'">{{ settings?.active_bundle ? '当前发布' : '尚未发布' }}</StatusBadge><strong class="ui-mono ui-key-value">{{ settings?.active_bundle || '—' }}</strong></span>
              <ToggleSwitch v-if="projectVersion" :model-value="!!settings?.access_enabled" label="允许客户端下载" :disabled="busy" @update:model-value="toggleAccess" />
              <span class="ui-spacer" /><button
                class="ui-action"
                :disabled="!settings?.active_bundle"
                :title="resourceUrl"
                @click="copy(resourceUrl)"
              >
                <Copy :size="14" />复制地址
              </button>
            </div>
            <div v-if="uploadDone || (busy && uploadPhase)" class="ui-upload-status">
              <span v-if="busy && uploadPhase" role="status">{{ uploadPhase }}</span>
              <span v-else>{{ uploadDone }} 已上传，尚未发布</span>
              <span class="ui-spacer" />
              <button v-if="uploadDone"
                :disabled="busy"
                @click="release(uploadDone)"
              >
                发布此版本
              </button>
            </div>
            <div v-if="!projectVersion" class="ui-empty">
              <p>先添加项目版本，再上传资源。</p>
              <button
                @click="
                  newVersion = 'v1';
                  renameVersion = '';
                  panel = 'version';
                "
              >
                添加项目版本
              </button>
            </div>
            <div v-else class="ui-table-wrap">
              <table class="ui-table">
                <thead>
                  <tr>
                    <th>资源版本</th>
                    <th>状态</th>
                    <th class="num">文件</th>
                    <th class="num">大小</th>
                    <th class="ui-secondary-column">更新时间</th>
                    <th>操作</th>
                  </tr>
                </thead>
                <tbody>
                  <template v-if="loading && !versions.length"
                    ><tr v-for="n in 8" :key="n">
                      <td
                        v-for="k in 6"
                        :key="k"
                        :class="k === 5 ? 'ui-secondary-column' : ''"
                      >
                        <div class="ui-skeleton" />
                      </td></tr
                  ></template>
                  <template v-else
                    ><tr
                      v-for="v in visibleVersions"
                      :key="v.version"
                      :class="{
                        published: v.version === settings?.active_bundle,
                      }"
                    >
                      <td class="ui-mono truncate ui-row-key" :title="v.version">
                        {{ v.version }}
                      </td>
                      <td>
                        <StatusBadge :tone="v.version === settings?.active_bundle ? 'success' : 'neutral'">{{ v.version === settings?.active_bundle ? '已发布' : '待发布' }}</StatusBadge>
                      </td>
                      <td class="num">{{ v.file_count }}</td>
                      <td class="num">{{ sizeText(v.total_size) }}</td>
                      <td class="ui-secondary-column ui-mono">
                        {{ timeText(v.modified_timestamp) }}
                      </td>
                      <td>
                        <div class="ui-inline">
                          <button class="ui-action" @click="openFiles(v.version)"><Eye :size="14" />查看文件</button
                          ><button
                            v-if="v.version !== settings?.active_bundle"
                            class="ui-action"
                            :disabled="busy"
                            @click="release(v.version)"
                          >
                            发布</button
                          ><button
                            v-else
                            class="ui-action"
                            :disabled="busy"
                            @click="release(v.version, true)"
                          >
                            取消发布</button
                          ><ActionButton label="删除" :accessible-label="'删除资源版本 ' + v.version" danger :disabled="busy || v.version === settings?.active_bundle" :reason="v.version === settings?.active_bundle ? '当前版本已发布，请先切换或取消发布。' : '正在处理，请稍候。'" @click="deleteBundle(v.version)"><template #icon><Trash2 :size="14" /></template></ActionButton>
                        </div>
                      </td></tr
                  ></template>
                </tbody>
              </table>
              <div v-if="!versions.length && !loading" class="ui-empty">
                <Upload :size="24" />
                <p>还没有资源版本，上传第一份资源即可开始。</p>
                <button @click="openUpload">上传资源</button>
              </div>
            </div>
            <div v-if="versions.length > 50" class="ui-pagination">
              <button :disabled="tablePage === 1" @click="tablePage--">
                上一页</button
              >{{ tablePage }} / {{ Math.ceil(versions.length / 50)
              }}<button
                :disabled="tablePage * 50 >= versions.length"
                @click="tablePage++"
              >
                下一页
              </button>
            </div>
          </template>
          <template v-else-if="page === 'logs'"
            ><div class="ui-toolbar">
              <p class="ui-muted">
                记录有权限项目的操作，最多每次读取 100 条。
              </p>
              <span class="ui-spacer" /><button
                @click="
                  run(async () => {
                    logs = await api.request('/api/audit');
                  })
                "
              >
                <RefreshCw :size="14" />刷新
              </button>
            </div>
            <div class="ui-table-wrap">
              <table class="ui-table">
                <thead>
                  <tr>
                    <th>时间</th>
                    <th>用户</th>
                    <th>项目</th>
                    <th>操作</th>
                    <th>对象</th>
                  </tr>
                </thead>
                <tbody>
                  <tr v-for="log in logs" :key="log.id">
                    <td class="ui-mono">{{ timeText(log.timestamp) }}</td>
                    <td>{{ log.actor_name }}</td>
                    <td class="truncate">
                      {{
                        projects.find((p) => p.id === log.project_id)
                          ?.project_name || "系统"
                      }}
                    </td>
                    <td>{{ log.message }}</td>
                    <td class="truncate ui-mono" :title="log.object">
                      {{ log.object }}
                    </td>
                  </tr>
                  <tr v-if="!logs.length">
                    <td colspan="5">暂无操作记录</td>
                  </tr>
                </tbody>
              </table>
            </div>
            <button
              v-if="logs.length >= 100"
              :disabled="busy"
              @click="run(moreLogs)"
            >
              加载更早记录
            </button></template
          >
          <UserAdmin
            v-else-if="page === 'users' && user.role === 'admin' && !native"
            :api="api"
            :projects="projects"
            :current-user="user"
            @changed="run(refresh)"
          />
          <form
            v-else-if="page === 'system' && user.role === 'admin'"
            class="ui-form"
            style="max-width: 480px"
            @submit.prevent="
              run(async () => {
                await api.mutate(
                  '/api/settings/protocol-access',
                  'PUT',
                  protocol,
                );
                feedback('系统设置已保存');
              })
            "
          >
            <h2>资源下载协议</h2>
            <p class="ui-muted">此设置控制游戏资源下载，不影响管理页面登录。</p>
            <p v-if="!protocol.tls_configured" class="ui-note">
              服务端尚未配置 TLS。如使用反向代理，请在代理侧配置 HTTPS。
            </p>
            <ToggleSwitch v-model="protocol.http_enabled" label="允许 HTTP 下载" :disabled="busy || !protocol.tls_configured" /><ToggleSwitch v-model="protocol.https_enabled" label="允许 HTTPS 下载" :disabled="busy || !protocol.tls_configured" />
            <button
              class="primary"
              :disabled="busy || !protocol.tls_configured"
              type="submit"
            >
              保存设置
            </button>
          </form>
        </div>
        <ClientLogPanel :key="user.id + api.getBaseUrl()" :entries="clientLogRows" :resource-entries="resourceLogRows" :connection-state="logState" :project-name="project?.project_name" :resource-logs-available="resourceLogsAvailable" @history="navigate('logs')" />
      </main>
    </template>
    <Dialog
      :open="panel === 'create' || panel === 'settings'"
      :title="panel === 'create' ? '创建项目' : '项目设置'"
      :drawer="panel === 'settings'"
      :busy="busy"
      @close="closePanel"
    >
      <form id="project-form" class="ui-form" @submit.prevent="saveProject">
        <div class="ui-field">
          <label for="project-name">项目名称</label
          ><input
            id="project-name"
            v-model="form.project_name"
            required
            maxlength="80"
            placeholder="例如：星海计划"
          />
          <small class="ui-muted">用于下载地址，全局唯一；改名后需更新下载地址。</small>
        </div>
        <div v-if="panel === 'create'" class="ui-field">
          <label for="project-version">初始项目版本</label
          ><input
            id="project-version"
            v-model="form.project_version"
            required
          /><small class="ui-muted"
            >不同客户端版本可使用不同资源，通常从 v1 开始。</small
          >
        </div>
        <div class="ui-field">
          <label>目标平台（可多选）</label>
          <div class="ui-checks">
            <button v-for="p in [...new Set([...platforms, ...form.platforms])]" :key="p" type="button" class="ui-platform-switch" role="switch" :aria-checked="form.platforms.includes(p)" :class="{ selected: form.platforms.includes(p) }" @click="toggleFormPlatform(p)"><span class="ui-toggle-track" aria-hidden="true"><span /></span>{{ p }}</button>
          </div>
        </div>
        <details>
          <summary>高级设置</summary>
          <div class="ui-field" style="margin-top: 8px">
            <label for="package-name">资源包名</label
            ><input
              id="package-name"
              v-model="form.package_name"
              required
            /><small class="ui-muted"
              >需与 Unity / YooAsset 的资源包名一致，默认
              DefaultPackage。</small
            >
          </div>
        </details>
        <div v-if="dialogError" class="ui-error" role="alert">
          {{ dialogError
          }}<button
            v-if="panel === 'settings'"
            type="button"
            @click="run(reloadSettings, true)"
          >
            合并最新配置
          </button>
        </div>
        <div
          v-for="conflict in settingsConflicts"
          :key="conflict.key"
          class="ui-note"
        >
          <p>
            {{ conflict.label }}：最新值为
            {{
              Array.isArray(conflict.latest)
                ? conflict.latest.join(" / ")
                : conflict.latest
            }}
          </p>
          <div class="ui-inline">
            <button type="button" @click="resolveSetting(conflict.key, true)">
              使用最新值
            </button>
            <button type="button" @click="resolveSetting(conflict.key, false)">
              保留我的修改
            </button>
          </div>
        </div>
        <template v-if="panel === 'settings' && project"
          ><hr class="ui-divider" />
          <div class="ui-field">
            <label>当前项目版本</label>
            <div class="ui-inline">
              <span class="ui-mono">{{ projectVersion || "尚未创建" }}</span
              ><span class="ui-spacer" /><button
                v-if="projectVersion"
                type="button"
                @click="editProjectVersion"
              >
                重命名</button
              ><button
                v-if="projectVersion"
                type="button"
                class="danger"
                @click="deleteProjectVersion"
              >
                删除版本
              </button>
            </div>
          </div>
          <div class="ui-danger-zone">
            <strong>删除整个项目</strong>
            <p class="ui-muted">
              删除全部平台、版本及资源，所有共享成员都将失去此项目。
            </p>
            <button
              type="button"
              class="danger"
              :disabled="!project.can_manage"
              :title="
                !project.can_manage ? '仅项目所有者或管理员可删除项目' : ''
              "
              @click="deleteProject"
            >
              删除项目
            </button>
          </div></template
        >
      </form>
      <template #footer
        ><button :disabled="busy" @click="closePanel">取消</button
        ><button
          class="primary"
          form="project-form"
          type="submit"
          :disabled="
            busy || (panel === 'settings' && settingsConflicts.length > 0)
          "
        >
          {{
            busy
              ? "正在保存…"
              : panel === "create"
                ? "创建并进入项目"
                : "保存修改"
          }}
        </button></template
      >
    </Dialog>
    <Dialog
      :open="panel === 'platform'" title="新增平台" :busy="busy" @close="closePanel"
    >
      <form id="platform-form" class="ui-form" @submit.prevent="addPlatform">
        <p class="ui-muted">平台会添加到当前项目的所有项目版本，资源分别管理。</p>
        <div class="ui-checks"><button v-for="p in platforms.filter(p => !project?.platforms.includes(p))" :key="p" type="button" :class="{ active: newPlatform === p }" :aria-pressed="newPlatform === p" :disabled="busy" @click="newPlatform = p">{{ p }}</button></div>
        <div class="ui-field"><label for="new-platform">平台名称</label><input id="new-platform" v-model="newPlatform" required :disabled="busy" placeholder="例如：Android、iOS 或自定义平台" /></div>
        <p v-if="dialogError" class="ui-error" role="alert">{{ dialogError }}</p>
      </form>
      <template #footer><button :disabled="busy" @click="closePanel">取消</button><button form="platform-form" type="submit" class="primary" :disabled="busy">添加平台</button></template>
    </Dialog>
    <Dialog
      :open="panel === 'version'"
      :title="renameVersion ? '重命名项目版本' : '添加项目版本'"
      :busy="busy"
      @close="closePanel"
      ><form id="version-form" class="ui-form" @submit.prevent="versionSubmit">
        <div class="ui-field">
          <label for="version-name">项目版本名称</label
          ><input id="version-name" v-model="newVersion" required /><small
            class="ui-muted"
            >{{ renameVersion ? '重命名会改变下载地址，请同步修改客户端配置。' : '不同客户端版本可使用不同资源，例如 v1、v2。' }}</small
          >
        </div>
        <div v-if="dialogError" class="ui-error" role="alert">
          {{ dialogError }}
        </div>
      </form>
      <template #footer
        ><button @click="closePanel">取消</button
        ><button
          class="primary"
          form="version-form"
          type="submit"
          :disabled="busy"
        >
          {{ renameVersion ? '保存修改' : '添加版本' }}
        </button></template
      ></Dialog
    >
    <Dialog
      :open="panel === 'members'"
      title="共享成员"
      drawer
      :busy="busy"
      @close="closePanel"
      ><div class="ui-form">
        <p v-if="members">
          所有者：<strong>{{ members.owner.username }}</strong>
        </p>
        <p class="ui-muted">
          共享成员可上传、发布和删除资源。项目所有者管理共享、转交及整个项目的删除。
        </p>
        <div v-for="m in members?.members" :key="m.id" class="ui-inline">
          <UserRound :size="16" />{{ m.username
          }}<span class="ui-spacer" /><button
            :disabled="busy || !project?.can_manage"
            @click="removeMember(m.id, m.username)"
          >
            移除
          </button>
        </div>
        <p v-if="members && !members.members.length" class="ui-muted">
          尚未分享给其他用户。
        </p>
        <form
          v-if="project?.can_manage"
          class="ui-form"
          @submit.prevent="memberAction()"
        >
          <div class="ui-field">
            <label for="member-name">对方的完整用户名</label
            ><input
              id="member-name"
              v-model="memberName"
              required
              placeholder="例如：xiaowang"
            />
          </div>
          <div class="ui-inline">
            <button class="primary" :disabled="busy" type="submit">
              添加共享成员</button
            ><button
              :disabled="busy || !memberName"
              type="button"
              @click="memberAction(true)"
            >
              转交给此用户
            </button>
          </div>
        </form>
        <p v-else class="ui-note">仅项目所有者或系统管理员可管理共享成员。</p>
        <div v-if="dialogError" class="ui-error" role="alert">
          {{ dialogError }}
        </div>
      </div></Dialog
    >
    <Dialog
      :open="panel === 'upload' && !diffOpen"
      title="上传资源"
      :busy="busy"
      @close="closePanel"
      ><form id="upload-form" class="ui-form" @submit.prevent="upload">
        <div class="ui-upload-target"><strong>{{ project?.project_name }} / {{ projectVersion }} / {{ platform }}</strong><span class="ui-muted">{{ autoPublish ? '自动发布：上传完成立即发布，客户端将读取新版本。' : '手动发布：上传后保持待发布，不影响客户端。' }}</span></div>
        <SegmentedControl v-if="native" :model-value="uploadNative ? 'directory' : 'files'" label="上传来源" :options="[{value:'files',label:'文件上传'},{value:'directory',label:'本地构建目录'}]" :disabled="busy" @update:model-value="uploadNative = $event === 'directory'" />
        <template v-if="uploadNative"
          ><div class="ui-toolbar"><button type="button" :disabled="busy" @click="chooseDirectory"><Folder :size="16" />{{ directory ? '更换目录' : '选择 Bundles 目录' }}</button><button v-if="directory" type="button" :disabled="busy" @click="rereadUpload"><RefreshCw :size="14" />重新读取</button></div>
          <p class="ui-muted" style="overflow-wrap: anywhere">
            {{ directory || "目录应包含 平台/包名/资源版本 文件夹" }}
          </p>
          <div class="ui-field">
            <label for="local-version">本地资源版本</label
            ><select id="local-version" v-model="uploadVersion" required>
              <option value="">选择版本</option>
              <option
                v-for="v in localVersions"
                :key="v.version"
                :value="v.version"
              >
                {{ v.version }} · {{ v.file_count }} 文件 ·
                {{ sizeText(v.total_size) }}
              </option>
            </select>
          </div>
          <label class="ui-inline"
            ><input
              v-model="incremental"
              type="checkbox"
              :disabled="!diffBase"
            />增量上传（复用未变化文件）</label>
          <div v-if="versions.length" class="ui-field">
            <label for="diff-base">对比基准版本</label>
            <select id="diff-base" v-model="diffBase"><option v-for="(version, index) in versions" :key="version.version" :value="version.version">{{ version.version }}{{ index === 0 ? ' · 最近上传' : '' }}{{ version.version === settings?.active_bundle ? ' · 当前发布' : '' }}</option></select>
          </div>
          <p v-if="!busy && localVersions.length" class="ui-muted">已自动读取最新版本，可手动切换本地版本与对比基准。</p>
          <p v-if="nativeUploadExists" class="ui-warning-note"><AlertCircle :size="16" /><span><strong>此资源版本已上传。</strong>构建新版本后点击“重新读取”，或选择其他本地版本。</span></p>
          <div v-if="diffLoading" class="ui-note" role="status">正在计算文件差异…</div>
          <div v-else-if="diffError" class="ui-error" role="alert">{{ diffError }}<button type="button" @click="computeUploadDiff">重新比较</button></div>
          <div v-else-if="uploadDiff" class="ui-diff-summary">
            <span class="ui-upload-metric">本次上传 <strong>{{ sizeText(incremental && diffBase ? uploadDiff.uploadSize : uploadDiff.totalSize) }}</strong></span><span class="ui-muted">完整资源 {{ sizeText(uploadDiff.totalSize) }}</span>
            <span v-if="incremental && diffBase" class="success">节省 {{ sizeText(savedBytes) }}（{{ savedPercent }}%）</span>
            <span v-else class="ui-muted">{{ uploadDiff.added.length + uploadDiff.modified.length + uploadDiff.unchanged.length }} 个文件</span>
            <span class="ui-spacer" /><button type="button" class="ui-action" @click="diffOpen = true"><GitCompareArrows :size="14" />查看差异</button>
          </div>
        </template
        >
        <template v-else
          ><div class="ui-field">
            <label for="resource-files">资源文件</label
            ><input
              id="resource-files"
              type="file"
              multiple
              :disabled="busy"
              @change="
                uploadFiles = Array.from(
                  ($event.target as HTMLInputElement).files || [],
                )
              "
            /><small class="ui-muted"
              >已选择 {{ uploadFiles.length }} 个文件 ·
              {{ sizeText(uploadSize) }}，单次最多 512MB。</small
            >
          </div>
          <div class="ui-field">
            <label for="upload-version">资源版本号（可选）</label
            ><input
              id="upload-version"
              v-model="uploadVersion"
              :disabled="busy"
              placeholder="留空自动生成，不能覆盖已有版本"
            /><small class="ui-muted"
              >对应 Bundle 版本，与上方项目版本独立。</small
            >
          </div></template
        >
        <div v-if="busy" class="ui-progress" role="status">
          {{ uploadPhase || '正在处理，请稍候…' }}
        </div>
        <div v-if="dialogError" class="ui-error" role="alert">
          {{ dialogError }}
        </div>
      </form>
      <template #footer
        ><button :disabled="busy" @click="closePanel">取消</button
        ><button
          class="primary"
          form="upload-form"
          type="submit"
          :disabled="busy || nativeUploadExists || (uploadNative && incremental && !!diffBase && (diffLoading || !!diffError))"
        >
          {{ busy ? '正在处理…' : autoPublish ? '上传并自动发布' : '确认上传' }}
        </button></template
      ></Dialog
    >
    <Dialog :open="diffOpen" :title="'上传差异 · ' + uploadVersion" wide :busy="busy" @close="diffOpen = false">
      <div v-if="uploadDiff && diffOpen" class="ui-form">
        <p class="ui-muted">{{ project?.project_name }} / {{ projectVersion }} / {{ platform }} · {{ uploadDiff.baseVersion ? '基于 ' + uploadDiff.baseVersion : '首次上传，所有文件均为新增' }}</p>
        <div class="ui-diff-summary">
          <span>完整上传 <strong>{{ sizeText(uploadDiff.totalSize) }}</strong></span>
          <span>增量上传 <strong>{{ sizeText(uploadDiff.uploadSize) }}</strong></span>
          <span class="success">节省 {{ sizeText(savedBytes) }}（{{ savedPercent }}%）</span>
        </div>
        <div class="ui-inline ui-diff-counts"><span v-for="group in diffGroups" :key="group.key" :class="group.tone">{{ group.label }} {{ group.files.length }}</span></div>
        <details v-for="group in diffGroups" :key="group.key" class="ui-diff-group" :open="['modified', 'added'].includes(group.key)">
          <summary :class="group.tone">{{ group.label }}（{{ group.files.length }}）</summary>
          <p v-if="group.key === 'removed'" class="ui-muted">这些文件不会进入新资源版本，服务器上的历史版本保持不变。</p>
          <p v-if="!group.files.length" class="ui-muted">无</p>
          <div v-else class="ui-diff-files"><table class="ui-table ui-file-table"><tbody><tr v-for="file in group.files" :key="file.name"><td class="ui-filename ui-mono">{{ file.name }}</td><td class="num ui-file-size">{{ sizeText(file.size) }}</td></tr></tbody></table></div>
        </details>
      </div>
      <template #footer><button @click="diffOpen = false">返回上传</button></template>
    </Dialog>
    <Dialog
      :open="panel === 'files'"
      :title="'资源文件 · ' + fileVersion"
      drawer
      wide
      :busy="busy"
      @close="closePanel"
      ><div class="ui-form">
        <p v-if="fileVersion !== settings?.active_bundle" class="ui-note">
          此版本尚未发布。下载地址始终指向当前发布版本。
        </p>
        <div class="ui-toolbar">
          <Search :size="16" /><input
            v-model="fileQuery"
            aria-label="搜索文件"
            placeholder="搜索文件名"
          /><span class="ui-muted">{{ files.length }} 个文件</span>
        </div>
        <div v-if="busy" role="status">正在读取文件…</div>
        <div v-if="dialogError" class="ui-error" role="alert">
          {{ dialogError }}
        </div>
        <div class="ui-table-wrap">
          <table class="ui-table ui-file-table">
            <thead>
              <tr>
                <th>文件名</th>
                <th class="num ui-file-size">大小</th>
                <th class="ui-file-actions">操作</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="f in visibleFiles" :key="f.name">
                <td class="ui-filename ui-mono">{{ f.name }}</td>
                <td class="num">{{ sizeText(f.size) }}</td>
                <td>
                  <div class="ui-inline">
                    <button class="ui-icon ui-action" :aria-label="'复制文件名 ' + f.name" title="复制完整文件名" @click="copy(f.name)"><Copy :size="14" /></button>
                    <ActionButton
                      label="地址"
                      accessible-label="复制文件地址"
                      reason="此版本尚未发布，发布后可以复制下载地址。"
                      :disabled="fileVersion !== settings?.active_bundle"
                      @click="
                        copy(
                          api.resourceUrl(
                            project!,
                            projectVersion,
                            platform,
                            f.name,
                          ),
                        )
                      "
                    />
                    <ActionButton
                      label="查看"
                      accessible-label="打开文件"
                      reason="此版本尚未发布，发布后可以打开文件。"
                      :disabled="fileVersion !== settings?.active_bundle"
                      @click="run(() => previewFile(f.name), true)"
                    />
                  </div>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        <div v-if="filteredFiles.length > 100" class="ui-pagination">
          <button :disabled="filePage === 1" @click="filePage--">上一页</button
          >{{ filePage }} / {{ Math.ceil(filteredFiles.length / 100)
          }}<button
            :disabled="filePage * 100 >= filteredFiles.length"
            @click="filePage++"
          >
            下一页
          </button>
        </div>
      </div></Dialog
    >
    <Dialog
      :open="panel === 'password' || !!user?.must_change_password"
      :title="user?.must_change_password ? '首次登录，请修改密码' : '修改密码'"
      :busy="busy"
      :dismissible="!user?.must_change_password"
      @close="closePanel"
      ><form
        id="password-form"
        class="ui-form"
        @submit.prevent="changePassword"
      >
        <p class="ui-muted">修改后所有设备会退出登录，需使用新密码重新登录。</p>
        <div class="ui-field">
          <label for="current-password">当前密码</label
          ><input
            id="current-password"
            v-model="currentPassword"
            type="password"
            autocomplete="current-password"
            required
          />
        </div>
        <div class="ui-field">
          <label for="new-password">新密码</label
          ><input
            id="new-password"
            v-model="newPassword"
            type="password"
            autocomplete="new-password"
            minlength="10"
            maxlength="256"
            required
          />
        </div>
        <div class="ui-field">
          <label for="repeat-password">再次输入新密码</label
          ><input
            id="repeat-password"
            v-model="repeatPassword"
            :aria-invalid="!!dialogError && newPassword !== repeatPassword"
            aria-describedby="password-error"
            type="password"
            autocomplete="new-password"
            minlength="10"
            maxlength="256"
            required
          />
        </div>
        <div
          v-if="dialogError"
          id="password-error"
          class="ui-error"
          role="alert"
        >
          {{ dialogError }}
        </div>
      </form>
      <template #footer
        ><button
          v-if="user?.must_change_password"
          :disabled="busy"
          @click="logout"
        >
          退出登录</button
        ><button v-else :disabled="busy" @click="closePanel">取消</button
        ><button
          class="primary"
          form="password-form"
          type="submit"
          :disabled="busy"
        >
          {{ busy ? "正在修改…" : "修改密码" }}
        </button></template
      ></Dialog
    >
    <Dialog :open="panel === 'help'" title="使用说明" @close="closePanel"
      ><div class="ui-form">
        <p>
          <strong>1. 创建项目</strong
          ><br />填写项目名，选择目标平台；项目版本通常从 v1 开始。资源包名需与
          Unity / YooAsset 保持一致。
        </p>
        <p>
          <strong>2. 上传资源</strong
          ><br />选择构建产物，检查项目版本与平台。资源版本对应
          Bundle，上传完成后客户端暂时不会切换。
        </p>
        <p>
          <strong>3. 发布并接入</strong
          ><br />确认发布后复制下载地址。游戏无需管理账号即可下载；关闭客户端下载可暂时下线该平台。
        </p>
        <p>
          <strong>共享协作</strong
          ><br />所有者可按完整用户名分享项目。共享成员可以操作资源，所有者管理共享名单和整个项目的删除。
        </p>
      </div></Dialog
    >
    <Dialog
      :open="confirmOpen"
      :title="confirm?.title || '确认操作'"
      :busy="busy"
      @close="confirmOpen = false"
      ><p>{{ confirm?.message }}</p>
      <div
        v-if="dialogError"
        class="ui-error"
        role="alert"
        style="margin-top: 8px"
      >
        {{ dialogError }}
      </div>
      <template #footer
        ><button :disabled="busy" @click="confirmOpen = false">取消</button
        ><button
          :class="confirm?.danger ? 'danger' : 'primary'"
          :disabled="busy"
          @click="accept"
        >
          {{ busy ? "正在处理…" : confirm?.label }}
        </button></template
      ></Dialog
    >
  </div>
</template>
