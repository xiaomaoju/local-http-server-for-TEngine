use crate::{
    auth::{self, Auth},
    db::{self, ApiError, ApiResult},
    AppState,
};
use axum::{
    extract::{
        ws::{Message, WebSocket},
        Extension, Query, State, WebSocketUpgrade,
    },
    response::Response,
    Json,
};
use serde::Deserialize;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub async fn ticket(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut tickets = state.tickets.lock().await;
    tickets.retain(|_, (_, issued)| issued.elapsed() < Duration::from_secs(30));
    if tickets.len() >= 4096 {
        return Err(ApiError::bad("日志连接过多，请稍后重试"));
    }
    let ticket = auth::token();
    tickets.insert(auth::digest(&ticket), (auth.hash, Instant::now()));
    Ok(Json(serde_json::json!({"ticket": ticket})))
}
#[derive(Deserialize)]
pub struct TicketQuery {
    ticket: String,
}
pub async fn ws_logs(
    ws: WebSocketUpgrade,
    Query(query): Query<TicketQuery>,
    State(state): State<Arc<AppState>>,
) -> ApiResult<Response> {
    let (hash, issued) = state
        .tickets
        .lock()
        .await
        .remove(&auth::digest(&query.ticket))
        .ok_or_else(ApiError::unauthorized)?;
    if issued.elapsed() >= Duration::from_secs(30) {
        return Err(ApiError::unauthorized());
    }
    let auth = auth::from_hash(&state, &hash).await?;
    if auth.user.must_change_password {
        return Err(ApiError::forbidden());
    }
    Ok(ws.on_upgrade(move |socket| handle(socket, state, hash)))
}
async fn handle(mut socket: WebSocket, state: Arc<AppState>, hash: String) {
    let mut logs = state.log_tx.subscribe();
    let mut resources = state.resource_tx.subscribe();
    let mut access = state.access_tx.subscribe();
    let mut timer = tokio::time::interval(Duration::from_secs(15));
    loop {
        tokio::select! {
            biased;
            _ = access.recv() => {
                let _ = socket.send(Message::Text("{\"type\":\"access_changed\"}".into())).await;
                let _ = socket.send(Message::Close(None)).await;
                break;
            }
            _ = timer.tick() => {
                if auth::from_hash(&state, &hash).await.is_err() {
                    let _ = socket.send(Message::Close(None)).await;
                    break;
                }
                // Keep idle sessions alive through reverse proxies with read timeouts.
                if socket.send(Message::Ping(Vec::new())).await.is_err() { break; }
            }
            msg = logs.recv() => {
                let log = match msg { Ok(v) => v, Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue, Err(_) => break };
                let auth = match auth::from_hash(&state, &hash).await { Ok(v) => v, Err(_) => break };
                let allowed = if auth.user.admin() { true } else if let Some(id) = &log.project_id { db::authorized(&state, &auth.user.id, id, false).await.is_ok() } else { false };
                if allowed && socket.send(Message::Text(serde_json::to_string(&log).unwrap().into())).await.is_err() { break; }
            }
            msg = resources.recv() => {
                let log = match msg { Ok(v) => v, Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue, Err(_) => break };
                let auth = match auth::from_hash(&state, &hash).await { Ok(v) => v, Err(_) => break };
                if crate::resource_logs::allowed(&state, &auth, &log).await && socket.send(Message::Text(serde_json::to_string(&log).unwrap().into())).await.is_err() { break; }
            }
            msg = socket.recv() => { if matches!(msg, Some(Ok(Message::Close(_))) | None | Some(Err(_))) { break; } }
        }
    }
}
