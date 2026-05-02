use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub id: String,
    pub project_name: String,
    pub platforms: Vec<String>,
    pub package_name: String,
    pub active_versions: HashMap<String, String>,
}

impl ProjectConfig {
    pub fn new(project_name: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            project_name,
            platforms: vec!["Android".to_string()],
            package_name: "DefaultPackage".to_string(),
            active_versions: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub projects: Vec<ProjectConfig>,
}

impl AppConfig {
    pub fn load(path: &PathBuf) -> Self {
        if path.exists() {
            match fs::read_to_string(path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(_) => Self::default(),
            }
        } else {
            Self::default()
        }
    }

    pub fn save(&self, path: &PathBuf) -> Result<(), String> {
        let content = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(path, content).map_err(|e| e.to_string())
    }
}

pub struct ServerConfig {
    pub password_hash: String,
    pub jwt_secret: String,
    pub token_expire_hours: u64,
    pub data_dir: PathBuf,
    pub port: u16,
}

impl ServerConfig {
    pub fn from_env() -> Self {
        let password = std::env::var("ADMIN_PASSWORD")
            .expect("ADMIN_PASSWORD environment variable is required");

        let sha256_hex = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(password.as_bytes());
            hex::encode(hasher.finalize())
        };

        let password_hash = {
            use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
            let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
            Argon2::default()
                .hash_password(sha256_hex.as_bytes(), &salt)
                .expect("Failed to hash password")
                .to_string()
        };

        let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| {
            uuid::Uuid::new_v4().to_string()
        });

        let token_expire_hours: u64 = std::env::var("TOKEN_EXPIRE_HOURS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(24);

        let data_dir = PathBuf::from(
            std::env::var("DATA_DIR").unwrap_or_else(|_| "/data".to_string()),
        );

        let port: u16 = std::env::var("PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8082);

        Self {
            password_hash,
            jwt_secret,
            token_expire_hours,
            data_dir,
            port,
        }
    }

    pub fn config_path(&self) -> PathBuf {
        self.data_dir.join("config.json")
    }

    pub fn resources_dir(&self) -> PathBuf {
        self.data_dir.join("resources")
    }
}

pub type SharedAppConfig = Arc<RwLock<AppConfig>>;
