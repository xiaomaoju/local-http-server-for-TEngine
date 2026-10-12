mod api;
mod auth;
mod config;
mod db;
mod serve;
mod resource_logs;
mod apk_downloads;
mod storage;
mod attachments;
#[path = "../../shared/attachments.rs"]
mod attachment_store;
#[path = "../../shared/resources.rs"]
mod resources;
#[cfg(test)]
mod tests;
mod users;
mod ws;
use config::ServerConfig;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{atomic::AtomicBool, Arc},
    time::Instant,
};
use tokio::sync::{broadcast, Mutex};
use tracing_subscriber::EnvFilter;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Http,
    Https,
}
pub struct AppState {
    pub server_config: ServerConfig,
    pub db: sqlx::SqlitePool,
    pub mutation: Mutex<()>,
    pub log_tx: broadcast::Sender<db::AuditLog>,
    pub resource_tx: broadcast::Sender<resource_logs::ResourceLog>,
    pub resource_history: std::sync::Mutex<std::collections::VecDeque<resource_logs::ResourceLog>>,
    pub access_tx: broadcast::Sender<()>,
    pub tickets: Mutex<HashMap<String, (String, Instant)>>,
    pub login_attempts: Mutex<HashMap<String, (u32, Instant)>>,
    pub dummy_hash: String,
    pub tls_configured: bool,
    pub http_enabled: AtomicBool,
    pub https_enabled: AtomicBool,
}
impl AppState {
    pub async fn new(config: ServerConfig, db: sqlx::SqlitePool) -> Self {
        let protocol: Option<String> =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'protocol'")
                .fetch_optional(&db)
                .await
                .expect("Failed to load settings");
        let value: serde_json::Value = protocol
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default();
        let tls_configured = config.tls_cert.is_some() && config.tls_key.is_some();
        Self {
            server_config: config,
            db,
            mutation: Mutex::new(()),
            log_tx: broadcast::channel(256).0,
            resource_tx: broadcast::channel(512).0,
            resource_history: std::sync::Mutex::new(std::collections::VecDeque::new()),
            access_tx: broadcast::channel(32).0,
            tickets: Mutex::new(HashMap::new()),
            login_attempts: Mutex::new(HashMap::new()),
            dummy_hash: auth::hash_password(auth::token())
                .await
                .expect("Password hasher unavailable"),
            tls_configured,
            http_enabled: AtomicBool::new(
                !tls_configured || value["http_enabled"].as_bool().unwrap_or(true),
            ),
            https_enabled: AtomicBool::new(value["https_enabled"].as_bool().unwrap_or(true)),
        }
    }
}
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("tengine_server=info".parse().unwrap()),
        )
        .init();
    let mut config = ServerConfig::from_env();
    let initialize_only = std::env::args().nth(1).as_deref() == Some("--initialize");
    if initialize_only {
        if config.data_dir.join("management.sqlite").exists() {
            eprintln!("数据库已存在，拒绝重新初始化");
            std::process::exit(1);
        }
        config.require_existing_db = false;
    }
    let pool = db::open(&config)
        .await
        .expect("Management database initialization failed");
    if initialize_only {
        pool.close().await;
        tracing::info!("实例数据库初始化完成，未启动服务");
        return;
    }
    let http_addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    let tls = (config.tls_cert.clone(), config.tls_key.clone());
    let https_port = config.https_port;
    let state = Arc::new(AppState::new(config, pool).await);
    if let Err(error) = attachments::cleanup(&state).await { tracing::warn!("附件孤立快照清理未完成，将在下次启动时重试：{}", error.1); }
    let listener = tokio::net::TcpListener::bind(http_addr)
        .await
        .expect("HTTP bind failed");
    tracing::info!("Management server listening on {http_addr}");
    if let (Some(cert), Some(key)) = tls {
        let tls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key)
            .await
            .expect("TLS configuration failed");
        let http = axum::serve(listener, api::build_router(state.clone(), Protocol::Http));
        let https =
            axum_server::bind_rustls(SocketAddr::from(([0, 0, 0, 0], https_port)), tls_config)
                .serve(api::build_router(state, Protocol::Https).into_make_service());
        tokio::select! { r = http => r.unwrap(), r = https => r.unwrap() }
    } else {
        axum::serve(listener, api::build_router(state, Protocol::Http))
            .await
            .unwrap();
    }
}
