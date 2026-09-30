use crate::{
    auth::{self, Auth},
    db::{self, ApiError, ApiResult, User},
    AppState,
};
use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};

pub async fn admin(state: &AppState, auth: &Auth) -> ApiResult<User> {
    let current = auth::from_hash(state, &auth.hash).await?;
    if !current.user.admin() || current.user.must_change_password {
        return Err(ApiError::forbidden());
    }
    Ok(current.user)
}
pub async fn list(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
) -> ApiResult<Json<Vec<User>>> {
    admin(&state, &auth).await?;
    Ok(Json(
        sqlx::query_as(
            "SELECT id, username, role, enabled, must_change_password FROM users ORDER BY username",
        )
        .fetch_all(&state.db)
        .await?,
    ))
}
#[derive(Deserialize)]
pub struct Lookup {
    username: String,
}
pub async fn lookup(
    State(state): State<Arc<AppState>>,
    Query(q): Query<Lookup>,
) -> ApiResult<Json<serde_json::Value>> {
    let u: User = sqlx::query_as("SELECT id, username, role, enabled, must_change_password FROM users WHERE username = ? AND enabled = 1").bind(q.username.trim()).fetch_optional(&state.db).await?.ok_or_else(|| ApiError::bad("未找到有效用户，请检查完整用户名"))?;
    Ok(Json(
        serde_json::json!({"id": u.id, "username": u.username}),
    ))
}
#[derive(Deserialize)]
pub struct CreateUser {
    username: String,
    password: String,
    role: String,
}
pub async fn create(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Json(req): Json<CreateUser>,
) -> ApiResult<Json<User>> {
    let u = admin(&state, &auth).await?;
    db::validate_username(req.username.trim())?;
    if req.role != "admin" && req.role != "user" {
        return Err(ApiError::bad("身份无效"));
    }
    let hash = auth::hash_password(req.password).await?;
    let _guard = state.mutation.lock().await;
    admin(&state, &auth).await?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE username = ?)")
        .bind(req.username.trim())
        .fetch_one(&state.db)
        .await?;
    if exists {
        return Err(ApiError::conflict("用户名已存在"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO users(id, username, password_hash, role) VALUES(?, ?, ?, ?)")
        .bind(&id)
        .bind(req.username.trim())
        .bind(hash)
        .bind(req.role)
        .execute(&state.db)
        .await?;
    db::audit(&state, &u, None, "user.create", &id, "创建用户").await?;
    Ok(Json(db::user(&state.db, &id).await?))
}
#[derive(Deserialize)]
pub struct UpdateUser {
    role: String,
    enabled: bool,
}
async fn protect_admin(state: &AppState, target: &User) -> ApiResult<()> {
    if target.admin() && target.enabled {
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'admin' AND enabled = 1")
                .fetch_one(&state.db)
                .await?;
        if count <= 1 {
            return Err(ApiError::conflict("必须保留至少一个有效管理员"));
        }
    }
    Ok(())
}
pub async fn update(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path(id): Path<String>,
    Json(req): Json<UpdateUser>,
) -> ApiResult<Json<User>> {
    let _guard = state.mutation.lock().await;
    let u = admin(&state, &auth).await?;
    if req.role != "admin" && req.role != "user" {
        return Err(ApiError::bad("身份无效"));
    }
    let target = db::user(&state.db, &id).await?;
    if req.role != "admin" || !req.enabled {
        protect_admin(&state, &target).await?;
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE users SET role = ?, enabled = ? WHERE id = ?")
        .bind(req.role)
        .bind(req.enabled)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = ?")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let _ = state.access_tx.send(());
    db::audit(
        &state,
        &u,
        None,
        "user.update",
        &id,
        "更新用户身份或启用状态",
    )
    .await?;
    Ok(Json(db::user(&state.db, &id).await?))
}
#[derive(Deserialize)]
pub struct ResetPassword {
    password: String,
}
pub async fn reset(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path(id): Path<String>,
    Json(req): Json<ResetPassword>,
) -> ApiResult<Json<serde_json::Value>> {
    admin(&state, &auth).await?;
    let hash = auth::hash_password(req.password).await?;
    let _guard = state.mutation.lock().await;
    let u = admin(&state, &auth).await?;
    db::user(&state.db, &id).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE users SET password_hash = ?, must_change_password = 1 WHERE id = ?")
        .bind(hash)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = ?")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let _ = state.access_tx.send(());
    db::audit(
        &state,
        &u,
        None,
        "user.password",
        &id,
        "重置密码并撤销全部会话",
    )
    .await?;
    Ok(Json(serde_json::json!({"success": true})))
}
#[derive(Deserialize)]
pub struct DeleteUser {
    transfers: HashMap<String, String>,
}
pub async fn delete(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path(id): Path<String>,
    Json(req): Json<DeleteUser>,
) -> ApiResult<Json<serde_json::Value>> {
    let _guard = state.mutation.lock().await;
    let u = admin(&state, &auth).await?;
    if id == u.id {
        return Err(ApiError::bad("请由另一位管理员删除当前账号"));
    }
    let target = db::user(&state.db, &id).await?;
    protect_admin(&state, &target).await?;
    let projects: Vec<String> = sqlx::query_scalar("SELECT id FROM projects WHERE owner_id = ?")
        .bind(&id)
        .fetch_all(&state.db)
        .await?;
    let mut tx = state.db.begin().await?;
    for project in projects {
        let recipient = req
            .transfers
            .get(&project)
            .ok_or_else(|| ApiError::conflict("请为每个项目指定接收者"))?;
        let valid: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM users WHERE id = ? AND enabled = 1 AND id != ?)",
        )
        .bind(recipient)
        .bind(&id)
        .fetch_one(&mut *tx)
        .await?;
        if !valid {
            return Err(ApiError::bad("接收者必须是其他有效用户"));
        }
        sqlx::query("UPDATE projects SET owner_id = ?, revision = revision + 1 WHERE id = ?")
            .bind(recipient)
            .bind(&project)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM project_members WHERE project_id = ? AND user_id = ?")
            .bind(&project)
            .bind(recipient)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO audit_logs(timestamp, actor_id, actor_name, project_id, action, object, message) VALUES(?, ?, ?, ?, 'project.transfer', ?, '删除用户前转交项目')").bind(chrono::Utc::now().to_rfc3339()).bind(&u.id).bind(&u.username).bind(&project).bind(recipient).execute(&mut *tx).await?;
    }
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO audit_logs(timestamp, actor_id, actor_name, action, object, message) VALUES(?, ?, ?, 'user.delete', ?, '删除账号并保留项目资源')").bind(chrono::Utc::now().to_rfc3339()).bind(&u.id).bind(&u.username).bind(&id).execute(&mut *tx).await?;
    tx.commit().await?;
    let _ = state.access_tx.send(());
    Ok(Json(serde_json::json!({"success": true})))
}
