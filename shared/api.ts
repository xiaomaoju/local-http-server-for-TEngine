export interface User {
  id: string;
  username: string;
  role: "admin" | "user";
  enabled: boolean;
  must_change_password: boolean;
}
export interface PlatformSettings {
  access_enabled: boolean;
  active_bundle: string | null;
  published_releases?: string[];
}
export interface ProjectVersion {
  name: string;
  attachment_snapshot?: string | null;
  platform_settings: Record<string, PlatformSettings>;
}
export interface ProjectConfig {
  id: string;
  project_name: string;
  project_type?: ProjectType;
  package_name: string;
  platforms: string[];
  project_versions: ProjectVersion[];
  owner_id: string;
  revision: number;
  can_manage: boolean;
  shared_with_me: boolean;
}
export type ProjectType = "bundle" | "apk" | "files";
export const projectTypes = [{value: "bundle", label: "Bundle", symbol: "Ⓑ"}, {value: "apk", label: "APP更新", symbol: "Ⓐ"}, {value: "files", label: "文件", symbol: "Ⓕ"}];
export const projectTypeLabel = (kind: ProjectType = "bundle") => projectTypes.find(item => item.value === kind)!.label;
export const projectTypeSymbol = (kind: ProjectType = "bundle") => projectTypes.find(item => item.value === kind)!.symbol;
export const publicResourcePath = (project: {project_name: string; project_type?: ProjectType}, version: string, platform: string, file = "", release?: string) => {
  const kind = project.project_type || "bundle";
  const prefix = {bundle: "res", apk: "appversion", files: "file"}[kind];
  const parts = [version, project.project_name];
  if (kind === "bundle" && platform) parts.push(platform);
  if (kind === "apk" && release) parts.push("releases", release);
  if (file) parts.push(file);
  return `/${prefix}/${parts.map(encodeURIComponent).join("/")}${file ? "" : "/"}`;
};
export interface NativeFile { path: string; name: string; size: number }
export interface ApkRelease { version: string; version_name: string; package_name: string; catalog_path: string; catalog_sha256: string; files: NativeFile[] }
export interface VersionEntry {
  version: string;
  file_count: number;
  total_size: number;
  modified_timestamp: number;
}
export interface ApkDownloadStats {
  total: number;
  releases: { release: string; downloads: number }[];
}
export interface FileEntry {
  name: string;
  size: number;
  modified_timestamp: number;
}
export interface AttachmentListing { files: FileEntry[]; snapshot: string | null; revision?: number }
export interface AttachmentResult<T> extends AttachmentListing { project: T }
export type AttachmentInput = File | NativeFile;
export interface AttachmentClient {
  list(): Promise<AttachmentListing>;
  upload(files: AttachmentInput[], current: AttachmentListing): Promise<AttachmentListing>;
  remove(name: string, current: AttachmentListing): Promise<AttachmentListing>;
  chooseFiles?: () => Promise<NativeFile[]>;
  chooseDirectory?: () => Promise<string | null>;
  readDirectory?: (directory: string) => Promise<NativeFile[]>;
  directoryKey?: string;
  url(name: string): string;
  open(url: string): Promise<void>;
}
export const attachmentPath = (project: {project_name: string}, version: string, name: string) =>
  `/appversion/${encodeURIComponent(version)}/${encodeURIComponent(project.project_name)}/docs/${encodeURIComponent(name)}`;
export interface FileManifestEntry {
  name: string;
  size: number;
  md5: string;
}
export interface LogEntry {
  id: number;
  timestamp: string;
  actor_id: string;
  actor_name: string;
  project_id: string | null;
  action: string;
  object: string;
  message: string;
}
export interface ResourceRequestLog {
  type: "resource_request";
  id: string;
  timestamp: string;
  project_id: string | null;
  method: string;
  path: string;
  status: number;
  outcome: "completed" | "failed" | "interrupted";
  message: string;
}
export interface LoginResponse {
  user: User;
  csrf: string;
  token?: string;
}
export interface LogConnection {
  close(): Promise<void> | void;
}
export type ConnectSocket = (
  url: string,
  message: (value: string) => void,
  closed: () => void,
) => Promise<LogConnection>;
export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}
export class RemoteApi {
  private token = "";
  private csrf = "";
  private revisions = new Map<string, number>();
  private baseUrl = "";
  constructor(
    private transport: typeof fetch,
    public client: "web" | "desktop",
    private socket: ConnectSocket,
  ) {}
  setBaseUrl(url: string) {
    this.baseUrl = url.replace(/\/+$/, "");
    this.clear();
  }
  getBaseUrl() {
    return this.baseUrl;
  }
  getToken() {
    return this.token;
  }
  apkDownloadStats(project: ProjectConfig, version: string) {
    return this.request<ApkDownloadStats>(`${resourcePath(project, version)}/download-stats`);
  }
  rememberProject(project: ProjectConfig) { this.revisions.set(project.id, project.revision); }
  clear() {
    this.token = "";
    this.csrf = "";
    this.revisions.clear();
  }
  async request<T>(path: string, options: RequestInit = {}): Promise<T> {
    const headers = new Headers(options.headers);
    if (this.token) headers.set("Authorization", `Bearer ${this.token}`);
    if (this.csrf) headers.set("X-CSRF-Token", this.csrf);
    if (!(options.body instanceof FormData))
      headers.set("Content-Type", "application/json");
    const projectId = path.match(/^\/api\/projects\/([^/?]+)/)?.[1];
    if (
      projectId &&
      options.method &&
      this.revisions.has(projectId) &&
      !headers.has("If-Match")
    )
      headers.set("If-Match", String(this.revisions.get(projectId)));
    let response: Response;
    try {
      response = await this.transport(this.baseUrl + path, {
        ...options,
        headers,
        credentials: this.client === "web" ? "same-origin" : "omit",
      });
    } catch {
      throw new ApiError(
        0,
        "连接中断，请检查服务地址或网络后重试；已填写内容仍保留。",
      );
    }
    if (!response.ok) {
      const text = await response.text();
      let message = text;
      try {
        message = JSON.parse(text).message || text;
      } catch {
        /* non-JSON proxy response */
      }
      throw new ApiError(
        response.status,
        message || `请求失败 (${response.status})`,
      );
    }
    const data = await response.json();
    const remember = (p: any) => {
      if (p && typeof p.id === "string" && typeof p.revision === "number")
        this.revisions.set(p.id, p.revision);
    };
    if (Array.isArray(data)) data.forEach(remember);
    else {
      remember(data);
      remember(data.project);
    }
    return data;
  }
  async login(username: string, password: string) {
    const data = await this.request<LoginResponse>("/api/auth/login", {
      method: "POST",
      body: JSON.stringify({ username, password, client: this.client }),
    });
    this.token = data.token || "";
    this.csrf = data.csrf;
    return data.user;
  }
  async me() {
    const data = await this.request<LoginResponse>("/api/auth/me");
    this.csrf = data.csrf;
    return data.user;
  }
  async logout() {
    try {
      await this.request("/api/auth/logout", { method: "POST" });
    } finally {
      this.clear();
    }
  }
  listProjects() {
    return this.request<ProjectConfig[]>("/api/projects");
  }
  project(id: string) {
    return this.request<ProjectConfig>(`/api/projects/${id}`);
  }
  mutate<T>(path: string, method: string, value?: unknown, revision?: number) {
    return this.request<T>(path, {
      method,
      body: value === undefined ? undefined : JSON.stringify(value),
      headers:
        revision === undefined ? undefined : { "If-Match": String(revision) },
    });
  }
  resourceUrl(
    project: ProjectConfig,
    version: string,
    platform: string,
    file = "",
  ) {
    return `${this.baseUrl || location.origin}${publicResourcePath(project, version, platform, file)}`;
  }
  apkReleaseUrl(project: ProjectConfig, version: string, release: string, file: string) {
    return `${this.baseUrl || location.origin}${publicResourcePath(project, version, "Android", file, release)}`;
  }
  async connectLogs(
    onMessage: (log: LogEntry | ResourceRequestLog | { type: "access_changed" }) => void,
    closed: () => void,
  ) {
    const { ticket } = await this.mutate<{ ticket: string }>(
      "/api/auth/ws-ticket",
      "POST",
    );
    const url =
      (this.baseUrl || location.origin).replace(/^http/, "ws") +
      `/api/ws/logs?ticket=${encodeURIComponent(ticket)}`;
    return this.socket(
      url,
      (text) => {
        try {
          onMessage(JSON.parse(text));
        } catch {
          /* malformed message */
        }
      },
      closed,
    );
  }
}
export interface ConnectionLogin { url: string; username: string; password: string; connectionId: string }
export interface ConnectionCredentials {
  get(id: string, url: string, username: string): Promise<string | null>;
  save(id: string, url: string, username: string, password: string): Promise<void>;
  remove(id: string): Promise<void>;
}
export interface NativeUpload {
  credentials?: ConnectionCredentials;
  chooseFiles?(kind: "apk" | "catalog" | "files"): Promise<NativeFile[]>;
  uploadFiles?(files: NativeFile[], project: ProjectConfig, projectVersion: string, version: string): Promise<{version: string}>;
  uploadAttachments?(files: NativeFile[], project: ProjectConfig, projectVersion: string, revision: number): Promise<AttachmentResult<ProjectConfig>>;
  readAttachmentDirectory?(directory: string): Promise<NativeFile[]>;
  apkReleases?(directory: string): Promise<ApkRelease[]>;
  uploadApkRelease?(directory: string, release: ApkRelease, project: ProjectConfig, projectVersion: string): Promise<{version: string}>;
  chooseDirectory(title?: string): Promise<string | null>;
  manifest(directory: string, project: ProjectConfig, platform: string, version: string): Promise<FileManifestEntry[]>;
  versions(
    directory: string,
    project: ProjectConfig,
    platform: string,
  ): Promise<VersionEntry[]>;
  upload(
    directory: string,
    project: ProjectConfig,
    projectVersion: string,
    platform: string,
    version: string,
    base: string | null,
    incremental: boolean,
  ): Promise<void>;
  openUrl(url: string): Promise<void>;
}
export const resourcePath = (p: ProjectConfig, version: string) =>
  `/api/projects/${p.id}/project-versions/${encodeURIComponent(version)}`;
export const sizeText = (n: number) =>
  n >= 1024 ** 3
    ? `${(n / 1024 ** 3).toFixed(1)} GB`
    : n >= 1024 ** 2
      ? `${(n / 1024 ** 2).toFixed(1)} MB`
      : n >= 1024
        ? `${(n / 1024).toFixed(1)} KB`
        : `${n} B`;
export const timeText = (n: number | string) =>
  new Date(typeof n === "number" ? n * 1000 : n).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
