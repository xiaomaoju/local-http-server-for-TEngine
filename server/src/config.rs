use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};
pub use crate::resources::ProjectType;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlatformSettings {
    pub access_enabled: bool,
    pub active_bundle: Option<String>,
    #[serde(default)]
    pub published_releases: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectVersion {
    pub name: String,
    #[serde(default)]
    pub attachment_snapshot: Option<String>,
    #[serde(default)]
    pub platform_settings: HashMap<String, PlatformSettings>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub id: String,
    pub project_name: String,
    #[serde(default)]
    pub project_type: ProjectType,
    pub platforms: Vec<String>,
    pub package_name: String,
    pub project_versions: Vec<ProjectVersion>,
    pub owner_id: String,
    pub revision: i64,
    #[serde(default)]
    pub can_manage: bool,
    #[serde(default)]
    pub shared_with_me: bool,
}
#[derive(Clone)]
pub struct ServerConfig {
    pub data_dir: PathBuf,
    pub port: u16,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub https_port: u16,
    pub cookie_secure: bool,
    pub session_hours: i64,
    pub require_existing_db: bool,
}
impl ServerConfig {
    pub fn from_env() -> Self {
        Self {
            data_dir: std::env::var("DATA_DIR")
                .unwrap_or_else(|_| "/data".into())
                .into(),
            port: std::env::var("PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8083),
            https_port: std::env::var("HTTPS_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8182),
            tls_cert: std::env::var("TLS_CERT").ok().map(PathBuf::from),
            tls_key: std::env::var("TLS_KEY").ok().map(PathBuf::from),
            cookie_secure: std::env::var("COOKIE_SECURE").as_deref() != Ok("false"),
            require_existing_db: std::env::var("REQUIRE_EXISTING_DB").as_deref() == Ok("true"),
            session_hours: std::env::var("TOKEN_EXPIRE_HOURS")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|v| *v > 0 && *v <= 168)
                .unwrap_or(24),
        }
    }
    pub fn resources_dir(&self) -> PathBuf {
        self.data_dir.join("resources")
    }
}
