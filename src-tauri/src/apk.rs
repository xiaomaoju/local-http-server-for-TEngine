use crate::{config::AppConfig, local_resources::{NativeFile, StagedRelease}, resources::{sanitize_path_component, ProjectType}};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::{Path, PathBuf}};

const MAX_CATALOG_SIZE: u64 = 256 * 1024;
const MAX_UPLOAD_SIZE: u64 = 512 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Catalog {
    schema_version: u32,
    package_name: String,
    version_code: u64,
    version_name: String,
    min_sdk: u32,
    changelog: String,
    apk_path: String,
    apk_size: u64,
    sha256: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApkRelease {
    pub version: String,
    pub version_name: String,
    pub package_name: String,
    pub catalog_path: String,
    pub catalog_sha256: String,
    pub files: Vec<NativeFile>,
}

fn read_release(root: &Path, catalog_path: &Path) -> Result<(ApkRelease, Catalog, Vec<u8>), String> {
    let catalog_path = catalog_path.canonicalize().map_err(|e| format!("无法读取 catalog：{e}"))?;
    if !catalog_path.starts_with(root) { return Err("catalog 路径超出所选目录".into()); }
    let mut raw = Vec::new();
    fs::File::open(&catalog_path).map_err(|e| e.to_string())?.take(MAX_CATALOG_SIZE + 1).read_to_end(&mut raw).map_err(|e| e.to_string())?;
    if raw.len() as u64 > MAX_CATALOG_SIZE { return Err("catalog.json 不能超过 256KB".into()); }
    let catalog: Catalog = serde_json::from_slice(&raw).map_err(|e| format!("catalog.json 格式无效：{e}"))?;
    let version = catalog.version_code.to_string();
    ProjectType::Apk.validate_version(&version)?;
    if catalog.schema_version != 1 || catalog.package_name.trim().is_empty() || catalog.version_name.trim().is_empty() || catalog.min_sdk == 0 { return Err("catalog 的协议版本、包名、版本名称或 minSdk 无效".into()); }
    if catalog.sha256.len() != 64 || !catalog.sha256.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) { return Err("catalog.sha256 必须是 64 位小写十六进制字符串".into()); }
    let parts: Vec<_> = catalog.apk_path.split('/').collect();
    if parts.len() != 3 || parts[0] != "releases" || parts[1] != version { return Err("apkPath 应为 releases/versionCode/APK文件名".into()); }
    let name = urlencoding::decode(parts[2]).map_err(|_| "APK 文件名编码无效")?.into_owned();
    sanitize_path_component(&name)?;
    if !name.to_lowercase().ends_with(".apk") { return Err("apkPath 必须指向 APK 文件".into()); }
    let apk_path = catalog_path.parent().unwrap().join(&name).canonicalize().map_err(|e| format!("缺少 APK {name}：{e}"))?;
    if !apk_path.starts_with(root) { return Err("APK 路径超出所选目录".into()); }
    let metadata = fs::metadata(&apk_path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() != catalog.apk_size { return Err("APK 大小与 catalog.apkSize 不一致，请重新构建完整版本".into()); }
    if metadata.len().saturating_add(raw.len() as u64) > MAX_UPLOAD_SIZE { return Err("单次上传最多 512MB".into()); }
    // Read all required fields while preserving the original catalog bytes.
    let _ = &catalog.changelog;
    let release = ApkRelease { version, version_name: catalog.version_name.clone(), package_name: catalog.package_name.clone(), catalog_path: catalog_path.to_string_lossy().into_owned(), catalog_sha256: format!("{:x}", Sha256::digest(&raw)), files: vec![NativeFile {path: apk_path.to_string_lossy().into_owned(), name, size: metadata.len()}, NativeFile {path: catalog_path.to_string_lossy().into_owned(), name: "catalog.json".into(), size: raw.len() as u64}] };
    Ok((release, catalog, raw))
}

pub fn list_releases(directory: &Path) -> Result<Vec<ApkRelease>, String> {
    let root = directory.canonicalize().map_err(|e| format!("构建目录不可用，请更换目录：{e}"))?;
    if !root.is_dir() { return Err("请选择构建目录".into()); }
    let mut catalogs = Vec::new();
    let mut incomplete = Vec::new();
    if root.join("catalog.json").is_file() { catalogs.push(root.join("catalog.json")); }
    for entry in fs::read_dir(&root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir() { continue; }
        if entry.path().join("catalog.json").is_file() { catalogs.push(entry.path().join("catalog.json")); }
        else if let Ok(number) = entry.file_name().to_string_lossy().parse::<u64>() { if number > 0 { incomplete.push(number); } }
    }
    if catalogs.is_empty() { return Err("未找到 catalog.json；根目录或一级版本子目录应包含 catalog 和 APK".into()); }
    let mut releases = Vec::new();
    for path in catalogs { releases.push(read_release(&root, &path).map_err(|e| format!("{}：{e}", path.display()))?.0); }
    releases.sort_by_key(|r| std::cmp::Reverse(r.version.parse::<u64>().unwrap()));
    if incomplete.iter().any(|v| *v > releases[0].version.parse::<u64>().unwrap()) { return Err("最新编号目录缺少 catalog.json，请等待构建完整后重新读取".into()); }
    if releases.windows(2).any(|r| r[0].version == r[1].version) { return Err("目录内多个 catalog 使用同一 versionCode，无法确定文件对".into()); }
    Ok(releases)
}

pub async fn prepare(directory: String, version: String, fingerprint: String, staging_root: PathBuf) -> Result<(StagedRelease, Vec<String>), String> {
    let (release, catalog, raw) = tokio::task::spawn_blocking(move || {
        let root = PathBuf::from(directory).canonicalize().map_err(|e| e.to_string())?;
        let selected = list_releases(&root)?.into_iter().find(|r| r.version == version).ok_or("所选版本已不存在，请重新读取")?;
        if selected.catalog_sha256 != fingerprint { return Err("catalog 已变化，请重新读取后上传".into()); }
        read_release(&root, Path::new(&selected.catalog_path))
    }).await.map_err(|e| e.to_string())??;
    let stage = StagedRelease {path: staging_root.join(uuid::Uuid::new_v4().to_string())};
    tokio::fs::create_dir_all(&stage.path).await.map_err(|e| e.to_string())?;
    let apk = stage.path.join(&release.files[0].name);
    tokio::fs::copy(&release.files[0].path, &apk).await.map_err(|e| format!("无法准备 APK：{e}"))?;
    tokio::fs::write(stage.path.join("catalog.json"), raw).await.map_err(|e| e.to_string())?;
    let check_path = apk.clone();
    tokio::task::spawn_blocking(move || {
        let mut file = fs::File::open(check_path).map_err(|e| e.to_string())?;
        if file.metadata().map_err(|e| e.to_string())?.len() != catalog.apk_size { return Err("APK 已变化，请重新构建后上传".into()); }
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop { let count = file.read(&mut buffer).map_err(|e| e.to_string())?; if count == 0 { break; } hash.update(&buffer[..count]); }
        if format!("{:x}", hash.finalize()) != catalog.sha256 { return Err("APK SHA-256 与 catalog 不一致，请重新构建完整版本".into()); }
        Ok::<_, String>(())
    }).await.map_err(|e| e.to_string())??;
    let paths = vec![apk.to_string_lossy().into_owned(), stage.path.join("catalog.json").to_string_lossy().into_owned()];
    Ok((stage, paths))
}

#[tauri::command]
pub async fn list_local_apk_releases(directory: String) -> Result<Vec<ApkRelease>, String> {
    tokio::task::spawn_blocking(move || list_releases(Path::new(&directory))).await.map_err(|e| e.to_string())?
}

pub fn staging_root() -> PathBuf { AppConfig::resources_dir().join("staging") }

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self { let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("Cache/tests/apk-directory").join(uuid::Uuid::new_v4().to_string()); fs::create_dir_all(&path).unwrap(); Self(path) }
        fn release(&self, directory: &str, code: u64, name: &str) -> PathBuf {
            let dir = self.0.join(directory); fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(name), b"fixture-apk").unwrap();
            let catalog = serde_json::json!({"schemaVersion":1,"packageName":"example.app","versionCode":code,"versionName":format!("1.{code}"),"minSdk":26,"changelog":"test","apkPath":format!("releases/{code}/{}",urlencoding::encode(name)),"apkSize":11,"sha256":format!("{:x}",Sha256::digest(b"fixture-apk"))});
            fs::write(dir.join("catalog.json"), serde_json::to_vec_pretty(&catalog).unwrap()).unwrap(); dir
        }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn any_root_name_uses_catalog_numbers_and_supports_flat_pairs() {
        let fixture = Fixture::new();
        let root = fixture.0.join("任意 release 名称"); fs::create_dir(&root).unwrap();
        fixture.release("任意 release 名称/older-folder", 9, "应用 空格.apk");
        fixture.release("任意 release 名称/newer-folder", 10, "app.apk");
        let releases = list_releases(&root).unwrap();
        assert_eq!(releases.iter().map(|r| r.version.as_str()).collect::<Vec<_>>(), vec!["10", "9"]);
        assert_eq!(releases[1].files[0].name, "应用 空格.apk");
        let flat = fixture.release("flat", 11, "flat.apk");
        assert_eq!(list_releases(&flat).unwrap()[0].version, "11");
        fs::create_dir(root.join("12")).unwrap();
        assert!(list_releases(&root).unwrap_err().contains("缺少 catalog"));
    }

    #[test]
    fn conflicting_or_incomplete_catalogs_and_unsafe_paths_are_rejected() {
        let fixture = Fixture::new();
        fixture.release("one", 70, "app.apk"); fixture.release("two", 70, "app.apk");
        assert!(list_releases(&fixture.0).unwrap_err().contains("同一 versionCode"));
        fs::remove_dir_all(fixture.0.join("two")).unwrap();
        let path = fixture.0.join("one/catalog.json");
        let mut catalog: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        catalog["apkPath"] = "releases/70/..%2Foutside.apk".into();
        fs::write(&path, serde_json::to_vec(&catalog).unwrap()).unwrap();
        assert!(list_releases(&fixture.0).is_err());
        fixture.release("one", 70, "app.apk"); fs::remove_file(fixture.0.join("one/app.apk")).unwrap();
        assert!(list_releases(&fixture.0).unwrap_err().contains("缺少 APK"));
    }

    #[tokio::test]
    async fn snapshot_preserves_raw_catalog_and_checks_hash_and_changes() {
        let fixture = Fixture::new();
        let root = fixture.release("any-output", 70, "app.apk");
        let release = list_releases(&root).unwrap().remove(0);
        let raw = fs::read(root.join("catalog.json")).unwrap();
        let stages = fixture.0.join("snapshots");
        let (snapshot, paths) = prepare(root.to_string_lossy().into(), "70".into(), release.catalog_sha256.clone(), stages.clone()).await.unwrap();
        assert_eq!(fs::read(&paths[1]).unwrap(), raw);
        fs::write(root.join("app.apk"), b"changed-apk").unwrap();
        assert_eq!(fs::read(&paths[0]).unwrap(), b"fixture-apk");
        drop(snapshot); assert_eq!(fs::read_dir(&stages).unwrap().count(), 0);
        assert!(prepare(root.to_string_lossy().into(), "70".into(), release.catalog_sha256, stages.clone()).await.err().unwrap().contains("SHA-256"));
        assert_eq!(fs::read_dir(&stages).unwrap().count(), 0);
        assert!(prepare(root.to_string_lossy().into(), "70".into(), "stale".into(), stages).await.err().unwrap().contains("catalog 已变化"));
    }
}
