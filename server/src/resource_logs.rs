use crate::{auth::Auth, db, AppState};
use axum::{extract::{Extension, State}, Json};
use serde::Serialize;
use std::{collections::HashSet, sync::Arc};

#[derive(Clone, Debug, Serialize)]
pub struct ResourceLog {
    pub id: String,
    pub timestamp: String,
    pub r#type: &'static str,
    pub project_id: Option<String>,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub outcome: &'static str,
    pub message: String,
}
impl ResourceLog {
    pub fn new(method: &str, path: &str) -> Self {
        Self { id: uuid::Uuid::new_v4().to_string(), timestamp: String::new(), r#type: "resource_request", project_id: None, method: method.into(), path: path.chars().take(2048).collect(), status: 200, outcome: "completed", message: String::new() }
    }
}
pub fn publish(state: &AppState, mut entry: ResourceLog, status: u16, outcome: &'static str, message: &str) {
    entry.timestamp = chrono::Utc::now().to_rfc3339();
    entry.status = status;
    entry.outcome = outcome;
    entry.message = message.into();
    let mut recent = state.resource_history.lock().unwrap();
    if recent.len() >= 500 { recent.pop_front(); }
    recent.push_back(entry.clone());
    let _ = state.resource_tx.send(entry);
}
pub async fn allowed(state: &AppState, auth: &Auth, entry: &ResourceLog) -> bool {
    auth.user.admin() || match &entry.project_id {
        Some(id) => db::authorized(state, &auth.user.id, id, false).await.is_ok(),
        None => false,
    }
}
pub async fn recent(State(state): State<Arc<AppState>>, Extension(auth): Extension<Auth>) -> db::ApiResult<Json<Vec<ResourceLog>>> {
    let ids: HashSet<String> = sqlx::query_scalar("SELECT id FROM projects WHERE owner_id = ? OR id IN (SELECT project_id FROM project_members WHERE user_id = ?)")
        .bind(&auth.user.id).bind(&auth.user.id).fetch_all(&state.db).await?.into_iter().collect();
    let logs = state.resource_history.lock().unwrap().iter().filter(|log| auth.user.admin() || log.project_id.as_ref().is_some_and(|id| ids.contains(id))).cloned().collect();
    Ok(Json(logs))
}
