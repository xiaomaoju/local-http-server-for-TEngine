use crate::{attachment_store::Store, config::{AppConfig, ProjectConfig, ProjectType}, local_resources, resources::sanitize_path_component, AppState};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};

pub fn store(project: &ProjectConfig) -> Result<Store, String> {
    if project.project_type != ProjectType::Apk { return Err("只有 APP更新项目支持教程附件".into()); }
    Ok(Store::new(local_resources::storage().project_version_dir(&project.id, "local")?))
}
fn response(project: &ProjectConfig) -> Result<Value, String> {
    Ok(json!({"files":store(project)?.list(project.attachment_snapshot.as_deref())?, "snapshot":project.attachment_snapshot, "project":project}))
}
pub fn cleanup(config: &AppConfig) {
    for project in &config.projects {
        if project.project_type == ProjectType::Apk {
            if let Err(error) = store(project).and_then(|store| store.cleanup(project.attachment_snapshot.as_deref())) { log::warn!("附件清理失败：{error}"); }
        }
    }
}

#[tauri::command]
pub async fn read_attachment_directory(directory: String) -> Result<Vec<local_resources::NativeFile>, String> {
    tokio::task::spawn_blocking(move || {
        let files = crate::attachment_store::directory_files(std::path::Path::new(&directory))?;
        local_resources::describe_files(&files.into_iter().map(|(_, path)| path.to_string_lossy().into_owned()).collect::<Vec<_>>())
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_local_attachments(project_id: String, state: tauri::State<'_, Arc<AppState>>) -> Result<Value, String> {
    let config = state.config.lock().await;
    response(config.projects.iter().find(|p| p.id == project_id).ok_or("项目不存在")?)
}

pub(crate) async fn mutate(project_id: String, project_version: String, expected_snapshot: Option<String>, paths: Vec<String>, remove: Option<String>, state: &AppState) -> Result<Value, String> {
    let mut config = state.config.lock().await;
    let mut next = config.clone();
    let project = next.projects.iter_mut().find(|p| p.id == project_id).ok_or("项目不存在")?;
    if project.project_version != project_version || project.attachment_snapshot != expected_snapshot { return Err("教程附件或项目版本已变化，请重新读取后再操作；已选择文件会保留".into()); }
    let store = store(project)?;
    let current = project.attachment_snapshot.clone();
    let copying = store.clone();
    let previous = current.clone();
    let mut snapshot = tokio::task::spawn_blocking(move || {
        let files = local_resources::describe_files(&paths)?;
        let updates = files.into_iter().map(|file| (file.name, PathBuf::from(file.path))).collect::<Vec<_>>();
        copying.prepare(previous.as_deref(), &updates, remove.as_deref())
    }).await.map_err(|e| e.to_string())??;
    project.attachment_snapshot = Some(snapshot.id.clone());
    let result = response(project)?;
    next.save()?;
    *config = next;
    snapshot.commit();
    if let Some(current) = current { store.retire(&current); }
    log::info!("教程附件已更新：{project_id}/{project_version}/docs");
    Ok(result)
}

#[tauri::command]
pub async fn upload_local_attachments(project_id: String, project_version: String, expected_snapshot: Option<String>, paths: Vec<String>, state: tauri::State<'_, Arc<AppState>>) -> Result<Value, String> {
    if paths.is_empty() { return Err("请选择至少一个附件".into()); }
    mutate(project_id, project_version, expected_snapshot, paths, None, &state).await
}

#[tauri::command]
pub async fn delete_local_attachment(project_id: String, project_version: String, expected_snapshot: Option<String>, name: String, state: tauri::State<'_, Arc<AppState>>) -> Result<Value, String> {
    mutate(project_id, project_version, expected_snapshot, vec![], Some(name), &state).await
}

#[tauri::command]
pub async fn upload_remote_attachments(paths: Vec<String>, project_id: String, project_version: String, revision: i64, server_url: String, token: String) -> Result<Value, String> {
    sanitize_path_component(&project_id)?;
    sanitize_path_component(&project_version)?;
    if paths.is_empty() { return Err("请选择至少一个附件".into()); }
    let staging = local_resources::StagedRelease { path: AppConfig::resources_dir().join("staging").join(uuid::Uuid::new_v4().to_string()) };
    let preparing = Store::new(staging.path.clone());
    let snapshot = tokio::task::spawn_blocking(move || {
        let files = local_resources::describe_files(&paths)?;
        preparing.prepare(None, &files.into_iter().map(|file| (file.name, PathBuf::from(file.path))).collect::<Vec<_>>(), None)
    }).await.map_err(|e| e.to_string())??;
    let store = Store::new(staging.path.clone());
    let mut form = reqwest::multipart::Form::new();
    for file in store.list(Some(&snapshot.id))? {
        let source = tokio::fs::File::open(store.file(&snapshot.id, &file.name)?).await.map_err(|e| e.to_string())?;
        let body = reqwest::Body::wrap_stream(tokio_util::io::ReaderStream::new(source));
        form = form.part("files", reqwest::multipart::Part::stream_with_length(body, file.size).file_name(file.name));
    }
    let url = format!("{}/api/projects/{}/project-versions/{}/attachments", server_url.trim_end_matches('/'), project_id, urlencoding::encode(&project_version));
    let response = reqwest::Client::builder().timeout(std::time::Duration::from_secs(600)).build().map_err(|e| e.to_string())?.post(url).bearer_auth(token).header("If-Match", revision).multipart(form).send().await.map_err(|e| format!("附件上传失败：{e}"))?;
    let status = response.status();
    if !status.is_success() { return Err(format!("服务器返回 {status}: {}", response.text().await.unwrap_or_default())); }
    response.json().await.map_err(|e| format!("解析附件响应失败：{e}"))
}
