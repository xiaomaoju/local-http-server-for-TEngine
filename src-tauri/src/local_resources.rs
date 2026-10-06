use crate::{config::{AppConfig, ProjectConfig, ProjectType}, resource_storage::{FileEntry, Storage}, resources::sanitize_path_component};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeFile { pub path: String, pub name: String, pub size: u64 }

pub fn describe_files(paths: &[String]) -> Result<Vec<NativeFile>, String> {
    paths.iter().map(|source| {
        let path = PathBuf::from(source);
        let metadata = std::fs::metadata(&path).map_err(|e| format!("无法读取文件：{e}"))?;
        if !metadata.is_file() { return Err("请选择普通文件".into()); }
        let name = path.file_name().and_then(|n| n.to_str()).ok_or("文件名编码无效")?.to_owned();
        sanitize_path_component(&name)?;
        Ok(NativeFile { path: source.clone(), name, size: metadata.len() })
    }).collect()
}

pub fn storage() -> Storage { Storage::new(AppConfig::resources_dir()) }
pub fn platform(project: &ProjectConfig) -> Result<&'static str, String> {
    project.project_type.fixed_platform().ok_or("Bundle 项目请使用资源同步".into())
}
pub fn files(project: &ProjectConfig, version: &str) -> Result<Vec<FileEntry>, String> {
    storage().list_files(&project.id, "local", platform(project)?, version)
}

pub struct StagedRelease { pub path: PathBuf }
impl Drop for StagedRelease { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.path); } }

pub async fn stage(project: &ProjectConfig, paths: &[String], version: &str) -> Result<StagedRelease, String> {
    platform(project)?;
    project.project_type.validate_version(version)?;
    let selected = describe_files(paths)?;
    project.project_type.validate_files(&selected.iter().map(|f| f.name.clone()).collect::<Vec<_>>())?;
    if selected.iter().map(|f| f.size).sum::<u64>() > 512 * 1024 * 1024 { return Err("单次导入最多 512MB".into()); }
    let stage = StagedRelease { path: AppConfig::resources_dir().join("staging").join(uuid::Uuid::new_v4().to_string()) };
    tokio::fs::create_dir_all(&stage.path).await.map_err(|e| e.to_string())?;
    for file in selected {
        let copied = tokio::fs::copy(&file.path, stage.path.join(&file.name)).await.map_err(|e| format!("导入失败：{e}"))?;
        if copied != file.size { return Err("导入期间源文件大小发生变化，请重新选择文件".into()); }
    }
    Ok(stage)
}

pub fn commit(project: &ProjectConfig, stage: &StagedRelease, version: &str) -> Result<(), String> {
    if project.project_type == ProjectType::Apk && project.published_releases.iter().any(|v| v == version) { return Err("此 APK 编号曾经发布，不能覆盖或复用".into()); }
    let destination = storage().version_dir(&project.id, "local", platform(project)?, version)?;
    if destination.exists() { return Err("版本已存在，不能覆盖".into()); }
    std::fs::create_dir_all(destination.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::rename(&stage.path, destination).map_err(|e| e.to_string())
}

pub fn activate(project: &mut ProjectConfig, version: &str, unpublish: bool) -> Result<(), String> {
    let plat = platform(project)?.to_owned();
    if unpublish { project.active_releases.remove(&plat); return Ok(()); }
    project.project_type.validate_version(version)?;
    let names = files(project, version)?.into_iter().map(|f| f.name).collect::<Vec<_>>();
    project.project_type.validate_files(&names)?;
    if project.project_type == ProjectType::Apk && !project.published_releases.iter().any(|v| v == version) { project.published_releases.push(version.into()); }
    project.active_releases.insert(plat, version.into());
    Ok(())
}

#[tauri::command]
pub fn describe_upload_files(paths: Vec<String>) -> Result<Vec<NativeFile>, String> { describe_files(&paths) }

#[tauri::command]
pub async fn list_local_releases(project_id: String, state: tauri::State<'_, std::sync::Arc<crate::AppState>>) -> Result<Vec<crate::resource_storage::VersionEntry>, String> {
    let config = state.config.lock().await;
    let project = config.projects.iter().find(|p| p.id == project_id).ok_or("项目不存在")?;
    Ok(storage().list_versions(&project.id, "local", platform(project)?))
}

#[tauri::command]
pub async fn list_local_release_files(project_id: String, version: String, state: tauri::State<'_, std::sync::Arc<crate::AppState>>) -> Result<Vec<FileEntry>, String> {
    let config = state.config.lock().await;
    let project = config.projects.iter().find(|p| p.id == project_id).ok_or("项目不存在")?;
    files(project, &version)
}

#[tauri::command]
pub async fn import_local_release(project_id: String, paths: Vec<String>, version: String, state: tauri::State<'_, std::sync::Arc<crate::AppState>>) -> Result<String, String> {
    let initial = state.config.lock().await.projects.iter().find(|p| p.id == project_id).cloned().ok_or("项目不存在")?;
    let version = if version.is_empty() && initial.project_type == ProjectType::Files {
        format!("{}-{}", chrono::Utc::now().format("%Y%m%d_%H%M%S"), &uuid::Uuid::new_v4().simple().to_string()[..8])
    } else { version };
    let stage = stage(&initial, &paths, &version).await?;
    let config = state.config.lock().await;
    let current = config.projects.iter().find(|p| p.id == project_id).ok_or("项目已删除")?;
    if current.project_type != initial.project_type || current.project_version != initial.project_version { return Err("导入期间项目设置发生变化，请重试".into()); }
    commit(current, &stage, &version)?;
    Ok(version)
}

#[tauri::command]
pub async fn activate_local_release(project_id: String, version: String, unpublish: bool, state: tauri::State<'_, std::sync::Arc<crate::AppState>>) -> Result<ProjectConfig, String> {
    let mut config = state.config.lock().await;
    let mut next = config.clone();
    let project = next.projects.iter_mut().find(|p| p.id == project_id).ok_or("项目不存在")?;
    activate(project, &version, unpublish)?;
    let result = project.clone();
    next.save()?;
    *config = next;
    Ok(result)
}

#[tauri::command]
pub async fn delete_local_release(project_id: String, version: String, state: tauri::State<'_, std::sync::Arc<crate::AppState>>) -> Result<(), String> {
    let config = state.config.lock().await;
    let project = config.projects.iter().find(|p| p.id == project_id).ok_or("项目不存在")?;
    let plat = platform(project)?;
    if project.active_releases.get(plat) == Some(&version) { return Err("当前版本已发布，请先切换或取消发布".into()); }
    storage().delete_version(&project.id, "local", plat, &version)
}

#[tauri::command]
pub async fn set_local_download_access(project_id: String, enabled: bool, state: tauri::State<'_, std::sync::Arc<crate::AppState>>) -> Result<ProjectConfig, String> {
    let mut config = state.config.lock().await;
    let mut next = config.clone();
    let project = next.projects.iter_mut().find(|p| p.id == project_id).ok_or("项目不存在")?;
    let plat = platform(project)?.to_owned();
    project.platform_access.insert(plat, enabled);
    let result = project.clone();
    next.save()?;
    *config = next;
    Ok(result)
}
