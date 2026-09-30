use serde::{Deserialize, Serialize};
use std::sync::Mutex;

// Serialize native keychain operations, including rapid changes between connections.
static CREDENTIAL_LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize)]
struct SavedPassword {
    url: String,
    username: String,
    password: String,
}

fn entry(service: &str, id: &str) -> Result<keyring::Entry, String> {
    if !cfg!(any(target_os = "macos", target_os = "windows")) {
        return Err("当前系统暂不支持记住密码，可取消勾选后保存连接。".into());
    }
    if id.is_empty() || id.len() > 128 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        return Err("连接标识无效，请新建连接。".into());
    }
    keyring::Entry::new(service, id).map_err(|_| "无法打开系统凭据库。".into())
}

#[tauri::command]
pub async fn get_connection_password(app: tauri::AppHandle, id: String, url: String, username: String) -> Result<Option<String>, String> {
    let service = format!("{}.connections", app.config().identifier);
    tauri::async_runtime::spawn_blocking(move || {
        let _lock = CREDENTIAL_LOCK.lock().map_err(|_| "系统凭据库繁忙。")?;
        match entry(&service, &id)?.get_password() {
            Ok(raw) => {
                let saved: SavedPassword = serde_json::from_str(&raw).map_err(|_| "已存密码格式无效，请重新保存。")?;
                // Never fill another server/account's password after editing a profile.
                Ok((saved.url == url && saved.username == username).then_some(saved.password))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("无法读取已存密码，请解锁系统凭据库或手动输入。".into()),
        }
    }).await.map_err(|_| "读取系统凭据库失败。".to_string())?
}

#[tauri::command]
pub async fn save_connection_password(app: tauri::AppHandle, id: String, url: String, username: String, password: String) -> Result<(), String> {
    let service = format!("{}.connections", app.config().identifier);
    tauri::async_runtime::spawn_blocking(move || {
        let _lock = CREDENTIAL_LOCK.lock().map_err(|_| "系统凭据库繁忙。")?;
        let raw = serde_json::to_string(&SavedPassword {url, username, password}).map_err(|_| "密码编码失败。")?;
        entry(&service, &id)?.set_password(&raw).map_err(|_| "密码未能保存，请解锁系统凭据库后重试。".into())
    }).await.map_err(|_| "保存系统凭据库失败。".to_string())?
}

#[tauri::command]
pub async fn remove_connection_password(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let service = format!("{}.connections", app.config().identifier);
    tauri::async_runtime::spawn_blocking(move || {
        let _lock = CREDENTIAL_LOCK.lock().map_err(|_| "系统凭据库繁忙。")?;
        match entry(&service, &id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("已存密码未能移除，请解锁系统凭据库后重试。".into()),
        }
    }).await.map_err(|_| "移除系统凭据库密码失败。".to_string())?
}
