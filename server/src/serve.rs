use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
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

    let file_path = state.server_config.resources_dir().join(&decoded);

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
    let project_id = path.split('/').next().unwrap_or("").to_string();
    ws::broadcast_log(state, ws::make_log("request", status, method, path, &project_id, ""));
}
