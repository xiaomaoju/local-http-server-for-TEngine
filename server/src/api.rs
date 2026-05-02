use axum::Router;
use std::sync::Arc;
use crate::AppState;

pub fn build_router(_state: Arc<AppState>) -> Router {
    Router::new()
}
