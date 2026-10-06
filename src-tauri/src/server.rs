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
    platform_access: Arc<std::collections::HashMap<String, bool>>,
    managed: Option<Arc<crate::AppState>>,
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
    platform_access: std::collections::HashMap<String, bool>,
    managed: Option<Arc<crate::AppState>>,
    log_sender: tokio::sync::mpsc::Sender<LogEntry>,
) -> Result<RunningServer, String> {
    let state = ServerState {
        server_root,
        bundles_dir,
        project_id: project_id.clone(),
        project_name,
        platform_access: Arc::new(platform_access),
        managed,
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
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();

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
async fn handle_root(State(state): State<ServerState>) -> Response {
    if state.managed.is_some() { return StatusCode::NOT_FOUND.into_response(); }
    let path = state.server_root.clone();
    serve_directory(&path, "/", &state).await.into_response()
}

/// 通用请求处理 (fallback handler)
async fn handle_request_fallback(
    State(state): State<ServerState>,
    req: axum::extract::Request,
) -> Response {
    if state.managed.is_some() {
        let path = req.uri().path().to_owned();
        let method = req.method().clone();
        let response = serve_managed(&state, &path, &method).await;
        log_request(&state, response.status().as_u16(), method.as_str(), &path).await;
        return response;
    }
    let raw_path = req.uri().path().to_string();
    let req_path = raw_path.strip_prefix("/res/").unwrap_or(raw_path.trim_start_matches('/')).to_string();
    let decoded = percent_decode_str(&req_path).decode_utf8_lossy().to_string();
    let file_path = state.server_root.join(&decoded);

    // Platform access check: 取路径首段作为平台
    if let Some(first_segment) = decoded.split('/').nth(2) {
        if !first_segment.is_empty() {
            if let Some(&enabled) = state.platform_access.get(first_segment) {
                if !enabled {
                    log_request(&state, 403, "GET", &req_path).await;
                    return (StatusCode::FORBIDDEN, "Forbidden").into_response();
                }
            }
        }
    }

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

async fn serve_managed(state: &ServerState, request_path: &str, method: &Method) -> Response {
    use crate::{config::ProjectType, resource_storage::safe_file, resources::{is_apk, sanitize_path_component}};
    let config = state.managed.as_ref().unwrap().config.lock().await;
    let project = match config.projects.iter().find(|p| p.id == state.project_id) {
        Some(project) => project,
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    let prefix = format!("/{}/", project.project_type.namespace());
    let raw = match request_path.strip_prefix(&prefix) { Some(path) => path, None => return StatusCode::NOT_FOUND.into_response() };
    let parts = match raw.split('/').map(|p| percent_decode_str(p).decode_utf8().map(|s| s.into_owned())).collect::<Result<Vec<_>, _>>() {
        Ok(parts) if parts.iter().all(|p| sanitize_path_component(p).is_ok()) => parts,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let pinned = project.project_type == ProjectType::Apk && parts.len() == 5 && parts[2] == "releases";
    let docs = project.project_type == ProjectType::Apk && parts.len() == 4 && parts[2] == "docs";
    if (parts.len() != 3 && !pinned && !docs) || parts[0] != project.project_version || parts[1] != project.project_name { return StatusCode::NOT_FOUND.into_response(); }
    let platform = project.project_type.fixed_platform().unwrap();
    if !project.platform_access.get(platform).copied().unwrap_or(true) { return StatusCode::FORBIDDEN.into_response(); }
    let name = parts.last().unwrap();
    if docs {
        let store = match crate::attachments::store(project) { Ok(store) => store, Err(_) => return StatusCode::NOT_FOUND.into_response() };
        let snapshot = match project.attachment_snapshot.as_deref() { Some(snapshot) => snapshot, None => return StatusCode::NOT_FOUND.into_response() };
        let path = match store.file(snapshot, name) { Ok(path) => path, Err(_) => return StatusCode::NOT_FOUND.into_response() };
        let lease = match store.lease(snapshot) { Ok(lease) => lease, Err(_) => return StatusCode::NOT_FOUND.into_response() };
        let file = match fs::File::open(path).await { Ok(file) => file, Err(_) => return StatusCode::NOT_FOUND.into_response() };
        drop(config);
        let size = match file.metadata().await { Ok(meta) => meta.len(), Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response() };
        let headers = crate::attachment_store::headers(name, size);
        let body = if *method == Method::HEAD { axum::body::Body::empty() } else { axum::body::Body::from_stream(tokio_util::io::ReaderStream::new(crate::attachment_store::Reader { file, _lease: Some(lease) })) };
        return (headers, body).into_response();
    }
    if project.project_type == ProjectType::Apk && ((!pinned && name != "catalog.json") || (pinned && !is_apk(name))) { return StatusCode::NOT_FOUND.into_response(); }
    let version = if pinned {
        if !project.published_releases.contains(&parts[3]) { return StatusCode::NOT_FOUND.into_response(); }
        &parts[3]
    } else {
        match project.active_releases.get(platform) { Some(version) => version, None => return StatusCode::NOT_FOUND.into_response() }
    };
    let path = match crate::local_resources::storage().version_dir(&project.id, "local", platform, version).and_then(|dir| safe_file(&dir, name)) {
        Ok(path) => path,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    drop(config);
    let file = match fs::File::open(&path).await { Ok(file) => file, Err(_) => return StatusCode::NOT_FOUND.into_response() };
    let size = match file.metadata().await { Ok(meta) => meta.len(), Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response() };
    let catalog = !pinned && prefix == "/appversion/";
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, mime_guess::from_path(&path).first_or_octet_stream().to_string().parse().unwrap());
    headers.insert(header::CONTENT_LENGTH, size.into());
    headers.insert(header::CACHE_CONTROL, if catalog { "no-store" } else { "no-cache" }.parse().unwrap());
    headers.insert("X-Content-Type-Options", "nosniff".parse().unwrap());
    headers.insert(header::CONTENT_SECURITY_POLICY, "sandbox; default-src 'none'".parse().unwrap());
    headers.insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
    if !catalog {
        let name = path.file_name().unwrap().to_string_lossy();
        let encoded = percent_encoding::utf8_percent_encode(&name, percent_encoding::NON_ALPHANUMERIC);
        headers.insert(header::CONTENT_DISPOSITION, format!("attachment; filename*=UTF-8''{encoded}").parse().unwrap());
    }
    let body = if *method == Method::HEAD { axum::body::Body::empty() } else { axum::body::Body::from_stream(tokio_util::io::ReaderStream::new(file)) };
    (headers, body).into_response()
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

#[cfg(test)]
mod resource_tests {
    use super::*;
    use crate::{config::{AppConfig, ProjectConfig, ProjectType}, local_resources, AppState};
    use std::collections::HashMap;

    struct TestDirectory { root: PathBuf, previous: Option<std::ffi::OsString> }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            if let Some(value) = &self.previous { std::env::set_var("TENGINE_LOCAL_DATA_DIR", value); } else { std::env::remove_var("TENGINE_LOCAL_DATA_DIR"); }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    #[tokio::test]
    async fn local_imports_publish_live_and_keep_history_after_restart() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("Cache/tests/resource-types").join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&root).unwrap();
        let _directory = TestDirectory { root: root.clone(), previous: std::env::var_os("TENGINE_LOCAL_DATA_DIR") };
        std::env::set_var("TENGINE_LOCAL_DATA_DIR", root.join("config"));
        let mut apk = ProjectConfig::default();
        apk.project_name = "RollCall".into();
        apk.project_type = ProjectType::Apk;
        let mut files = ProjectConfig::default();
        files.project_name = "RollCall".into();
        files.project_type = ProjectType::Files;
        files.platforms = vec!["Any".into()];
        files.platform_access = HashMap::from([("Any".into(),true)]);
        let mut bundle = ProjectConfig::default();
        bundle.project_name = "RollCall".into();
        bundle.bundles_dir = root.join("Bundles").to_string_lossy().into_owned();
        std::fs::create_dir_all(root.join("Bundles/Android")).unwrap();
        std::fs::write(root.join("Bundles/Android/test.txt"),b"bundle").unwrap();
        let config = AppConfig { projects: vec![apk.clone(),files.clone(),bundle.clone()],remote_connections: vec![] };
        config.save().unwrap();
        let state = Arc::new(AppState { config: tokio::sync::Mutex::new(config), servers: tokio::sync::Mutex::new(HashMap::new()) });
        let source = root.join("sources");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("catalog.json"),b"catalog A").unwrap();
        std::fs::write(source.join("flip.apk"),b"APK A").unwrap();
        let selected = vec![source.join("catalog.json").to_string_lossy().into_owned(),source.join("flip.apk").to_string_lossy().into_owned()];
        let stage = local_resources::stage(&apk,&selected,"69").await.unwrap();
        local_resources::commit(&apk,&stage,"69").unwrap();
        assert!(local_resources::stage(&apk,&selected[..1],"70").await.is_err());
        std::fs::write(source.join("说明"),b"plain file").unwrap();
        std::fs::write(source.join("empty"),b"").unwrap();
        let stage = local_resources::stage(&files,&[source.join("说明").to_string_lossy().into_owned(),source.join("empty").to_string_lossy().into_owned()],"a").await.unwrap();
        local_resources::commit(&files,&stage,"a").unwrap();
        let (logs,_receiver) = tokio::sync::mpsc::channel(128);
        let mut running_apk = start_server(AppConfig::resources_dir(),PathBuf::new(),apk.project_name.clone(),apk.id.clone(),0,true,apk.platform_access.clone(),Some(state.clone()),logs.clone()).await.unwrap();
        let mut running_files = start_server(AppConfig::resources_dir(),PathBuf::new(),files.project_name.clone(),files.id.clone(),0,true,files.platform_access.clone(),Some(state.clone()),logs.clone()).await.unwrap();
        let bundle_root = crate::sync::setup_server_root(&bundle.bundles_dir,&bundle.project_name,&bundle.project_version).unwrap();
        let mut running_bundle = start_server(bundle_root,PathBuf::from(&bundle.bundles_dir),bundle.project_name.clone(),bundle.id.clone(),0,true,bundle.platform_access.clone(),None,logs.clone()).await.unwrap();
        let client = reqwest::Client::new();
        let apk_base = format!("http://127.0.0.1:{}/appversion/v1/RollCall",running_apk.port);
        let files_base = format!("http://127.0.0.1:{}/file/v1/RollCall",running_files.port);
        let tutorial_dir = source.join("通用说明目录");
        std::fs::create_dir(&tutorial_dir).unwrap();
        let tutorial = tutorial_dir.join("tutorial.html");
        std::fs::write(&tutorial, "<h1>first</h1>").unwrap();
        let directory_files = crate::attachments::read_attachment_directory(tutorial_dir.to_string_lossy().into_owned()).await.unwrap();
        assert_eq!(directory_files.len(), 1); assert_eq!(directory_files[0].name, "tutorial.html");
        let first = crate::attachments::mutate(apk.id.clone(), "v1".into(), None, vec![tutorial.to_string_lossy().into_owned()], None, &state).await.unwrap();
        let first_snapshot = first["snapshot"].as_str().unwrap().to_owned();
        let response = client.get(format!("{apk_base}/docs/tutorial.html")).send().await.unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-security-policy"], crate::attachment_store::DOCUMENT_CSP);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(response.text().await.unwrap(), "<h1>first</h1>");
        std::fs::write(&tutorial, "<h1>second</h1>").unwrap();
        let second = crate::attachments::mutate(apk.id.clone(), "v1".into(), Some(first_snapshot.clone()), vec![tutorial.to_string_lossy().into_owned()], None, &state).await.unwrap();
        let attachment_snapshot = second["snapshot"].as_str().unwrap().to_owned();
        assert!(crate::attachments::mutate(apk.id.clone(), "v1".into(), Some(first_snapshot), vec![tutorial.to_string_lossy().into_owned()], None, &state).await.is_err());
        // A failed config save must keep both the old pointer and its files.
        let config_path = AppConfig::config_path();
        let backup = config_path.with_extension("backup");
        std::fs::rename(&config_path, &backup).unwrap(); std::fs::create_dir(&config_path).unwrap();
        assert!(crate::attachments::mutate(apk.id.clone(), "v1".into(), Some(attachment_snapshot.clone()), vec![], Some("tutorial.html".into()), &state).await.is_err());
        std::fs::remove_dir(&config_path).unwrap(); std::fs::rename(backup, config_path).unwrap();
        assert_eq!(state.config.lock().await.projects[0].attachment_snapshot.as_deref(), Some(attachment_snapshot.as_str()));
        let head = client.head(format!("{apk_base}/docs/tutorial.html")).send().await.unwrap();
        assert_eq!(head.status(), 200); assert_eq!(head.headers()["content-length"], "15");
        assert!(head.bytes().await.unwrap().is_empty());
        assert_eq!(client.get(format!("{apk_base}/releases/69/flip.apk")).send().await.unwrap().status(),404);
        {
            let mut config = state.config.lock().await;
            for project in &mut config.projects {
                if project.project_type != ProjectType::Bundle { local_resources::activate(project,if project.project_type == ProjectType::Apk {"69"} else {"a"},false).unwrap(); }
            }
            config.save().unwrap();
        }
        let response = client.get(format!("{apk_base}/catalog.json")).send().await.unwrap();
        assert_eq!(response.headers()["cache-control"],"no-store");
        assert_eq!(response.text().await.unwrap(),"catalog A");
        std::fs::write(source.join("catalog.json"),b"catalog B").unwrap();
        std::fs::write(source.join("flip.apk"),b"APK B").unwrap();
        let stage = local_resources::stage(&apk,&selected,"70").await.unwrap();
        local_resources::commit(&apk,&stage,"70").unwrap();
        {
            let mut config = state.config.lock().await;
            local_resources::activate(&mut config.projects[0],"70",false).unwrap();
            config.save().unwrap();
        }
        std::fs::remove_dir_all(&source).unwrap();
        assert_eq!(client.get(format!("{apk_base}/releases/69/flip.apk")).send().await.unwrap().text().await.unwrap(),"APK A");
        assert_eq!(client.get(format!("{apk_base}/catalog.json")).send().await.unwrap().text().await.unwrap(),"catalog B");
        assert_eq!(client.get(format!("{files_base}/%E8%AF%B4%E6%98%8E")).send().await.unwrap().text().await.unwrap(),"plain file");
        let head = client.head(format!("{files_base}/empty")).send().await.unwrap();
        assert_eq!(head.status(),200);
        assert_eq!(head.headers()["content-length"],"0");
        assert!(head.headers().contains_key("content-disposition"));
        for path in ["/v1/RollCall/Android/test.txt","/res/v1/RollCall/Android/test.txt"] {
            assert_eq!(client.get(format!("http://127.0.0.1:{}{path}",running_bundle.port)).send().await.unwrap().text().await.unwrap(),"bundle");
        }
        assert_eq!(client.get(format!("http://127.0.0.1:{}/file/v1/RollCall/test.txt",running_bundle.port)).send().await.unwrap().status(),404);
        {
            let mut config = state.config.lock().await;
            local_resources::activate(&mut config.projects[0],"70",true).unwrap();
        }
        assert_eq!(client.get(format!("{apk_base}/catalog.json")).send().await.unwrap().status(),404);
        assert_eq!(client.get(format!("{apk_base}/docs/tutorial.html")).send().await.unwrap().text().await.unwrap(), "<h1>second</h1>");
        assert_eq!(client.get(format!("{apk_base}/releases/69/flip.apk")).send().await.unwrap().status(),200);
        state.config.lock().await.projects[0].platform_access.insert("Android".into(),false);
        assert_eq!(client.get(format!("{apk_base}/releases/69/flip.apk")).send().await.unwrap().status(),403);
        assert_eq!(client.get(format!("{apk_base}/docs/tutorial.html")).send().await.unwrap().status(),403);
        running_apk.stop();
        let reloaded = AppConfig::load();
        assert_eq!(reloaded.projects[0].active_releases["Android"],"70");
        assert_eq!(reloaded.projects[0].published_releases,["69","70"]);
        assert_eq!(reloaded.projects[0].attachment_snapshot.as_deref(), Some(attachment_snapshot.as_str()));
        crate::attachments::cleanup(&reloaded);
        *state.config.lock().await = reloaded;
        let mut restarted = start_server(AppConfig::resources_dir(),PathBuf::new(),apk.project_name.clone(),apk.id.clone(),0,true,apk.platform_access.clone(),Some(state.clone()),logs).await.unwrap();
        assert_eq!(client.get(format!("http://127.0.0.1:{}/appversion/v1/RollCall/releases/69/flip.apk",restarted.port)).send().await.unwrap().status(),200);
        let tutorial_url = format!("http://127.0.0.1:{}/appversion/v1/RollCall/docs/tutorial.html", restarted.port);
        assert_eq!(client.get(&tutorial_url).send().await.unwrap().text().await.unwrap(), "<h1>second</h1>");
        crate::attachments::mutate(apk.id.clone(), "v1".into(), Some(attachment_snapshot), vec![], Some("tutorial.html".into()), &state).await.unwrap();
        assert_eq!(client.get(tutorial_url).send().await.unwrap().status(), 404);
        local_resources::storage().delete_version(&apk.id,"local","Android","69").unwrap();
        let staged = local_resources::StagedRelease { path: root.join("dummy-stage") };
        assert!(local_resources::commit(&state.config.lock().await.projects[0],&staged,"69").is_err());
        restarted.stop(); running_files.stop(); running_bundle.stop();
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
