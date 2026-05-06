use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{Method, StatusCode},
    middleware,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

use crate::auth;
use crate::config::ProjectConfig;
use crate::serve;
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
        .route("/api/projects/:id", put(update_project))
        .route("/api/projects/:id", delete(delete_project))
        // 新增：项目版本 CRUD
        .route("/api/projects/:id/project-versions", get(list_project_versions))
        .route("/api/projects/:id/project-versions", post(create_project_version))
        .route("/api/projects/:id/project-versions/:pver", delete(delete_project_version))
        .route("/api/projects/:id/project-versions/:pver", put(rename_project_version))
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

#[derive(Deserialize)]
struct UpdateProjectRequest {
    project_name: Option<String>,
    platforms: Option<Vec<String>>,
    package_name: Option<String>,
}

async fn update_project(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateProjectRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut config = state.app_config.write().await;
    let project = config.projects.iter_mut().find(|p| p.id == id)
        .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))?;
    if let Some(name) = req.project_name {
        if name.contains('/') || name.contains('\\') || name.contains("..") || name.is_empty() {
            return Err((StatusCode::BAD_REQUEST, "Invalid project name".to_string()));
        }
        project.project_name = name;
    }
    if let Some(platforms) = req.platforms {
        project.platforms = platforms;
    }
    if let Some(package_name) = req.package_name {
        project.package_name = package_name;
    }
    config.save(&state.server_config.config_path())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
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
    if !project.project_versions.iter().any(|v| v.name == pver) {
        return Err((StatusCode::NOT_FOUND, "Project version not found".to_string()));
    }
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
    if !project.project_versions.iter().any(|v| v.name == pver) {
        return Err(StatusCode::NOT_FOUND);
    }
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
    {
        let mut config = state.app_config.write().await;
        let project = config.projects.iter_mut().find(|p| p.id == id)
            .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))?;
        if !project.platforms.contains(&platform) {
            return Err((StatusCode::NOT_FOUND, "Platform not declared on project".to_string()));
        }
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
    }

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
    if !project.project_versions.iter().any(|v| v.name == pver) {
        return Err((StatusCode::NOT_FOUND, "Project version not found".to_string()));
    }
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
    if !project.project_versions.iter().any(|v| v.name == pver) {
        return Err((StatusCode::NOT_FOUND, "Project version not found".to_string()));
    }
    let project_name = project.project_name.clone();
    drop(config);
    let platform = params.platform.unwrap_or_else(|| "Android".to_string());
    let storage = Storage::new(state.server_config.resources_dir());
    let files = storage.list_files(&project_name, &pver, &platform, &params.version)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(files))
}

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
    let project_name = {
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
        project_name
    };
    // Lock dropped. Disk delete is best-effort (matches delete_project pattern).
    let storage = Storage::new(state.server_config.resources_dir());
    let _ = storage.delete_project_version(&project_name, &pver);
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
struct RenameProjectVersionRequest {
    name: String,
}

async fn rename_project_version(
    State(state): State<Arc<AppState>>,
    Path((id, pver)): Path<(String, String)>,
    Json(req): Json<RenameProjectVersionRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let new_name = req.name.trim().to_string();
    if new_name.is_empty()
        || new_name.contains('/')
        || new_name.contains('\\')
        || new_name.contains("..")
    {
        return Err((StatusCode::BAD_REQUEST, "Invalid project version name".to_string()));
    }
    if new_name == pver {
        return Ok(StatusCode::OK);
    }
    let project_name = {
        let mut config = state.app_config.write().await;
        let project = config
            .projects
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))?;
        if project.project_versions.iter().any(|v| v.name == new_name) {
            return Err((StatusCode::CONFLICT, "Project version already exists".to_string()));
        }
        let pv = project
            .project_versions
            .iter_mut()
            .find(|v| v.name == pver)
            .ok_or((StatusCode::NOT_FOUND, "Project version not found".to_string()))?;
        pv.name = new_name.clone();
        let project_name = project.project_name.clone();
        config
            .save(&state.server_config.config_path())
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
        project_name
    };

    let storage = Storage::new(state.server_config.resources_dir());
    if let Err(e) = storage.rename_project_version(&project_name, &pver, &new_name) {
        // 元数据已改但磁盘改名失败：回滚元数据
        let mut config = state.app_config.write().await;
        if let Some(project) = config.projects.iter_mut().find(|p| p.id == id) {
            if let Some(pv) = project.project_versions.iter_mut().find(|v| v.name == new_name) {
                pv.name = pver.clone();
                let _ = config.save(&state.server_config.config_path());
            }
        }
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e));
    }

    ws::broadcast_log(&state, ws::make_log(
        "rename", 200, "PUT",
        &format!("/api/projects/{}/project-versions/{}", id, pver),
        &id,
        &format!("项目版本重命名: {} → {}", pver, new_name),
    ));

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
    {
        let mut config = state.app_config.write().await;
        let project = config
            .projects
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))?;
        if !project.platforms.contains(&plat) {
            return Err((StatusCode::NOT_FOUND, "Platform not declared on project".to_string()));
        }
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
    }

    let action = if req.enabled { "开启" } else { "关闭" };
    ws::broadcast_log(&state, ws::make_log(
        "access", 200, "PUT",
        &format!("/api/projects/{}/project-versions/{}/platforms/{}/access", id, pver, plat),
        &id,
        &format!("{} 平台访问: {} ({})", action, plat, pver),
    ));

    Ok(StatusCode::OK)
}
