# TEngine Remote Server Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the existing local HTTP asset server into a remote resource distribution service with Docker deployment, web admin UI, and Tauri remote mode.

**Architecture:** Independent Axum HTTP backend (`server/`) with REST API + WebSocket logs. Web admin SPA (`web-admin/`) embedded via rust-embed. Tauri client gets a new "remote mode" alongside existing local mode. All management clients share the same `/api/*` endpoints.

**Tech Stack:** Rust (Axum 0.7, argon2, jsonwebtoken, tokio), Vue 3 + TypeScript + Vite, Docker multi-stage build.

**Spec:** `docs/superpowers/specs/2026-05-02-remote-server-design.md`

---

## File Structure

### New files: `server/` (independent Rust backend)

| File | Responsibility |
|------|----------------|
| `server/Cargo.toml` | Dependencies for standalone Axum server |
| `server/src/main.rs` | Entry point: load config, start Axum |
| `server/src/config.rs` | Server config: env vars, project config CRUD, persistence |
| `server/src/auth.rs` | Password hashing (argon2), JWT creation/validation, auth middleware |
| `server/src/api.rs` | REST API routes: projects CRUD, upload, versions, activate |
| `server/src/storage.rs` | Resource storage: file operations, version management |
| `server/src/serve.rs` | Static file serving for `/res/*` and embedded SPA for `/` |
| `server/src/ws.rs` | WebSocket log broadcast |
| `server/Dockerfile` | Multi-stage build (node → rust → debian-slim) |
| `server/docker-compose.yml` | Single-service compose with volume and env |
| `server/.env.example` | Example environment variables |

### New files: `web-admin/` (web management SPA)

| File | Responsibility |
|------|----------------|
| `web-admin/package.json` | Vue 3 + Vite project config |
| `web-admin/vite.config.ts` | Vite config with API proxy for dev |
| `web-admin/index.html` | HTML entry |
| `web-admin/tsconfig.json` | TypeScript config |
| `web-admin/src/main.ts` | Vue app entry |
| `web-admin/src/App.vue` | Root component with login gate |
| `web-admin/src/api/remote.ts` | API client (fetch + JWT + SHA-256) |
| `web-admin/src/components/RemoteLogin.vue` | Login form |
| `web-admin/src/components/ProjectManager.vue` | Project CRUD + version management + upload |
| `web-admin/src/components/LogPanel.vue` | WebSocket log viewer |
| `web-admin/src/styles/main.css` | Styles (reuse from existing app) |

### Modified files: `src/` (Tauri frontend)

| File | Change |
|------|--------|
| `src/App.vue` | Add mode switcher, delegate to LocalMode / RemoteMode |
| `src/components/LocalMode.vue` | Extract current App.vue logic here |
| `src/components/RemoteMode.vue` | Remote mode UI (connect, projects, upload, versions, logs) |
| `src/components/RemoteLogin.vue` | Server address + password login |
| `src/components/LogPanel.vue` | Shared log panel (props-driven) |
| `src/api/remote.ts` | Same API client as web-admin (fetch + JWT) |

### Modified files: `src-tauri/` (Tauri backend)

| File | Change |
|------|--------|
| `src-tauri/src/config.rs` | Add `RemoteConnection` struct for persisting server address |
| `src-tauri/Cargo.toml` | No changes needed (remote calls go through frontend fetch) |

---

## Task 1: Server project scaffold and config

**Files:**
- Create: `server/Cargo.toml`
- Create: `server/src/main.rs`
- Create: `server/src/config.rs`

- [ ] **Step 1: Create `server/Cargo.toml`**

```toml
[package]
name = "tengine-server"
version = "1.0.0"
edition = "2021"

[dependencies]
axum = { version = "0.7", features = ["ws", "multipart"] }
axum-extra = { version = "0.9", features = ["typed-header"] }
tokio = { version = "1", features = ["full"] }
tower-http = { version = "0.5", features = ["cors"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }
argon2 = "0.5"
jsonwebtoken = "9"
sha2 = "0.10"
rust-embed = { version = "8", features = ["interpolate-folder-path"] }
mime_guess = "2"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
percent-encoding = "2"
hex = "0.4"
```

- [ ] **Step 2: Create `server/src/config.rs`**

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub id: String,
    pub project_name: String,
    pub platforms: Vec<String>,
    pub package_name: String,
    pub active_versions: HashMap<String, String>,
}

impl ProjectConfig {
    pub fn new(project_name: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            project_name,
            platforms: vec!["Android".to_string()],
            package_name: "DefaultPackage".to_string(),
            active_versions: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub projects: Vec<ProjectConfig>,
}

impl AppConfig {
    pub fn load(path: &PathBuf) -> Self {
        if path.exists() {
            match fs::read_to_string(path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(_) => Self::default(),
            }
        } else {
            Self::default()
        }
    }

    pub fn save(&self, path: &PathBuf) -> Result<(), String> {
        let content = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(path, content).map_err(|e| e.to_string())
    }
}

pub struct ServerConfig {
    pub password_hash: String,
    pub jwt_secret: String,
    pub token_expire_hours: u64,
    pub data_dir: PathBuf,
    pub port: u16,
}

impl ServerConfig {
    pub fn from_env() -> Self {
        let password = std::env::var("ADMIN_PASSWORD")
            .expect("ADMIN_PASSWORD environment variable is required");

        let sha256_hex = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(password.as_bytes());
            hex::encode(hasher.finalize())
        };

        let password_hash = {
            use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
            let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
            Argon2::default()
                .hash_password(sha256_hex.as_bytes(), &salt)
                .expect("Failed to hash password")
                .to_string()
        };

        let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| {
            uuid::Uuid::new_v4().to_string()
        });

        let token_expire_hours: u64 = std::env::var("TOKEN_EXPIRE_HOURS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(24);

        let data_dir = PathBuf::from(
            std::env::var("DATA_DIR").unwrap_or_else(|_| "/data".to_string()),
        );

        let port: u16 = std::env::var("PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8080);

        Self {
            password_hash,
            jwt_secret,
            token_expire_hours,
            data_dir,
            port,
        }
    }

    pub fn config_path(&self) -> PathBuf {
        self.data_dir.join("config.json")
    }

    pub fn resources_dir(&self) -> PathBuf {
        self.data_dir.join("resources")
    }
}

pub type SharedAppConfig = Arc<RwLock<AppConfig>>;
```

- [ ] **Step 3: Create `server/src/main.rs`**

```rust
mod api;
mod auth;
mod config;
mod serve;
mod storage;
mod ws;

use config::{AppConfig, ServerConfig, SharedAppConfig};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing_subscriber::EnvFilter;

pub struct AppState {
    pub server_config: ServerConfig,
    pub app_config: SharedAppConfig,
    pub log_tx: broadcast::Sender<ws::LogMessage>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("tengine_server=info".parse().unwrap()))
        .init();

    let server_config = ServerConfig::from_env();
    let app_config = AppConfig::load(&server_config.config_path());

    std::fs::create_dir_all(server_config.resources_dir())
        .expect("Failed to create resources directory");

    let (log_tx, _) = broadcast::channel::<ws::LogMessage>(256);

    let port = server_config.port;
    let state = Arc::new(AppState {
        server_config,
        app_config: Arc::new(RwLock::new(app_config)),
        log_tx,
    });

    let app = api::build_router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Server starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

- [ ] **Step 4: Create stub modules so it compiles**

Create empty stub files for modules that will be implemented in later tasks:

`server/src/auth.rs`:
```rust
// Implemented in Task 2
```

`server/src/api.rs`:
```rust
use axum::Router;
use std::sync::Arc;
use crate::AppState;

pub fn build_router(_state: Arc<AppState>) -> Router {
    Router::new()
}
```

`server/src/storage.rs`:
```rust
// Implemented in Task 3
```

`server/src/serve.rs`:
```rust
// Implemented in Task 5
```

`server/src/ws.rs`:
```rust
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LogMessage {
    pub timestamp: String,
    pub r#type: String,
    pub status: u16,
    pub method: String,
    pub path: String,
    pub project_id: String,
    pub message: String,
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cd server && cargo check`
Expected: compiles with no errors (may have unused warnings, that's fine)

- [ ] **Step 6: Commit**

```bash
git add server/
git commit -m "feat: scaffold server project with config and main entry"
```

---

## Task 2: Authentication module

**Files:**
- Modify: `server/src/auth.rs`

- [ ] **Step 1: Implement auth module**

Replace `server/src/auth.rs` with:

```rust
use axum::{
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    exp: usize,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub token: String,
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, StatusCode> {
    use argon2::{Argon2, PasswordHash, PasswordVerifier};

    let stored_hash = PasswordHash::new(&state.server_config.password_hash)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Argon2::default()
        .verify_password(req.password.as_bytes(), &stored_hash)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let exp = chrono::Utc::now()
        + chrono::Duration::hours(state.server_config.token_expire_hours as i64);

    let claims = Claims {
        exp: exp.timestamp() as usize,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.server_config.jwt_secret.as_bytes()),
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(LoginResponse { token }))
}

pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(h) if h.starts_with("Bearer ") => &h[7..],
        _ => return StatusCode::UNAUTHORIZED.into_response(),
    };

    let validation = Validation::default();
    match decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.server_config.jwt_secret.as_bytes()),
        &validation,
    ) {
        Ok(_) => next.run(req).await,
        Err(_) => StatusCode::UNAUTHORIZED.into_response(),
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cd server && cargo check`
Expected: compiles successfully

- [ ] **Step 3: Commit**

```bash
git add server/src/auth.rs
git commit -m "feat: implement auth module with argon2 + JWT"
```

---

## Task 3: Storage module

**Files:**
- Modify: `server/src/storage.rs`

- [ ] **Step 1: Implement storage module**

Replace `server/src/storage.rs` with:

```rust
use std::fs;
use std::path::{Path, PathBuf};

pub struct Storage {
    resources_dir: PathBuf,
}

impl Storage {
    pub fn new(resources_dir: PathBuf) -> Self {
        Self { resources_dir }
    }

    pub fn project_dir(&self, project_name: &str) -> PathBuf {
        self.resources_dir.join(project_name)
    }

    pub fn platform_dir(&self, project_name: &str, platform: &str) -> PathBuf {
        self.project_dir(project_name).join(platform)
    }

    pub fn versions_dir(&self, project_name: &str, platform: &str) -> PathBuf {
        self.platform_dir(project_name, platform).join("_versions")
    }

    pub fn version_dir(&self, project_name: &str, platform: &str, version: &str) -> PathBuf {
        self.versions_dir(project_name, platform).join(version)
    }

    pub fn save_uploaded_file(
        &self,
        project_name: &str,
        platform: &str,
        version: &str,
        file_name: &str,
        data: &[u8],
    ) -> Result<(), String> {
        let dir = self.version_dir(project_name, platform, version);
        fs::create_dir_all(&dir).map_err(|e| format!("Failed to create version dir: {}", e))?;
        let path = dir.join(file_name);
        fs::write(&path, data).map_err(|e| format!("Failed to write file: {}", e))?;
        Ok(())
    }

    pub fn list_versions(&self, project_name: &str, platform: &str) -> Vec<VersionEntry> {
        let dir = self.versions_dir(project_name, platform);
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

    pub fn activate_version(
        &self,
        project_name: &str,
        platform: &str,
        version: &str,
    ) -> Result<u32, String> {
        let version_dir = self.version_dir(project_name, platform, version);
        if !version_dir.exists() {
            return Err(format!("Version directory does not exist: {}", version));
        }

        let platform_dir = self.platform_dir(project_name, platform);
        fs::create_dir_all(&platform_dir)
            .map_err(|e| format!("Failed to create platform dir: {}", e))?;

        // Clean old activated files (keep _versions dir)
        if let Ok(entries) = fs::read_dir(&platform_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name == "_versions" {
                    continue;
                }
                if entry.path().is_file() {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }

        // Copy version files to platform root
        let mut count = 0u32;
        if let Ok(entries) = fs::read_dir(&version_dir) {
            for entry in entries.flatten() {
                if !entry.path().is_file() {
                    continue;
                }
                let name = entry.file_name();
                let dest = platform_dir.join(&name);
                fs::copy(entry.path(), &dest)
                    .map_err(|e| format!("Failed to copy {}: {}", name.to_string_lossy(), e))?;
                count += 1;
            }
        }

        Ok(count)
    }

    pub fn delete_version(
        &self,
        project_name: &str,
        platform: &str,
        version: &str,
    ) -> Result<(), String> {
        let dir = self.version_dir(project_name, platform, version);
        if dir.exists() {
            fs::remove_dir_all(&dir)
                .map_err(|e| format!("Failed to delete version: {}", e))?;
        }
        Ok(())
    }

    pub fn delete_project(&self, project_name: &str) -> Result<(), String> {
        let dir = self.project_dir(project_name);
        if dir.exists() {
            fs::remove_dir_all(&dir)
                .map_err(|e| format!("Failed to delete project: {}", e))?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VersionEntry {
    pub version: String,
    pub file_count: u32,
    pub total_size: u64,
    pub modified_timestamp: u64,
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cd server && cargo check`
Expected: compiles successfully

- [ ] **Step 3: Commit**

```bash
git add server/src/storage.rs
git commit -m "feat: implement storage module for resource management"
```

---

## Task 4: API routes and WebSocket logs

**Files:**
- Modify: `server/src/api.rs`
- Modify: `server/src/ws.rs`

- [ ] **Step 1: Implement WebSocket log broadcast**

Replace `server/src/ws.rs` with:

```rust
use axum::{
    extract::{Query, State, WebSocketUpgrade},
    response::Response,
};
use axum::extract::ws::{Message, WebSocket};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogMessage {
    pub timestamp: String,
    pub r#type: String,
    pub status: u16,
    pub method: String,
    pub path: String,
    pub project_id: String,
    pub message: String,
}

#[derive(Deserialize)]
pub struct WsQuery {
    pub token: String,
}

pub async fn ws_logs(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(state): State<Arc<AppState>>,
) -> Response {
    use jsonwebtoken::{decode, DecodingKey, Validation};
    use crate::auth::Claims;

    let validation = Validation::default();
    let result = decode::<Claims>(
        &query.token,
        &DecodingKey::from_secret(state.server_config.jwt_secret.as_bytes()),
        &validation,
    );

    if result.is_err() {
        return (axum::http::StatusCode::UNAUTHORIZED, "Invalid token").into_response();
    }

    let rx = state.log_tx.subscribe();
    ws.on_upgrade(move |socket| handle_ws(socket, rx))
}

async fn handle_ws(mut socket: WebSocket, mut rx: tokio::sync::broadcast::Receiver<LogMessage>) {
    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(log) => {
                        let json = serde_json::to_string(&log).unwrap_or_default();
                        if socket.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}

use axum::response::IntoResponse;

pub fn broadcast_log(state: &Arc<AppState>, log: LogMessage) {
    let _ = state.log_tx.send(log);
}

pub fn make_log(
    r#type: &str,
    status: u16,
    method: &str,
    path: &str,
    project_id: &str,
    message: &str,
) -> LogMessage {
    LogMessage {
        timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
        r#type: r#type.to_string(),
        status,
        method: method.to_string(),
        path: path.to_string(),
        project_id: project_id.to_string(),
        message: message.to_string(),
    }
}
```

Note: Make `Claims` in `auth.rs` public: change `struct Claims` to `pub struct Claims`.

- [ ] **Step 2: Implement API routes**

Replace `server/src/api.rs` with:

```rust
use axum::{
    extract::{Multipart, Path, State},
    http::{Method, StatusCode},
    middleware,
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

use crate::auth;
use crate::config::ProjectConfig;
use crate::storage::Storage;
use crate::ws;
use crate::AppState;

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
        .route("/api/projects/{id}", put(update_project))
        .route("/api/projects/{id}", delete(delete_project))
        .route("/api/projects/{id}/upload", post(upload_resources))
        .route("/api/projects/{id}/versions", get(list_versions))
        .route("/api/projects/{id}/versions/{ver}/activate", put(activate_version))
        .route("/api/projects/{id}/versions/{ver}", delete(delete_version))
        .route("/api/projects/{id}/status", get(project_status))
        .layer(middleware::from_fn_with_state(state.clone(), auth::auth_middleware));

    let ws_route = Router::new()
        .route("/api/ws/logs", get(ws::ws_logs));

    // Resource serving and SPA will be added in Task 5
    Router::new()
        .merge(public)
        .merge(protected)
        .merge(ws_route)
        .layer(cors)
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn list_projects(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<ProjectConfig>> {
    let config = state.app_config.read().await;
    Json(config.projects.clone())
}

#[derive(Deserialize)]
struct CreateProjectRequest {
    project_name: String,
}

async fn create_project(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateProjectRequest>,
) -> Result<Json<ProjectConfig>, StatusCode> {
    let mut config = state.app_config.write().await;
    let project = ProjectConfig::new(req.project_name);
    config.projects.push(project.clone());
    config.save(&state.server_config.config_path())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    ws::broadcast_log(&state, ws::make_log("system", 200, "POST", "/api/projects", &project.id, "Project created"));

    Ok(Json(project))
}

async fn update_project(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(updated): Json<ProjectConfig>,
) -> Result<StatusCode, StatusCode> {
    let mut config = state.app_config.write().await;
    let project = config.projects.iter_mut().find(|p| p.id == id)
        .ok_or(StatusCode::NOT_FOUND)?;
    *project = updated;
    config.save(&state.server_config.config_path())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

async fn delete_project(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let mut config = state.app_config.write().await;
    let project = config.projects.iter().find(|p| p.id == id)
        .ok_or(StatusCode::NOT_FOUND)?;
    let project_name = project.project_name.clone();
    config.projects.retain(|p| p.id != id);
    config.save(&state.server_config.config_path())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let storage = Storage::new(state.server_config.resources_dir());
    let _ = storage.delete_project(&project_name);

    ws::broadcast_log(&state, ws::make_log("system", 200, "DELETE", "/api/projects", &id, "Project deleted"));

    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
struct UploadQuery {
    platform: Option<String>,
    version: Option<String>,
}

async fn upload_resources(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let config = state.app_config.read().await;
    let project = config.projects.iter().find(|p| p.id == id)
        .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))?;
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

                storage.save_uploaded_file(&project_name, &platform, &version, &file_name, &data)
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
                file_count += 1;
            }
            _ => {}
        }
    }

    ws::broadcast_log(&state, ws::make_log(
        "upload", 200, "POST",
        &format!("/api/projects/{}/upload", id),
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

async fn list_versions(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<ListVersionsQuery>,
) -> Result<Json<Vec<crate::storage::VersionEntry>>, StatusCode> {
    let config = state.app_config.read().await;
    let project = config.projects.iter().find(|p| p.id == id)
        .ok_or(StatusCode::NOT_FOUND)?;
    let project_name = project.project_name.clone();
    drop(config);

    let platform = params.platform.unwrap_or_else(|| "Android".to_string());
    let storage = Storage::new(state.server_config.resources_dir());
    Ok(Json(storage.list_versions(&project_name, &platform)))
}

#[derive(Deserialize)]
struct ListVersionsQuery {
    platform: Option<String>,
}

async fn activate_version(
    State(state): State<Arc<AppState>>,
    Path((id, ver)): Path<(String, String)>,
    axum::extract::Query(params): axum::extract::Query<ActivateQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut config = state.app_config.write().await;
    let project = config.projects.iter_mut().find(|p| p.id == id)
        .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))?;
    let project_name = project.project_name.clone();

    let platform = params.platform.unwrap_or_else(|| "Android".to_string());

    let storage = Storage::new(state.server_config.resources_dir());
    let count = storage.activate_version(&project_name, &platform, &ver)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    project.active_versions.insert(platform.clone(), ver.clone());
    config.save(&state.server_config.config_path())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    ws::broadcast_log(&state, ws::make_log(
        "sync", 200, "PUT",
        &format!("/api/projects/{}/versions/{}/activate", id, ver),
        &id,
        &format!("Activated version {} on {}, {} files", ver, platform, count),
    ));

    Ok(Json(serde_json::json!({
        "success": true,
        "version": ver,
        "platform": platform,
        "file_count": count
    })))
}

#[derive(Deserialize)]
struct ActivateQuery {
    platform: Option<String>,
}

async fn delete_version(
    State(state): State<Arc<AppState>>,
    Path((id, ver)): Path<(String, String)>,
    axum::extract::Query(params): axum::extract::Query<ActivateQuery>,
) -> Result<StatusCode, (StatusCode, String)> {
    let config = state.app_config.read().await;
    let project = config.projects.iter().find(|p| p.id == id)
        .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))?;
    let project_name = project.project_name.clone();
    drop(config);

    let platform = params.platform.unwrap_or_else(|| "Android".to_string());
    let storage = Storage::new(state.server_config.resources_dir());
    storage.delete_version(&project_name, &platform, &ver)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(StatusCode::OK)
}

async fn project_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let config = state.app_config.read().await;
    let project = config.projects.iter().find(|p| p.id == id)
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(serde_json::json!({
        "id": project.id,
        "project_name": project.project_name,
        "active_versions": project.active_versions,
    })))
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cd server && cargo check`
Expected: compiles successfully

- [ ] **Step 4: Commit**

```bash
git add server/src/api.rs server/src/ws.rs server/src/auth.rs
git commit -m "feat: implement API routes and WebSocket log broadcast"
```

---

## Task 5: Static file serving and SPA embedding

**Files:**
- Modify: `server/src/serve.rs`
- Modify: `server/src/api.rs`

- [ ] **Step 1: Implement static file serving and SPA**

Replace `server/src/serve.rs` with:

```rust
use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
};
use mime_guess::from_path;
use percent_encoding::percent_decode_str;
use rust_embed::Embed;
use std::sync::Arc;

use crate::ws;
use crate::AppState;

#[derive(Embed)]
#[folder = "../web-admin/dist"]
#[prefix = ""]
struct WebAdminAssets;

pub async fn serve_spa(req: Request) -> Response {
    let path = req.uri().path().trim_start_matches('/');

    // Try exact file match first
    if let Some(content) = WebAdminAssets::get(path) {
        let mime = from_path(path).first_or_octet_stream().to_string();
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, mime.parse().unwrap());
        return (StatusCode::OK, headers, content.data.to_vec()).into_response();
    }

    // SPA fallback: serve index.html for non-file paths
    match WebAdminAssets::get("index.html") {
        Some(content) => {
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, "text/html; charset=utf-8".parse().unwrap());
            (StatusCode::OK, headers, content.data.to_vec()).into_response()
        }
        None => (StatusCode::NOT_FOUND, "Web admin not available").into_response(),
    }
}

pub async fn serve_resource(
    State(state): State<Arc<AppState>>,
    req: Request,
) -> Response {
    let raw_path = req.uri().path().trim_start_matches("/res/").to_string();
    let decoded = percent_decode_str(&raw_path).decode_utf8_lossy().to_string();

    let file_path = state.server_config.resources_dir().join(&decoded);

    // Security: prevent path traversal
    let canonical = match file_path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            broadcast_request_log(&state, 404, "GET", &raw_path);
            return (StatusCode::NOT_FOUND, "Not found").into_response();
        }
    };

    let resources_canonical = state.server_config.resources_dir()
        .canonicalize()
        .unwrap_or_else(|_| state.server_config.resources_dir().clone());

    if !canonical.starts_with(&resources_canonical) {
        broadcast_request_log(&state, 403, "GET", &raw_path);
        return (StatusCode::FORBIDDEN, "Forbidden").into_response();
    }

    if canonical.is_file() {
        match tokio::fs::read(&canonical).await {
            Ok(bytes) => {
                broadcast_request_log(&state, 200, "GET", &raw_path);
                let mime = from_path(&canonical).first_or_octet_stream().to_string();
                let mut headers = HeaderMap::new();
                headers.insert(header::CONTENT_TYPE, format!("{}; charset=utf-8", mime).parse().unwrap());
                headers.insert(header::ACCEPT_RANGES, "bytes".parse().unwrap());
                (StatusCode::OK, headers, bytes).into_response()
            }
            Err(_) => {
                broadcast_request_log(&state, 404, "GET", &raw_path);
                (StatusCode::NOT_FOUND, "Not found").into_response()
            }
        }
    } else {
        broadcast_request_log(&state, 404, "GET", &raw_path);
        (StatusCode::NOT_FOUND, "Not found").into_response()
    }
}

fn broadcast_request_log(state: &Arc<AppState>, status: u16, method: &str, path: &str) {
    // Extract project_id from path: res/<project_name>/<platform>/...
    let project_id = path.split('/').next().unwrap_or("").to_string();
    ws::broadcast_log(state, ws::make_log("request", status, method, path, &project_id, ""));
}
```

- [ ] **Step 2: Add SPA and resource routes to the router**

In `server/src/api.rs`, update `build_router` to add resource serving and SPA fallback:

Add these imports at the top:
```rust
use crate::serve;
```

Replace the final `Router::new()` block in `build_router` with:
```rust
    Router::new()
        .merge(public)
        .merge(protected)
        .merge(ws_route)
        .route("/res/{*path}", get(serve::serve_resource))
        .fallback(get(serve::serve_spa))
        .layer(cors)
        .with_state(state)
```

- [ ] **Step 3: Verify it compiles**

The SPA embedding will fail at compile time if `web-admin/dist` doesn't exist yet. Create a placeholder:

```bash
mkdir -p web-admin/dist
echo '<!DOCTYPE html><html><body>Placeholder</body></html>' > web-admin/dist/index.html
```

Run: `cd server && cargo check`
Expected: compiles successfully

- [ ] **Step 4: Commit**

```bash
git add server/src/serve.rs server/src/api.rs web-admin/dist/index.html
git commit -m "feat: implement resource serving and SPA embedding"
```

---

## Task 6: Docker deployment files

**Files:**
- Create: `server/Dockerfile`
- Create: `server/docker-compose.yml`
- Create: `server/.env.example`

- [ ] **Step 1: Create `server/Dockerfile`**

```dockerfile
# Stage 1: Build web-admin
FROM node:20-alpine AS web-builder
WORKDIR /app/web-admin
COPY web-admin/package.json web-admin/package-lock.json* ./
RUN npm ci
COPY web-admin/ ./
RUN npm run build

# Stage 2: Build server
FROM rust:1.78-bookworm AS server-builder
WORKDIR /app
COPY server/Cargo.toml server/Cargo.lock* ./server/
COPY server/src/ ./server/src/
COPY --from=web-builder /app/web-admin/dist ./web-admin/dist
WORKDIR /app/server
RUN cargo build --release

# Stage 3: Runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=server-builder /app/server/target/release/tengine-server /usr/local/bin/tengine-server
RUN mkdir -p /data
EXPOSE 8080
CMD ["tengine-server"]
```

- [ ] **Step 2: Create `server/docker-compose.yml`**

```yaml
services:
  tengine-server:
    build:
      context: ..
      dockerfile: server/Dockerfile
    ports:
      - "${PORT:-8080}:${PORT:-8080}"
    volumes:
      - tengine-data:/data
    env_file:
      - .env
    restart: unless-stopped

volumes:
  tengine-data:
```

- [ ] **Step 3: Create `server/.env.example`**

```env
ADMIN_PASSWORD=changeme
# JWT_SECRET=your_random_secret
# PORT=8080
# DATA_DIR=/data
# TOKEN_EXPIRE_HOURS=24
```

- [ ] **Step 4: Commit**

```bash
git add server/Dockerfile server/docker-compose.yml server/.env.example
git commit -m "feat: add Docker deployment files"
```

---

## Task 7: Web admin SPA

**Files:**
- Create: `web-admin/package.json`
- Create: `web-admin/vite.config.ts`
- Create: `web-admin/index.html`
- Create: `web-admin/tsconfig.json`
- Create: `web-admin/src/main.ts`
- Create: `web-admin/src/App.vue`
- Create: `web-admin/src/api/remote.ts`
- Create: `web-admin/src/components/RemoteLogin.vue`
- Create: `web-admin/src/components/ProjectManager.vue`
- Create: `web-admin/src/components/LogPanel.vue`
- Create: `web-admin/src/styles/main.css`

- [ ] **Step 1: Create `web-admin/package.json`**

```json
{
  "name": "tengine-web-admin",
  "private": true,
  "version": "1.0.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vue-tsc --noEmit && vite build",
    "preview": "vite preview"
  },
  "dependencies": {
    "vue": "^3.5.13"
  },
  "devDependencies": {
    "@vitejs/plugin-vue": "^5.2.0",
    "typescript": "^5.7.0",
    "vite": "^6.0.0",
    "vue-tsc": "^2.2.0"
  }
}
```

- [ ] **Step 2: Create `web-admin/vite.config.ts`**

```typescript
import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  plugins: [vue()],
  server: {
    port: 5174,
    proxy: {
      "/api": "http://localhost:8080",
      "/res": "http://localhost:8080",
    },
  },
});
```

- [ ] **Step 3: Create `web-admin/index.html`**

Replace the placeholder with:

```html
<!DOCTYPE html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>TEngine Server Admin</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
```

- [ ] **Step 4: Create `web-admin/tsconfig.json`**

```json
{
  "compilerOptions": {
    "target": "ES2020",
    "useDefineForClassFields": true,
    "module": "ESNext",
    "lib": ["ES2020", "DOM", "DOM.Iterable"],
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "preserve",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true
  },
  "include": ["src/**/*.ts", "src/**/*.tsx", "src/**/*.vue"],
  "references": [{ "path": "./tsconfig.node.json" }]
}
```

Create `web-admin/tsconfig.node.json`:
```json
{
  "compilerOptions": {
    "composite": true,
    "skipLibCheck": true,
    "module": "ESNext",
    "moduleResolution": "bundler",
    "allowSyntheticDefaultImports": true
  },
  "include": ["vite.config.ts"]
}
```

- [ ] **Step 5: Create `web-admin/src/main.ts`**

```typescript
import { createApp } from "vue";
import App from "./App.vue";
import "./styles/main.css";

createApp(App).mount("#app");
```

- [ ] **Step 6: Create `web-admin/src/api/remote.ts`**

```typescript
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

  getToken(): string | null {
    return this.token;
  }

  isLoggedIn(): boolean {
    return this.token !== null;
  }

  async login(password: string): Promise<boolean> {
    const encoder = new TextEncoder();
    const data = encoder.encode(password);
    const hashBuffer = await crypto.subtle.digest("SHA-256", data);
    const hashArray = Array.from(new Uint8Array(hashBuffer));
    const hashHex = hashArray.map((b) => b.toString(16).padStart(2, "0")).join("");

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
    onProgress?: (percent: number) => void,
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
```

- [ ] **Step 7: Create `web-admin/src/components/RemoteLogin.vue`**

```vue
<script setup lang="ts">
import { ref } from "vue";
import { api } from "../api/remote";

const emit = defineEmits<{ (e: "login"): void }>();

const serverUrl = ref("");
const password = ref("");
const error = ref("");
const loading = ref(false);

async function handleLogin() {
  error.value = "";
  loading.value = true;
  try {
    if (serverUrl.value) {
      api.setBaseUrl(serverUrl.value);
    }
    const ok = await api.login(password.value);
    if (ok) {
      emit("login");
    } else {
      error.value = "密码错误";
    }
  } catch (e: any) {
    error.value = `连接失败: ${e.message}`;
  } finally {
    loading.value = false;
  }
}
</script>

<template>
  <div class="login-container">
    <div class="login-card">
      <h2>TEngine Server</h2>
      <div class="login-field" v-if="serverUrl !== undefined">
        <label>服务器地址</label>
        <input v-model="serverUrl" placeholder="留空则使用当前页面地址" @keyup.enter="handleLogin" />
      </div>
      <div class="login-field">
        <label>管理密码</label>
        <input v-model="password" type="password" placeholder="输入管理密码" @keyup.enter="handleLogin" />
      </div>
      <div v-if="error" class="login-error">{{ error }}</div>
      <button class="btn btn-primary login-btn" @click="handleLogin" :disabled="loading || !password">
        {{ loading ? "连接中..." : "登录" }}
      </button>
    </div>
  </div>
</template>
```

- [ ] **Step 8: Create `web-admin/src/components/ProjectManager.vue`**

```vue
<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { api, type ProjectConfig, type VersionEntry, type LogEntry } from "../api/remote";
import LogPanel from "./LogPanel.vue";

const emit = defineEmits<{ (e: "logout"): void }>();

const projects = ref<ProjectConfig[]>([]);
const activeProjectId = ref("");
const versions = ref<VersionEntry[]>([]);
const logs = ref<LogEntry[]>([]);
const uploading = ref(false);
const uploadProgress = ref(0);
const selectedPlatform = ref("Android");
const uploadVersion = ref("");
let ws: WebSocket | null = null;

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
  uploadProgress.value = 0;
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
</script>

<template>
  <div class="app-container">
    <div class="title-bar">
      <h1>TEngine Server Admin</h1>
      <button class="btn btn-secondary" @click="logout" style="font-size:12px;">退出登录</button>
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

        <!-- Upload Section -->
        <div class="control-bar">
          <div class="config-field" style="width:120px">
            <label>上传平台</label>
            <select v-model="selectedPlatform" @change="loadVersions">
              <option v-for="p in activeProject.platforms" :key="p" :value="p">{{ p }}</option>
            </select>
          </div>
          <div class="config-field" style="width:120px">
            <label>版本号</label>
            <input v-model="uploadVersion" placeholder="自动生成" />
          </div>
          <div style="display:flex;align-items:flex-end;gap:8px;">
            <label class="btn btn-primary" style="cursor:pointer;margin-bottom:0;">
              选择文件上传
              <input type="file" multiple style="display:none" @change="handleUpload" :disabled="uploading" />
            </label>
          </div>
          <div v-if="uploading" style="color:var(--accent);font-size:12px;align-self:flex-end;">上传中...</div>

          <!-- Active version display -->
          <div v-if="activeProject.active_versions[selectedPlatform]" class="server-url" style="margin-left:auto;">
            当前激活: <strong>{{ activeProject.active_versions[selectedPlatform] }}</strong>
          </div>
        </div>

        <!-- Version List -->
        <div class="version-list" v-if="versions.length > 0" style="margin:8px 0;max-height:200px;overflow-y:auto;">
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
    <LogPanel :logs="filteredLogs" @clear="logs = []" />
  </div>
</template>
```

- [ ] **Step 9: Create `web-admin/src/components/LogPanel.vue`**

```vue
<script setup lang="ts">
import { computed, nextTick, watch, ref } from "vue";
import type { LogEntry } from "../api/remote";

const props = defineProps<{ logs: LogEntry[] }>();
const emit = defineEmits<{ (e: "clear"): void }>();

const logPanelOpen = ref(true);
const logPanelHeight = ref(220);

watch(() => props.logs.length, () => {
  nextTick(() => {
    const el = document.querySelector(".log-body");
    if (el) el.scrollTop = el.scrollHeight;
  });
});

function getStatusClass(status: number): string {
  if (status >= 200 && status < 300) return "s200";
  if (status >= 300 && status < 400) return "s301";
  if (status >= 400 && status < 500) return "s404";
  return "s500";
}

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
  <div v-if="logPanelOpen" class="resize-handle" @mousedown="onResizeStart"></div>
  <div class="log-panel" :class="{ collapsed: !logPanelOpen }"
    :style="{ height: logPanelOpen ? logPanelHeight + 'px' : '36px' }">
    <div class="log-header" @click="logPanelOpen = !logPanelOpen">
      <h3>
        <span class="toggle-icon" :class="{ expanded: logPanelOpen }">&#9650;</span>
        日志 <span class="log-count">{{ logs.length }}</span>
      </h3>
      <div class="log-actions" @click.stop>
        <button @click="emit('clear')">清空</button>
      </div>
    </div>
    <div v-if="logPanelOpen" class="log-body">
      <div v-if="logs.length === 0" class="empty-state" style="height:100%">
        <p style="font-size:12px;color:var(--text-muted)">暂无日志</p>
      </div>
      <div v-for="(log, idx) in logs" :key="idx" class="log-entry">
        <span class="time">{{ log.timestamp }}</span>
        <span class="status" :class="getStatusClass(log.status)">
          {{ log.type === "request" ? log.status : log.type?.toUpperCase() }}
        </span>
        <span class="path">{{ log.message || log.path }}</span>
      </div>
    </div>
  </div>
</template>
```

- [ ] **Step 10: Create `web-admin/src/App.vue`**

```vue
<script setup lang="ts">
import { ref } from "vue";
import RemoteLogin from "./components/RemoteLogin.vue";
import ProjectManager from "./components/ProjectManager.vue";

const loggedIn = ref(false);
</script>

<template>
  <RemoteLogin v-if="!loggedIn" @login="loggedIn = true" />
  <ProjectManager v-else @logout="loggedIn = false" />
</template>
```

- [ ] **Step 11: Create `web-admin/src/styles/main.css`**

Copy the CSS from the existing `src/styles/main.css` file. Add these additional styles at the end:

```css
/* Login page styles */
.login-container {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100vh;
  background: var(--bg-primary);
}

.login-card {
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 32px;
  width: 360px;
}

.login-card h2 {
  color: var(--accent);
  text-align: center;
  margin-bottom: 24px;
  font-size: 20px;
}

.login-field {
  margin-bottom: 16px;
}

.login-field label {
  display: block;
  color: var(--text-secondary);
  font-size: 12px;
  margin-bottom: 4px;
}

.login-field input {
  width: 100%;
  box-sizing: border-box;
}

.login-error {
  color: #ff6b6b;
  font-size: 12px;
  margin-bottom: 12px;
}

.login-btn {
  width: 100%;
  padding: 10px;
  font-size: 14px;
}
```

- [ ] **Step 12: Create Vue env.d.ts**

Create `web-admin/src/env.d.ts`:
```typescript
/// <reference types="vite/client" />

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent<{}, {}, any>;
  export default component;
}
```

- [ ] **Step 13: Install dependencies and verify build**

```bash
cd web-admin && npm install && npm run build
```

Expected: builds successfully, output in `web-admin/dist/`

- [ ] **Step 14: Verify server compiles with real SPA**

```bash
cd server && cargo check
```

Expected: compiles with embedded SPA assets

- [ ] **Step 15: Commit**

```bash
git add web-admin/
git commit -m "feat: implement web admin SPA with login, project management, and logs"
```

---

## Task 8: Tauri frontend refactor — extract LocalMode

**Files:**
- Modify: `src/App.vue`
- Create: `src/components/LocalMode.vue`

- [ ] **Step 1: Create `src/components/LocalMode.vue`**

Move all the current `App.vue` script logic and template (everything inside `<div class="app-container">` except the title bar mode switcher) into `LocalMode.vue`. Keep it as a self-contained component with all existing imports (`invoke`, `listen`, `open`).

The component should be a direct extraction — same code, same template, same styles. The only change is wrapping it in a component that can be toggled.

Extract the full `<script setup>` block (lines 1-405 of current App.vue) and the template from the tab bar through the help modal (lines 419-846). The title bar stays in App.vue.

- [ ] **Step 2: Update `src/App.vue` to be a mode switcher**

Replace `src/App.vue` with:

```vue
<script setup lang="ts">
import { ref } from "vue";
import LocalMode from "./components/LocalMode.vue";
import RemoteMode from "./components/RemoteMode.vue";

const mode = ref<"local" | "remote">("local");
</script>

<template>
  <div class="app-container">
    <div class="title-bar">
      <h1>TEngine Http Server</h1>
      <div class="mode-switcher">
        <button
          :class="{ active: mode === 'local' }"
          @click="mode = 'local'"
        >本地模式</button>
        <button
          :class="{ active: mode === 'remote' }"
          @click="mode = 'remote'"
        >远程模式</button>
      </div>
      <span class="version">v1.0.0</span>
    </div>

    <LocalMode v-if="mode === 'local'" />
    <RemoteMode v-else />
  </div>
</template>

<style scoped>
.mode-switcher {
  display: flex;
  gap: 2px;
  background: var(--bg-tertiary);
  border-radius: 6px;
  padding: 2px;
}

.mode-switcher button {
  padding: 4px 16px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  border-radius: 4px;
  cursor: pointer;
  font-size: 13px;
  transition: all 0.2s;
}

.mode-switcher button.active {
  background: var(--accent);
  color: var(--bg-primary);
}

.mode-switcher button:hover:not(.active) {
  color: var(--text-primary);
}
</style>
```

- [ ] **Step 3: Create stub `src/components/RemoteMode.vue`**

```vue
<script setup lang="ts">
</script>

<template>
  <div style="padding: 40px; text-align: center; color: var(--text-muted);">
    远程模式（下一步实现）
  </div>
</template>
```

- [ ] **Step 4: Verify Tauri dev builds**

Run: `npm run tauri dev`
Expected: app launches with mode switcher visible, local mode works as before

- [ ] **Step 5: Commit**

```bash
git add src/App.vue src/components/LocalMode.vue src/components/RemoteMode.vue
git commit -m "refactor: extract LocalMode component and add mode switcher"
```

---

## Task 9: Tauri frontend — RemoteMode implementation

**Files:**
- Modify: `src/components/RemoteMode.vue`
- Create: `src/api/remote.ts`

- [ ] **Step 1: Create `src/api/remote.ts`**

Copy `web-admin/src/api/remote.ts` verbatim to `src/api/remote.ts`. The API client is identical — both use `fetch` and `crypto.subtle`.

- [ ] **Step 2: Implement `src/components/RemoteMode.vue`**

```vue
<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, nextTick, watch } from "vue";
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
```

- [ ] **Step 3: Verify Tauri dev builds**

Run: `npm run tauri dev`
Expected: mode switcher works, local mode fully functional, remote mode shows login form

- [ ] **Step 4: Commit**

```bash
git add src/components/RemoteMode.vue src/api/remote.ts
git commit -m "feat: implement RemoteMode with login, project management, upload, and logs"
```

---

## Task 10: Tauri backend — persist remote connection config

**Files:**
- Modify: `src-tauri/src/config.rs`

- [ ] **Step 1: Add remote connection config**

Add to `src-tauri/src/config.rs` after the `AppConfig` struct:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemoteConnection {
    pub server_url: String,
}
```

Add a field to `AppConfig`:
```rust
pub struct AppConfig {
    pub projects: Vec<ProjectConfig>,
    #[serde(default)]
    pub remote_connections: Vec<RemoteConnection>,
}
```

Update `Default for AppConfig` to include `remote_connections: vec![]`.

- [ ] **Step 2: Verify Tauri builds**

Run: `cd src-tauri && cargo check`
Expected: compiles successfully

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/config.rs
git commit -m "feat: add remote connection config persistence"
```

---

## Task 11: End-to-end verification

- [ ] **Step 1: Build web-admin**

```bash
cd web-admin && npm run build
```

Expected: successful build in `web-admin/dist/`

- [ ] **Step 2: Build and run server locally**

```bash
cd server
ADMIN_PASSWORD=test123 DATA_DIR=/tmp/tengine-test cargo run
```

Expected: server starts on port 8080, prints "Server starting on 0.0.0.0:8080"

- [ ] **Step 3: Test health endpoint**

```bash
curl http://localhost:8080/api/health
```

Expected: `ok`

- [ ] **Step 4: Test login**

```bash
# SHA-256 of "test123"
HASH=$(echo -n "test123" | shasum -a 256 | cut -d' ' -f1)
curl -X POST http://localhost:8080/api/auth/login \
  -H "Content-Type: application/json" \
  -d "{\"password\": \"$HASH\"}"
```

Expected: `{"token":"eyJ..."}`

- [ ] **Step 5: Test project CRUD with token**

```bash
TOKEN=<token from step 4>
curl -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/projects
```

Expected: `[]`

```bash
curl -X POST -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"project_name": "TestProject"}' \
  http://localhost:8080/api/projects
```

Expected: `{"id":"...","project_name":"TestProject",...}`

- [ ] **Step 6: Test web admin in browser**

Open `http://localhost:8080` in browser.
Expected: login page loads, can log in with "test123", see project management UI.

- [ ] **Step 7: Test Tauri app**

```bash
npm run tauri dev
```

Expected: app launches, mode switcher works, can switch to remote mode and connect to localhost:8080.

- [ ] **Step 8: Commit any fixes from verification**

```bash
git add -A
git commit -m "fix: end-to-end verification fixes"
```
