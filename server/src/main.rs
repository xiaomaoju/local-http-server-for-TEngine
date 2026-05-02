mod api;
mod auth;
mod config;
mod serve;
mod storage;
mod ws;

use config::{AppConfig, ServerConfig, SharedAppConfig};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing_subscriber::EnvFilter;

pub struct AppState {
    pub server_config: ServerConfig,
    pub app_config: SharedAppConfig,
    pub log_tx: broadcast::Sender<ws::LogMessage>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("tengine_server=info".parse().unwrap()))
        .init();

    let server_config = ServerConfig::from_env();
    let app_config = AppConfig::load(&server_config.config_path());

    std::fs::create_dir_all(server_config.resources_dir())
        .expect("Failed to create resources directory");

    let (log_tx, _) = broadcast::channel::<ws::LogMessage>(256);

    let port = server_config.port;
    let state = Arc::new(AppState {
        server_config,
        app_config: Arc::new(RwLock::new(app_config)),
        log_tx,
    });

    let app = api::build_router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Server starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
