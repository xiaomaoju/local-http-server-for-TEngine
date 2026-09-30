use crate::{
    db::{self, ApiError, ApiResult, User},
    AppState,
};
use argon2::{
    password_hash::{rand_core::OsRng, SaltString},
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
};
use axum::{
    extract::{Extension, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct Auth {
    pub user: User,
    pub hash: String,
    pub csrf: String,
    pub client: String,
}
pub fn token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
pub fn digest(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}
pub async fn hash_password(password: String) -> ApiResult<String> {
    if password.chars().count() < 10 || password.len() > 256 {
        return Err(ApiError::bad("密码需为 10–256 个字符"));
    }
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|v| v.to_string())
            .map_err(|_| ApiError::bad("密码处理失败"))
    })
    .await
    .map_err(|_| ApiError::bad("密码处理失败"))?
}
async fn verify(password: String, hash: String) -> bool {
    if password.len() > 256 {
        return false;
    }
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash)
            .map(|h| {
                Argon2::default()
                    .verify_password(password.as_bytes(), &h)
                    .is_ok()
            })
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false)
}
pub async fn from_hash(state: &AppState, hash: &str) -> ApiResult<Auth> {
    let row = sqlx::query(
        "SELECT user_id, csrf, client FROM sessions WHERE token_hash = ? AND expires_at > ?",
    )
    .bind(hash)
    .bind(chrono::Utc::now().timestamp())
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(ApiError::unauthorized)?;
    let user = db::user(&state.db, row.get("user_id")).await?;
    if !user.enabled {
        return Err(ApiError::unauthorized());
    }
    Ok(Auth {
        user,
        hash: hash.into(),
        csrf: row.get("csrf"),
        client: row.get("client"),
    })
}
pub async fn middleware(
    State(state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Response {
    let bearer = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let cookie = req
        .headers()
        .get("Cookie")
        .and_then(|h| h.to_str().ok())
        .and_then(|v| {
            v.split(';')
                .find_map(|s| s.trim().strip_prefix("tengine_session="))
        });
    let (credential, client) = if let Some(token) = bearer {
        (token, "desktop")
    } else if let Some(token) = cookie {
        (token, "web")
    } else {
        return ApiError::unauthorized().into_response();
    };
    let auth = match from_hash(&state, &digest(credential)).await {
        Ok(a) => a,
        Err(e) => return e.into_response(),
    };
    if auth.client != client {
        return ApiError::unauthorized().into_response();
    }
    if client == "web"
        && !matches!(req.method().as_str(), "GET" | "HEAD" | "OPTIONS")
        && req
            .headers()
            .get("X-CSRF-Token")
            .and_then(|v| v.to_str().ok())
            != Some(&auth.csrf)
    {
        return ApiError::forbidden().into_response();
    }
    if auth.user.must_change_password
        && !matches!(
            req.uri().path(),
            "/api/auth/me" | "/api/auth/password" | "/api/auth/logout"
        )
    {
        return ApiError(StatusCode::FORBIDDEN, "请先修改初始密码".into()).into_response();
    }
    req.extensions_mut().insert(auth);
    let mut response = next.run(req).await;
    response
        .headers_mut()
        .insert("Cache-Control", HeaderValue::from_static("no-store"));
    response
}
#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    pub client: String,
}
#[derive(Serialize)]
pub struct LoginResponse {
    pub user: User,
    pub csrf: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}
fn cookie(state: &AppState, token: &str, age: i64) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "tengine_session={token}; Path=/api; HttpOnly; SameSite=Strict; Max-Age={age}{}",
        if state.server_config.cookie_secure {
            "; Secure"
        } else {
            ""
        }
    ))
    .unwrap()
}
pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> ApiResult<Response> {
    if req.client != "web" && req.client != "desktop" {
        return Err(ApiError::bad("不支持的客户端"));
    }
    let username = req.username.trim().to_lowercase();
    if username.len() > 40 {
        return Err(ApiError::unauthorized());
    }
    {
        let mut attempts = state.login_attempts.lock().await;
        attempts.retain(|_, (_, start)| start.elapsed() < Duration::from_secs(60));
        if attempts.len() >= 4096 {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "登录请求过多，请稍后再试".into(),
            ));
        }
        let entry = attempts
            .entry(username.clone())
            .or_insert((0, Instant::now()));
        entry.0 += 1;
        if entry.0 > 10 {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "尝试过于频繁，请一分钟后重试".into(),
            ));
        }
    }
    let row = sqlx::query("SELECT id, password_hash FROM users WHERE username = ? AND enabled = 1")
        .bind(&username)
        .fetch_optional(&state.db)
        .await?;
    let stored_hash = row
        .as_ref()
        .map(|r| r.get::<String, _>("password_hash"))
        .unwrap_or_else(|| state.dummy_hash.clone());
    if !verify(req.password, stored_hash.clone()).await || row.is_none() {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "用户名或密码错误".into(),
        ));
    }
    let _guard = state.mutation.lock().await;
    let row = row.unwrap();
    // Recheck after hashing: a concurrent reset or disable must win over this login.
    let current: Option<String> =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ? AND enabled = 1")
            .bind(row.get::<&str, _>("id"))
            .fetch_optional(&state.db)
            .await?;
    if current.as_deref() != Some(&stored_hash) {
        return Err(ApiError::unauthorized());
    }
    let user = db::user(&state.db, row.get("id")).await?;
    let token = token();
    let csrf = self::token();
    let seconds = state.server_config.session_hours * 3600;
    sqlx::query("DELETE FROM sessions WHERE expires_at <= ?")
        .bind(chrono::Utc::now().timestamp())
        .execute(&state.db)
        .await?;
    sqlx::query(
        "INSERT INTO sessions(token_hash, user_id, csrf, client, expires_at) VALUES(?, ?, ?, ?, ?)",
    )
    .bind(digest(&token))
    .bind(&user.id)
    .bind(&csrf)
    .bind(&req.client)
    .bind(chrono::Utc::now().timestamp() + seconds)
    .execute(&state.db)
    .await?;
    let mut headers = HeaderMap::new();
    if req.client == "web" {
        headers.insert("Set-Cookie", cookie(&state, &token, seconds));
    }
    headers.insert("Cache-Control", HeaderValue::from_static("no-store"));
    db::audit(&state, &user, None, "login", "account", "登录成功").await?;
    Ok((
        headers,
        Json(LoginResponse {
            user,
            csrf,
            token: (req.client == "desktop").then_some(token),
        }),
    )
        .into_response())
}
pub async fn me(Extension(auth): Extension<Auth>) -> Json<LoginResponse> {
    Json(LoginResponse {
        user: auth.user,
        csrf: auth.csrf,
        token: None,
    })
}
pub async fn logout(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
) -> ApiResult<Response> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
        .bind(auth.hash)
        .execute(&state.db)
        .await?;
    let _ = state.access_tx.send(());
    let mut headers = HeaderMap::new();
    headers.insert("Set-Cookie", cookie(&state, "", 0));
    Ok((headers, Json(serde_json::json!({"success": true}))).into_response())
}
#[derive(Deserialize)]
pub struct ChangePassword {
    pub current_password: String,
    pub password: String,
}
pub async fn change_password(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Json(req): Json<ChangePassword>,
) -> ApiResult<Json<serde_json::Value>> {
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?")
        .bind(&auth.user.id)
        .fetch_one(&state.db)
        .await?;
    if !verify(req.current_password, hash.clone()).await {
        return Err(ApiError::bad("当前密码不正确"));
    }
    let new_hash = hash_password(req.password).await?;
    let _guard = state.mutation.lock().await;
    from_hash(&state, &auth.hash).await?;
    let mut tx = state.db.begin().await?;
    let updated = sqlx::query("UPDATE users SET password_hash = ?, must_change_password = 0 WHERE id = ? AND password_hash = ?").bind(new_hash).bind(&auth.user.id).bind(hash).execute(&mut *tx).await?;
    if updated.rows_affected() != 1 {
        return Err(ApiError::conflict("密码已改变，请重新登录"));
    }
    sqlx::query("DELETE FROM sessions WHERE user_id = ?")
        .bind(&auth.user.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let _ = state.access_tx.send(());
    db::audit(
        &state,
        &auth.user,
        None,
        "password",
        "account",
        "修改密码并撤销全部会话",
    )
    .await?;
    Ok(Json(serde_json::json!({"success": true})))
}
