import { sha256 } from "js-sha256";

async function sha256Hex(input: string): Promise<string> {
  if (typeof crypto !== "undefined" && crypto.subtle) {
    const data = new TextEncoder().encode(input);
    const hashBuffer = await crypto.subtle.digest("SHA-256", data);
    return Array.from(new Uint8Array(hashBuffer))
      .map((b) => b.toString(16).padStart(2, "0"))
      .join("");
  }
  return sha256(input);
}

export interface ProjectConfig {
  id: string;
  project_name: string;
  platforms: string[];
  package_name: string;
  active_versions: Record<string, string>;
}

export interface VersionEntry {
  version: string;
  file_count: number;
  total_size: number;
  modified_timestamp: number;
}

export interface FileEntry {
  name: string;
  size: number;
  modified_timestamp: number;
}

export interface LogEntry {
  timestamp: string;
  type: string;
  status: number;
  method: string;
  path: string;
  project_id: string;
  message: string;
}

class RemoteApi {
  private baseUrl: string;
  private token: string | null = null;

  constructor(baseUrl: string = "") {
    this.baseUrl = baseUrl;
  }

  setBaseUrl(url: string) {
    this.baseUrl = url.replace(/\/$/, "");
  }

  isLoggedIn(): boolean {
    return this.token !== null;
  }

  getToken(): string {
    return this.token || "";
  }

  getBaseUrl(): string {
    return this.baseUrl;
  }

  async login(password: string): Promise<boolean> {
    const hashHex = await sha256Hex(password);

    const res = await fetch(`${this.baseUrl}/api/auth/login`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ password: hashHex }),
    });

    if (!res.ok) return false;
    const body = await res.json();
    this.token = body.token;
    return true;
  }

  logout() {
    this.token = null;
  }

  private async request<T>(path: string, options: RequestInit = {}): Promise<T> {
    const headers: Record<string, string> = {
      ...(options.headers as Record<string, string>),
    };
    if (this.token) {
      headers["Authorization"] = `Bearer ${this.token}`;
    }
    if (!headers["Content-Type"] && !(options.body instanceof FormData)) {
      headers["Content-Type"] = "application/json";
    }

    const res = await fetch(`${this.baseUrl}${path}`, { ...options, headers });

    if (res.status === 401) {
      this.token = null;
      throw new Error("Unauthorized");
    }
    if (!res.ok) {
      const text = await res.text();
      throw new Error(text || `HTTP ${res.status}`);
    }

    const contentType = res.headers.get("content-type");
    if (contentType?.includes("application/json")) {
      return res.json();
    }
    return res.text() as unknown as T;
  }

  async listProjects(): Promise<ProjectConfig[]> {
    return this.request("/api/projects");
  }

  async createProject(projectName: string): Promise<ProjectConfig> {
    return this.request("/api/projects", {
      method: "POST",
      body: JSON.stringify({ project_name: projectName }),
    });
  }

  async updateProject(project: ProjectConfig): Promise<void> {
    await this.request(`/api/projects/${project.id}`, {
      method: "PUT",
      body: JSON.stringify(project),
    });
  }

  async deleteProject(id: string): Promise<void> {
    await this.request(`/api/projects/${id}`, { method: "DELETE" });
  }

  async uploadResources(
    projectId: string,
    platform: string,
    version: string,
    files: File[],
  ): Promise<{ success: boolean; version: string; file_count: number }> {
    const formData = new FormData();
    formData.append("platform", platform);
    if (version) formData.append("version", version);
    for (const file of files) {
      formData.append("files", file);
    }

    return this.request(`/api/projects/${projectId}/upload`, {
      method: "POST",
      body: formData,
    });
  }

  async listVersions(projectId: string, platform: string): Promise<VersionEntry[]> {
    return this.request(`/api/projects/${projectId}/versions?platform=${encodeURIComponent(platform)}`);
  }

  async activateVersion(projectId: string, version: string, platform: string): Promise<void> {
    await this.request(
      `/api/projects/${projectId}/versions/${encodeURIComponent(version)}/activate?platform=${encodeURIComponent(platform)}`,
      { method: "PUT" },
    );
  }

  async deleteVersion(projectId: string, version: string, platform: string): Promise<void> {
    await this.request(
      `/api/projects/${projectId}/versions/${encodeURIComponent(version)}?platform=${encodeURIComponent(platform)}`,
      { method: "DELETE" },
    );
  }

  async getProjectStatus(projectId: string): Promise<{ active_versions: Record<string, string> }> {
    return this.request(`/api/projects/${projectId}/status`);
  }

  /**
   * List files in a platform.
   * - version omitted/empty: list active version files (platform root)
   * - version provided: list files in _versions/<version>/
   */
  async listFiles(projectId: string, platform: string, version?: string): Promise<FileEntry[]> {
    const params = new URLSearchParams({ platform });
    if (version) params.set("version", version);
    return this.request(`/api/projects/${projectId}/files?${params.toString()}`);
  }

  connectLogs(onMessage: (log: LogEntry) => void, onError?: (err: Event) => void): WebSocket {
    const wsProtocol = this.baseUrl.startsWith("https") ? "wss" : "ws";
    const wsHost = this.baseUrl.replace(/^https?:\/\//, "");
    const url = `${wsProtocol}://${wsHost}/api/ws/logs?token=${this.token}`;

    const ws = new WebSocket(url);
    ws.onmessage = (event) => {
      try {
        const log: LogEntry = JSON.parse(event.data);
        onMessage(log);
      } catch {}
    };
    ws.onerror = (e) => onError?.(e);
    return ws;
  }
}

export const api = new RemoteApi();
