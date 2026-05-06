use std::fs;
use std::path::PathBuf;

fn sanitize_path_component(s: &str) -> Result<&str, String> {
    if s.is_empty() || s.contains('/') || s.contains('\\') || s.contains("..") {
        return Err(format!("Invalid path component: {}", s));
    }
    Ok(s)
}

pub struct Storage {
    resources_dir: PathBuf,
}

impl Storage {
    pub fn new(resources_dir: PathBuf) -> Self {
        Self { resources_dir }
    }

    pub fn project_dir(&self, project_name: &str) -> Result<PathBuf, String> {
        Ok(self.resources_dir.join(sanitize_path_component(project_name)?))
    }

    pub fn project_version_dir(
        &self,
        project_name: &str,
        project_version: &str,
    ) -> Result<PathBuf, String> {
        Ok(self.project_dir(project_name)?.join(sanitize_path_component(project_version)?))
    }

    pub fn platform_dir(
        &self,
        project_name: &str,
        project_version: &str,
        platform: &str,
    ) -> Result<PathBuf, String> {
        Ok(self
            .project_version_dir(project_name, project_version)?
            .join(sanitize_path_component(platform)?))
    }

    pub fn versions_dir(
        &self,
        project_name: &str,
        project_version: &str,
        platform: &str,
    ) -> Result<PathBuf, String> {
        Ok(self.platform_dir(project_name, project_version, platform)?.join("_versions"))
    }

    pub fn version_dir(
        &self,
        project_name: &str,
        project_version: &str,
        platform: &str,
        version: &str,
    ) -> Result<PathBuf, String> {
        Ok(self
            .versions_dir(project_name, project_version, platform)?
            .join(sanitize_path_component(version)?))
    }

    pub fn save_uploaded_file(
        &self,
        project_name: &str,
        project_version: &str,
        platform: &str,
        version: &str,
        file_name: &str,
        data: &[u8],
    ) -> Result<(), String> {
        let safe_name = sanitize_path_component(file_name)?;
        let dir = self.version_dir(project_name, project_version, platform, version)?;
        fs::create_dir_all(&dir).map_err(|e| format!("Failed to create version dir: {}", e))?;
        let path = dir.join(safe_name);
        fs::write(&path, data).map_err(|e| format!("Failed to write file: {}", e))?;
        Ok(())
    }

    pub fn list_versions(
        &self,
        project_name: &str,
        project_version: &str,
        platform: &str,
    ) -> Vec<VersionEntry> {
        let dir = match self.versions_dir(project_name, project_version, platform) {
            Ok(d) => d,
            Err(_) => return vec![],
        };
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

    pub fn delete_version(
        &self,
        project_name: &str,
        project_version: &str,
        platform: &str,
        version: &str,
    ) -> Result<(), String> {
        let dir = self.version_dir(project_name, project_version, platform, version)?;
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|e| format!("Failed to delete version: {}", e))?;
        }
        Ok(())
    }

    pub fn list_files(
        &self,
        project_name: &str,
        project_version: &str,
        platform: &str,
        version: &str,
    ) -> Result<Vec<FileEntry>, String> {
        let dir = self.version_dir(project_name, project_version, platform, version)?;
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut entries = Vec::new();
        let read_dir = fs::read_dir(&dir).map_err(|e| format!("读取目录失败: {}", e))?;
        for entry in read_dir.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let metadata = match fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let size = metadata.len();
            let modified_timestamp = metadata
                .modified()
                .ok()
                .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())
                .unwrap_or(0);
            entries.push(FileEntry { name, size, modified_timestamp });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    pub fn list_files_with_hash(
        &self,
        project_name: &str,
        project_version: &str,
        platform: &str,
        version: &str,
    ) -> Result<Vec<FileManifestEntry>, String> {
        use md5::{Digest, Md5};
        let dir = self.version_dir(project_name, project_version, platform, version)?;
        if !dir.exists() {
            return Err(format!("版本目录不存在: {}", version));
        }
        let mut entries = Vec::new();
        let read_dir = fs::read_dir(&dir).map_err(|e| format!("读取目录失败: {}", e))?;
        for entry in read_dir.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let bytes = fs::read(&path).map_err(|e| format!("读取文件 {} 失败: {}", name, e))?;
            let size = bytes.len() as u64;
            let mut hasher = Md5::new();
            hasher.update(&bytes);
            let md5 = format!("{:x}", hasher.finalize());
            entries.push(FileManifestEntry { name, size, md5 });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    pub fn copy_files_from_version(
        &self,
        project_name: &str,
        project_version: &str,
        platform: &str,
        new_version: &str,
        base_version: &str,
        copy_files: &[String],
    ) -> Result<u32, String> {
        let base_dir = self.version_dir(project_name, project_version, platform, base_version)?;
        if !base_dir.exists() {
            return Err(format!("基础版本不存在: {}", base_version));
        }
        let new_dir = self.version_dir(project_name, project_version, platform, new_version)?;
        fs::create_dir_all(&new_dir).map_err(|e| format!("创建版本目录失败: {}", e))?;
        let mut count = 0u32;
        for name in copy_files {
            let safe_name = sanitize_path_component(name)?;
            let src = base_dir.join(safe_name);
            if !src.exists() {
                return Err(format!("基础版本中不存在文件: {}", name));
            }
            let dst = new_dir.join(safe_name);
            fs::copy(&src, &dst).map_err(|e| format!("复制文件 {} 失败: {}", name, e))?;
            count += 1;
        }
        Ok(count)
    }

    pub fn delete_project_version(
        &self,
        project_name: &str,
        project_version: &str,
    ) -> Result<(), String> {
        let dir = self.project_version_dir(project_name, project_version)?;
        if dir.exists() {
            fs::remove_dir_all(&dir)
                .map_err(|e| format!("删除项目版本目录失败: {}", e))?;
        }
        Ok(())
    }

    pub fn rename_project_version(
        &self,
        project_name: &str,
        old_name: &str,
        new_name: &str,
    ) -> Result<(), String> {
        let old_dir = self.project_version_dir(project_name, old_name)?;
        let new_dir = self.project_version_dir(project_name, new_name)?;
        if !old_dir.exists() {
            return Ok(());
        }
        if new_dir.exists() {
            return Err(format!("目标目录已存在: {}", new_name));
        }
        fs::rename(&old_dir, &new_dir)
            .map_err(|e| format!("重命名项目版本目录失败: {}", e))
    }

    pub fn delete_project(&self, project_name: &str) -> Result<(), String> {
        let dir = self.project_dir(project_name)?;
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

#[derive(Debug, Clone, serde::Serialize)]
pub struct FileEntry {
    pub name: String,
    pub size: u64,
    pub modified_timestamp: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileManifestEntry {
    pub name: String,
    pub size: u64,
    pub md5: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_dir_includes_project_version_segment() {
        let s = Storage::new(PathBuf::from("/tmp/res"));
        let p = s.version_dir("MyProject", "v1", "Android", "1.0.0").unwrap();
        assert_eq!(
            p,
            PathBuf::from("/tmp/res/MyProject/v1/Android/_versions/1.0.0")
        );
    }

    #[test]
    fn rejects_path_traversal_in_components() {
        let s = Storage::new(PathBuf::from("/tmp/res"));
        assert!(s.version_dir("..", "v1", "Android", "1.0.0").is_err());
        assert!(s.version_dir("p", "..", "Android", "1.0.0").is_err());
        assert!(s.version_dir("p", "v1", "/etc", "1.0.0").is_err());
    }
}
