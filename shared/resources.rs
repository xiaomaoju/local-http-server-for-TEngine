use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectType {
    #[default]
    Bundle,
    Apk,
    Files,
}

impl ProjectType {
    pub fn namespace(self) -> &'static str {
        match self { Self::Bundle => "res", Self::Apk => "appversion", Self::Files => "file" }
    }

    pub fn fixed_platform(self) -> Option<&'static str> {
        match self { Self::Bundle => None, Self::Apk => Some("Android"), Self::Files => Some("Any") }
    }

    pub fn as_str(self) -> &'static str {
        match self { Self::Bundle => "bundle", Self::Apk => "apk", Self::Files => "files" }
    }

    pub fn validate_version(self, version: &str) -> Result<(), String> {
        sanitize_path_component(version)?;
        if self == Self::Apk && (version.parse::<u64>().ok().filter(|v| *v > 0).map(|v| v.to_string()).as_deref() != Some(version)) {
            return Err("APK 发布编号必须是 versionCode 的正整数十进制字符串".into());
        }
        Ok(())
    }

    pub fn validate_files(self, names: &[String]) -> Result<(), String> {
        if names.is_empty() { return Err("请选择至少一个文件".into()); }
        let mut seen = std::collections::HashSet::new();
        for name in names {
            sanitize_path_component(name)?;
            // Keep imported snapshots portable to case-insensitive desktop filesystems.
            if !seen.insert(name.to_lowercase()) { return Err("上传包含同名文件（不区分大小写）".into()); }
        }
        if self == Self::Apk && (names.len() != 2 || !names.iter().any(|n| n == "catalog.json") || names.iter().filter(|n| is_apk(n)).count() != 1) {
            return Err("APK 更新需上传一个 .apk 和一个 catalog.json".into());
        }
        Ok(())
    }
}

pub fn is_apk(name: &str) -> bool { name.to_ascii_lowercase().ends_with(".apk") }

pub fn sanitize_path_component(s: &str) -> Result<&str, String> {
    if s.is_empty() || s.len() > 200 || s.trim() != s || s == "." || s.contains('/') || s.contains('\\') || s.contains("..") || s.chars().any(|c| c.is_control() || ":*?\"<>|".contains(c)) {
        return Err(format!("Invalid path component: {}", s));
    }
    Ok(s)
}
