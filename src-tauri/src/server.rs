use axum::{
    extract::State,
    http::{header, HeaderMap, Method, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use percent_encoding::percent_decode_str;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::oneshot;
use tower_http::cors::{Any, CorsLayer};

/// 服务器状态
#[derive(Clone)]
struct ServerState {
    server_root: PathBuf,
    /// symlink 实际指向的目录（Bundles 根目录），用于安全检查
    bundles_dir: PathBuf,
    project_id: String,
    project_name: String,
    log_sender: Arc<tokio::sync::mpsc::Sender<LogEntry>>,
}

/// 日志条目
#[derive(Debug, Clone, serde::Serialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub status: u16,
    pub method: String,
    pub path: String,
    pub project_id: String,
}

/// 正在运行的服务器句柄
pub struct RunningServer {
    pub port: u16,
    pub project_id: String,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl RunningServer {
    /// 停止服务器
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 启动 HTTP 静态文件服务器
pub async fn start_server(
    server_root: PathBuf,
    bundles_dir: PathBuf,
    project_name: String,
    project_id: String,
    port: u16,
    cors_enabled: bool,
    log_sender: tokio::sync::mpsc::Sender<LogEntry>,
) -> Result<RunningServer, String> {
    let state = ServerState {
        server_root,
        bundles_dir,
        project_id: project_id.clone(),
        project_name,
        log_sender: Arc::new(log_sender),
    };

    let mut app = Router::new()
        .route("/", get(handle_root))
        .fallback(get(handle_request_fallback))
        .with_state(state);

    if cors_enabled {
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods([Method::GET, Method::HEAD, Method::OPTIONS])
            .allow_headers(Any);
        app = app.layer(cors);
    }

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("端口 {} 绑定失败: {}", port, e))?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let pid = project_id.clone();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .ok();
        log::info!("服务器 {} 已停止", pid);
    });

    Ok(RunningServer {
        port,
        project_id,
        shutdown_tx: Some(shutdown_tx),
    })
}

/// 根路径处理
async fn handle_root(State(state): State<ServerState>) -> impl IntoResponse {
    let path = state.server_root.clone();
    serve_directory(&path, "/", &state).await
}

/// 通用请求处理 (fallback handler)
async fn handle_request_fallback(
    State(state): State<ServerState>,
    req: axum::extract::Request,
) -> Response {
    let raw_path = req.uri().path().to_string();
    let req_path = raw_path.trim_start_matches('/').to_string();
    let decoded = percent_decode_str(&req_path).decode_utf8_lossy().to_string();
    let file_path = state.server_root.join(&decoded);

    // 安全检查：防止路径穿越
    let canonical = match file_path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            log_request(&state, 404, "GET", &req_path).await;
            return not_found_page().into_response();
        }
    };

    // 安全检查：允许 server_root 或 bundles_dir（symlink 目标）下的路径
    let root_canonical = state.server_root.canonicalize().unwrap_or(state.server_root.clone());
    let bundles_canonical = state.bundles_dir.canonicalize().unwrap_or(state.bundles_dir.clone());
    if !canonical.starts_with(&root_canonical) && !canonical.starts_with(&bundles_canonical) {
        log_request(&state, 403, "GET", &req_path).await;
        return (StatusCode::FORBIDDEN, "Forbidden").into_response();
    }

    if canonical.is_dir() {
        if !req_path.ends_with('/') {
            // 重定向到带斜杠的路径
            let location = format!("/{}/", req_path);
            log_request(&state, 301, "GET", &req_path).await;
            return (
                StatusCode::MOVED_PERMANENTLY,
                [(header::LOCATION, location)],
                "",
            )
                .into_response();
        }
        log_request(&state, 200, "GET", &req_path).await;
        serve_directory(&canonical, &format!("/{}", req_path), &state)
            .await
            .into_response()
    } else if canonical.is_file() {
        log_request(&state, 200, "GET", &req_path).await;
        serve_file(&canonical).await.into_response()
    } else {
        log_request(&state, 404, "GET", &req_path).await;
        not_found_page().into_response()
    }
}

/// 目录浏览页面
async fn serve_directory(dir: &Path, req_path: &str, _state: &ServerState) -> Html<String> {
    let mut entries = Vec::new();

    // 父目录链接
    if req_path != "/" {
        let parent = Path::new(req_path)
            .parent()
            .unwrap_or(Path::new("/"))
            .to_string_lossy()
            .to_string();
        let parent = if parent.is_empty() || parent == "." {
            "/".to_string()
        } else if !parent.ends_with('/') {
            format!("{}/", parent)
        } else {
            parent
        };
        entries.push(format!(
            r#"<tr class="dir"><td><a href="{}">📁 ..</a></td><td>-</td><td>-</td></tr>"#,
            parent
        ));
    }

    if let Ok(mut read_dir) = fs::read_dir(dir).await {
        let mut items = Vec::new();
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            let meta = entry.metadata().await.ok();
            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            items.push((name, is_dir, size));
        }

        // 目录在前，文件在后
        items.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

        for (name, is_dir, size) in items {
            let icon = if is_dir { "📁" } else { "📄" };
            let link = if is_dir {
                format!("{}{}/", req_path, name)
            } else {
                format!("{}{}", req_path, name)
            };
            let size_str = if is_dir {
                "-".to_string()
            } else {
                format_size(size)
            };
            entries.push(format!(
                r#"<tr class="{}"><td><a href="{}">{} {}</a></td><td>{}</td><td>-</td></tr>"#,
                if is_dir { "dir" } else { "file" },
                link,
                icon,
                name,
                size_str
            ));
        }
    }

    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>Index of {}</title>
<style>
  body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; margin: 20px; background: #1a1a2e; color: #e0e0e0; }}
  h1 {{ color: #64ffda; font-size: 18px; border-bottom: 1px solid #333; padding-bottom: 10px; }}
  table {{ border-collapse: collapse; width: 100%; }}
  th {{ text-align: left; padding: 8px 16px; color: #888; border-bottom: 1px solid #333; }}
  td {{ padding: 6px 16px; }}
  a {{ color: #82aaff; text-decoration: none; }}
  a:hover {{ color: #64ffda; }}
  tr:hover {{ background: rgba(100,255,218,0.05); }}
  tr.dir a {{ color: #c3e88d; }}
</style>
</head>
<body>
<h1>Index of {}</h1>
<table>
<thead><tr><th>Name</th><th>Size</th><th>Modified</th></tr></thead>
<tbody>{}</tbody>
</table>
</body>
</html>"#,
        req_path,
        req_path,
        entries.join("\n")
    );

    Html(html)
}

/// 文件服务
async fn serve_file(path: &Path) -> Response {
    match fs::read(path).await {
        Ok(bytes) => {
            let mime = mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string();
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                format!("{}; charset=utf-8", mime).parse().unwrap(),
            );
            headers.insert(header::ACCEPT_RANGES, "bytes".parse().unwrap());
            (StatusCode::OK, headers, bytes).into_response()
        }
        Err(_) => not_found_page().into_response(),
    }
}

fn not_found_page() -> (StatusCode, Html<String>) {
    (
        StatusCode::NOT_FOUND,
        Html("<html><body><h1>404 - Not Found</h1></body></html>".to_string()),
    )
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

async fn log_request(state: &ServerState, status: u16, method: &str, path: &str) {
    let entry = LogEntry {
        timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
        status,
        method: method.to_string(),
        path: path.to_string(),
        project_id: state.project_id.clone(),
    };
    let _ = state.log_sender.send(entry).await;
}
