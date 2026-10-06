use crate::{
    config::{ProjectConfig, ProjectType, ServerConfig},
    AppState,
};
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    Row, SqlitePool,
};
use std::time::Duration;

pub type ApiResult<T> = Result<T, ApiError>;
#[derive(Debug)]
pub struct ApiError(pub StatusCode, pub String);
impl ApiError {
    pub fn not_found(message: impl Into<String>) -> Self {
        Self(StatusCode::NOT_FOUND, message.into())
    }
    pub fn bad(message: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, message.into())
    }
    pub fn forbidden() -> Self {
        Self(StatusCode::FORBIDDEN, "没有执行此操作的权限".into())
    }
    pub fn missing() -> Self {
        Self(StatusCode::NOT_FOUND, "项目不存在或未分享给你".into())
    }
    pub fn conflict(message: &str) -> Self {
        Self(StatusCode::CONFLICT, message.into())
    }
    pub fn unauthorized() -> Self {
        Self(StatusCode::UNAUTHORIZED, "登录已失效，请重新登录".into())
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({"message": self.1}))).into_response()
    }
}
impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!(error = %e, "database operation failed");
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "数据保存失败，请重试".into(),
        )
    }
}
impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        tracing::error!(error = %e, "storage operation failed");
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "资源文件操作失败，请重试".into(),
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: String,
    pub username: String,
    pub role: String,
    pub enabled: bool,
    pub must_change_password: bool,
}
impl User {
    pub fn admin(&self) -> bool {
        self.role == "admin"
    }
}

pub async fn open(config: &ServerConfig) -> Result<SqlitePool, Box<dyn std::error::Error>> {
    let path = config.data_dir.join("management.sqlite");
    if config.require_existing_db && !path.is_file() {
        return Err("实例数据库不存在，已拒绝自动初始化。请检查数据挂载或恢复备份".into());
    }
    std::fs::create_dir_all(&config.data_dir)?;
    if !path.exists()
        && (config.data_dir.join("config.json").exists()
            || (config.resources_dir().exists()
                && std::fs::read_dir(config.resources_dir())?.next().is_some()))
    {
        return Err("检测到旧数据，请将 DATA_DIR 指向新的独立目录；旧数据不会被修改".into());
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(!config.require_existing_db)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(10));
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;
    if config.require_existing_db {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .map_err(|_| "实例数据库缺少用户表，已拒绝初始化。请确认挂载的是正确数据库")?;
        if count == 0 {
            return Err("实例数据库没有用户，已拒绝自动重建账号。请检查数据目录或备份".into());
        }
    }
    sqlx::migrate!("./migrations").run(&pool).await?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await?;
    if count == 0 {
        let username = std::env::var("BOOTSTRAP_ADMIN_USERNAME")
            .map_err(|_| "首次启动需设置 BOOTSTRAP_ADMIN_USERNAME")?;
        let password = std::env::var("BOOTSTRAP_ADMIN_PASSWORD")
            .map_err(|_| "首次启动需设置 BOOTSTRAP_ADMIN_PASSWORD")?;
        validate_username(&username).map_err(|e| e.1)?;
        let hash = crate::auth::hash_password(password)
            .await
            .map_err(|e| e.1)?;
        sqlx::query(
            "INSERT INTO users(id, username, password_hash, role) VALUES(?, ?, ?, 'admin')",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(username.trim())
        .bind(hash)
        .execute(&pool)
        .await?;
    }
    std::fs::create_dir_all(config.resources_dir())?;
    let staging = config.data_dir.join("staging");
    // A single service process owns DATA_DIR. Interrupted uploads are never published.
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    std::fs::create_dir_all(staging)?;
    sqlx::query("DELETE FROM sessions WHERE expires_at <= ?")
        .bind(chrono::Utc::now().timestamp())
        .execute(&pool)
        .await?;
    Ok(pool)
}
pub fn validate_username(value: &str) -> ApiResult<()> {
    if value.len() < 3
        || value.len() > 40
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
    {
        return Err(ApiError::bad(
            "用户名需为 3–40 位英文字母、数字、下划线或短横线",
        ));
    }
    Ok(())
}
pub async fn user(pool: &SqlitePool, id: &str) -> ApiResult<User> {
    sqlx::query_as::<_, User>(
        "SELECT id, username, role, enabled, must_change_password FROM users WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(ApiError::unauthorized)
}
pub async fn project(pool: &SqlitePool, id: &str) -> ApiResult<ProjectConfig> {
    let row = sqlx::query("SELECT owner_id, config, revision FROM projects WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(ApiError::missing)?;
    let mut p: ProjectConfig =
        serde_json::from_str(row.get("config")).map_err(|_| ApiError::bad("项目配置损坏"))?;
    p.owner_id = row.get("owner_id");
    p.revision = row.get("revision");
    p.can_manage = false;
    p.shared_with_me = false;
    Ok(p)
}
pub async fn project_by_name(pool: &SqlitePool, kind: ProjectType, name: &str) -> ApiResult<ProjectConfig> {
    let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM projects WHERE COALESCE(json_extract(config, '$.project_type'), 'bundle') = ? AND json_extract(config, '$.project_name') = ? LIMIT 2").bind(kind.as_str()).bind(name).fetch_all(pool).await?;
    match ids.as_slice() {
        [id] => project(pool, id).await,
        [] => Err(ApiError::missing()),
        _ => Err(ApiError::conflict("存在同名项目，请修改项目名称后再下载")),
    }
}
pub async fn check_project_name(pool: &SqlitePool, kind: ProjectType, name: &str, except: &str) -> ApiResult<()> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects WHERE COALESCE(json_extract(config, '$.project_type'), 'bundle') = ? AND json_extract(config, '$.project_name') = ? AND id != ?)").bind(kind.as_str()).bind(name).bind(except).fetch_one(pool).await?;
    if exists { return Err(ApiError::conflict("此类型已存在同名项目，请使用其他名称")); }
    Ok(())
}
pub async fn authorized(
    state: &AppState,
    user_id: &str,
    id: &str,
    manage: bool,
) -> ApiResult<(User, ProjectConfig)> {
    let u = user(&state.db, user_id).await?;
    if !u.enabled || u.must_change_password {
        return Err(ApiError::forbidden());
    }
    let mut p = project(&state.db, id).await?;
    p.can_manage = u.admin() || p.owner_id == u.id;
    p.shared_with_me = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM project_members WHERE project_id = ? AND user_id = ?)",
    )
    .bind(id)
    .bind(&u.id)
    .fetch_one(&state.db)
    .await?;
    if !p.can_manage {
        if !p.shared_with_me {
            return Err(ApiError::missing());
        }
        if manage {
            return Err(ApiError::forbidden());
        }
    }
    Ok((u, p))
}
pub async fn save(pool: &SqlitePool, p: &mut ProjectConfig) -> ApiResult<()> {
    let config = serde_json::to_string(p).map_err(|_| ApiError::bad("项目配置无效"))?;
    let result = sqlx::query(
        "UPDATE projects SET config = ?, revision = revision + 1 WHERE id = ? AND revision = ?",
    )
    .bind(config)
    .bind(&p.id)
    .bind(p.revision)
    .execute(pool)
    .await?;
    if result.rows_affected() != 1 {
        return Err(ApiError::conflict("项目已被他人修改，请重新加载后再保存"));
    }
    p.revision += 1;
    Ok(())
}
#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct AuditLog {
    pub id: i64,
    pub timestamp: String,
    pub actor_id: String,
    pub actor_name: String,
    pub project_id: Option<String>,
    pub action: String,
    pub object: String,
    pub message: String,
}
pub async fn audit(
    state: &AppState,
    user: &User,
    project_id: Option<&str>,
    action: &str,
    object: &str,
    message: &str,
) -> ApiResult<()> {
    let timestamp = chrono::Utc::now().to_rfc3339();
    let result = sqlx::query("INSERT INTO audit_logs(timestamp, actor_id, actor_name, project_id, action, object, message) VALUES(?, ?, ?, ?, ?, ?, ?)")
        .bind(&timestamp).bind(&user.id).bind(&user.username).bind(project_id).bind(action).bind(object).bind(message).execute(&state.db).await?;
    let _ = state.log_tx.send(AuditLog {
        id: result.last_insert_rowid(),
        timestamp,
        actor_id: user.id.clone(),
        actor_name: user.username.clone(),
        project_id: project_id.map(str::to_owned),
        action: action.into(),
        object: object.into(),
        message: message.into(),
    });
    Ok(())
}
