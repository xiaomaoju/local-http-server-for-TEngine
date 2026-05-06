# 项目版本与平台访问开关实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 TEngineHttp 远程模式中引入两层版本模型（项目版本 / Bundle 版本），并为本地与远程模式的每个平台增加独立访问开关。

**Architecture:** 服务端引入 `ProjectVersion` 数据结构，URL 改造为 `/res/{pver}/{project}/{platform}/{file}`；激活降级为纯元数据（不再复制文件）；客户端 RemoteMode 使用两级标签（项目 / 项目版本），LocalMode 在平台标签旁内联访问开关。

**Tech Stack:** Rust（axum 服务端、Tauri 桌面端）、Vue 3 + TypeScript（前端）、JSON 配置存储

**Spec:** [docs/superpowers/specs/2026-05-06-project-version-and-access-toggle-design.md](../specs/2026-05-06-project-version-and-access-toggle-design.md)

**前置说明：**
- 本仓库目前没有测试框架（server/、src-tauri/ 都没有 `tests/` 或 `#[cfg(test)]`），因此本计划用 `cargo build` + 手动 `curl` + 浏览器手测代替 TDD。少数纯函数级别的逻辑（路径拼装、URL 解析）会就近添加 `#[cfg(test)]` 单测。
- 不考虑数据迁移；旧 URL 与旧目录直接弃用。

---

## Task 1: 服务端 - 新数据模型（config.rs）

**目标：** 把 `ProjectConfig.active_versions` 替换为 `project_versions: Vec<ProjectVersion>` 树形结构。

**Files:**
- Modify: `server/src/config.rs`

**Why this is first:** 后续所有改动（存储、API、URL 路由）都依赖这个数据结构。

- [ ] **Step 1: 用新结构替换 ProjectConfig**

把 `server/src/config.rs` 第 8-27 行的 `ProjectConfig` 与 `ProjectConfig::new` 替换为：

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlatformSettings {
    pub access_enabled: bool,
    pub active_bundle: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectVersion {
    pub name: String,
    #[serde(default)]
    pub platform_settings: HashMap<String, PlatformSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub id: String,
    pub project_name: String,
    pub platforms: Vec<String>,
    pub package_name: String,
    #[serde(default)]
    pub project_versions: Vec<ProjectVersion>,
}

impl ProjectConfig {
    pub fn new(project_name: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            project_name,
            platforms: vec!["Android".to_string()],
            package_name: "DefaultPackage".to_string(),
            project_versions: Vec::new(),
        }
    }
}
```

注意：`PlatformSettings` 的 `access_enabled` 默认 `false`（`bool::default()`），创建项目版本时要显式设为 `true`。我们在创建项目版本的 API 里负责显式初始化。

- [ ] **Step 2: 编译验证**

```bash
cd server && cargo build 2>&1 | head -60
```

Expected: 由于 `api.rs`、`storage.rs`、`serve.rs` 还引用 `active_versions`，会有多处编译错误。这是预期的，下一个任务开始修复。

- [ ] **Step 3: 提交**

```bash
git add server/src/config.rs
git commit -m "refactor(server): replace active_versions with project_versions tree"
```

---

## Task 2: 服务端 - Storage 层重构（storage.rs）

**目标：** 让所有路径方法多带 `project_version` 段；新增 `delete_project_version`；删除 `activate_version` 中的文件复制逻辑。

**Files:**
- Modify: `server/src/storage.rs`

- [ ] **Step 1: 改造路径方法**

把 `server/src/storage.rs` 第 20-34 行的路径方法替换为：

```rust
pub fn project_dir(&self, project_name: &str) -> Result<PathBuf, String> {
    Ok(self.resources_dir.join(sanitize_path_component(project_name)?))
}

pub fn project_version_dir(
    &self,
    project_name: &str,
    project_version: &str,
) -> Result<PathBuf, String> {
    Ok(self.project_dir(project_name)?.join(sanitize_path_component(project_version)?))
}

pub fn platform_dir(
    &self,
    project_name: &str,
    project_version: &str,
    platform: &str,
) -> Result<PathBuf, String> {
    Ok(self
        .project_version_dir(project_name, project_version)?
        .join(sanitize_path_component(platform)?))
}

pub fn versions_dir(
    &self,
    project_name: &str,
    project_version: &str,
    platform: &str,
) -> Result<PathBuf, String> {
    Ok(self.platform_dir(project_name, project_version, platform)?.join("_versions"))
}

pub fn version_dir(
    &self,
    project_name: &str,
    project_version: &str,
    platform: &str,
    version: &str,
) -> Result<PathBuf, String> {
    Ok(self
        .versions_dir(project_name, project_version, platform)?
        .join(sanitize_path_component(version)?))
}
```

- [ ] **Step 2: 改造 save_uploaded_file（多带 project_version 参数）**

把 `save_uploaded_file` 替换为：

```rust
pub fn save_uploaded_file(
    &self,
    project_name: &str,
    project_version: &str,
    platform: &str,
    version: &str,
    file_name: &str,
    data: &[u8],
) -> Result<(), String> {
    sanitize_path_component(file_name)?;
    let dir = self.version_dir(project_name, project_version, platform, version)?;
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create version dir: {}", e))?;
    let path = dir.join(file_name);
    fs::write(&path, data).map_err(|e| format!("Failed to write file: {}", e))?;
    Ok(())
}
```

- [ ] **Step 3: 改造 list_versions / list_files / list_files_with_hash / copy_files_from_version / delete_version**

每个方法在签名里增加 `project_version: &str`，并把内部 `self.versions_dir(...)` / `self.version_dir(...)` / `self.platform_dir(...)` 调用都补上 `project_version` 参数。

具体修改：

```rust
pub fn list_versions(
    &self,
    project_name: &str,
    project_version: &str,
    platform: &str,
) -> Vec<VersionEntry> {
    let dir = match self.versions_dir(project_name, project_version, platform) {
        Ok(d) => d,
        Err(_) => return vec![],
    };
    // 其余逻辑不变
    if !dir.exists() {
        return vec![];
    }
    let mut entries = Vec::new();
    if let Ok(read_dir) = fs::read_dir(&dir) {
        for entry in read_dir.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let version = entry.file_name().to_string_lossy().to_string();
            let mut file_count = 0u32;
            let mut total_size = 0u64;
            if let Ok(files) = fs::read_dir(entry.path()) {
                for f in files.flatten() {
                    if f.path().is_file() {
                        file_count += 1;
                        total_size += fs::metadata(f.path()).map(|m| m.len()).unwrap_or(0);
                    }
                }
            }
            let modified_timestamp = fs::metadata(entry.path())
                .and_then(|m| m.modified())
                .ok()
                .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())
                .unwrap_or(0);
            entries.push(VersionEntry {
                version,
                file_count,
                total_size,
                modified_timestamp,
            });
        }
    }
    entries.sort_by(|a, b| b.modified_timestamp.cmp(&a.modified_timestamp));
    entries
}

pub fn delete_version(
    &self,
    project_name: &str,
    project_version: &str,
    platform: &str,
    version: &str,
) -> Result<(), String> {
    let dir = self.version_dir(project_name, project_version, platform, version)?;
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| format!("Failed to delete version: {}", e))?;
    }
    Ok(())
}

pub fn list_files(
    &self,
    project_name: &str,
    project_version: &str,
    platform: &str,
    version: &str,
) -> Result<Vec<FileEntry>, String> {
    let dir = self.version_dir(project_name, project_version, platform, version)?;
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut entries = Vec::new();
    let read_dir = fs::read_dir(&dir).map_err(|e| format!("读取目录失败: {}", e))?;
    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let metadata = match fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let size = metadata.len();
        let modified_timestamp = metadata
            .modified()
            .ok()
            .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())
            .unwrap_or(0);
        entries.push(FileEntry { name, size, modified_timestamp });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

pub fn list_files_with_hash(
    &self,
    project_name: &str,
    project_version: &str,
    platform: &str,
    version: &str,
) -> Result<Vec<FileManifestEntry>, String> {
    use md5::{Digest, Md5};
    let dir = self.version_dir(project_name, project_version, platform, version)?;
    if !dir.exists() {
        return Err(format!("版本目录不存在: {}", version));
    }
    let mut entries = Vec::new();
    let read_dir = fs::read_dir(&dir).map_err(|e| format!("读取目录失败: {}", e))?;
    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let bytes = fs::read(&path).map_err(|e| format!("读取文件 {} 失败: {}", name, e))?;
        let size = bytes.len() as u64;
        let mut hasher = Md5::new();
        hasher.update(&bytes);
        let md5 = format!("{:x}", hasher.finalize());
        entries.push(FileManifestEntry { name, size, md5 });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

pub fn copy_files_from_version(
    &self,
    project_name: &str,
    project_version: &str,
    platform: &str,
    new_version: &str,
    base_version: &str,
    copy_files: &[String],
) -> Result<u32, String> {
    let base_dir = self.version_dir(project_name, project_version, platform, base_version)?;
    if !base_dir.exists() {
        return Err(format!("基础版本不存在: {}", base_version));
    }
    let new_dir = self.version_dir(project_name, project_version, platform, new_version)?;
    fs::create_dir_all(&new_dir).map_err(|e| format!("创建版本目录失败: {}", e))?;
    let mut count = 0u32;
    for name in copy_files {
        sanitize_path_component(name)?;
        let src = base_dir.join(name);
        if !src.exists() {
            return Err(format!("基础版本中不存在文件: {}", name));
        }
        let dst = new_dir.join(name);
        fs::copy(&src, &dst).map_err(|e| format!("复制文件 {} 失败: {}", name, e))?;
        count += 1;
    }
    Ok(count)
}
```

- [ ] **Step 4: 删除 activate_version；新增 delete_project_version**

把原 `activate_version` 方法（旧第 99-143 行，复制文件到平台根目录的逻辑）整体删除。新增：

```rust
pub fn delete_project_version(
    &self,
    project_name: &str,
    project_version: &str,
) -> Result<(), String> {
    let dir = self.project_version_dir(project_name, project_version)?;
    if dir.exists() {
        fs::remove_dir_all(&dir)
            .map_err(|e| format!("删除项目版本目录失败: {}", e))?;
    }
    Ok(())
}
```

（`delete_project` 保留不变，仍用于级联删除整个项目。）

- [ ] **Step 5: 添加路径单测**

在 `storage.rs` 末尾添加：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_dir_includes_project_version_segment() {
        let s = Storage::new(PathBuf::from("/tmp/res"));
        let p = s.version_dir("MyProject", "v1", "Android", "1.0.0").unwrap();
        assert_eq!(
            p,
            PathBuf::from("/tmp/res/MyProject/v1/Android/_versions/1.0.0")
        );
    }

    #[test]
    fn rejects_path_traversal_in_components() {
        let s = Storage::new(PathBuf::from("/tmp/res"));
        assert!(s.version_dir("..", "v1", "Android", "1.0.0").is_err());
        assert!(s.version_dir("p", "..", "Android", "1.0.0").is_err());
        assert!(s.version_dir("p", "v1", "/etc", "1.0.0").is_err());
    }
}
```

- [ ] **Step 6: 单测通过**

```bash
cd server && cargo test --lib storage 2>&1 | tail -20
```

Expected: 两个测试 PASS。如果整体 `cargo test` 失败是因为 api.rs/serve.rs 还没改完，通过 `--lib storage` 单独跑 storage 模块即可。

- [ ] **Step 7: 提交**

```bash
git add server/src/storage.rs
git commit -m "refactor(server): storage paths include project_version segment"
```

---

## Task 3: 服务端 - 资源 URL 路由（serve.rs）

**目标：** 把 `/res/*path` 处理改为 `/res/{pver}/{project}/{platform}/{file...}`；按设计文档的 5 步流程返回 200/403/404。

**Files:**
- Modify: `server/src/serve.rs`

- [ ] **Step 1: 重写 serve_resource**

把 `server/src/serve.rs` 第 39-90 行的 `serve_resource` 整体替换为：

```rust
pub async fn serve_resource(
    State(state): State<Arc<AppState>>,
    req: Request,
) -> Response {
    let raw_path = req.uri().path().trim_start_matches("/res/").to_string();
    let decoded = percent_decode_str(&raw_path).decode_utf8_lossy().to_string();

    // 拆出前 3 段：project_version / project_name / platform
    let mut parts = decoded.splitn(4, '/');
    let project_version = parts.next().unwrap_or("");
    let project_name = parts.next().unwrap_or("");
    let platform = parts.next().unwrap_or("");
    let file_path = parts.next().unwrap_or("");

    if project_version.is_empty() || project_name.is_empty() || platform.is_empty() || file_path.is_empty() {
        broadcast_request_log(&state, 404, "GET", &raw_path, "");
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    }

    // 1) 查项目
    let config = state.app_config.read().await;
    let project = match config.projects.iter().find(|p| p.project_name == project_name) {
        Some(p) => p.clone(),
        None => {
            drop(config);
            broadcast_request_log(&state, 404, "GET", &raw_path, "");
            return (StatusCode::NOT_FOUND, "Not found").into_response();
        }
    };

    // 2) 查项目版本
    let pv = match project.project_versions.iter().find(|v| v.name == project_version) {
        Some(v) => v.clone(),
        None => {
            drop(config);
            broadcast_request_log(&state, 404, "GET", &raw_path, &project.id);
            return (StatusCode::NOT_FOUND, "Not found").into_response();
        }
    };
    drop(config);

    // 3) 平台访问开关
    let settings = match pv.platform_settings.get(platform) {
        Some(s) => s.clone(),
        None => {
            broadcast_request_log(&state, 404, "GET", &raw_path, &project.id);
            return (StatusCode::NOT_FOUND, "Not found").into_response();
        }
    };
    if !settings.access_enabled {
        broadcast_request_log(&state, 403, "GET", &raw_path, &project.id);
        return (StatusCode::FORBIDDEN, "Forbidden").into_response();
    }

    // 4) 激活的 bundle
    let active_bundle = match settings.active_bundle.as_deref() {
        Some(v) if !v.is_empty() => v.to_string(),
        _ => {
            broadcast_request_log(&state, 404, "GET", &raw_path, &project.id);
            return (StatusCode::NOT_FOUND, "No active bundle").into_response();
        }
    };

    // 5) 文件读取
    let storage = crate::storage::Storage::new(state.server_config.resources_dir());
    let version_dir = match storage.version_dir(project_name, project_version, platform, &active_bundle) {
        Ok(d) => d,
        Err(_) => {
            broadcast_request_log(&state, 404, "GET", &raw_path, &project.id);
            return (StatusCode::NOT_FOUND, "Not found").into_response();
        }
    };

    let file_full = version_dir.join(file_path);
    let canonical = match file_full.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            broadcast_request_log(&state, 404, "GET", &raw_path, &project.id);
            return (StatusCode::NOT_FOUND, "Not found").into_response();
        }
    };

    let resources_canonical = state
        .server_config
        .resources_dir()
        .canonicalize()
        .unwrap_or_else(|_| state.server_config.resources_dir().clone());
    if !canonical.starts_with(&resources_canonical) {
        broadcast_request_log(&state, 403, "GET", &raw_path, &project.id);
        return (StatusCode::FORBIDDEN, "Forbidden").into_response();
    }

    if !canonical.is_file() {
        broadcast_request_log(&state, 404, "GET", &raw_path, &project.id);
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    }

    match tokio::fs::read(&canonical).await {
        Ok(bytes) => {
            broadcast_request_log(&state, 200, "GET", &raw_path, &project.id);
            let ext = canonical.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
            let mime = match ext.as_str() {
                "version" | "hash" | "report" => "text/plain".to_string(),
                "json" => "application/json".to_string(),
                _ => from_path(&canonical).first_or_octet_stream().to_string(),
            };
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, format!("{}; charset=utf-8", mime).parse().unwrap());
            headers.insert(header::ACCEPT_RANGES, "bytes".parse().unwrap());
            (StatusCode::OK, headers, bytes).into_response()
        }
        Err(_) => {
            broadcast_request_log(&state, 404, "GET", &raw_path, &project.id);
            (StatusCode::NOT_FOUND, "Not found").into_response()
        }
    }
}
```

- [ ] **Step 2: 改造日志辅助函数**

把第 92-95 行的 `broadcast_request_log` 替换为带 `project_id` 参数的版本：

```rust
fn broadcast_request_log(state: &Arc<AppState>, status: u16, method: &str, path: &str, project_id: &str) {
    ws::broadcast_log(state, ws::make_log("request", status, method, path, project_id, ""));
}
```

- [ ] **Step 3: 提交（编译还会失败，api.rs 未改）**

```bash
git add server/src/serve.rs
git commit -m "refactor(server): resource URL parses project_version segment"
```

---

## Task 4: 服务端 - API 路由与处理函数（api.rs）

**目标：** 改造现有 8 个处理函数，新增 4 个项目版本相关 API，并更新 `build_router`。

**Files:**
- Modify: `server/src/api.rs`

- [ ] **Step 1: 更新 build_router 路由表**

替换 `server/src/api.rs` 第 19-55 行的 `build_router`：

```rust
pub fn build_router(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS])
        .allow_headers(Any);

    let public = Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/login", post(auth::login));

    let protected = Router::new()
        .route("/api/projects", get(list_projects))
        .route("/api/projects", post(create_project))
        .route("/api/projects/:id", put(update_project))
        .route("/api/projects/:id", delete(delete_project))
        // 新增：项目版本 CRUD
        .route("/api/projects/:id/project-versions", get(list_project_versions))
        .route("/api/projects/:id/project-versions", post(create_project_version))
        .route("/api/projects/:id/project-versions/:pver", delete(delete_project_version))
        .route(
            "/api/projects/:id/project-versions/:pver/platforms/:plat/access",
            put(set_platform_access),
        )
        // 改造：上传 / 版本 / 文件 / 状态（路径前缀加 project-versions/:pver）
        .route(
            "/api/projects/:id/project-versions/:pver/upload",
            post(upload_resources).layer(DefaultBodyLimit::max(512 * 1024 * 1024)),
        )
        .route(
            "/api/projects/:id/project-versions/:pver/incremental-upload",
            post(incremental_upload).layer(DefaultBodyLimit::max(512 * 1024 * 1024)),
        )
        .route(
            "/api/projects/:id/project-versions/:pver/manifest",
            get(get_version_manifest),
        )
        .route(
            "/api/projects/:id/project-versions/:pver/versions",
            get(list_versions),
        )
        .route(
            "/api/projects/:id/project-versions/:pver/versions/:ver/activate",
            put(activate_version),
        )
        .route(
            "/api/projects/:id/project-versions/:pver/versions/:ver",
            delete(delete_version),
        )
        .route(
            "/api/projects/:id/project-versions/:pver/status",
            get(project_status),
        )
        .route(
            "/api/projects/:id/project-versions/:pver/files",
            get(list_files),
        )
        .layer(middleware::from_fn_with_state(state.clone(), auth::auth_middleware));

    let ws_route = Router::new().route("/api/ws/logs", get(ws::ws_logs));

    Router::new()
        .merge(public)
        .merge(protected)
        .merge(ws_route)
        .route("/res/*path", get(serve::serve_resource))
        .fallback(get(serve::serve_spa))
        .layer(cors)
        .with_state(state)
}
```

- [ ] **Step 2: 新增 4 个项目版本 API**

在 `api.rs` 文件末尾追加：

```rust
#[derive(Deserialize)]
struct CreateProjectVersionRequest {
    name: String,
}

async fn list_project_versions(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<crate::config::ProjectVersion>>, StatusCode> {
    let config = state.app_config.read().await;
    let project = config.projects.iter().find(|p| p.id == id).ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(project.project_versions.clone()))
}

async fn create_project_version(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<CreateProjectVersionRequest>,
) -> Result<Json<crate::config::ProjectVersion>, (StatusCode, String)> {
    let name = req.name.trim().to_string();
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
    {
        return Err((StatusCode::BAD_REQUEST, "Invalid project version name".to_string()));
    }
    let mut config = state.app_config.write().await;
    let project = config
        .projects
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))?;
    if project.project_versions.iter().any(|v| v.name == name) {
        return Err((StatusCode::CONFLICT, "Project version already exists".to_string()));
    }
    let mut platform_settings = std::collections::HashMap::new();
    for plat in &project.platforms {
        platform_settings.insert(
            plat.clone(),
            crate::config::PlatformSettings {
                access_enabled: true,
                active_bundle: None,
            },
        );
    }
    let pv = crate::config::ProjectVersion {
        name: name.clone(),
        platform_settings,
    };
    project.project_versions.push(pv.clone());
    config
        .save(&state.server_config.config_path())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(pv))
}

async fn delete_project_version(
    State(state): State<Arc<AppState>>,
    Path((id, pver)): Path<(String, String)>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut config = state.app_config.write().await;
    let project = config
        .projects
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))?;
    let project_name = project.project_name.clone();
    let before = project.project_versions.len();
    project.project_versions.retain(|v| v.name != pver);
    if project.project_versions.len() == before {
        return Err((StatusCode::NOT_FOUND, "Project version not found".to_string()));
    }
    config
        .save(&state.server_config.config_path())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let storage = Storage::new(state.server_config.resources_dir());
    storage
        .delete_project_version(&project_name, &pver)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
struct SetAccessRequest {
    enabled: bool,
}

async fn set_platform_access(
    State(state): State<Arc<AppState>>,
    Path((id, pver, plat)): Path<(String, String, String)>,
    Json(req): Json<SetAccessRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut config = state.app_config.write().await;
    let project = config
        .projects
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))?;
    let pv = project
        .project_versions
        .iter_mut()
        .find(|v| v.name == pver)
        .ok_or((StatusCode::NOT_FOUND, "Project version not found".to_string()))?;
    let entry = pv
        .platform_settings
        .entry(plat.clone())
        .or_insert_with(|| crate::config::PlatformSettings {
            access_enabled: false,
            active_bundle: None,
        });
    entry.access_enabled = req.enabled;
    config
        .save(&state.server_config.config_path())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::OK)
}
```

- [ ] **Step 3: 改造 8 个现有处理函数：函数签名加 :pver；内部 storage 调用补参数；activate_version 改为只更元数据**

逐个替换。

```rust
async fn upload_resources(
    State(state): State<Arc<AppState>>,
    Path((id, pver)): Path<(String, String)>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let config = state.app_config.read().await;
    let project = config.projects.iter().find(|p| p.id == id)
        .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))?;
    if !project.project_versions.iter().any(|v| v.name == pver) {
        return Err((StatusCode::NOT_FOUND, "Project version not found".to_string()));
    }
    let project_name = project.project_name.clone();
    drop(config);

    let storage = Storage::new(state.server_config.resources_dir());

    let mut platform = String::new();
    let mut version = String::new();
    let mut file_count = 0u32;

    while let Some(field) = multipart.next_field().await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "platform" => {
                platform = field.text().await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            }
            "version" => {
                version = field.text().await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            }
            "files" => {
                let file_name = field.file_name().unwrap_or("unknown").to_string();
                let data = field.bytes().await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if platform.is_empty() {
                    return Err((StatusCode::BAD_REQUEST, "platform field must come before files".to_string()));
                }
                if version.is_empty() {
                    version = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
                }
                storage.save_uploaded_file(&project_name, &pver, &platform, &version, &file_name, &data)
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
                file_count += 1;
            }
            _ => {}
        }
    }

    ws::broadcast_log(&state, ws::make_log(
        "upload", 200, "POST",
        &format!("/api/projects/{}/project-versions/{}/upload", id, pver),
        &id,
        &format!("Uploaded {} files, version: {}, platform: {}", file_count, version, platform),
    ));

    Ok(Json(serde_json::json!({
        "success": true,
        "version": version,
        "platform": platform,
        "file_count": file_count
    })))
}

async fn incremental_upload(
    State(state): State<Arc<AppState>>,
    Path((id, pver)): Path<(String, String)>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let config = state.app_config.read().await;
    let project = config.projects.iter().find(|p| p.id == id)
        .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))?;
    if !project.project_versions.iter().any(|v| v.name == pver) {
        return Err((StatusCode::NOT_FOUND, "Project version not found".to_string()));
    }
    let project_name = project.project_name.clone();
    drop(config);

    let storage = Storage::new(state.server_config.resources_dir());

    let mut platform = String::new();
    let mut version = String::new();
    let mut base_version = String::new();
    let mut copy_files: Vec<String> = Vec::new();
    let mut copied_count = 0u32;
    let mut copied_done = false;
    let mut uploaded_count = 0u32;

    while let Some(field) = multipart.next_field().await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "platform" => {
                platform = field.text().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            }
            "version" => {
                version = field.text().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            }
            "base_version" => {
                base_version = field.text().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            }
            "copy_files" => {
                let text = field.text().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                copy_files = serde_json::from_str(&text)
                    .map_err(|e| (StatusCode::BAD_REQUEST, format!("解析 copy_files 失败: {}", e)))?;
            }
            "files" => {
                if platform.is_empty() {
                    return Err((StatusCode::BAD_REQUEST, "platform 字段必须先于 files".to_string()));
                }
                if version.is_empty() {
                    version = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
                }
                if base_version.is_empty() {
                    return Err((StatusCode::BAD_REQUEST, "base_version 字段必须先于 files".to_string()));
                }
                if !copied_done {
                    copied_count = storage
                        .copy_files_from_version(&project_name, &pver, &platform, &version, &base_version, &copy_files)
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
                    copied_done = true;
                }
                let file_name = field.file_name().unwrap_or("unknown").to_string();
                let data = field.bytes().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                storage.save_uploaded_file(&project_name, &pver, &platform, &version, &file_name, &data)
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
                uploaded_count += 1;
            }
            _ => {}
        }
    }

    if !copied_done {
        if platform.is_empty() || base_version.is_empty() {
            return Err((StatusCode::BAD_REQUEST, "缺少 platform 或 base_version".to_string()));
        }
        if version.is_empty() {
            version = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
        }
        copied_count = storage
            .copy_files_from_version(&project_name, &pver, &platform, &version, &base_version, &copy_files)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    }

    ws::broadcast_log(&state, ws::make_log(
        "upload", 200, "POST",
        &format!("/api/projects/{}/project-versions/{}/incremental-upload", id, pver),
        &id,
        &format!("增量上传 v{}: 复用 {} 个 + 新上传 {} 个 (基于 {})", version, copied_count, uploaded_count, base_version),
    ));

    Ok(Json(serde_json::json!({
        "success": true,
        "version": version,
        "platform": platform,
        "base_version": base_version,
        "copied_count": copied_count,
        "uploaded_count": uploaded_count,
        "file_count": copied_count + uploaded_count
    })))
}

#[derive(Deserialize)]
struct ManifestQuery {
    platform: String,
    version: String,
}

async fn get_version_manifest(
    State(state): State<Arc<AppState>>,
    Path((id, pver)): Path<(String, String)>,
    axum::extract::Query(params): axum::extract::Query<ManifestQuery>,
) -> Result<Json<Vec<crate::storage::FileManifestEntry>>, (StatusCode, String)> {
    let config = state.app_config.read().await;
    let project = config.projects.iter().find(|p| p.id == id)
        .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))?;
    let project_name = project.project_name.clone();
    drop(config);
    let storage = Storage::new(state.server_config.resources_dir());
    let entries = storage
        .list_files_with_hash(&project_name, &pver, &params.platform, &params.version)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(entries))
}

#[derive(Deserialize)]
struct ListVersionsQuery {
    platform: Option<String>,
}

async fn list_versions(
    State(state): State<Arc<AppState>>,
    Path((id, pver)): Path<(String, String)>,
    axum::extract::Query(params): axum::extract::Query<ListVersionsQuery>,
) -> Result<Json<Vec<crate::storage::VersionEntry>>, StatusCode> {
    let config = state.app_config.read().await;
    let project = config.projects.iter().find(|p| p.id == id).ok_or(StatusCode::NOT_FOUND)?;
    let project_name = project.project_name.clone();
    drop(config);
    let platform = params.platform.unwrap_or_else(|| "Android".to_string());
    let storage = Storage::new(state.server_config.resources_dir());
    Ok(Json(storage.list_versions(&project_name, &pver, &platform)))
}

#[derive(Deserialize)]
struct ActivateQuery {
    platform: Option<String>,
}

async fn activate_version(
    State(state): State<Arc<AppState>>,
    Path((id, pver, ver)): Path<(String, String, String)>,
    axum::extract::Query(params): axum::extract::Query<ActivateQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let platform = params.platform.unwrap_or_else(|| "Android".to_string());
    let mut config = state.app_config.write().await;
    let project = config.projects.iter_mut().find(|p| p.id == id)
        .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))?;
    let pv = project.project_versions.iter_mut().find(|v| v.name == pver)
        .ok_or((StatusCode::NOT_FOUND, "Project version not found".to_string()))?;
    let entry = pv
        .platform_settings
        .entry(platform.clone())
        .or_insert_with(|| crate::config::PlatformSettings {
            access_enabled: true,
            active_bundle: None,
        });
    entry.active_bundle = Some(ver.clone());
    config.save(&state.server_config.config_path())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    ws::broadcast_log(&state, ws::make_log(
        "activate", 200, "PUT",
        &format!("/api/projects/{}/project-versions/{}/versions/{}/activate", id, pver, ver),
        &id,
        &format!("Activated {} on {} ({})", ver, platform, pver),
    ));

    Ok(Json(serde_json::json!({
        "success": true,
        "project_version": pver,
        "version": ver,
        "platform": platform,
    })))
}

async fn delete_version(
    State(state): State<Arc<AppState>>,
    Path((id, pver, ver)): Path<(String, String, String)>,
    axum::extract::Query(params): axum::extract::Query<ActivateQuery>,
) -> Result<StatusCode, (StatusCode, String)> {
    let platform = params.platform.unwrap_or_else(|| "Android".to_string());
    let config = state.app_config.read().await;
    let project = config.projects.iter().find(|p| p.id == id)
        .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))?;
    let project_name = project.project_name.clone();
    drop(config);
    let storage = Storage::new(state.server_config.resources_dir());
    storage.delete_version(&project_name, &pver, &platform, &ver)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::OK)
}

async fn project_status(
    State(state): State<Arc<AppState>>,
    Path((id, pver)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let config = state.app_config.read().await;
    let project = config.projects.iter().find(|p| p.id == id).ok_or(StatusCode::NOT_FOUND)?;
    let pv = project.project_versions.iter().find(|v| v.name == pver).ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(serde_json::json!({
        "id": project.id,
        "project_name": project.project_name,
        "project_version": pv.name,
        "platform_settings": pv.platform_settings,
    })))
}

#[derive(Deserialize)]
struct ListFilesQuery {
    platform: Option<String>,
    version: String,
}

async fn list_files(
    State(state): State<Arc<AppState>>,
    Path((id, pver)): Path<(String, String)>,
    axum::extract::Query(params): axum::extract::Query<ListFilesQuery>,
) -> Result<Json<Vec<crate::storage::FileEntry>>, (StatusCode, String)> {
    let config = state.app_config.read().await;
    let project = config.projects.iter().find(|p| p.id == id)
        .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))?;
    let project_name = project.project_name.clone();
    drop(config);
    let platform = params.platform.unwrap_or_else(|| "Android".to_string());
    let storage = Storage::new(state.server_config.resources_dir());
    let files = storage.list_files(&project_name, &pver, &platform, &params.version)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(files))
}
```

注意：`list_files` 的 `version` 参数现在是必填（不再有"激活=平台根目录"的语义）。前端调用时永远要传具体 bundle 版本。

`make_log` 的签名是 `(type, status, method, path, project_id, message)` —— 上面的 `broadcast_log` 调用都对应了 6 个参数。

- [ ] **Step 4: 编译并修复任何残余错误**

```bash
cd server && cargo build 2>&1 | tail -40
```

Expected: 应当编译通过。如果有 import / 类型错误根据提示修复（常见：`std::collections::HashMap` 在 api.rs 顶部未引入，按需 `use std::collections::HashMap;`）。

- [ ] **Step 5: 单测通过**

```bash
cd server && cargo test 2>&1 | tail -10
```

Expected: PASS（含 storage 的单测）。

- [ ] **Step 6: 提交**

```bash
git add server/src/api.rs server/src/serve.rs
git commit -m "feat(server): project-versions API + new resource URL routing"
```

---

## Task 5: 服务端 - 启动冒烟测试

**目标：** 启动服务端，用 curl 验证关键 API。

**Files:** 无新增。

- [ ] **Step 1: 启动服务端**

```bash
cd server && DATA_DIR=/tmp/tengine-smoke-data ADMIN_PASSWORD=test123 PORT=18082 cargo run 2>&1 &
sleep 3
```

- [ ] **Step 2: 登录拿 token**

```bash
HASH=$(printf 'test123' | shasum -a 256 | awk '{print $1}')
TOKEN=$(curl -s -X POST http://127.0.0.1:18082/api/auth/login \
  -H 'Content-Type: application/json' \
  -d "{\"password\":\"$HASH\"}" | python3 -c 'import sys,json;print(json.load(sys.stdin)["token"])')
echo "TOKEN=$TOKEN"
```

Expected: 打印一段 JWT。

- [ ] **Step 3: 创建项目 + 项目版本 + 上传一个文件**

```bash
PID=$(curl -s -X POST http://127.0.0.1:18082/api/projects \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"project_name":"Demo"}' | python3 -c 'import sys,json;print(json.load(sys.stdin)["id"])')
echo "PID=$PID"

curl -s -X POST "http://127.0.0.1:18082/api/projects/$PID/project-versions" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"name":"v1"}'

echo 'hello' > /tmp/hello.bundle
curl -s -X POST "http://127.0.0.1:18082/api/projects/$PID/project-versions/v1/upload" \
  -H "Authorization: Bearer $TOKEN" \
  -F 'platform=Android' -F 'version=1.0.0' -F 'files=@/tmp/hello.bundle'

curl -s -X PUT "http://127.0.0.1:18082/api/projects/$PID/project-versions/v1/versions/1.0.0/activate?platform=Android" \
  -H "Authorization: Bearer $TOKEN"
```

Expected: 各步均返回 200 / JSON。

- [ ] **Step 4: 访问资源（开关默认开）**

```bash
curl -i "http://127.0.0.1:18082/res/v1/Demo/Android/hello.bundle"
```

Expected: HTTP 200，body 为 `hello`。

- [ ] **Step 5: 关闭平台访问开关，再请求应 403**

```bash
curl -s -X PUT "http://127.0.0.1:18082/api/projects/$PID/project-versions/v1/platforms/Android/access" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"enabled":false}'

curl -i "http://127.0.0.1:18082/res/v1/Demo/Android/hello.bundle"
```

Expected: HTTP 403。

- [ ] **Step 6: 老 URL 应 404**

```bash
curl -i "http://127.0.0.1:18082/res/Demo/Android/hello.bundle"
```

Expected: HTTP 404。

- [ ] **Step 7: 收尾**

```bash
kill %1 2>/dev/null; rm -rf /tmp/tengine-smoke-data /tmp/hello.bundle
```

冒烟通过则继续。

- [ ] **Step 8: 提交**（无代码改动则跳过）

---

## Task 6: 前端 API 层（src/api/remote.ts）

**目标：** TS 客户端方法签名加 `projectVersion` 参数；新增项目版本 CRUD 与平台开关方法。

**Files:**
- Modify: `src/api/remote.ts`

- [ ] **Step 1: 更新类型定义**

把 `src/api/remote.ts` 第 15-50 行的接口块替换为：

```ts
export interface PlatformSettings {
  access_enabled: boolean;
  active_bundle: string | null;
}

export interface ProjectVersion {
  name: string;
  platform_settings: Record<string, PlatformSettings>;
}

export interface ProjectConfig {
  id: string;
  project_name: string;
  platforms: string[];
  package_name: string;
  project_versions: ProjectVersion[];
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

export interface FileManifestEntry {
  name: string;
  size: number;
  md5: string;
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
```

- [ ] **Step 2: 改造 RemoteApi 方法**

把 `RemoteApi` 类中第 124-208 行（自 `listProjects` 起到 `listFiles` 止）替换为：

```ts
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

async listProjectVersions(projectId: string): Promise<ProjectVersion[]> {
  return this.request(`/api/projects/${projectId}/project-versions`);
}

async createProjectVersion(projectId: string, name: string): Promise<ProjectVersion> {
  return this.request(`/api/projects/${projectId}/project-versions`, {
    method: "POST",
    body: JSON.stringify({ name }),
  });
}

async deleteProjectVersion(projectId: string, name: string): Promise<void> {
  await this.request(
    `/api/projects/${projectId}/project-versions/${encodeURIComponent(name)}`,
    { method: "DELETE" },
  );
}

async setPlatformAccess(
  projectId: string,
  projectVersion: string,
  platform: string,
  enabled: boolean,
): Promise<void> {
  await this.request(
    `/api/projects/${projectId}/project-versions/${encodeURIComponent(projectVersion)}/platforms/${encodeURIComponent(platform)}/access`,
    {
      method: "PUT",
      body: JSON.stringify({ enabled }),
    },
  );
}

async uploadResources(
  projectId: string,
  projectVersion: string,
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
  return this.request(
    `/api/projects/${projectId}/project-versions/${encodeURIComponent(projectVersion)}/upload`,
    {
      method: "POST",
      body: formData,
    },
  );
}

async listVersions(
  projectId: string,
  projectVersion: string,
  platform: string,
): Promise<VersionEntry[]> {
  return this.request(
    `/api/projects/${projectId}/project-versions/${encodeURIComponent(projectVersion)}/versions?platform=${encodeURIComponent(platform)}`,
  );
}

async activateVersion(
  projectId: string,
  projectVersion: string,
  version: string,
  platform: string,
): Promise<void> {
  await this.request(
    `/api/projects/${projectId}/project-versions/${encodeURIComponent(projectVersion)}/versions/${encodeURIComponent(version)}/activate?platform=${encodeURIComponent(platform)}`,
    { method: "PUT" },
  );
}

async deleteVersion(
  projectId: string,
  projectVersion: string,
  version: string,
  platform: string,
): Promise<void> {
  await this.request(
    `/api/projects/${projectId}/project-versions/${encodeURIComponent(projectVersion)}/versions/${encodeURIComponent(version)}?platform=${encodeURIComponent(platform)}`,
    { method: "DELETE" },
  );
}

async getProjectStatus(
  projectId: string,
  projectVersion: string,
): Promise<{ platform_settings: Record<string, PlatformSettings> }> {
  return this.request(
    `/api/projects/${projectId}/project-versions/${encodeURIComponent(projectVersion)}/status`,
  );
}

async getVersionManifest(
  projectId: string,
  projectVersion: string,
  platform: string,
  version: string,
): Promise<FileManifestEntry[]> {
  const params = new URLSearchParams({ platform, version });
  return this.request(
    `/api/projects/${projectId}/project-versions/${encodeURIComponent(projectVersion)}/manifest?${params.toString()}`,
  );
}

async listFiles(
  projectId: string,
  projectVersion: string,
  platform: string,
  version: string,
): Promise<FileEntry[]> {
  const params = new URLSearchParams({ platform, version });
  return this.request(
    `/api/projects/${projectId}/project-versions/${encodeURIComponent(projectVersion)}/files?${params.toString()}`,
  );
}
```

- [ ] **Step 3: TypeScript 编译检查**

```bash
npx tsc --noEmit -p tsconfig.json 2>&1 | head -40
```

Expected: 仅 `RemoteMode.vue` 的引用错误（因 vue 文件还未改），`remote.ts` 自身无错。

- [ ] **Step 4: 提交**

```bash
git add src/api/remote.ts
git commit -m "feat(client): API client supports project versions and platform access"
```

---

## Task 7: Tauri 上传命令支持 project_version

**目标：** Tauri 端 `upload_version_to_remote` / `incremental_upload_to_remote` 把 `project_version` 拼进上传 URL。

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 给两个上传命令的签名加 project_version 参数，URL 模板加上对应段**

修改 `upload_version_to_remote`（约 422-503 行）：

签名加参数 `project_version: String`，把 URL 拼接行替换为：

```rust
let url = format!(
    "{}/api/projects/{}/project-versions/{}/upload",
    server_url.trim_end_matches('/'),
    project_id,
    urlencoding::encode(&project_version),
);
```

修改 `incremental_upload_to_remote`（约 555-628 行）：

签名加参数 `project_version: String`，把 URL 拼接行替换为：

```rust
let url = format!(
    "{}/api/projects/{}/project-versions/{}/incremental-upload",
    server_url.trim_end_matches('/'),
    project_id,
    urlencoding::encode(&project_version),
);
```

- [ ] **Step 2: 添加 urlencoding 依赖**

修改 `src-tauri/Cargo.toml`，在 `[dependencies]` 段下追加（如已存在请跳过）：

```toml
urlencoding = "2"
```

- [ ] **Step 3: 编译验证**

```bash
cd src-tauri && cargo build 2>&1 | tail -20
```

Expected: 编译通过（`run_in_background` 可能耗时几十秒到一分钟）。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/lib.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(tauri): upload commands carry project_version segment"
```

---

## Task 8: 前端 RemoteMode UI - 二级标签 + 数据流改造

**目标：** 在 `RemoteMode.vue` 中加二级标签栏（项目版本），所有版本操作绑定到当前选中的 ProjectVersion。

**Files:**
- Modify: `src/components/RemoteMode.vue`

- [ ] **Step 1: 在 `<script setup>` 顶部增加状态**

在 `const projects = ref<...>` 附近新增：

```ts
const activeProjectVersionName = ref<string>("");
const newProjectVersionName = ref<string>("");
const showCreateProjectVersion = ref(false);
const projectSettingsExpanded = ref(false);

const activeProjectVersion = computed<ProjectVersion | undefined>(() => {
  const proj = activeProject.value;
  if (!proj) return undefined;
  return proj.project_versions.find((v) => v.name === activeProjectVersionName.value);
});
```

将文件顶部的 `import` 行更新，确保引入 `ProjectVersion` 类型：

```ts
import { api, type ProjectConfig, type ProjectVersion, type VersionEntry, type LogEntry, type FileEntry, type FileManifestEntry } from "../api/remote";
```

- [ ] **Step 2: 切换项目时自动选中第一个项目版本**

修改 `loadProjects`：

```ts
async function loadProjects() {
  try {
    projects.value = await api.listProjects();
    if (projects.value.length > 0 && !activeProjectId.value) {
      activeProjectId.value = projects.value[0].id;
    }
    syncActiveProjectVersion();
    if (activeProjectVersion.value) await loadVersions();
  } catch {}
}

function syncActiveProjectVersion() {
  const proj = activeProject.value;
  if (!proj) {
    activeProjectVersionName.value = "";
    return;
  }
  if (
    !activeProjectVersionName.value ||
    !proj.project_versions.some((v) => v.name === activeProjectVersionName.value)
  ) {
    activeProjectVersionName.value = proj.project_versions[0]?.name || "";
  }
}

watch(() => activeProjectId.value, () => {
  syncActiveProjectVersion();
  if (activeProjectVersion.value) loadVersions();
});

watch(() => activeProjectVersionName.value, () => {
  if (activeProjectVersion.value) loadVersions();
});
```

- [ ] **Step 3: 新增项目版本的 CRUD 与平台开关方法**

```ts
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
```

- [ ] **Step 4: 改造 loadVersions / activateVersion / deleteVersion / openFileBrowser / startSync / confirmSync 等已有函数，使用 activeProjectVersionName**

```ts
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
  } catch {}
}

async function deleteVersion(version: string) {
  const project = activeProject.value;
  const pvName = activeProjectVersionName.value;
  if (!project || !pvName) return;
  try {
    await api.deleteVersion(project.id, pvName, version, selectedPlatform.value);
    await loadVersions();
  } catch {}
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
```

把现有 `startSync / confirmSync / computeDiff` 中的 `api.getVersionManifest(...)` 调用补 `activeProjectVersionName.value` 参数；`invoke` 调用 `upload_version_to_remote` / `incremental_upload_to_remote` 时传 `projectVersion: activeProjectVersionName.value`。

具体改动：

```ts
// computeDiff 中调用 api.getVersionManifest
const [local, remote] = await Promise.all([
  invoke<FileManifestEntry[]>("compute_local_manifest", { /* unchanged */ }),
  api.getVersionManifest(project.id, activeProjectVersionName.value, selectedPlatform.value, baseVersion),
]);

// confirmSync 中调用 invoke
if (useIncremental && diff.value) {
  // ... compute uploadFiles, copyFiles ...
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
```

- [ ] **Step 5: 替换 RemoteMode 的 connected 分支模板**

把 `<template>` 内 `<!-- Connected -->` 块（约第 754-916 行）替换为：

```vue
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
      v-for="pv in activeProject.project_versions"
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
        <div class="rm-fold-head" @click="projectSettingsExpanded = !projectSettingsExpanded">
          <span class="rm-fold-arrow" :class="{ open: projectSettingsExpanded }">▶</span>
          <span>项目设置</span>
          <span class="rm-fold-meta">{{ activeProject.project_name }} · {{ activeProject.platforms.join(', ') }}</span>
        </div>
        <div v-if="projectSettingsExpanded" class="rm-fold-body">
          <div class="config-row">
            <div class="config-field"><label>项目名称</label><input v-model="activeProject.project_name" /></div>
            <div class="config-field"><label>包名</label><input v-model="activeProject.package_name" /></div>
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

      <!-- Bundles dir -->
      <div class="config-row">
        <div class="config-field" style="flex:1">
          <label>BUNDLES 目录</label>
          <div style="display:flex;gap:8px;">
            <input :value="currentBundlesDir" readonly placeholder="选择本地 Bundles 目录..." style="flex:1;cursor:pointer" @click="selectBundlesDir" />
            <button class="btn btn-secondary" @click="selectBundlesDir">浏览</button>
          </div>
        </div>
      </div>

      <!-- Sync controls -->
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
        <div
          v-if="activeProjectVersion.platform_settings[selectedPlatform]?.active_bundle"
          class="server-url"
          style="margin-left:auto;"
        >
          当前激活: <strong>{{ activeProjectVersion.platform_settings[selectedPlatform].active_bundle }}</strong>
        </div>
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

  <!-- Sync / Diff / FileBrowser dialogs (unchanged) -->
  <!-- (保留原有的 syncDialog、diffDialogOpen、fileBrowser 三个对话框模板，原样不动) -->

  <!-- Log Panel (unchanged) -->
  <!-- (保留原有日志面板) -->
</template>
```

注意：本步骤的目的是**重写 `<template v-else>` 直到 Bundle 版本列表为止**。原模板里的三个 dialog（同步选版本、差异详情、文件浏览）和底部日志面板**保留不动**，只修改主面板部分。

- [ ] **Step 6: 添加新增样式**

在 `<style scoped>` 末尾追加：

```css
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
  padding: 8px 12px;
  cursor: pointer;
  background: var(--bg-tertiary);
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
}
.rm-fold-head:hover { background: var(--bg-secondary); }
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
.rm-fold-body { padding: 10px 12px; }

.rm-access-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 12px;
  background: var(--bg-tertiary);
  border: 1px solid var(--border);
  border-radius: 8px;
  margin: 8px 0;
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
```

- [ ] **Step 7: TypeScript / Vue 编译检查**

```bash
npx tsc --noEmit -p tsconfig.json 2>&1 | head -40
```

Expected: 0 错误。

- [ ] **Step 8: 提交**

```bash
git add src/components/RemoteMode.vue
git commit -m "feat(remote-mode): two-level tabs and platform access toggles"
```

---

## Task 9: 前端 LocalMode UI - 平台访问开关

**目标：** 本地模式不引入项目版本，只在平台多选 tag 旁边内联一个访问开关。

**Files:**
- Modify: `src-tauri/src/config.rs`、`src-tauri/src/server.rs`、`src/components/LocalMode.vue`

- [ ] **Step 1: Tauri 配置增加 platform_access 字段**

修改 `src-tauri/src/config.rs` 第 6-22 行的 `ProjectConfig`：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub id: String,
    pub project_name: String,
    pub bundles_dir: String,
    pub port: u16,
    pub platforms: Vec<String>,
    pub cors_enabled: bool,
    pub package_name: String,
    #[serde(default)]
    pub platform_access: std::collections::HashMap<String, bool>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        let mut platform_access = std::collections::HashMap::new();
        platform_access.insert("Android".to_string(), true);
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            project_name: "TEngine".to_string(),
            bundles_dir: String::new(),
            port: 8081,
            platforms: vec!["Android".to_string()],
            cors_enabled: true,
            package_name: "DefaultPackage".to_string(),
            platform_access,
        }
    }
}
```

- [ ] **Step 2: Tauri server 在响应文件请求时检查开关**

修改 `src-tauri/src/server.rs`：

把 `ServerState` 第 17-25 行替换为：

```rust
#[derive(Clone)]
struct ServerState {
    server_root: PathBuf,
    bundles_dir: PathBuf,
    project_id: String,
    project_name: String,
    platform_access: Arc<std::collections::HashMap<String, bool>>,
    log_sender: Arc<tokio::sync::mpsc::Sender<LogEntry>>,
}
```

把 `start_server` 函数签名（第 60-67 行）替换为：

```rust
pub async fn start_server(
    server_root: PathBuf,
    bundles_dir: PathBuf,
    project_name: String,
    project_id: String,
    port: u16,
    cors_enabled: bool,
    platform_access: std::collections::HashMap<String, bool>,
    log_sender: tokio::sync::mpsc::Sender<LogEntry>,
) -> Result<RunningServer, String> {
```

且在构造 `state` 时加上：

```rust
let state = ServerState {
    server_root,
    bundles_dir,
    project_id: project_id.clone(),
    project_name,
    platform_access: Arc::new(platform_access),
    log_sender: Arc::new(log_sender),
};
```

在 `handle_request_fallback` 文件路径解析后、安全检查前插入访问检查（建议在第 132 行 canonicalize 之前）：

```rust
// Platform access check: 取路径首段作为平台
if let Some(first_segment) = req_path.split('/').next() {
    if !first_segment.is_empty() {
        if let Some(&enabled) = state.platform_access.get(first_segment) {
            if !enabled {
                log_request(&state, 403, "GET", &req_path).await;
                return (StatusCode::FORBIDDEN, "Forbidden").into_response();
            }
        }
    }
}
```

- [ ] **Step 3: lib.rs 启动服务器时传递 platform_access**

修改 `src-tauri/src/lib.rs` 第 116-125 行 `server::start_server` 调用：

```rust
let running = server::start_server(
    server_root,
    std::path::PathBuf::from(&project.bundles_dir),
    project.project_name.clone(),
    project_id.clone(),
    project.port,
    project.cors_enabled,
    project.platform_access.clone(),
    log_tx,
)
.await?;
```

- [ ] **Step 4: 编译验证**

```bash
cd src-tauri && cargo build 2>&1 | tail -20
```

Expected: 编译通过。

- [ ] **Step 5: LocalMode.vue 加平台访问开关 UI**

修改 `src/components/LocalMode.vue`：

a. 在 `interface ProjectConfig` 中增加 `platform_access: Record<string, boolean>`：

```ts
interface ProjectConfig {
  id: string;
  project_name: string;
  bundles_dir: string;
  port: number;
  platforms: string[];
  cors_enabled: boolean;
  package_name: string;
  platform_access: Record<string, boolean>;
}
```

b. 修改 `togglePlatform`：增加平台时初始化 `platform_access[platform] = true`；删除时从 map 清掉。

```ts
function togglePlatform(platform: string) {
  const project = activeProject.value;
  if (!project) return;
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
  if (!project) return;
  if (!project.platform_access) project.platform_access = {};
  project.platform_access[platform] = !(project.platform_access[platform] ?? true);
}
```

c. 在 `<template>` 的平台标签区域（约第 466-470 行 `<div class="platform-tags">`）替换为：

```vue
<div class="platform-tags">
  <span
    v-for="p in AVAILABLE_PLATFORMS"
    :key="p"
    class="platform-tag"
    :class="{ selected: activeProject.platforms.includes(p) }"
    @click="togglePlatform(p)"
  >
    {{ p }}
    <span
      v-if="activeProject.platforms.includes(p)"
      class="lm-access-dot"
      :class="{ on: activeProject.platform_access?.[p] !== false }"
      :title="`访问 ${activeProject.platform_access?.[p] !== false ? '已开启（点击关闭）' : '已关闭（点击开启）'}`"
      @click.stop="toggleLocalAccess(p)"
    ></span>
  </span>
</div>
```

d. 在 `<style scoped>` 末尾追加：

```css
.lm-access-dot {
  display: inline-block;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  margin-left: 6px;
  background: var(--text-muted);
  cursor: pointer;
  transition: background 0.15s, box-shadow 0.15s;
}
.lm-access-dot.on {
  background: #4ade80;
  box-shadow: 0 0 6px rgba(74, 222, 128, 0.5);
}
.lm-access-dot:hover { transform: scale(1.2); }
```

- [ ] **Step 6: TypeScript 编译验证**

```bash
npx tsc --noEmit -p tsconfig.json 2>&1 | head -20
```

Expected: 0 错误。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/config.rs src-tauri/src/server.rs src-tauri/src/lib.rs src/components/LocalMode.vue
git commit -m "feat(local-mode): per-platform access toggle"
```

---

## Task 10: 端到端冒烟测试

**目标：** 运行完整应用栈（前端 dev + 服务端），验证主要流程。

**Files:** 无新增。

- [ ] **Step 1: 启动服务端**

```bash
cd server && DATA_DIR=/tmp/tengine-e2e ADMIN_PASSWORD=test123 PORT=18082 cargo run 2>&1 &
sleep 5
```

- [ ] **Step 2: 启动前端 dev server**

```bash
npm run dev -- --host 127.0.0.1 --port 18083 2>&1 &
sleep 5
```

- [ ] **Step 3: 在浏览器打开 http://127.0.0.1:18083，按以下顺序验证**

1. 切到"远程模式"，新建连接 `http://127.0.0.1:18082`，密码 `test123`，登录
2. 创建项目"Demo"
3. 在二级标签栏点 + 创建项目版本"v1"
4. 选 Android 平台，浏览选 Bundles 目录（用任意有 YooAsset 输出的目录）
5. 点击"同步资源"，选一个本地版本上传
6. 上传完成后，"激活"该 bundle
7. 浏览器访问 `http://127.0.0.1:18082/res/v1/Demo/Android/<某文件名>` 应 200
8. 在 UI 上点 Android 平台访问 chip 关闭，再访问同 URL 应 403
9. 二级标签创建"v2"，独立上传/激活，验证 v1 / v2 互不影响
10. 删除 v2，确认 `resources/Demo/v2/` 整棵目录消失

- [ ] **Step 4: 关闭服务**

```bash
kill %1 %2 2>/dev/null
rm -rf /tmp/tengine-e2e
```

- [ ] **Step 5: 提交（如有任何修复）**

如冒烟过程中发现 bug，回到对应 Task 修复并补充小提交；否则跳过。

---

## Self-Review

- **Spec 覆盖**：
  - 数据模型 ✅ Task 1
  - 磁盘布局 ✅ Task 2
  - URL 路由 ✅ Task 3
  - 8 个改造 API + 4 个新增 API ✅ Task 4
  - 服务端冒烟 ✅ Task 5
  - 客户端 API 层 ✅ Task 6
  - Tauri 上传命令带 project_version ✅ Task 7
  - RemoteMode UI（二级标签 + 平台开关 + 折叠） ✅ Task 8
  - LocalMode 平台开关 + Tauri server 检查 ✅ Task 9
  - 端到端 ✅ Task 10
- **占位符扫描**：无 TBD/TODO；每段代码自含。
- **类型一致性**：`ProjectVersion`、`PlatformSettings` 在 spec/Rust/TS 三处一致；`platform_access` 在 LocalMode 用 `Record<string, boolean>`（与 Rust HashMap 序列化一致）。
- **方法签名一致性**：`activate_version`、`delete_version`、`list_files`、`list_versions`、`list_files_with_hash`、`copy_files_from_version`、`save_uploaded_file` 全部新增了 `project_version` 参数；TS 客户端方法名与服务端路由一一对应。
