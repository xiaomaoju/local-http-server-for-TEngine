use std::fs;
use std::path::PathBuf;

pub struct Storage {
    resources_dir: PathBuf,
}

impl Storage {
    pub fn new(resources_dir: PathBuf) -> Self {
        Self { resources_dir }
    }

    pub fn project_dir(&self, project_name: &str) -> PathBuf {
        self.resources_dir.join(project_name)
    }

    pub fn platform_dir(&self, project_name: &str, platform: &str) -> PathBuf {
        self.project_dir(project_name).join(platform)
    }

    pub fn versions_dir(&self, project_name: &str, platform: &str) -> PathBuf {
        self.platform_dir(project_name, platform).join("_versions")
    }

    pub fn version_dir(&self, project_name: &str, platform: &str, version: &str) -> PathBuf {
        self.versions_dir(project_name, platform).join(version)
    }

    pub fn save_uploaded_file(
        &self,
        project_name: &str,
        platform: &str,
        version: &str,
        file_name: &str,
        data: &[u8],
    ) -> Result<(), String> {
        let dir = self.version_dir(project_name, platform, version);
        fs::create_dir_all(&dir).map_err(|e| format!("Failed to create version dir: {}", e))?;
        let path = dir.join(file_name);
        fs::write(&path, data).map_err(|e| format!("Failed to write file: {}", e))?;
        Ok(())
    }

    pub fn list_versions(&self, project_name: &str, platform: &str) -> Vec<VersionEntry> {
        let dir = self.versions_dir(project_name, platform);
        if !dir.exists() {
            return vec![];
        }

        let mut entries = Vec::new();
        if let Ok(read_dir) = fs::read_dir(&dir) {
            for entry in read_dir.flatten() {
                if !entry.path().is_dir() {
                    continue;
                }
                let version = entry.file_name().to_string_lossy().to_string();
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

                let modified_timestamp = fs::metadata(entry.path())
                    .and_then(|m| m.modified())
                    .ok()
                    .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())
                    .unwrap_or(0);

                entries.push(VersionEntry {
                    version,
                    file_count,
                    total_size,
                    modified_timestamp,
                });
            }
        }

        entries.sort_by(|a, b| b.modified_timestamp.cmp(&a.modified_timestamp));
        entries
    }

    pub fn activate_version(
        &self,
        project_name: &str,
        platform: &str,
        version: &str,
    ) -> Result<u32, String> {
        let version_dir = self.version_dir(project_name, platform, version);
        if !version_dir.exists() {
            return Err(format!("Version directory does not exist: {}", version));
        }

        let platform_dir = self.platform_dir(project_name, platform);
        fs::create_dir_all(&platform_dir)
            .map_err(|e| format!("Failed to create platform dir: {}", e))?;

        // Clean old activated files (keep _versions dir)
        if let Ok(entries) = fs::read_dir(&platform_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name == "_versions" {
                    continue;
                }
                if entry.path().is_file() {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }

        // Copy version files to platform root
        let mut count = 0u32;
        if let Ok(entries) = fs::read_dir(&version_dir) {
            for entry in entries.flatten() {
                if !entry.path().is_file() {
                    continue;
                }
                let name = entry.file_name();
                let dest = platform_dir.join(&name);
                fs::copy(entry.path(), &dest)
                    .map_err(|e| format!("Failed to copy {}: {}", name.to_string_lossy(), e))?;
                count += 1;
            }
        }

        Ok(count)
    }

    pub fn delete_version(
        &self,
        project_name: &str,
        platform: &str,
        version: &str,
    ) -> Result<(), String> {
        let dir = self.version_dir(project_name, platform, version);
        if dir.exists() {
            fs::remove_dir_all(&dir)
                .map_err(|e| format!("Failed to delete version: {}", e))?;
        }
        Ok(())
    }

    pub fn delete_project(&self, project_name: &str) -> Result<(), String> {
        let dir = self.project_dir(project_name);
        if dir.exists() {
            fs::remove_dir_all(&dir)
                .map_err(|e| format!("Failed to delete project: {}", e))?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VersionEntry {
    pub version: String,
    pub file_count: u32,
    pub total_size: u64,
    pub modified_timestamp: u64,
}
