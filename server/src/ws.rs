use axum::{
    extract::{Query, State, WebSocketUpgrade},
    response::Response,
};
use axum::extract::ws::{Message, WebSocket};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogMessage {
    pub timestamp: String,
    pub r#type: String,
    pub status: u16,
    pub method: String,
    pub path: String,
    pub project_id: String,
    pub message: String,
}

#[derive(Deserialize)]
pub struct WsQuery {
    pub token: String,
}

pub async fn ws_logs(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(state): State<Arc<AppState>>,
) -> Response {
    use jsonwebtoken::{decode, DecodingKey, Validation};
    use crate::auth::Claims;

    let validation = Validation::default();
    let result = decode::<Claims>(
        &query.token,
        &DecodingKey::from_secret(state.server_config.jwt_secret.as_bytes()),
        &validation,
    );

    if result.is_err() {
        return (axum::http::StatusCode::UNAUTHORIZED, "Invalid token").into_response();
    }

    let rx = state.log_tx.subscribe();
    ws.on_upgrade(move |socket| handle_ws(socket, rx))
}

async fn handle_ws(mut socket: WebSocket, mut rx: tokio::sync::broadcast::Receiver<LogMessage>) {
    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(log) => {
                        let json = serde_json::to_string(&log).unwrap_or_default();
                        if socket.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}

use axum::response::IntoResponse;

pub fn broadcast_log(state: &Arc<AppState>, log: LogMessage) {
    let _ = state.log_tx.send(log);
}

pub fn make_log(
    r#type: &str,
    status: u16,
    method: &str,
    path: &str,
    project_id: &str,
    message: &str,
) -> LogMessage {
    LogMessage {
        timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
        r#type: r#type.to_string(),
        status,
        method: method.to_string(),
        path: path.to_string(),
        project_id: project_id.to_string(),
        message: message.to_string(),
    }
}
