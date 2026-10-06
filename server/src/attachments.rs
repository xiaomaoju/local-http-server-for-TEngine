use crate::{api, attachment_store::{self, Store}, auth::Auth, config::{ProjectConfig, ProjectType}, db::{self, ApiError, ApiResult}, storage::Storage, AppState};
use axum::{extract::{Extension, Multipart, Path, State}, http::HeaderMap, Json};
use serde_json::{json, Value};
use std::{collections::HashSet, path::PathBuf, sync::Arc};
use tokio::io::AsyncWriteExt;

fn store(state: &AppState, project: &ProjectConfig, version: &str) -> ApiResult<Store> {
    if project.project_type != ProjectType::Apk { return Err(ApiError::bad("只有 APP更新项目支持教程附件")); }
    if !project.project_versions.iter().any(|v| v.name == version) { return Err(ApiError::not_found("项目版本不存在")); }
    let root = Storage::new(state.server_config.resources_dir()).project_version_dir(&project.id, version).map_err(ApiError::bad)?;
    Ok(Store::new(root))
}
fn snapshot(project: &ProjectConfig, version: &str) -> Option<String> {
    project.project_versions.iter().find(|v| v.name == version).and_then(|v| v.attachment_snapshot.clone())
}
fn response(store: &Store, project: ProjectConfig, version: &str) -> ApiResult<Json<Value>> {
    let snapshot = snapshot(&project, version);
    let files = store.list(snapshot.as_deref()).map_err(ApiError::bad)?;
    Ok(Json(json!({"files":files, "snapshot":snapshot, "project":project})))
}

// Commit metadata and its audit entry together; a failed audit must not publish files.
async fn save(state: &AppState, auth: &Auth, project: &mut ProjectConfig, action: &str, object: &str, message: &str) -> ApiResult<()> {
    let mut transaction = state.db.begin().await?;
    let config = serde_json::to_string(project).map_err(|_| ApiError::bad("项目配置无效"))?;
    let update = sqlx::query("UPDATE projects SET config = ?, revision = revision + 1 WHERE id = ? AND revision = ?").bind(config).bind(&project.id).bind(project.revision).execute(&mut *transaction).await?;
    if update.rows_affected() != 1 { return Err(ApiError::conflict("项目已被修改，请重新读取后重试")); }
    let timestamp = chrono::Utc::now().to_rfc3339();
    let inserted = sqlx::query("INSERT INTO audit_logs(timestamp, actor_id, actor_name, project_id, action, object, message) VALUES(?, ?, ?, ?, ?, ?, ?)").bind(&timestamp).bind(&auth.user.id).bind(&auth.user.username).bind(&project.id).bind(action).bind(object).bind(message).execute(&mut *transaction).await?;
    transaction.commit().await?;
    project.revision += 1;
    let _ = state.log_tx.send(db::AuditLog { id: inserted.last_insert_rowid(), timestamp, actor_id: auth.user.id.clone(), actor_name: auth.user.username.clone(), project_id: Some(project.id.clone()), action: action.into(), object: object.into(), message: message.into() });
    Ok(())
}

pub async fn list(State(state): State<Arc<AppState>>, Extension(auth): Extension<Auth>, Path((id, version)): Path<(String, String)>) -> ApiResult<Json<Value>> {
    let _guard = state.mutation.lock().await;
    let project = api::access(&state, &auth, &id, false).await?;
    let store = store(&state, &project, &version)?;
    response(&store, project, &version)
}

struct UploadStage(PathBuf);
impl Drop for UploadStage { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }

pub async fn upload(State(state): State<Arc<AppState>>, Extension(auth): Extension<Auth>, Path((id, version)): Path<(String, String)>, headers: HeaderMap, mut multipart: Multipart) -> ApiResult<Json<Value>> {
    let initial = api::access(&state, &auth, &id, false).await?;
    api::revision(&headers, &initial)?;
    store(&state, &initial, &version)?;
    let stage = UploadStage(state.server_config.data_dir.join("staging").join(uuid::Uuid::new_v4().to_string()));
    tokio::fs::create_dir_all(&stage.0).await?;
    let mut updates = Vec::new();
    let mut seen = HashSet::new();
    let mut total = 0u64;
    while let Some(mut field) = multipart.next_field().await.map_err(|_| ApiError::bad("附件上传中断、格式无效或超过 512MB 限制"))? {
        if field.name() != Some("files") { return Err(ApiError::bad("附件上传只接受 files 字段")); }
        let name = field.file_name().ok_or_else(|| ApiError::bad("附件文件名缺失"))?.to_owned();
        attachment_store::validate_name(&name).map_err(ApiError::bad)?;
        if !seen.insert(name.to_lowercase()) { return Err(ApiError::bad("上传包含同名附件（不区分大小写）")); }
        let path = stage.0.join(&name);
        let mut file = tokio::fs::File::create(&path).await?;
        while let Some(chunk) = field.chunk().await.map_err(|_| ApiError::bad("附件上传中断或超过 512MB 限制"))? {
            total = total.saturating_add(chunk.len() as u64);
            if total > attachment_store::MAX_SIZE { return Err(ApiError::bad("单次上传最多 512MB")); }
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        updates.push((name, path));
    }
    if updates.is_empty() { return Err(ApiError::bad("请选择至少一个附件")); }
    let _guard = state.mutation.lock().await;
    let mut project = api::access(&state, &auth, &id, false).await?;
    api::revision(&headers, &project)?;
    let store = store(&state, &project, &version)?;
    let previous = snapshot(&project, &version);
    let copying = store.clone();
    let current = previous.clone();
    let mut next = tokio::task::spawn_blocking(move || copying.prepare(current.as_deref(), &updates, None)).await.map_err(|e| ApiError::bad(e.to_string()))?.map_err(ApiError::bad)?;
    project.project_versions.iter_mut().find(|v| v.name == version).unwrap().attachment_snapshot = Some(next.id.clone());
    // Once commit begins, cancellation may leave its outcome unknown. Keep the
    // snapshot for startup reconciliation rather than deleting a committed target.
    next.commit();
    if let Err(error) = save(&state, &auth, &mut project, "attachments_upload", &format!("{version}/docs"), "教程附件已更新并立即生效").await {
        store.retire(&next.id);
        return Err(error);
    }
    if let Some(previous) = previous { store.retire(&previous); }
    response(&store, project, &version)
}

pub async fn delete(State(state): State<Arc<AppState>>, Extension(auth): Extension<Auth>, Path((id, version, name)): Path<(String, String, String)>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let _guard = state.mutation.lock().await;
    let mut project = api::access(&state, &auth, &id, false).await?;
    api::revision(&headers, &project)?;
    let store = store(&state, &project, &version)?;
    let previous = snapshot(&project, &version);
    attachment_store::validate_name(&name).map_err(ApiError::bad)?;
    store.file(previous.as_deref().ok_or_else(|| ApiError::not_found("附件不存在"))?, &name).map_err(|_| ApiError::not_found("附件不存在"))?;
    let copying = store.clone();
    let current = previous.clone();
    let deleted = name.clone();
    let mut next = tokio::task::spawn_blocking(move || copying.prepare(current.as_deref(), &[], Some(&deleted))).await.map_err(|e| ApiError::bad(e.to_string()))?.map_err(ApiError::bad)?;
    project.project_versions.iter_mut().find(|v| v.name == version).unwrap().attachment_snapshot = Some(next.id.clone());
    next.commit();
    if let Err(error) = save(&state, &auth, &mut project, "attachments_delete", &format!("{version}/docs/{name}"), "教程附件已删除").await {
        store.retire(&next.id);
        return Err(error);
    }
    store.retire(previous.as_deref().unwrap());
    response(&store, project, &version)
}

pub async fn cleanup(state: &AppState) -> ApiResult<()> {
    let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM projects").fetch_all(&state.db).await?;
    for id in ids {
        let project = db::project(&state.db, &id).await?;
        if project.project_type != ProjectType::Apk { continue; }
        for version in &project.project_versions {
            store(state, &project, &version.name)?.cleanup(version.attachment_snapshot.as_deref()).map_err(ApiError::bad)?;
        }
    }
    Ok(())
}
