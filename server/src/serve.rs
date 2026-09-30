use crate::{
    db,
    storage::{safe_file, Storage},
    AppState, Protocol,
};
use axum::{
    body::Body,
    extract::{Extension, Request, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use mime_guess::from_path;
use percent_encoding::percent_decode_str;
use rust_embed::Embed;
use std::sync::{atomic::Ordering, Arc};
use tokio_util::io::ReaderStream;

#[derive(Embed)]
#[folder = "../web-admin/dist"]
#[prefix = ""]
struct WebAdminAssets;
pub async fn serve_spa(req: Request) -> Response {
    let path = req.uri().path().trim_start_matches('/');
    if path.starts_with("api/") || path.starts_with("res/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    if req.method() != "GET" {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let content = WebAdminAssets::get(path).or_else(|| WebAdminAssets::get("index.html"));
    match content {
        Some(content) => {
            let mime = if WebAdminAssets::get(path).is_some() {
                from_path(path).first_or_octet_stream().to_string()
            } else {
                "text/html; charset=utf-8".into()
            };
            (
                [
                    (header::CONTENT_TYPE, mime),
                    (header::CACHE_CONTROL, "no-cache".into()),
                ],
                content.data.to_vec(),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "Web admin not available").into_response(),
    }
}
pub async fn serve_resource(
    State(state): State<Arc<AppState>>,
    Extension(protocol): Extension<Protocol>,
    req: Request,
) -> Response {
    use crate::resource_logs::{self, ResourceLog};
    let mut log = ResourceLog::new(req.method().as_str(), req.uri().path());
    let fail = |log: ResourceLog, status: StatusCode, message: &str| {
        resource_logs::publish(&state, log, status.as_u16(), "failed", message);
        (status, message.to_owned()).into_response()
    };
    let raw = req.uri().path().trim_start_matches("/res/");
    let decoded: Result<Vec<_>, _> = raw.split('/').map(|part| percent_decode_str(part).decode_utf8()).collect();
    let parts = match decoded {
        Ok(parts) if parts.len() == 4 => parts,
        _ => return fail(log, StatusCode::NOT_FOUND, "资源路径格式无效"),
    };
    // The public name path is /res/version/name/platform/file. Existing UUID paths
    // still resolve without relocating resources or mutating stored project IDs.
    let legacy = if uuid::Uuid::parse_str(&parts[0]).is_ok() { db::project(&state.db, &parts[0]).await.ok() } else { None };
    let named = db::project_by_name(&state.db, &parts[1]).await;
    if let Err(error) = &named {
        // A known UUID keeps its legacy namespace even if unrelated projects
        // happen to share the name of its version (for example "v1").
        if error.0 != StatusCode::NOT_FOUND && !(legacy.is_some() && error.0 == StatusCode::CONFLICT) { return fail(log, error.0, &error.1); }
    }
    if let (Some(old), Ok(new)) = (&legacy, &named) {
        if new.project_versions.iter().any(|v| v.name == parts[0]) && (old.id != new.id || parts[0] != parts[1]) {
            return fail(log, StatusCode::CONFLICT, "资源地址存在歧义，请使用明确的项目版本及项目名称");
        }
    }
    let (project, version) = match (legacy, named) {
        (Some(project), _) => (project, parts[1].as_ref()),
        (None, Ok(project)) => (project, parts[0].as_ref()),
        _ => return fail(log, StatusCode::NOT_FOUND, "项目不存在"),
    };
    log.project_id = Some(project.id.clone());
    if crate::storage::sanitize_path_component(&parts[3]).is_err() {
        return fail(log, StatusCode::NOT_FOUND, "文件路径无效");
    }
    if req.method() != "GET" && req.method() != "HEAD" {
        return fail(log, StatusCode::METHOD_NOT_ALLOWED, "资源仅支持 GET 和 HEAD 请求");
    }
    if (protocol == Protocol::Http && !state.http_enabled.load(Ordering::Relaxed))
        || (protocol == Protocol::Https && !state.https_enabled.load(Ordering::Relaxed)) {
        return fail(log, StatusCode::FORBIDDEN, "此下载协议已关闭");
    }
    let settings = match project.project_versions.iter().find(|v| v.name == version).and_then(|v| v.platform_settings.get(parts[2].as_ref())) {
        Some(settings) => settings,
        None => return fail(log, StatusCode::NOT_FOUND, "项目版本或平台不存在"),
    };
    if !settings.access_enabled { return fail(log, StatusCode::FORBIDDEN, "该平台已关闭客户端下载"); }
    let active = match &settings.active_bundle {
        Some(active) => active,
        None => return fail(log, StatusCode::NOT_FOUND, "该平台尚未发布资源版本"),
    };
    let store = Storage::new(state.server_config.resources_dir());
    let path = match store.version_dir(&project.id, version, &parts[2], active).and_then(|dir| safe_file(&dir, &parts[3])) {
        Ok(path) => path,
        Err(_) => return fail(log, StatusCode::NOT_FOUND, "文件不存在或路径无效"),
    };
    let file = match tokio::fs::File::open(&path).await {
        Ok(file) => file,
        Err(error) => return fail(log, if error.kind() == std::io::ErrorKind::NotFound { StatusCode::NOT_FOUND } else { StatusCode::INTERNAL_SERVER_ERROR }, "资源文件读取失败"),
    };
    let size = match file.metadata().await {
        Ok(meta) if meta.is_file() => meta.len(),
        _ => return fail(log, StatusCode::INTERNAL_SERVER_ERROR, "资源文件信息读取失败"),
    };
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let mime = if ["version", "hash", "report"].contains(&ext) { "text/plain".into() } else { from_path(&path).first_or_octet_stream().to_string() };
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, mime.parse().unwrap());
    headers.insert(header::CONTENT_LENGTH, size.into());
    headers.insert(header::CACHE_CONTROL, "no-cache".parse().unwrap());
    headers.insert("X-Content-Type-Options", "nosniff".parse().unwrap());
    headers.insert(header::CONTENT_SECURITY_POLICY, "sandbox; default-src 'none'".parse().unwrap());
    headers.insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
    if req.method() == "HEAD" || size == 0 {
        resource_logs::publish(&state, log, 200, "completed", if req.method() == "HEAD" { "资源存在（HEAD）" } else { "空文件响应完成" });
        return (headers, Body::empty()).into_response();
    }
    let reader = LoggedReader { file, state: state.clone(), log: Some(log), remaining: size };
    (headers, Body::from_stream(ReaderStream::new(reader))).into_response()
}

struct LoggedReader {
    file: tokio::fs::File,
    state: Arc<AppState>,
    log: Option<crate::resource_logs::ResourceLog>,
    remaining: u64,
}
impl tokio::io::AsyncRead for LoggedReader {
    fn poll_read(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, buf: &mut tokio::io::ReadBuf<'_>) -> std::task::Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let result = std::pin::Pin::new(&mut self.file).poll_read(cx, buf);
        if let std::task::Poll::Ready(ref read) = result {
            let count = (buf.filled().len() - before) as u64;
            self.remaining = self.remaining.saturating_sub(count);
            let outcome = if read.is_err() { Some(("failed", "响应开始后读取失败，HTTP 状态已发送")) }
                else if self.remaining == 0 { Some(("completed", "资源响应读取完成")) }
                else if count == 0 { Some(("failed", "资源响应不完整，文件提前结束")) } else { None };
            if let (Some((outcome, message)), Some(log)) = (outcome, self.log.as_ref()) {
                crate::resource_logs::publish(&self.state, log.clone(), 200, outcome, message);
                self.log = None;
            }
        }
        result
    }
}
impl Drop for LoggedReader {
    fn drop(&mut self) {
        if let Some(log) = self.log.take() {
            crate::resource_logs::publish(&self.state, log, 200, "interrupted", "响应流提前关闭，可能是客户端断开连接");
        }
    }
}
