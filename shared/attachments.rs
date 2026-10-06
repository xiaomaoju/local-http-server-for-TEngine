//! Flat tutorial snapshots shared by the standalone and desktop servers.
use crate::resources::{is_apk, sanitize_path_component};
use serde::Serialize;
use std::{collections::{HashMap, HashSet}, fs, io::{Read, Write}, path::{Path, PathBuf}, sync::{Arc, Mutex, OnceLock, Weak, atomic::{AtomicBool, Ordering}}};

pub const MAX_SIZE: u64 = 512 * 1024 * 1024;
pub const DOCUMENT_CSP: &str = "sandbox allow-scripts allow-popups allow-downloads; default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; media-src 'self' blob:; frame-src 'self'; connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";

#[derive(Clone, Debug, Serialize)]
pub struct FileEntry { pub name: String, pub size: u64, pub modified_timestamp: u64 }

pub fn validate_name(name: &str) -> Result<(), String> {
    sanitize_path_component(name)?;
    if is_apk(name) || name.eq_ignore_ascii_case("catalog.json") { return Err("教程附件不能包含 APK 或 catalog.json".into()); }
    Ok(())
}

fn regular_file(path: &Path) -> Result<fs::Metadata, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| format!("无法读取附件：{e}"))?;
    if !metadata.file_type().is_file() { return Err("附件必须是普通文件，不支持子目录或符号链接".into()); }
    Ok(metadata)
}

#[allow(dead_code)] // Desktop picker; the standalone server receives multipart files instead.
pub fn directory_files(directory: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let root = directory.canonicalize().map_err(|e| format!("附件目录不可用，请重新选择：{e}"))?;
    if !root.is_dir() { return Err("请选择教程附件目录".into()); }
    let mut files = Vec::new();
    let mut names = HashSet::new();
    let mut total = 0u64;
    for entry in fs::read_dir(&root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().into_string().map_err(|_| "附件文件名编码无效")?;
        if name.starts_with('.') { continue; }
        validate_name(&name)?;
        total = total.saturating_add(regular_file(&entry.path())?.len());
        if total > MAX_SIZE { return Err("每套教程附件最多 512MB".into()); }
        if !names.insert(name.to_lowercase()) { return Err("目录中包含同名附件（不区分大小写）".into()); }
        files.push((name, entry.path()));
    }
    if files.is_empty() { return Err("目录中没有可上传附件".into()); }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

#[derive(Clone)]
pub struct Store { root: PathBuf }
impl Store {
    pub fn new(project_version_dir: PathBuf) -> Self { Self { root: project_version_dir.join("_attachments") } }
    fn check_root(&self) -> Result<(), String> {
        if self.root.exists() && !fs::symlink_metadata(&self.root).map_err(|e| e.to_string())?.file_type().is_dir() { return Err("附件目录无效".into()); }
        Ok(())
    }
    pub fn snapshot_dir(&self, snapshot: &str) -> Result<PathBuf, String> {
        self.check_root()?;
        if uuid::Uuid::parse_str(snapshot).is_err() { return Err("附件快照编号无效".into()); }
        let path = self.root.join(snapshot);
        if path.exists() && !fs::symlink_metadata(&path).map_err(|e| e.to_string())?.file_type().is_dir() { return Err("附件快照目录无效".into()); }
        Ok(path)
    }
    pub fn file(&self, snapshot: &str, name: &str) -> Result<PathBuf, String> {
        validate_name(name)?;
        let path = self.snapshot_dir(snapshot)?.join(name);
        regular_file(&path)?;
        Ok(path)
    }
    pub fn list(&self, snapshot: Option<&str>) -> Result<Vec<FileEntry>, String> {
        let Some(snapshot) = snapshot else { return Ok(vec![]); };
        let dir = self.snapshot_dir(snapshot)?;
        let mut files = Vec::new();
        for entry in fs::read_dir(&dir).map_err(|e| format!("无法读取附件快照：{e}"))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name().into_string().map_err(|_| "附件文件名编码无效")?;
            validate_name(&name)?;
            let meta = regular_file(&entry.path())?;
            files.push(FileEntry { name, size: meta.len(), modified_timestamp: meta.modified().ok().and_then(|v| v.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |v| v.as_secs()) });
        }
        files.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(files)
    }
    /// Build an immutable merged set. Dropping an uncommitted snapshot rolls it back.
    pub fn prepare(&self, current: Option<&str>, updates: &[(String, PathBuf)], remove: Option<&str>) -> Result<Snapshot, String> {
        self.check_root()?;
        let mut names = HashSet::new();
        let mut sources = HashMap::new();
        for entry in self.list(current)? {
            if remove == Some(entry.name.as_str()) { continue; }
            sources.insert(entry.name.clone(), self.file(current.unwrap(), &entry.name)?);
        }
        if let Some(name) = remove {
            validate_name(name)?;
            self.file(current.ok_or("附件不存在")?, name)?;
        }
        for (name, path) in updates {
            validate_name(name)?;
            if !names.insert(name.to_lowercase()) { return Err("上传包含同名附件（不区分大小写）".into()); }
            regular_file(path)?;
            if sources.keys().any(|old| old != name && old.to_lowercase() == name.to_lowercase()) { return Err("附件名称与已有文件仅大小写不同，请先删除旧文件".into()); }
            sources.insert(name.clone(), path.clone());
        }
        let size = sources.values().try_fold(0u64, |total, path| regular_file(path).map(|m| total.saturating_add(m.len())))?;
        if size > MAX_SIZE { return Err("每套教程附件最多 512MB".into()); }
        let id = uuid::Uuid::new_v4().to_string();
        let path = self.snapshot_dir(&id)?;
        fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        fs::create_dir(&path).map_err(|e| e.to_string())?;
        let snapshot = Snapshot { id, path, committed: false };
        let mut total = 0;
        for (name, source) in sources {
            let expected = regular_file(&source)?.len();
            let mut input = fs::File::open(source).map_err(|e| e.to_string())?.take(MAX_SIZE - total + 1);
            let mut output = fs::File::create(snapshot.path.join(name)).map_err(|e| e.to_string())?;
            let copied = std::io::copy(&mut input, &mut output).map_err(|e| e.to_string())?;
            total += copied;
            if total > MAX_SIZE || copied != expected { return Err("附件大小超限或源文件在复制期间发生变化，请重新选择".into()); }
            output.flush().map_err(|e| e.to_string())?;
            output.sync_all().map_err(|e| e.to_string())?;
        }
        Ok(snapshot)
    }
    // Caller holds the project mutation/config lock while acquiring or retiring a lease.
    pub fn lease(&self, snapshot: &str) -> Result<Arc<Lease>, String> {
        let path = self.snapshot_dir(snapshot)?;
        let mut leases = leases().lock().unwrap();
        leases.retain(|_, lease| lease.strong_count() > 0);
        if let Some(lease) = leases.get(&path).and_then(Weak::upgrade) { return Ok(lease); }
        let lease = Arc::new(Lease { path: path.clone(), retired: AtomicBool::new(false) });
        leases.insert(path, Arc::downgrade(&lease));
        Ok(lease)
    }
    pub fn retire(&self, snapshot: &str) {
        if let Ok(path) = self.snapshot_dir(snapshot) {
            let lease = leases().lock().unwrap().remove(&path).and_then(|v| v.upgrade());
            if let Some(lease) = lease { lease.retired.store(true, Ordering::Release); }
            else { let _ = fs::remove_dir_all(path); }
        }
    }
    pub fn cleanup(&self, current: Option<&str>) -> Result<(), String> {
        self.check_root()?;
        if let Some(current) = current { self.snapshot_dir(current)?; }
        if !self.root.exists() { return Ok(()); }
        for entry in fs::read_dir(&self.root).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if current != Some(name.as_str()) && uuid::Uuid::parse_str(&name).is_ok() && entry.file_type().map_err(|e| e.to_string())?.is_dir() { self.retire(&name); }
        }
        Ok(())
    }
}

pub struct Snapshot { pub id: String, path: PathBuf, committed: bool }
impl Snapshot { pub fn commit(&mut self) { self.committed = true; } }
impl Drop for Snapshot { fn drop(&mut self) { if !self.committed { let _ = fs::remove_dir_all(&self.path); } } }
pub struct Lease { path: PathBuf, retired: AtomicBool }
impl Drop for Lease { fn drop(&mut self) { if self.retired.load(Ordering::Acquire) { let _ = fs::remove_dir_all(&self.path); } } }
fn leases() -> &'static Mutex<HashMap<PathBuf, Weak<Lease>>> {
    static LEASES: OnceLock<Mutex<HashMap<PathBuf, Weak<Lease>>>> = OnceLock::new();
    LEASES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub struct Reader { pub file: tokio::fs::File, pub _lease: Option<Arc<Lease>> }
impl tokio::io::AsyncRead for Reader {
    fn poll_read(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, buf: &mut tokio::io::ReadBuf<'_>) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.file).poll_read(cx, buf)
    }
}

pub fn inline(name: &str) -> bool {
    matches!(Path::new(name).extension().and_then(|v| v.to_str()).unwrap_or("").to_ascii_lowercase().as_str(), "html" | "htm" | "css" | "js" | "mjs" | "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" | "avif" | "woff" | "woff2" | "ttf" | "otf" | "mp3" | "mp4" | "webm" | "ogg" | "wav")
}

pub fn headers(name: &str, size: u64) -> axum::http::HeaderMap {
    use axum::http::{header, HeaderMap};
    let mut headers = HeaderMap::new();
    let mime = if name.to_ascii_lowercase().ends_with(".html") || name.to_ascii_lowercase().ends_with(".htm") { "text/html; charset=utf-8".into() } else { mime_guess::from_path(name).first_or_octet_stream().to_string() };
    headers.insert(header::CONTENT_TYPE, mime.parse().unwrap());
    headers.insert(header::CONTENT_LENGTH, size.into());
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    let encoded = percent_encoding::utf8_percent_encode(name, percent_encoding::NON_ALPHANUMERIC);
    headers.insert(header::CONTENT_DISPOSITION, format!("{}; filename*=UTF-8''{encoded}", if inline(name) { "inline" } else { "attachment" }).parse().unwrap());
    headers.insert(header::CONTENT_SECURITY_POLICY, DOCUMENT_CSP.parse().unwrap());
    headers.insert("X-Content-Type-Options", "nosniff".parse().unwrap());
    headers.insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
    headers
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("Cache/tests/attachments-store").join(uuid::Uuid::new_v4().to_string());
            fs::create_dir_all(root.join("source")).unwrap();
            Self(root)
        }
        fn source(&self, name: &str, contents: &[u8]) -> (String, PathBuf) {
            let path = self.0.join("source").join(name);
            fs::write(&path, contents).unwrap();
            (name.into(), path)
        }
        fn store(&self) -> Store { Store::new(self.0.join("project")) }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn snapshots_merge_replace_rollback_and_keep_readers_until_finished() {
        let fixture = Fixture::new(); let store = fixture.store();
        let mut first = store.prepare(None, &[fixture.source("index.html", b"old"), fixture.source("style.css", b"style")], None).unwrap();
        first.commit();
        let lease = store.lease(&first.id).unwrap();
        let mut second = store.prepare(Some(&first.id), &[fixture.source("index.html", b"new")], None).unwrap();
        assert_eq!(store.list(Some(&second.id)).unwrap().len(), 2);
        assert_eq!(fs::read(store.file(&second.id, "style.css").unwrap()).unwrap(), b"style");
        assert_eq!(fs::read(store.file(&first.id, "index.html").unwrap()).unwrap(), b"old");
        let rollback = store.prepare(Some(&second.id), &[], Some("style.css")).unwrap();
        let rollback_path = store.snapshot_dir(&rollback.id).unwrap();
        drop(rollback); assert!(!rollback_path.exists());
        second.commit(); store.retire(&first.id);
        assert!(store.snapshot_dir(&first.id).unwrap().exists());
        drop(lease); assert!(!store.snapshot_dir(&first.id).unwrap().exists());
        let mut orphan = store.prepare(None, &[], None).unwrap(); orphan.commit();
        store.cleanup(Some(&second.id)).unwrap();
        assert!(!store.snapshot_dir(&orphan.id).unwrap().exists());
        assert!(store.snapshot_dir(&second.id).unwrap().exists());
    }

    #[test]
    fn invalid_paths_duplicates_reserved_files_and_total_size_are_rejected() {
        let fixture = Fixture::new(); let store = fixture.store();
        for name in ["../index.html", "assets/a.js", "a\\b", "test.apk", "CATALOG.JSON", "a..html", "/absolute", ""] { assert!(validate_name(name).is_err(), "{name}"); }
        assert!(store.prepare(None, &[fixture.source("a.html", b"a"), fixture.source("A.html", b"b")], None).is_err());
        let mut original = store.prepare(None, &[fixture.source("index.html", b"old")], None).unwrap(); original.commit();
        assert!(store.prepare(Some(&original.id), &[fixture.source("INDEX.html", b"new")], None).is_err());
        assert!(store.prepare(Some(&original.id), &[], Some("missing.html")).is_err());
        let huge = fixture.source("large.bin", b"");
        fs::File::create(&huge.1).unwrap().set_len(MAX_SIZE).unwrap();
        assert!(store.prepare(Some(&original.id), &[huge], None).err().unwrap().contains("512MB"));
        assert_eq!(fs::read(store.file(&original.id, "index.html").unwrap()).unwrap(), b"old");
    }

    #[test]
    fn arbitrary_flat_directory_includes_standalone_html_and_skips_hidden_files() {
        let fixture = Fixture::new();
        fixture.source("one.html", b"<h1>standalone</h1>");
        fixture.source(".DS_Store", b"ignored");
        let files = directory_files(&fixture.0.join("source")).unwrap();
        assert_eq!(files.len(), 1); assert_eq!(files[0].0, "one.html");
        fs::create_dir(fixture.0.join("source/assets")).unwrap();
        assert!(directory_files(&fixture.0.join("source")).is_err());
        fs::remove_dir(fixture.0.join("source/assets")).unwrap();
        fixture.source("catalog.json", b"{}");
        assert!(directory_files(&fixture.0.join("source")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_files_and_snapshot_directories_are_rejected() {
        let fixture = Fixture::new(); let store = fixture.store();
        let source = fixture.source("source.html", b"secret");
        let link = fixture.0.join("source/link.html");
        std::os::unix::fs::symlink(&source.1, &link).unwrap();
        assert!(store.prepare(None, &[("link.html".into(), link)], None).is_err());
        fs::create_dir_all(&store.root).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        std::os::unix::fs::symlink(fixture.0.join("source"), store.root.join(&id)).unwrap();
        assert!(store.file(&id, "source.html").is_err());
    }
}
