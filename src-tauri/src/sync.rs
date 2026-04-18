use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// 同步结果
#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncResult {
    pub success: bool,
    pub message: String,
    pub version: Option<String>,
    pub synced_files: Vec<String>,
}

/// 版本条目
#[derive(Debug, Clone, serde::Serialize)]
pub struct VersionEntry {
    pub version: String,
    pub modified_timestamp: u64,
    pub file_count: u32,
    pub total_size: u64,
}

/// 资源同步器 - 实现 start.bat 中 PowerShell 的同步逻辑
pub struct ResourceSyncer {
    bundles_dir: PathBuf,
    package_name: String,
    platform: String,
}

impl ResourceSyncer {
    pub fn new(bundles_dir: &str, package_name: &str, platform: &str) -> Self {
        Self {
            bundles_dir: PathBuf::from(bundles_dir),
            package_name: package_name.to_string(),
            platform: platform.to_string(),
        }
    }

    /// 获取平台目录
    fn platform_dir(&self) -> PathBuf {
        self.bundles_dir.join(&self.platform)
    }

    /// 获取包目录
    fn package_dir(&self) -> PathBuf {
        self.platform_dir().join(&self.package_name)
    }

    /// 查找最新的 .version 文件
    /// 递归搜索 package_dir 下的 {PackageName}.version 文件
    /// 排除 OutputCache, Simulate, _server_root 目录
    fn find_version_file(&self) -> Option<PathBuf> {
        let pkg_dir = self.package_dir();
        if !pkg_dir.exists() {
            return None;
        }

        let version_filename = format!("{}.version", self.package_name);
        let exclude_dirs = ["OutputCache", "Simulate", "_server_root"];

        let mut candidates: Vec<PathBuf> = Vec::new();

        for entry in WalkDir::new(&pkg_dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            // 排除特定目录
            let path_str = path.to_string_lossy();
            if exclude_dirs.iter().any(|d| path_str.contains(d)) {
                continue;
            }

            if path.is_file() && path.file_name().map(|f| f.to_string_lossy()) == Some(version_filename.as_str().into()) {
                candidates.push(path.to_path_buf());
            }
        }

        // 按文件修改时间排序，取最新的
        candidates.sort_by(|a, b| {
            let time_a = fs::metadata(a)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            let time_b = fs::metadata(b)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            time_a.cmp(&time_b)
        });

        candidates.last().cloned()
    }

    /// 读取版本号
    fn read_version(&self, version_file: &Path) -> Result<String, String> {
        fs::read_to_string(version_file)
            .map(|s| s.trim().to_string())
            .map_err(|e| format!("读取版本文件失败: {}", e))
    }

    /// 读取最新可用版本号（从构建输出目录递归查找）
    pub fn read_latest_version(&self) -> Option<String> {
        let version_file = self.find_version_file()?;
        self.read_version(&version_file).ok()
    }

    /// 读取当前已同步的版本号（从平台根目录的 .version 文件）
    pub fn read_synced_version(&self) -> Option<String> {
        let synced_file = self.platform_dir().join(format!("{}.version", self.package_name));
        if synced_file.exists() {
            fs::read_to_string(&synced_file).ok().map(|s| s.trim().to_string())
        } else {
            None
        }
    }

    /// 列出所有可用的版本（版本文件夹名就是版本号）
    /// 返回按修改时间降序排列（最新在前）
    pub fn list_versions(&self) -> Vec<VersionEntry> {
        let pkg_dir = self.package_dir();
        if !pkg_dir.exists() {
            return vec![];
        }

        let exclude_dirs = ["OutputCache", "Simulate", "_server_root"];
        let mut entries = Vec::new();

        if let Ok(read_dir) = fs::read_dir(&pkg_dir) {
            for entry in read_dir.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();

                // 跳过排除目录
                if exclude_dirs.contains(&name.as_str()) {
                    continue;
                }

                if !entry.path().is_dir() {
                    continue;
                }

                // 检查该目录下是否有 .version 文件
                let version_file = entry.path().join(format!("{}.version", self.package_name));
                if !version_file.exists() {
                    continue;
                }

                let modified = fs::metadata(entry.path())
                    .and_then(|m| m.modified())
                    .ok();

                // 统计文件数量和总大小
                let mut file_count = 0u32;
                let mut total_size = 0u64;
                if let Ok(files) = fs::read_dir(entry.path()) {
                    for f in files.flatten() {
                        if f.path().is_file() {
                            file_count += 1;
                            total_size += fs::metadata(f.path()).map(|m| m.len()).unwrap_or(0);
                        }
                    }
                }

                entries.push(VersionEntry {
                    version: name,
                    modified_timestamp: modified
                        .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())
                        .unwrap_or(0),
                    file_count,
                    total_size,
                });
            }
        }

        // 按修改时间降序排列（最新在前）
        entries.sort_by(|a, b| b.modified_timestamp.cmp(&a.modified_timestamp));
        entries
    }

    /// 同步指定版本（如果 target_version 为 None，同步最新版本）
    pub fn sync_version(&self, target_version: Option<&str>) -> SyncResult {
        let platform_dir = self.platform_dir();
        if !platform_dir.exists() {
            return SyncResult {
                success: false,
                message: format!("平台目录不存在: {}", platform_dir.display()),
                version: None,
                synced_files: vec![],
            };
        }

        // 确定版本目录
        let (version, version_dir) = if let Some(target) = target_version {
            let dir = self.package_dir().join(target);
            if !dir.exists() {
                return SyncResult {
                    success: false,
                    message: format!("版本目录不存在: {}", target),
                    version: Some(target.to_string()),
                    synced_files: vec![],
                };
            }
            (target.to_string(), dir)
        } else {
            // 查找最新版本
            match self.find_version_file() {
                Some(f) => {
                    let version = self.read_version(&f).unwrap_or_default();
                    let dir = f.parent().unwrap().to_path_buf();
                    (version, dir)
                }
                None => {
                    return SyncResult {
                        success: false,
                        message: format!("未找到 {}.version 文件!", self.package_name),
                        version: None,
                        synced_files: vec![],
                    };
                }
            }
        };

        let mut synced_files = Vec::new();

        // 清理旧文件
        if let Ok(entries) = fs::read_dir(&platform_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let prefix = format!("{}_", self.package_name);
                if name.starts_with(&prefix) || name.ends_with(".bundle")
                    || name == format!("{}.version", self.package_name)
                {
                    if entry.path().is_file() {
                        if let Err(e) = fs::remove_file(entry.path()) {
                            log::warn!("清理文件失败 {}: {}", name, e);
                        }
                    }
                }
            }
        }

        // 复制版本目录下的所有文件到平台根目录
        if let Ok(entries) = fs::read_dir(&version_dir) {
            for entry in entries.flatten() {
                if !entry.path().is_file() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                let dest = platform_dir.join(&name);
                match fs::copy(entry.path(), &dest) {
                    Ok(_) => {
                        synced_files.push(name);
                    }
                    Err(e) => {
                        log::warn!("复制文件失败 {}: {}", name, e);
                    }
                }
            }
        }

        SyncResult {
            success: true,
            message: format!("版本 {} 同步完成", version),
            version: Some(version),
            synced_files,
        }
    }

    /// 兼容旧接口
    pub fn sync(&self) -> SyncResult {
        self.sync_version(None)
    }
}

/// 设置服务根目录的符号链接
/// 对应 start.bat 中的:
/// mklink /J "%SERVER_ROOT%\%PROJECT_NAME%" "%BUNDLES_DIR%"
pub fn setup_server_root(bundles_dir: &str, project_name: &str) -> Result<PathBuf, String> {
    let bundles_path = PathBuf::from(bundles_dir);
    if !bundles_path.exists() {
        return Err(format!("Bundles 目录不存在: {}", bundles_dir));
    }

    let server_root = bundles_path.join("_server_root");

    // 清理旧的 _server_root
    if server_root.exists() {
        fs::remove_dir_all(&server_root).map_err(|e| format!("清理 _server_root 失败: {}", e))?;
    }
    fs::create_dir_all(&server_root).map_err(|e| format!("创建 _server_root 失败: {}", e))?;

    let link_path = server_root.join(project_name);

    // 创建符号链接/Junction
    create_symlink(&bundles_path, &link_path)?;

    Ok(server_root)
}

/// 跨平台符号链接创建
#[cfg(target_os = "windows")]
fn create_symlink(target: &Path, link: &Path) -> Result<(), String> {
    junction::create(target, link).map_err(|e| format!("创建 Junction 失败: {}", e))
}

#[cfg(not(target_os = "windows"))]
fn create_symlink(target: &Path, link: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, link).map_err(|e| format!("创建符号链接失败: {}", e))
}
