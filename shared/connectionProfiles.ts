export interface ConnectionProfile {
  id: string;
  name: string;
  url: string;
  username: string;
  passwordStored: boolean;
  lastUsed: number;
}
const storageKey = "tengine_remote_connections";
export function loadConnections(): ConnectionProfile[] {
  const raw: unknown = JSON.parse(localStorage.getItem(storageKey) || "[]");
  if (!Array.isArray(raw)) throw new Error("已保存的连接格式无法读取，请检查客户端配置。");
  const profiles: ConnectionProfile[] = raw.filter(p => p && typeof p.id === "string" && typeof p.url === "string").map(p => ({
    id: p.id, name: typeof p.name === "string" ? p.name : p.url, url: p.url,
    username: typeof p.username === "string" ? p.username : "",
    passwordStored: p.passwordStored === true, lastUsed: Number(p.lastUsed) || 0,
  }));
  // Migrate legacy metadata; never retain its plaintext password or rememberPassword field.
  storeConnections(profiles);
  return profiles.sort((a, b) => b.lastUsed - a.lastUsed);
}
export function storeConnections(profiles: ConnectionProfile[]) {
  localStorage.setItem(storageKey, JSON.stringify(profiles));
}
export function normalizeServerUrl(value: string) {
  const url = new URL(value.trim());
  if (!["http:", "https:"].includes(url.protocol) || url.username || url.password || url.search || url.hash)
    throw new Error("请输入 HTTP 或 HTTPS 服务地址，不要包含账号、查询参数或 #。");
  return url.href.replace(/\/+$/, "");
}
