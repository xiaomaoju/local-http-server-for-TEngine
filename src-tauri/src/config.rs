use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// 单个项目的配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// 唯一标识
    pub id: String,
    /// 项目显示名称
    pub project_name: String,
    /// Bundles 根目录路径 (如 UnityProject/Bundles)
    pub bundles_dir: String,
    /// 服务端口
    pub port: u16,
    /// 目标平台列表
    pub platforms: Vec<String>,
    /// 是否启用 CORS
    pub cors_enabled: bool,
    /// 包名 (YooAsset package name, 如 DefaultPackage)
    pub package_name: String,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            project_name: "TEngine".to_string(),
            bundles_dir: String::new(),
            port: 8081,
            platforms: vec!["Android".to_string()],
            cors_enabled: true,
            package_name: "DefaultPackage".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemoteConnection {
    pub server_url: String,
}

/// 全局应用配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub projects: Vec<ProjectConfig>,
    #[serde(default)]
    pub remote_connections: Vec<RemoteConnection>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            projects: vec![ProjectConfig::default()],
            remote_connections: vec![],
        }
    }
}

impl AppConfig {
    /// 获取配置文件路径
    fn config_path() -> PathBuf {
        let mut path = dirs_next().unwrap_or_else(|| PathBuf::from("."));
        path.push("tengine-http-server");
        fs::create_dir_all(&path).ok();
        path.push("config.json");
        path
    }

    /// 加载配置
    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(_) => Self::default(),
            }
        } else {
            let config = Self::default();
            config.save().ok();
            config
        }
    }

    /// 保存配置
    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path();
        let content = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, content).map_err(|e| e.to_string())
    }
}

/// 获取用户配置目录
fn dirs_next() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME")
            .ok()
            .map(|home| PathBuf::from(home).join("Library").join("Application Support"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA").ok().map(PathBuf::from)
    }
    #[cfg(target_os = "linux")]
    {
        std::env::var("HOME")
            .ok()
            .map(|home| PathBuf::from(home).join(".config"))
    }
}
