use crate::{
    auth::{self, Auth},
    config::{PlatformSettings, ProjectConfig, ProjectType, ProjectVersion},
    db::{self, ApiError, ApiResult},
    serve,
    storage::{self, Storage},
    users, ws, AppState, Protocol,
};
use axum::{
    extract::{DefaultBodyLimit, Extension, Multipart, Path, Query, State},
    http::HeaderMap,
    middleware,
    routing::{get, post, put},
    Json, Router,
};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    sync::{atomic::Ordering, Arc},
};
use tokio::io::AsyncWriteExt;
use tower_http::cors::CorsLayer;

pub fn build_router(state: Arc<AppState>, protocol: Protocol) -> Router {
    let protected = Router::new()
        .route("/api/projects/:id/project-versions/:pver/download-stats", get(crate::apk_downloads::stats))
        .route("/api/projects/:id/project-versions/:pver/attachments", get(crate::attachments::list).post(crate::attachments::upload).layer(DefaultBodyLimit::max(512 * 1024 * 1024)))
        .route("/api/projects/:id/project-versions/:pver/attachments/:name", axum::routing::delete(crate::attachments::delete))
        .route("/api/auth/me", get(auth::me))
        .route("/api/auth/logout", post(auth::logout))
        .route("/api/auth/password", put(auth::change_password))
        .route("/api/auth/ws-ticket", post(ws::ticket))
        .route("/api/users/lookup", get(users::lookup))
        .route("/api/admin/users", get(users::list).post(users::create))
        .route(
            "/api/admin/users/:id",
            put(users::update).delete(users::delete),
        )
        .route("/api/admin/users/:id/password", put(users::reset))
        .route("/api/projects", get(list_projects).post(create_project))
        .route(
            "/api/projects/:id",
            get(get_project).put(update_project).delete(delete_project),
        )
        .route(
            "/api/projects/:id/members",
            get(list_members).post(add_member),
        )
        .route(
            "/api/projects/:id/members/:user",
            axum::routing::delete(remove_member),
        )
        .route("/api/projects/:id/owner", put(transfer))
        .route(
            "/api/projects/:id/project-versions",
            get(list_project_versions).post(create_project_version),
        )
        .route(
            "/api/projects/:id/project-versions/:pver",
            put(rename_project_version).delete(delete_project_version),
        )
        .route(
            "/api/projects/:id/project-versions/:pver/platforms/:plat/access",
            put(set_platform_access),
        )
        .route(
            "/api/projects/:id/project-versions/:pver/upload",
            post(upload_resources).layer(DefaultBodyLimit::max(512 * 1024 * 1024)),
        )
        .route(
            "/api/projects/:id/project-versions/:pver/incremental-upload",
            post(upload_resources).layer(DefaultBodyLimit::max(512 * 1024 * 1024)),
        )
        .route(
            "/api/projects/:id/project-versions/:pver/versions",
            get(list_versions),
        )
        .route(
            "/api/projects/:id/project-versions/:pver/versions/:ver/activate",
            put(activate_version),
        )
        .route(
            "/api/projects/:id/project-versions/:pver/versions/:ver",
            axum::routing::delete(delete_version),
        )
        .route(
            "/api/projects/:id/project-versions/:pver/files",
            get(list_files),
        )
        .route(
            "/api/projects/:id/project-versions/:pver/manifest",
            get(manifest),
        )
        .route(
            "/api/projects/:id/project-versions/:pver/status",
            get(project_status),
        )
        .route("/api/audit", get(audit_logs))
        .route("/api/resource-logs", get(crate::resource_logs::recent))
        .route(
            "/api/settings/protocol-access",
            get(get_protocol_access).put(set_protocol_access),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::middleware,
        ));
    Router::new()
        .merge(protected)
        .route("/api/health", get(|| async { "ok" }))
        .route("/api/auth/login", post(auth::login))
        .route("/api/ws/logs", get(ws::ws_logs))
        .route(
            "/res/*path",
            axum::routing::any(serve::serve_resource).layer(CorsLayer::permissive()),
        )
        .route("/appversion/*path", axum::routing::any(serve::serve_resource).layer(CorsLayer::permissive()))
        .route("/file/*path", axum::routing::any(serve::serve_resource).layer(CorsLayer::permissive()))
        .fallback(serve::serve_spa)
        .layer(Extension(protocol))
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .with_state(state)
}
fn storage(state: &AppState) -> Storage {
    Storage::new(state.server_config.resources_dir())
}
fn validate_component(value: &str) -> ApiResult<()> {
    storage::sanitize_path_component(value)
        .map(|_| ())
        .map_err(ApiError::bad)
}
pub(crate) fn revision(headers: &HeaderMap, project: &ProjectConfig) -> ApiResult<()> {
    let expected = headers
        .get("If-Match")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim_matches('"').parse::<i64>().ok());
    if expected != Some(project.revision) {
        return Err(ApiError::conflict(
            "项目已被修改，请刷新最新信息后重试；未保存的输入会保留",
        ));
    }
    Ok(())
}
pub(crate) async fn access(state: &AppState, auth: &Auth, id: &str, manage: bool) -> ApiResult<ProjectConfig> {
    auth::from_hash(state, &auth.hash).await?;
    Ok(db::authorized(state, &auth.user.id, id, manage).await?.1)
}
fn platform<'a>(
    project: &'a ProjectConfig,
    pver: &str,
    plat: &str,
) -> ApiResult<&'a PlatformSettings> {
    validate_component(pver)?;
    validate_component(plat)?;
    if !project.platforms.iter().any(|p| p == plat) {
        return Err(ApiError::bad("项目未配置此平台"));
    }
    project
        .project_versions
        .iter()
        .find(|v| v.name == pver)
        .and_then(|v| v.platform_settings.get(plat))
        .ok_or_else(|| ApiError::bad("项目版本不存在"))
}
async fn list_projects(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
) -> ApiResult<Json<Vec<ProjectConfig>>> {
    let ids: Vec<String> = if auth.user.admin() {
        sqlx::query_scalar("SELECT id FROM projects ORDER BY rowid DESC")
            .fetch_all(&state.db)
            .await?
    } else {
        sqlx::query_scalar("SELECT id FROM projects WHERE owner_id = ? OR id IN (SELECT project_id FROM project_members WHERE user_id = ?) ORDER BY rowid DESC").bind(&auth.user.id).bind(&auth.user.id).fetch_all(&state.db).await?
    };
    let mut result = Vec::new();
    for id in ids {
        let (_, p) = db::authorized(&state, &auth.user.id, &id, false).await?;
        result.push(p);
    }
    Ok(Json(result))
}
async fn get_project(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path(id): Path<String>,
) -> ApiResult<Json<ProjectConfig>> {
    Ok(Json(access(&state, &auth, &id, false).await?))
}
#[derive(Deserialize)]
struct ProjectInput {
    project_name: String,
    #[serde(default)]
    project_type: Option<ProjectType>,
    #[serde(default)]
    package_name: String,
    #[serde(default)]
    platforms: Vec<String>,
    #[serde(default = "initial_version")]
    project_version: String,
}
fn initial_version() -> String {
    "v1".into()
}
fn validate_project(req: &mut ProjectInput, kind: ProjectType) -> ApiResult<()> {
    if req.project_name.trim().is_empty() || req.project_name.chars().count() > 80 {
        return Err(ApiError::bad("项目名称需为 1–80 个字符"));
    }
    validate_component(req.project_name.trim())?;
    if let Some(plat) = kind.fixed_platform() {
        if !req.platforms.is_empty() && req.platforms != [plat] { return Err(ApiError::bad("此项目类型的平台不可修改")); }
        req.platforms = vec![plat.into()];
        req.package_name.clear();
    } else { validate_component(&req.package_name)?; }
    if req.platforms.is_empty() || req.platforms.len() > 16 {
        return Err(ApiError::bad("请选择至少一个平台"));
    }
    let mut set = HashSet::new();
    for p in &req.platforms {
        validate_component(p)?;
        if !set.insert(p) {
            return Err(ApiError::bad("平台重复"));
        }
    }
    Ok(())
}
fn new_version(name: String, platforms: &[String]) -> ProjectVersion {
    ProjectVersion {
        attachment_snapshot: None,
        name,
        platform_settings: platforms
            .iter()
            .map(|p| {
                (
                    p.clone(),
                    PlatformSettings {
                        access_enabled: true,
                        active_bundle: None,
                        published_releases: vec![],
                    },
                )
            })
            .collect(),
    }
}
async fn create_project(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Json(mut req): Json<ProjectInput>,
) -> ApiResult<Json<ProjectConfig>> {
    let kind = req.project_type.unwrap_or_default();
    validate_project(&mut req, kind)?;
    validate_component(&req.project_version)?;
    let _guard = state.mutation.lock().await;
    auth::from_hash(&state, &auth.hash).await?;
    db::check_project_name(&state.db, kind, req.project_name.trim(), "").await?;
    let p = ProjectConfig {
        id: uuid::Uuid::new_v4().to_string(),
        project_name: req.project_name.trim().into(),
        project_type: kind,
        owner_id: auth.user.id.clone(),
        project_versions: vec![new_version(req.project_version, &req.platforms)],
        platforms: req.platforms,
        package_name: req.package_name,
        revision: 1,
        can_manage: true,
        shared_with_me: false,
    };
    sqlx::query("INSERT INTO projects(id, owner_id, config) VALUES(?, ?, ?)")
        .bind(&p.id)
        .bind(&p.owner_id)
        .bind(serde_json::to_string(&p).unwrap())
        .execute(&state.db)
        .await?;
    db::audit(
        &state,
        &auth.user,
        Some(&p.id),
        "project.create",
        &p.project_name,
        "创建项目",
    )
    .await?;
    Ok(Json(p))
}
async fn update_project(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(mut req): Json<ProjectInput>,
) -> ApiResult<Json<ProjectConfig>> {
    let _guard = state.mutation.lock().await;
    let mut p = access(&state, &auth, &id, false).await?;
    if req.project_type.is_some_and(|kind| kind != p.project_type) { return Err(ApiError::bad("项目类型创建后不可修改")); }
    validate_project(&mut req, p.project_type)?;
    revision(&headers, &p)?;
    if p.project_name != req.project_name.trim() {
        db::check_project_name(&state.db, p.project_type, req.project_name.trim(), &id).await?;
    }
    for v in &p.project_versions {
        for (plat, settings) in &v.platform_settings {
            if !req.platforms.contains(plat) && settings.active_bundle.is_some() {
                return Err(ApiError::conflict("请先取消被移除平台的发布"));
            }
        }
    }
    p.project_name = req.project_name.trim().into();
    p.package_name = req.package_name;
    p.platforms = req.platforms;
    for v in &mut p.project_versions {
        v.platform_settings.retain(|k, _| p.platforms.contains(k));
        for plat in &p.platforms {
            v.platform_settings
                .entry(plat.clone())
                .or_insert(PlatformSettings {
                    access_enabled: true,
                    active_bundle: None,
                    published_releases: vec![],
                });
        }
    }
    db::save(&state.db, &mut p).await?;
    db::audit(
        &state,
        &auth.user,
        Some(&id),
        "project.update",
        &p.project_name,
        "保存项目配置",
    )
    .await?;
    Ok(Json(p))
}
async fn delete_project(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    let _guard = state.mutation.lock().await;
    let p = access(&state, &auth, &id, true).await?;
    revision(&headers, &p)?;
    // Removing metadata first prevents any new download. Files left by an I/O error can be cleaned by retrying maintenance.
    sqlx::query("DELETE FROM projects WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await?;
    storage(&state).delete_project(&id).map_err(ApiError::bad)?;
    let _ = state.access_tx.send(());
    db::audit(
        &state,
        &auth.user,
        Some(&id),
        "project.delete",
        &p.project_name,
        "删除项目及资源",
    )
    .await?;
    Ok(Json(serde_json::json!({"success": true})))
}
async fn list_members(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let p = access(&state, &auth, &id, false).await?;
    let members: Vec<db::User> = sqlx::query_as("SELECT id, username, role, enabled, must_change_password FROM users WHERE id IN (SELECT user_id FROM project_members WHERE project_id = ?)").bind(&id).fetch_all(&state.db).await?;
    let owner = db::user(&state.db, &p.owner_id).await?;
    Ok(Json(
        serde_json::json!({"owner": {"id": owner.id, "username": owner.username}, "members": members.iter().map(|u| serde_json::json!({"id": u.id,"username": u.username})).collect::<Vec<_>>()}),
    ))
}
#[derive(Deserialize)]
struct MemberInput {
    user_id: String,
}
async fn add_member(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<MemberInput>,
) -> ApiResult<Json<ProjectConfig>> {
    let _guard = state.mutation.lock().await;
    let mut p = access(&state, &auth, &id, true).await?;
    revision(&headers, &p)?;
    let target = db::user(&state.db, &req.user_id).await?;
    if !target.enabled || target.id == p.owner_id {
        return Err(ApiError::bad("请选择其他有效用户"));
    }
    sqlx::query("INSERT OR IGNORE INTO project_members(project_id, user_id) VALUES(?, ?)")
        .bind(&id)
        .bind(&target.id)
        .execute(&state.db)
        .await?;
    db::save(&state.db, &mut p).await?;
    db::audit(
        &state,
        &auth.user,
        Some(&id),
        "member.add",
        &target.username,
        "分享项目",
    )
    .await?;
    Ok(Json(p))
}
async fn remove_member(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path((id, user)): Path<(String, String)>,
    headers: HeaderMap,
) -> ApiResult<Json<ProjectConfig>> {
    let _guard = state.mutation.lock().await;
    let mut p = access(&state, &auth, &id, true).await?;
    revision(&headers, &p)?;
    sqlx::query("DELETE FROM project_members WHERE project_id = ? AND user_id = ?")
        .bind(&id)
        .bind(&user)
        .execute(&state.db)
        .await?;
    db::save(&state.db, &mut p).await?;
    let _ = state.access_tx.send(());
    db::audit(
        &state,
        &auth.user,
        Some(&id),
        "member.remove",
        &user,
        "移除共享成员",
    )
    .await?;
    Ok(Json(p))
}
async fn transfer(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<MemberInput>,
) -> ApiResult<Json<ProjectConfig>> {
    let _guard = state.mutation.lock().await;
    let p = access(&state, &auth, &id, true).await?;
    revision(&headers, &p)?;
    let target = db::user(&state.db, &req.user_id).await?;
    if !target.enabled || target.id == p.owner_id {
        return Err(ApiError::bad("请选择其他有效用户"));
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("INSERT OR IGNORE INTO project_members(project_id, user_id) VALUES(?, ?)")
        .bind(&id)
        .bind(&p.owner_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM project_members WHERE project_id = ? AND user_id = ?")
        .bind(&id)
        .bind(&target.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE projects SET owner_id = ?, revision = revision + 1 WHERE id = ?")
        .bind(&target.id)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let _ = state.access_tx.send(());
    db::audit(
        &state,
        &auth.user,
        Some(&id),
        "project.transfer",
        &target.username,
        "转交项目所有权",
    )
    .await?;
    Ok(Json(access(&state, &auth, &id, false).await?))
}
async fn list_project_versions(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<ProjectVersion>>> {
    Ok(Json(
        access(&state, &auth, &id, false).await?.project_versions,
    ))
}
#[derive(Deserialize)]
struct NameInput {
    name: String,
}
async fn create_project_version(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<NameInput>,
) -> ApiResult<Json<ProjectConfig>> {
    validate_component(&req.name)?;
    let _guard = state.mutation.lock().await;
    let mut p = access(&state, &auth, &id, false).await?;
    revision(&headers, &p)?;
    if p.project_versions.iter().any(|v| v.name == req.name) {
        return Err(ApiError::conflict("项目版本已存在"));
    }
    p.project_versions
        .push(new_version(req.name.clone(), &p.platforms));
    db::save(&state.db, &mut p).await?;
    db::audit(
        &state,
        &auth.user,
        Some(&id),
        "project-version.create",
        &req.name,
        "创建项目版本",
    )
    .await?;
    Ok(Json(p))
}
async fn rename_project_version(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path((id, pver)): Path<(String, String)>,
    headers: HeaderMap,
    Json(req): Json<NameInput>,
) -> ApiResult<Json<ProjectConfig>> {
    validate_component(&req.name)?;
    let _guard = state.mutation.lock().await;
    let mut p = access(&state, &auth, &id, false).await?;
    revision(&headers, &p)?;
    if p.project_versions.iter().any(|v| v.name == req.name) {
        return Err(ApiError::conflict("项目版本已存在"));
    }
    let v = p
        .project_versions
        .iter_mut()
        .find(|v| v.name == pver)
        .ok_or_else(ApiError::missing)?;
    v.name = req.name.clone();
    storage(&state)
        .rename_project_version(&id, &pver, &req.name)
        .map_err(ApiError::bad)?;
    if let Err(e) = db::save_renamed_version(&state.db, &mut p, &pver, &req.name).await {
        let _ = storage(&state).rename_project_version(&id, &req.name, &pver);
        return Err(e);
    }
    db::audit(
        &state,
        &auth.user,
        Some(&id),
        "project-version.rename",
        &pver,
        &format!("重命名为 {}", req.name),
    )
    .await?;
    Ok(Json(p))
}
async fn delete_project_version(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path((id, pver)): Path<(String, String)>,
    headers: HeaderMap,
) -> ApiResult<Json<ProjectConfig>> {
    let _guard = state.mutation.lock().await;
    let mut p = access(&state, &auth, &id, false).await?;
    revision(&headers, &p)?;
    let v = p
        .project_versions
        .iter()
        .find(|v| v.name == pver)
        .ok_or_else(ApiError::missing)?;
    if v.platform_settings
        .values()
        .any(|s| s.active_bundle.is_some())
    {
        return Err(ApiError::conflict("请先取消该项目版本全部平台的发布"));
    }
    p.project_versions.retain(|v| v.name != pver);
    storage(&state)
        .delete_project_version(&id, &pver)
        .map_err(ApiError::bad)?;
    db::save(&state.db, &mut p).await?;
    db::audit(
        &state,
        &auth.user,
        Some(&id),
        "project-version.delete",
        &pver,
        "删除项目版本",
    )
    .await?;
    Ok(Json(p))
}
#[derive(Deserialize)]
struct EnabledInput {
    enabled: bool,
}
async fn set_platform_access(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path((id, pver, plat)): Path<(String, String, String)>,
    headers: HeaderMap,
    Json(req): Json<EnabledInput>,
) -> ApiResult<Json<ProjectConfig>> {
    let _guard = state.mutation.lock().await;
    let mut p = access(&state, &auth, &id, false).await?;
    revision(&headers, &p)?;
    platform(&p, &pver, &plat)?;
    p.project_versions
        .iter_mut()
        .find(|v| v.name == pver)
        .unwrap()
        .platform_settings
        .get_mut(&plat)
        .unwrap()
        .access_enabled = req.enabled;
    db::save(&state.db, &mut p).await?;
    db::audit(
        &state,
        &auth.user,
        Some(&id),
        "access",
        &format!("{pver}/{plat}"),
        if req.enabled {
            "开启客户端下载"
        } else {
            "关闭客户端下载"
        },
    )
    .await?;
    Ok(Json(p))
}
#[derive(Deserialize)]
struct ResourceQuery {
    platform: String,
    version: Option<String>,
}
async fn list_versions(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path((id, pver)): Path<(String, String)>,
    Query(q): Query<ResourceQuery>,
) -> ApiResult<Json<Vec<storage::VersionEntry>>> {
    let p = access(&state, &auth, &id, false).await?;
    platform(&p, &pver, &q.platform)?;
    Ok(Json(storage(&state).list_versions(&id, &pver, &q.platform)))
}
async fn list_files(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path((id, pver)): Path<(String, String)>,
    Query(q): Query<ResourceQuery>,
) -> ApiResult<Json<Vec<storage::FileEntry>>> {
    let p = access(&state, &auth, &id, false).await?;
    platform(&p, &pver, &q.platform)?;
    Ok(Json(
        storage(&state)
            .list_files(&id, &pver, &q.platform, q.version.as_deref().unwrap_or(""))
            .map_err(ApiError::bad)?,
    ))
}
async fn manifest(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path((id, pver)): Path<(String, String)>,
    Query(q): Query<ResourceQuery>,
) -> ApiResult<Json<Vec<storage::FileManifestEntry>>> {
    let _guard = state.mutation.lock().await;
    let p = access(&state, &auth, &id, false).await?;
    platform(&p, &pver, &q.platform)?;
    let storage = storage(&state);
    let files = tokio::task::spawn_blocking(move || {
        storage.list_files_with_hash(&id, &pver, &q.platform, q.version.as_deref().unwrap_or(""))
    })
    .await
    .map_err(|_| ApiError::bad("文件扫描失败"))?
    .map_err(ApiError::bad)?;
    Ok(Json(files))
}
#[derive(Deserialize)]
struct ActivateInput {
    #[serde(default)]
    unpublish: bool,
}
async fn activate_version(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path((id, pver, ver)): Path<(String, String, String)>,
    Query(q): Query<ResourceQuery>,
    headers: HeaderMap,
    Json(req): Json<ActivateInput>,
) -> ApiResult<Json<ProjectConfig>> {
    let _guard = state.mutation.lock().await;
    let mut p = access(&state, &auth, &id, false).await?;
    revision(&headers, &p)?;
    platform(&p, &pver, &q.platform)?;
    if !req.unpublish {
        let files = storage(&state)
            .list_files(&id, &pver, &q.platform, &ver)
            .map_err(ApiError::bad)?;
        if files.is_empty() {
            return Err(ApiError::bad("资源版本不存在或为空，无法发布"));
        }
        if p.project_type != ProjectType::Bundle {
            p.project_type.validate_version(&ver).map_err(ApiError::bad)?;
            p.project_type.validate_files(&files.into_iter().map(|f| f.name).collect::<Vec<_>>()).map_err(ApiError::bad)?;
        }
    }
    let kind = p.project_type;
    let settings = p.project_versions
        .iter_mut()
        .find(|v| v.name == pver)
        .unwrap()
        .platform_settings
        .get_mut(&q.platform)
        .unwrap();
    if kind == ProjectType::Apk && !req.unpublish && !settings.published_releases.contains(&ver) {
        settings.published_releases.push(ver.clone());
    }
    settings.active_bundle = if req.unpublish {
        None
    } else {
        Some(ver.clone())
    };
    db::save(&state.db, &mut p).await?;
    db::audit(
        &state,
        &auth.user,
        Some(&id),
        "release",
        &format!("{pver}/{}/{ver}", q.platform),
        if req.unpublish {
            "取消发布"
        } else {
            "发布资源版本"
        },
    )
    .await?;
    Ok(Json(p))
}
async fn delete_version(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path((id, pver, ver)): Path<(String, String, String)>,
    Query(q): Query<ResourceQuery>,
    headers: HeaderMap,
) -> ApiResult<Json<ProjectConfig>> {
    let _guard = state.mutation.lock().await;
    let mut p = access(&state, &auth, &id, false).await?;
    revision(&headers, &p)?;
    if platform(&p, &pver, &q.platform)?.active_bundle.as_deref() == Some(&ver) {
        return Err(ApiError::conflict("此版本正在发布，请先切换或取消发布"));
    }
    storage(&state)
        .delete_version(&id, &pver, &q.platform, &ver)
        .map_err(ApiError::bad)?;
    db::save(&state.db, &mut p).await?;
    db::audit(
        &state,
        &auth.user,
        Some(&id),
        "bundle.delete",
        &format!("{pver}/{}/{ver}", q.platform),
        "删除资源版本",
    )
    .await?;
    Ok(Json(p))
}
async fn project_status(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path((id, pver)): Path<(String, String)>,
) -> ApiResult<Json<ProjectVersion>> {
    access(&state, &auth, &id, false)
        .await?
        .project_versions
        .into_iter()
        .find(|v| v.name == pver)
        .map(Json)
        .ok_or_else(ApiError::missing)
}
struct Staging(std::path::PathBuf);
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
async fn upload_resources(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path((id, pver)): Path<(String, String)>,
    mut multipart: Multipart,
) -> ApiResult<Json<serde_json::Value>> {
    let initial = access(&state, &auth, &id, false).await?;
    validate_component(&pver)?;
    let stage = Staging(
        state
            .server_config
            .data_dir
            .join("staging")
            .join(uuid::Uuid::new_v4().to_string()),
    );
    tokio::fs::create_dir(&stage.0).await?;
    let mut fields = HashMap::<String, String>::new();
    let mut names = HashSet::new();
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::bad("上传中断或数据格式无效"))?
    {
        let name = field.name().unwrap_or("").to_owned();
        if name == "files" {
            let file_name = field
                .file_name()
                .ok_or_else(|| ApiError::bad("文件名缺失"))?
                .to_owned();
            validate_component(&file_name)?;
            if !names.insert(file_name.clone()) {
                return Err(ApiError::bad("上传包含同名文件"));
            }
            let mut file = tokio::fs::File::create(stage.0.join(&file_name)).await?;
            while let Some(chunk) = field
                .chunk()
                .await
                .map_err(|_| ApiError::bad("上传中断或超过 512MB 限制"))?
            {
                file.write_all(&chunk).await?;
            }
            file.flush().await?;
        } else {
            if !["platform", "version", "base_version", "copy_files"].contains(&name.as_str())
                || fields.contains_key(&name)
            {
                return Err(ApiError::bad("上传字段无效或重复"));
            }
            fields.insert(
                name,
                field
                    .text()
                    .await
                    .map_err(|_| ApiError::bad("上传参数无效"))?,
            );
        }
    }
    let plat = fields
        .get("platform")
        .ok_or_else(|| ApiError::bad("请选择目标平台"))?;
    if initial.project_type != ProjectType::Bundle && (fields.contains_key("base_version") || fields.contains_key("copy_files")) {
        return Err(ApiError::bad("APK 和普通文件仅支持完整上传"));
    }
    if initial.project_type == ProjectType::Apk && fields.get("version").is_none_or(|v| v.is_empty()) {
        return Err(ApiError::bad("请填写 APK 的 versionCode 作为发布编号"));
    }
    let version = fields
        .get("version")
        .filter(|v| !v.is_empty())
        .cloned()
        .unwrap_or_else(|| {
            format!(
                "{}-{}",
                chrono::Utc::now().format("%Y%m%d_%H%M%S"),
                &uuid::Uuid::new_v4().simple().to_string()[..8]
            )
        });
    initial.project_type.validate_version(&version).map_err(ApiError::bad)?;
    platform(&initial, &pver, plat)?;
    let _guard = state.mutation.lock().await;
    let mut p = access(&state, &auth, &id, false).await?;
    let settings = platform(&p, &pver, plat)?;
    if p.project_type == ProjectType::Apk && settings.published_releases.contains(&version) {
        return Err(ApiError::conflict("此 APK 编号曾经发布，不能覆盖或复用"));
    }
    if p.revision != initial.revision {
        return Err(ApiError::conflict(
            "上传期间项目发生变化，请确认最新配置后重试",
        ));
    }
    let store = storage(&state);
    let destination = store
        .version_dir(&id, &pver, plat, &version)
        .map_err(ApiError::bad)?;
    if destination.exists() {
        return Err(ApiError::conflict("资源版本已存在，请换一个版本号"));
    }
    let copy_files: Vec<String> =
        serde_json::from_str(fields.get("copy_files").map(String::as_str).unwrap_or("[]"))
            .map_err(|_| ApiError::bad("增量文件清单无效"))?;
    for name in &copy_files {
        validate_component(name)?;
        if !names.insert(name.clone()) {
            return Err(ApiError::bad("增量文件与上传文件重复"));
        }
        let base = fields
            .get("base_version")
            .ok_or_else(|| ApiError::bad("缺少增量基础版本"))?;
        let dir = store
            .version_dir(&id, &pver, plat, base)
            .map_err(ApiError::bad)?;
        let source = storage::safe_file(&dir, name).map_err(ApiError::bad)?;
        tokio::fs::copy(source, stage.0.join(name)).await?;
    }
    if names.is_empty() {
        return Err(ApiError::bad("请选择至少一个文件"));
    }
    if p.project_type != ProjectType::Bundle {
        p.project_type.validate_files(&names.iter().cloned().collect::<Vec<_>>()).map_err(ApiError::bad)?;
    }
    let file_count = names.len();
    tokio::fs::create_dir_all(destination.parent().unwrap()).await?;
    tokio::fs::rename(&stage.0, &destination).await?;
    if let Err(e) = db::save(&state.db, &mut p).await {
        let _ = tokio::fs::remove_dir_all(&destination).await;
        return Err(e);
    }
    db::audit(
        &state,
        &auth.user,
        Some(&id),
        "upload",
        &format!("{pver}/{plat}/{version}"),
        &format!("上传完成，尚未发布：{} 个文件", file_count),
    )
    .await?;
    Ok(Json(
        serde_json::json!({"success": true, "version": version, "platform": plat, "file_count": file_count, "copied_count": copy_files.len(), "uploaded_count": file_count-copy_files.len(), "project": p}),
    ))
}
#[derive(Deserialize)]
struct AuditQuery {
    project_id: Option<String>,
    before: Option<i64>,
}
async fn audit_logs(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Query(q): Query<AuditQuery>,
) -> ApiResult<Json<Vec<db::AuditLog>>> {
    if let Some(id) = &q.project_id {
        access(&state, &auth, id, false).await?;
    }
    let logs = sqlx::query_as::<_,db::AuditLog>("SELECT * FROM audit_logs WHERE id < ? AND (? IS NULL OR project_id = ?) AND (? OR project_id IN (SELECT id FROM projects WHERE owner_id = ? OR id IN (SELECT project_id FROM project_members WHERE user_id = ?))) ORDER BY id DESC LIMIT 100")
        .bind(q.before.unwrap_or(i64::MAX)).bind(&q.project_id).bind(&q.project_id).bind(auth.user.admin()).bind(&auth.user.id).bind(&auth.user.id).fetch_all(&state.db).await?;
    Ok(Json(logs))
}
async fn get_protocol_access(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
) -> ApiResult<Json<serde_json::Value>> {
    users::admin(&state, &auth).await?;
    Ok(Json(
        serde_json::json!({"tls_configured": state.tls_configured, "http_enabled": state.http_enabled.load(Ordering::Relaxed), "https_enabled": state.https_enabled.load(Ordering::Relaxed)}),
    ))
}
#[derive(Deserialize, serde::Serialize)]
struct ProtocolInput {
    http_enabled: bool,
    https_enabled: bool,
}
async fn set_protocol_access(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Json(req): Json<ProtocolInput>,
) -> ApiResult<Json<ProtocolInput>> {
    let _guard = state.mutation.lock().await;
    users::admin(&state, &auth).await?;
    if !state.tls_configured || (!req.http_enabled && !req.https_enabled) {
        return Err(ApiError::bad("需配置服务端 TLS，且保留至少一种下载协议"));
    }
    sqlx::query("INSERT INTO settings(key, value) VALUES('protocol', ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value").bind(serde_json::to_string(&req).unwrap()).execute(&state.db).await?;
    state
        .http_enabled
        .store(req.http_enabled, Ordering::Relaxed);
    state
        .https_enabled
        .store(req.https_enabled, Ordering::Relaxed);
    db::audit(
        &state,
        &auth.user,
        None,
        "settings.protocol",
        "server",
        "修改资源下载协议设置",
    )
    .await?;
    Ok(Json(req))
}
