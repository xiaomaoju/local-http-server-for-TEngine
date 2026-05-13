use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use mime_guess::from_path;
use percent_encoding::percent_decode_str;
use rust_embed::Embed;
use std::sync::Arc;
use tokio_util::io::ReaderStream;

use crate::ws;
use crate::AppState;

#[derive(Embed)]
#[folder = "../web-admin/dist"]
#[prefix = ""]
struct WebAdminAssets;

pub async fn serve_spa(req: Request) -> Response {
    let path = req.uri().path().trim_start_matches('/');

    if let Some(content) = WebAdminAssets::get(path) {
        let mime = from_path(path).first_or_octet_stream().to_string();
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, mime.parse().unwrap());
        return (StatusCode::OK, headers, content.data.to_vec()).into_response();
    }

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

    // 拆出 4 段：project_version / project_name / platform / file_path
    let mut parts = decoded.splitn(4, '/');
    let project_version = parts.next().unwrap_or("");
    let project_name = parts.next().unwrap_or("");
    let platform = parts.next().unwrap_or("");
    let file_path = parts.next().unwrap_or("");

    if project_version.is_empty() || project_name.is_empty() || platform.is_empty() || file_path.is_empty() {
        broadcast_request_log(&state, 404, "GET", &raw_path, "");
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    }

    // 1) 查项目（克隆后立即释放读锁）
    let project = {
        let config = state.app_config.read().await;
        match config.projects.iter().find(|p| p.project_name == project_name) {
            Some(p) => p.clone(),
            None => {
                broadcast_request_log(&state, 404, "GET", &raw_path, "");
                return (StatusCode::NOT_FOUND, "Not found").into_response();
            }
        }
    };

    // 2) 查项目版本
    let pv = match project.project_versions.iter().find(|v| v.name == project_version) {
        Some(v) => v.clone(),
        None => {
            broadcast_request_log(&state, 404, "GET", &raw_path, &project.id);
            return (StatusCode::NOT_FOUND, "Not found").into_response();
        }
    };

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

    let file = match tokio::fs::File::open(&canonical).await {
        Ok(f) => f,
        Err(_) => {
            broadcast_request_log(&state, 404, "GET", &raw_path, &project.id);
            return (StatusCode::NOT_FOUND, "Not found").into_response();
        }
    };

    let file_size = file.metadata().await.map(|m| m.len()).unwrap_or(0);

    broadcast_request_log(&state, 200, "GET", &raw_path, &project.id);
    let ext = canonical.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    let mime = match ext.as_str() {
        "version" | "hash" | "report" => "text/plain".to_string(),
        "json" => "application/json".to_string(),
        _ => from_path(&canonical).first_or_octet_stream().to_string(),
    };
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, format!("{}; charset=utf-8", mime).parse().unwrap());
    headers.insert(header::CONTENT_LENGTH, file_size.to_string().parse().unwrap());

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);
    (StatusCode::OK, headers, body).into_response()
}

fn broadcast_request_log(state: &Arc<AppState>, status: u16, method: &str, path: &str, project_id: &str) {
    ws::broadcast_log(state, ws::make_log("request", status, method, path, project_id, ""));
}
