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
        .route("/api/projects/:id/upload", post(upload_resources).layer(DefaultBodyLimit::max(512 * 1024 * 1024)))
        .route("/api/projects/:id/versions", get(list_versions))
        .route("/api/projects/:id/versions/:ver/activate", put(activate_version))
        .route("/api/projects/:id/versions/:ver", delete(delete_version))
        .route("/api/projects/:id/status", get(project_status))
        .layer(middleware::from_fn_with_state(state.clone(), auth::auth_middleware));

    let ws_route = Router::new()
        .route("/api/ws/logs", get(ws::ws_logs));

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
