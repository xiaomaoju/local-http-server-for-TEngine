mod config;
mod server;
mod sync;

use config::{AppConfig, ProjectConfig};
use server::{LogEntry, RunningServer};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

/// 应用全局状态
pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub servers: Mutex<HashMap<String, RunningServer>>,
}

// ===================== Tauri Commands =====================

/// 获取所有项目配置
#[tauri::command]
async fn get_projects(state: tauri::State<'_, Arc<AppState>>) -> Result<Vec<ProjectConfig>, String> {
    let config = state.config.lock().await;
    Ok(config.projects.clone())
}

/// 添加新项目
#[tauri::command]
async fn add_project(state: tauri::State<'_, Arc<AppState>>) -> Result<ProjectConfig, String> {
    let mut config = state.config.lock().await;
    let mut project = ProjectConfig::default();
    // 自动分配不冲突的端口
    let used_ports: Vec<u16> = config.projects.iter().map(|p| p.port).collect();
    while used_ports.contains(&project.port) {
        project.port += 1;
    }
    project.project_name = format!("Project_{}", config.projects.len() + 1);
    config.projects.push(project.clone());
    config.save()?;
    Ok(project)
}

/// 删除项目
#[tauri::command]
async fn remove_project(
    project_id: String,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    // 先停止服务器
    {
        let mut servers = state.servers.lock().await;
        if let Some(mut server) = servers.remove(&project_id) {
            server.stop();
        }
    }
    let mut config = state.config.lock().await;
    config.projects.retain(|p| p.id != project_id);
    config.save()
}

/// 更新项目配置
#[tauri::command]
async fn update_project(
    project: ProjectConfig,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let mut config = state.config.lock().await;
    if let Some(existing) = config.projects.iter_mut().find(|p| p.id == project.id) {
        *existing = project;
    }
    config.save()
}

/// 启动服务器
#[tauri::command]
async fn start_server(
    project_id: String,
    app: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<String, String> {
    let config = state.config.lock().await;
    let project = config
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or("项目不存在")?
        .clone();
    drop(config);

    // 检查是否已启动
    {
        let servers = state.servers.lock().await;
        if servers.contains_key(&project_id) {
            return Err("服务器已在运行".to_string());
        }
    }

    // 设置 server root 符号链接
    let server_root =
        sync::setup_server_root(&project.bundles_dir, &project.project_name)?;

    // 创建日志通道
    let (log_tx, mut log_rx) = tokio::sync::mpsc::channel::<LogEntry>(256);

    // 转发日志到前端
    let app_clone = app.clone();
    let pid = project_id.clone();
    tokio::spawn(async move {
        while let Some(entry) = log_rx.recv().await {
            let _ = app_clone.emit("server-log", &entry);
        }
        log::info!("日志通道关闭: {}", pid);
    });

    // 启动服务器
    let running = server::start_server(
        server_root,
        std::path::PathBuf::from(&project.bundles_dir),
        project.project_name.clone(),
        project_id.clone(),
        project.port,
        project.cors_enabled,
        log_tx,
    )
    .await?;

    let url = format!("http://127.0.0.1:{}/{}/", running.port, project.project_name);

    let mut servers = state.servers.lock().await;
    servers.insert(project_id, running);

    // 发送启动事件
    let _ = app.emit("server-started", &url);

    Ok(url)
}

/// 停止服务器
#[tauri::command]
async fn stop_server(
    project_id: String,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let mut servers = state.servers.lock().await;
    if let Some(mut server) = servers.remove(&project_id) {
        server.stop();
        Ok(())
    } else {
        Err("服务器未运行".to_string())
    }
}

/// 获取服务器运行状态
#[tauri::command]
async fn get_server_status(
    project_id: String,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<bool, String> {
    let servers = state.servers.lock().await;
    Ok(servers.contains_key(&project_id))
}

/// 同步资源（不需要重启服务器）
#[tauri::command]
async fn sync_resources(
    project_id: String,
    app: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<sync::SyncResult, String> {
    let config = state.config.lock().await;
    let project = config
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or("项目不存在")?
        .clone();
    drop(config);

    // 对每个平台执行同步
    let mut all_synced_files = Vec::new();
    let mut last_version = None;
    let mut messages = Vec::new();

    for platform in &project.platforms {
        let syncer =
            sync::ResourceSyncer::new(&project.bundles_dir, &project.package_name, platform);
        let result = syncer.sync();

        // 发送同步日志
        let log_msg = format!("[{}] {}", platform, result.message);
        let _ = app.emit(
            "sync-log",
            serde_json::json!({
                "project_id": project_id,
                "message": log_msg,
                "success": result.success,
            }),
        );

        if result.success {
            last_version = result.version;
            all_synced_files.extend(result.synced_files);
        }
        messages.push(format!("[{}] {}", platform, result.message));
    }

    Ok(sync::SyncResult {
        success: true,
        message: messages.join("; "),
        version: last_version,
        synced_files: all_synced_files,
    })
}

/// 版本信息（每个平台）
#[derive(Debug, Clone, serde::Serialize)]
pub struct PlatformVersionInfo {
    pub platform: String,
    pub latest: Option<String>,
    pub synced: Option<String>,
}

/// 读取资源版本信息（最新可用版本 + 已同步版本）
#[tauri::command]
async fn get_resource_version(
    project_id: String,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Vec<PlatformVersionInfo>, String> {
    let config = state.config.lock().await;
    let project = config
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or("项目不存在")?
        .clone();
    drop(config);

    let mut infos = Vec::new();
    for platform in &project.platforms {
        let syncer =
            sync::ResourceSyncer::new(&project.bundles_dir, &project.package_name, platform);
        infos.push(PlatformVersionInfo {
            platform: platform.clone(),
            latest: syncer.read_latest_version(),
            synced: syncer.read_synced_version(),
        });
    }
    Ok(infos)
}

/// 列出所有可用版本
#[tauri::command]
async fn list_versions(
    project_id: String,
    platform: String,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Vec<sync::VersionEntry>, String> {
    let config = state.config.lock().await;
    let project = config
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or("项目不存在")?
        .clone();
    drop(config);

    let syncer = sync::ResourceSyncer::new(&project.bundles_dir, &project.package_name, &platform);
    Ok(syncer.list_versions())
}

/// 同步指定版本的资源
#[tauri::command]
async fn sync_specific_version(
    project_id: String,
    version: String,
    app: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<sync::SyncResult, String> {
    let config = state.config.lock().await;
    let project = config
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or("项目不存在")?
        .clone();
    drop(config);

    let mut all_synced_files = Vec::new();
    let mut messages = Vec::new();

    for platform in &project.platforms {
        let syncer =
            sync::ResourceSyncer::new(&project.bundles_dir, &project.package_name, platform);
        let result = syncer.sync_version(Some(&version));

        let log_msg = format!("[{}] {}", platform, result.message);
        let _ = app.emit(
            "sync-log",
            serde_json::json!({
                "project_id": project_id,
                "message": log_msg,
                "success": result.success,
            }),
        );

        if result.success {
            all_synced_files.extend(result.synced_files);
        }
        messages.push(format!("[{}] {}", platform, result.message));
    }

    Ok(sync::SyncResult {
        success: true,
        message: messages.join("; "),
        version: Some(version),
        synced_files: all_synced_files,
    })
}

/// 获取所有运行中服务器的 ID
#[tauri::command]
async fn get_running_servers(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Vec<String>, String> {
    let servers = state.servers.lock().await;
    Ok(servers.keys().cloned().collect())
}

/// 获取本机局域网 IP 地址（遍历所有网卡）
#[tauri::command]
fn get_local_ips() -> Vec<String> {
    let mut ips = Vec::new();

    // 判断是否为局域网 IP
    fn is_lan_ip(ip: &str) -> bool {
        ip.starts_with("192.168.") || ip.starts_with("10.") || {
            // 172.16.0.0 - 172.31.255.255
            if let Some(rest) = ip.strip_prefix("172.") {
                if let Some(second) = rest.split('.').next() {
                    if let Ok(n) = second.parse::<u8>() {
                        return (16..=31).contains(&n);
                    }
                }
            }
            false
        }
    }

    // macOS / Linux: ifconfig
    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(output) = std::process::Command::new("ifconfig").output() {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                let line = line.trim();
                if line.starts_with("inet ") && !line.contains("127.0.0.1") {
                    if let Some(ip) = line.split_whitespace().nth(1) {
                        if is_lan_ip(ip) && !ips.contains(&ip.to_string()) {
                            ips.push(ip.to_string());
                        }
                    }
                }
            }
        }
    }

    // Windows: ipconfig
    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = std::process::Command::new("ipconfig").output() {
            // Windows ipconfig 输出可能是 GBK 编码，先尝试 UTF-8，再尝试逐字节处理
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                let line = line.trim();
                // 匹配 "IPv4 Address" (英文) 或 "IPv4 地址" (中文)
                if line.contains("IPv4") {
                    // 格式: "IPv4 Address. . . . . . . . . . . : 192.168.x.x"
                    if let Some(ip_part) = line.split(':').last() {
                        let ip = ip_part.trim();
                        if is_lan_ip(ip) && !ips.contains(&ip.to_string()) {
                            ips.push(ip.to_string());
                        }
                    }
                }
            }
        }
    }

    // 优先排序：192.168 开头的排在最前面
    ips.sort_by(|a, b| {
        let a_pri = if a.starts_with("192.168.") { 0 } else { 1 };
        let b_pri = if b.starts_with("192.168.") { 0 } else { 1 };
        a_pri.cmp(&b_pri)
    });

    ips
}

// ===================== Remote Mode Commands =====================

/// 列出本地 Bundles 目录中某平台的所有版本
#[tauri::command]
async fn list_local_bundle_versions(
    bundles_dir: String,
    package_name: String,
    platform: String,
) -> Result<Vec<sync::VersionEntry>, String> {
    let syncer = sync::ResourceSyncer::new(&bundles_dir, &package_name, &platform);
    Ok(syncer.list_versions())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RemoteUploadResult {
    pub success: bool,
    pub version: String,
    pub platform: String,
    pub file_count: u32,
}

/// 把本地 Bundles 目录中指定版本的所有文件上传到远程服务器
#[tauri::command]
async fn upload_version_to_remote(
    bundles_dir: String,
    package_name: String,
    platform: String,
    version: String,
    project_id: String,
    server_url: String,
    token: String,
) -> Result<RemoteUploadResult, String> {
    use std::path::PathBuf;

    let version_dir = PathBuf::from(&bundles_dir)
        .join(&platform)
        .join(&package_name)
        .join(&version);

    if !version_dir.exists() {
        return Err(format!("版本目录不存在: {}", version_dir.display()));
    }

    // 收集要上传的文件
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let read_dir = std::fs::read_dir(&version_dir)
        .map_err(|e| format!("读取版本目录失败: {}", e))?;

    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let filename = entry.file_name().to_string_lossy().to_string();
        let bytes = std::fs::read(&path)
            .map_err(|e| format!("读取文件 {} 失败: {}", filename, e))?;
        files.push((filename, bytes));
    }

    if files.is_empty() {
        return Err("版本目录为空".to_string());
    }

    // 构造 multipart 请求
    let mut form = reqwest::multipart::Form::new()
        .text("platform", platform.clone())
        .text("version", version.clone());

    for (filename, bytes) in files {
        let part = reqwest::multipart::Part::bytes(bytes).file_name(filename);
        form = form.part("files", part);
    }

    let url = format!(
        "{}/api/projects/{}/upload",
        server_url.trim_end_matches('/'),
        project_id
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| format!("构造 HTTP 客户端失败: {}", e))?;

    let res = client
        .post(&url)
        .bearer_auth(&token)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("上传请求失败: {}", e))?;

    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(format!("服务器返回 {}: {}", status, body));
    }

    let result: RemoteUploadResult = res
        .json()
        .await
        .map_err(|e| format!("解析响应失败: {}", e))?;

    Ok(result)
}

// ===================== App Entry =====================

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config = AppConfig::load();
    let app_state = Arc::new(AppState {
        config: Mutex::new(config),
        servers: Mutex::new(HashMap::new()),
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_projects,
            add_project,
            remove_project,
            update_project,
            start_server,
            stop_server,
            get_server_status,
            sync_resources,
            get_running_servers,
            get_resource_version,
            list_versions,
            sync_specific_version,
            get_local_ips,
            list_local_bundle_versions,
            upload_version_to_remote,
        ])
        .run(tauri::generate_context!())
        .expect("启动应用失败");
}
