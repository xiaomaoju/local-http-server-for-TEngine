mod api;
mod auth;
mod config;
mod serve;
mod storage;
mod ws;

use config::{AppConfig, ServerConfig, SharedAppConfig};
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Http,
    Https,
}

pub struct AppState {
    pub server_config: ServerConfig,
    pub app_config: SharedAppConfig,
    pub log_tx: broadcast::Sender<ws::LogMessage>,
    pub tls_configured: bool,
    pub http_enabled: AtomicBool,
    pub https_enabled: AtomicBool,
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
    let tls_cert = server_config.tls_cert.clone();
    let tls_key = server_config.tls_key.clone();
    let https_port = server_config.https_port;
    let tls_configured = tls_cert.is_some() && tls_key.is_some();

    let state = Arc::new(AppState {
        server_config,
        app_config: Arc::new(RwLock::new(app_config)),
        log_tx,
        tls_configured,
        http_enabled: AtomicBool::new(true),
        https_enabled: AtomicBool::new(true),
    });

    let http_addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("HTTP server starting on {}", http_addr);

    if let (Some(cert_path), Some(key_path)) = (tls_cert, tls_key) {
        let tls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert_path, &key_path)
            .await
            .expect("Failed to load TLS certificate");

        let https_addr = SocketAddr::from(([0, 0, 0, 0], https_port));
        tracing::info!("HTTPS server starting on {}", https_addr);

        let http_app = api::build_router(state.clone(), Protocol::Http);
        let https_app = api::build_router(state, Protocol::Https);

        let http_handle = tokio::spawn(async move {
            let listener = tokio::net::TcpListener::bind(http_addr).await.unwrap();
            axum::serve(listener, http_app).await.unwrap();
        });

        let https_handle = tokio::spawn(async move {
            axum_server::bind_rustls(https_addr, tls_config)
                .serve(https_app.into_make_service())
                .await
                .unwrap();
        });

        tokio::select! {
            r = http_handle => r.unwrap(),
            r = https_handle => r.unwrap(),
        }
    } else {
        let app = api::build_router(state, Protocol::Http);
        let listener = tokio::net::TcpListener::bind(http_addr).await.unwrap();
        axum::serve(listener, app).await.unwrap();
    }
}
