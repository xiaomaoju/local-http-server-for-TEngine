use super::*;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use tower::ServiceExt;

#[tokio::test]
async fn protected_startup_never_creates_a_missing_database() {
    let fixture = Fixture::new().await;
    let mut config = fixture.state.server_config.clone();
    config.require_existing_db = true;
    assert!(db::open(&config).await.is_err());
    assert!(!config.data_dir.join("management.sqlite").exists());
}

#[tokio::test]
async fn protected_startup_rejects_empty_users_and_preserves_existing_accounts() {
    let fixture = Fixture::new().await;
    let mut config = fixture.state.server_config.clone();
    config.require_existing_db = true;
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(config.data_dir.join("management.sqlite"))
                .create_if_missing(true),
        )
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    assert!(db::open(&config).await.is_err());
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    sqlx::query("INSERT INTO users(id, username, password_hash, role, must_change_password) VALUES('retained', 'retained', 'existing-hash', 'admin', 0)").execute(&pool).await.unwrap();
    pool.close().await;
    let reopened = db::open(&config).await.unwrap();
    let account: (String, String, bool) =
        sqlx::query_as("SELECT username, password_hash, must_change_password FROM users")
            .fetch_one(&reopened)
            .await
            .unwrap();
    assert_eq!(account, ("retained".into(), "existing-hash".into(), false));
    reopened.close().await;
}

struct Fixture {
    root: PathBuf,
    state: Arc<AppState>,
    app: Router,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
impl Fixture {
    async fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("Cache/tests/multiuser")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(root.join("staging")).unwrap();
        std::fs::create_dir_all(root.join("resources")).unwrap();
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let hash = auth::hash_password("initial-password".into())
            .await
            .unwrap();
        for (id, role) in [("admin", "admin"), ("alice", "user"), ("bob", "user")] {
            sqlx::query("INSERT INTO users(id, username, password_hash, role, must_change_password) VALUES(?, ?, ?, ?, 0)").bind(id).bind(id).bind(&hash).bind(role).execute(&pool).await.unwrap();
        }
        let config = ServerConfig {
            data_dir: root.clone(),
            port: 0,
            tls_cert: None,
            tls_key: None,
            https_port: 0,
            cookie_secure: false,
            session_hours: 24,
            require_existing_db: false,
        };
        let state = Arc::new(AppState::new(config, pool).await);
        let app = api::build_router(state.clone(), Protocol::Http);
        Self { root, state, app }
    }
    async fn call(
        &self,
        method: &str,
        path: &str,
        token: &str,
        body: Option<Value>,
        revision: Option<i64>,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(path);
        if !token.is_empty() {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        if let Some(rev) = revision {
            req = req.header("If-Match", rev.to_string());
        }
        req = req.header("Content-Type", "application/json");
        let res = self
            .app
            .clone()
            .oneshot(
                req.body(Body::from(body.map(|b| b.to_string()).unwrap_or_default()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = res.status();
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
    async fn login(&self, name: &str) -> String {
        let (status, data) = self
            .call(
                "POST",
                "/api/auth/login",
                "",
                Some(json!({"username": name,"password":"initial-password","client":"desktop"})),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{data}");
        data["token"].as_str().unwrap().into()
    }
    async fn project(&self, token: &str) -> Value {
        let (s,p)=self.call("POST","/api/projects",token,Some(json!({"project_name":format!("测试项目-{}", uuid::Uuid::new_v4()),"project_version":"v1","package_name":"DefaultPackage","platforms":["Android"]})),None).await;
        assert_eq!(s, StatusCode::OK, "{p}");
        p
    }
    async fn upload(
        &self,
        token: &str,
        id: &str,
        version: &str,
        name: &str,
        contents: &str,
    ) -> StatusCode {
        let body = format!("--boundary\r\nContent-Disposition: form-data; name=\"platform\"\r\n\r\nAndroid\r\n--boundary\r\nContent-Disposition: form-data; name=\"version\"\r\n\r\n{version}\r\n--boundary\r\nContent-Disposition: form-data; name=\"files\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n{contents}\r\n--boundary--\r\n");
        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/projects/{id}/project-versions/v1/upload"))
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "multipart/form-data; boundary=boundary")
            .body(Body::from(body))
            .unwrap();
        self.app.clone().oneshot(req).await.unwrap().status()
    }
}
#[tokio::test]
async fn administrator_can_distinguish_shared_projects_from_system_access() {
    let f = Fixture::new().await;
    let alice = f.login("alice").await;
    let admin = f.login("admin").await;
    let p = f.project(&alice).await;
    let path = format!("/api/projects/{}", p["id"].as_str().unwrap());
    let list = f.call("GET", "/api/projects", &admin, None, None).await.1;
    assert_eq!(list[0]["can_manage"], true);
    assert_eq!(list[0]["shared_with_me"], false);
    assert_eq!(
        f.call(
            "POST",
            &format!("{path}/members"),
            &alice,
            Some(json!({"user_id":"admin"})),
            Some(1)
        )
        .await
        .0,
        StatusCode::OK
    );
    let list = f.call("GET", "/api/projects", &admin, None, None).await.1;
    assert_eq!(list[0]["shared_with_me"], true);
    assert_eq!(
        f.call("GET", &path, &admin, None, None).await.1["shared_with_me"],
        true
    );
}
#[tokio::test]
async fn sharing_enforces_ownership_and_revocation() {
    let f = Fixture::new().await;
    let alice = f.login("alice").await;
    let bob = f.login("bob").await;
    let p = f.project(&alice).await;
    let id = p["id"].as_str().unwrap();
    let path = format!("/api/projects/{id}");
    assert_eq!(
        f.call("GET", &path, &bob, None, None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        f.call("GET", "/api/projects", &bob, None, None).await.1,
        json!([])
    );
    assert_eq!(
        f.call(
            "POST",
            &format!("{path}/members"),
            &alice,
            Some(json!({"user_id":"bob"})),
            Some(1)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        f.call("GET", &path, &bob, None, None).await.0,
        StatusCode::OK
    );
    assert_eq!(
        f.call("DELETE", &path, &bob, None, Some(2)).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.call(
            "POST",
            &format!("{path}/members"),
            &bob,
            Some(json!({"user_id":"admin"})),
            Some(2)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.call(
            "DELETE",
            &format!("{path}/members/bob"),
            &alice,
            None,
            Some(1)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        f.call(
            "DELETE",
            &format!("{path}/members/bob"),
            &alice,
            None,
            Some(2)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        f.call("GET", &path, &bob, None, None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        f.call("GET", "/api/audit", &bob, None, None).await.1,
        json!([])
    );
    assert_eq!(
        f.call("GET", "/api/admin/users", &alice, None, None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.call("GET", "/api/settings/protocol-access", &bob, None, None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.call(
            "POST",
            &format!("{path}/members"),
            &alice,
            Some(json!({"user_id":"bob"})),
            Some(3)
        )
        .await
        .0,
        StatusCode::OK
    );
    let ticket = f
        .call("POST", "/api/auth/ws-ticket", &bob, None, None)
        .await
        .1["ticket"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(f
        .state
        .tickets
        .lock()
        .await
        .remove(&auth::digest(&ticket))
        .is_some());
    assert!(f
        .state
        .tickets
        .lock()
        .await
        .remove(&auth::digest(&ticket))
        .is_none());
}
#[tokio::test]
async fn uploads_publish_atomically_and_names_do_not_collide() {
    let f = Fixture::new().await;
    let alice = f.login("alice").await;
    let bob = f.login("bob").await;
    let p = f.project(&alice).await;
    let other = f.project(&bob).await;
    assert_ne!(p["id"], other["id"]);
    let id = p["id"].as_str().unwrap();
    let path = format!("/api/projects/{id}");
    assert_eq!(
        f.upload(&bob, id, "r1", "manifest.txt", "secret").await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        f.upload(&alice, id, "r1", "manifest.txt", "hello").await,
        StatusCode::OK
    );
    assert_eq!(
        f.upload(&alice, id, "r1", "manifest.txt", "overwrite")
            .await,
        StatusCode::CONFLICT
    );
    assert_eq!(
        f.upload(&alice, id, "r2", "../escape", "bad").await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        std::fs::read_dir(f.root.join("staging")).unwrap().count(),
        0
    );
    let public = format!("/res/{id}/v1/Android/manifest.txt");
    assert_eq!(
        f.call("GET", &public, "", None, None).await.0,
        StatusCode::NOT_FOUND
    );
    let release = format!("{path}/project-versions/v1/versions/r1/activate?platform=Android");
    assert_eq!(
        f.call("PUT", &release, &alice, Some(json!({})), Some(2))
            .await
            .0,
        StatusCode::OK
    );
    let res = f
        .app
        .clone()
        .oneshot(Request::builder().uri(&public).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(res.into_body(), 1024).await.unwrap().as_ref(),
        b"hello"
    );
    assert_eq!(
        f.call(
            "DELETE",
            &format!("{path}/project-versions/v1/versions/r1?platform=Android"),
            &alice,
            None,
            Some(3)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        f.call(
            "PUT",
            &format!("{path}/project-versions/v1/platforms/Android/access"),
            &alice,
            Some(json!({"enabled":false})),
            Some(3)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        f.call("GET", &public, "", None, None).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.call(
            "PUT",
            &release,
            &alice,
            Some(json!({"unpublish":true})),
            Some(4)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        f.call(
            "DELETE",
            &format!("{path}/project-versions/v1/versions/r1?platform=Android"),
            &alice,
            None,
            Some(5)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        f.call(
            "GET",
            &format!("/res/{id}/v1/Android/%2e%2e%2fother"),
            "",
            None,
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}
#[tokio::test]
async fn deleting_users_transfers_projects_transactionally_and_revokes_sessions() {
    let f = Fixture::new().await;
    let admin = f.login("admin").await;
    let alice = f.login("alice").await;
    let bob = f.login("bob").await;
    let p = f.project(&alice).await;
    let id = p["id"].as_str().unwrap();
    assert_eq!(
        f.call(
            "DELETE",
            "/api/admin/users/alice",
            &admin,
            Some(json!({"transfers":{}})),
            None
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        f.call("GET", "/api/auth/me", &alice, None, None).await.0,
        StatusCode::OK
    );
    assert_eq!(
        f.call(
            "DELETE",
            "/api/admin/users/alice",
            &admin,
            Some(json!({"transfers":{id:"bob"}})),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        f.call("GET", "/api/auth/me", &alice, None, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        f.call("GET", &format!("/api/projects/{id}"), &bob, None, None)
            .await
            .1["owner_id"],
        "bob"
    );
    assert_eq!(
        f.call(
            "PUT",
            "/api/admin/users/admin",
            &admin,
            Some(json!({"role":"user","enabled":true})),
            None
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        f.call(
            "PUT",
            "/api/admin/users/bob",
            &admin,
            Some(json!({"role":"user","enabled":false})),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        f.call("GET", "/api/auth/me", &bob, None, None).await.0,
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn web_cookie_requires_csrf_and_first_login_requires_password_change() {
    let f = Fixture::new().await;
    sqlx::query("UPDATE users SET must_change_password=1 WHERE id='alice'")
        .execute(&f.state.db)
        .await
        .unwrap();
    let request = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({"username":"alice","password":"initial-password","client":"web"}).to_string(),
        ))
        .unwrap();
    let response = f.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()["Set-Cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let data: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert!(data.get("token").is_none());
    let make = |path: &str, csrf: bool| {
        let mut b = Request::builder()
            .method("PUT")
            .uri(path)
            .header("Cookie", &cookie)
            .header("Content-Type", "application/json");
        if csrf {
            b = b.header("X-CSRF-Token", data["csrf"].as_str().unwrap());
        }
        b.body(Body::from(
            json!({"current_password":"initial-password","password":"changed-password"})
                .to_string(),
        ))
        .unwrap()
    };
    assert_eq!(
        f.app
            .clone()
            .oneshot(make("/api/auth/password", false))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/projects")
                    .header("Cookie", &cookie)
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.app
            .clone()
            .oneshot(make("/api/auth/password", true))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        f.app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/auth/me")
                    .header("Cookie", &cookie)
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn revoked_member_cannot_commit_an_upload_in_flight() {
    let f = Fixture::new().await;
    let alice = f.login("alice").await;
    let bob = f.login("bob").await;
    let p = f.project(&alice).await;
    let id = p["id"].as_str().unwrap();
    let path = format!("/api/projects/{id}");
    f.call(
        "POST",
        &format!("{path}/members"),
        &alice,
        Some(json!({"user_id":"bob"})),
        Some(1),
    )
    .await;
    let (sender, receiver) =
        tokio::sync::mpsc::channel::<Result<axum::body::Bytes, std::io::Error>>(2);
    let req = Request::builder()
        .method("POST")
        .uri(format!("{path}/project-versions/v1/upload"))
        .header("Authorization", format!("Bearer {bob}"))
        .header("Content-Type", "multipart/form-data; boundary=boundary")
        .body(Body::from_stream(
            tokio_stream::wrappers::ReceiverStream::new(receiver),
        ))
        .unwrap();
    let pending = tokio::spawn(f.app.clone().oneshot(req));
    sender
        .send(Ok(axum::body::Bytes::from_static(
            b"--boundary\r\nContent-Disposition: form-data; name=\"platform\"\r\n\r\nAndroid\r\n",
        )))
        .await
        .unwrap();
    for _ in 0..100 {
        if std::fs::read_dir(f.root.join("staging")).unwrap().count() > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(
        std::fs::read_dir(f.root.join("staging")).unwrap().count(),
        1
    );
    assert_eq!(
        f.call(
            "DELETE",
            &format!("{path}/members/bob"),
            &alice,
            None,
            Some(2)
        )
        .await
        .0,
        StatusCode::OK
    );
    sender.send(Ok(axum::body::Bytes::from_static(b"--boundary\r\nContent-Disposition: form-data; name=\"version\"\r\n\r\nr1\r\n--boundary\r\nContent-Disposition: form-data; name=\"files\"; filename=\"manifest.txt\"\r\n\r\ncontent\r\n--boundary--\r\n"))).await.unwrap();
    drop(sender);
    assert_eq!(
        pending.await.unwrap().unwrap().status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        std::fs::read_dir(f.root.join("staging")).unwrap().count(),
        0
    );
    assert!(!f.root.join("resources").join(id).exists());
}
#[tokio::test]
async fn failed_transfer_rolls_back_all_projects_and_password_reset_revokes() {
    let f = Fixture::new().await;
    let admin = f.login("admin").await;
    let alice = f.login("alice").await;
    let first = f.project(&alice).await;
    let second = f.project(&alice).await;
    let a = first["id"].as_str().unwrap();
    let b = second["id"].as_str().unwrap();
    assert_eq!(
        f.call(
            "DELETE",
            "/api/admin/users/alice",
            &admin,
            Some(json!({"transfers":{a:"bob",b:"missing"}})),
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    for id in [a, b] {
        assert_eq!(
            f.call("GET", &format!("/api/projects/{id}"), &alice, None, None)
                .await
                .1["owner_id"],
            "alice"
        );
    }
    assert_eq!(
        f.call(
            "PUT",
            "/api/admin/users/alice/password",
            &admin,
            Some(json!({"password":"new-initial-password"})),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        f.call("GET", "/api/auth/me", &alice, None, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, data) = f
        .call(
            "POST",
            "/api/auth/login",
            "",
            Some(json!({"username":"alice","password":"new-initial-password","client":"desktop"})),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(data["user"]["must_change_password"], true);
    assert_eq!(
        f.call(
            "GET",
            "/api/projects",
            data["token"].as_str().unwrap(),
            None,
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}
#[cfg(unix)]
#[tokio::test]
async fn symlinks_cannot_escape_through_download_or_manifest() {
    let f = Fixture::new().await;
    let alice = f.login("alice").await;
    let p = f.project(&alice).await;
    let id = p["id"].as_str().unwrap();
    assert_eq!(
        f.upload(&alice, id, "r1", "normal.txt", "safe").await,
        StatusCode::OK
    );
    let outside = f.root.join("outside.txt");
    std::fs::write(&outside, "private").unwrap();
    let dir = f
        .root
        .join("resources")
        .join(id)
        .join("v1/Android/_versions/r1");
    std::os::unix::fs::symlink(&outside, dir.join("linked.txt")).unwrap();
    let (status, files) = f
        .call(
            "GET",
            &format!("/api/projects/{id}/project-versions/v1/manifest?platform=Android&version=r1"),
            &alice,
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(files.as_array().unwrap().len(), 1);
    f.call(
        "PUT",
        &format!("/api/projects/{id}/project-versions/v1/versions/r1/activate?platform=Android"),
        &alice,
        Some(json!({})),
        Some(2),
    )
    .await;
    assert_eq!(
        f.call(
            "GET",
            &format!("/res/{id}/v1/Android/linked.txt"),
            "",
            None,
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let response = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/res/{id}/v1/Android/normal.txt"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.headers()["Content-Security-Policy"],
        "sandbox; default-src 'none'"
    );
}

#[tokio::test]
async fn named_downloads_and_resource_logs_remain_isolated() {
    let f = Fixture::new().await;
    let alice = f.login("alice").await;
    let bob = f.login("bob").await;
    let admin = f.login("admin").await;
    let input = json!({"project_name":"RollCall","project_version":"v1","package_name":"DefaultPackage","platforms":["Android"]});
    let (status, p) = f.call("POST", "/api/projects", &alice, Some(input.clone()), None).await;
    assert_eq!(status, StatusCode::OK);
    let id = p["id"].as_str().unwrap();
    assert_eq!(f.call("POST", "/api/projects", &bob, Some(input.clone()), None).await.0, StatusCode::CONFLICT);
    assert_eq!(f.upload(&alice, id, "r1", "manifest.txt", "resource-content").await, StatusCode::OK);
    assert_eq!(f.call("PUT", &format!("/api/projects/{id}/project-versions/v1/versions/r1/activate?platform=Android"), &alice, Some(json!({})), Some(2)).await.0, StatusCode::OK);
    for path in ["/res/v1/RollCall/Android/manifest.txt".to_string(), format!("/res/{id}/v1/Android/manifest.txt")] {
        let response = f.app.clone().oneshot(Request::builder().uri(path).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(to_bytes(response.into_body(), usize::MAX).await.unwrap(), "resource-content");
    }
    assert_eq!(f.call("HEAD", "/res/v1/RollCall/Android/manifest.txt", "", None, None).await.0, StatusCode::OK);
    assert_eq!(f.call("GET", "/res/v1/RollCall/Android/missing.hash?token=not-for-logs", "", None, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(f.call("GET", "/res/v1/RollCall/Android/%2e%2e%2fprivate", "", None, None).await.0, StatusCode::NOT_FOUND);
    let owner_logs = f.call("GET", "/api/resource-logs", &alice, None, None).await.1;
    assert!(owner_logs.as_array().unwrap().iter().any(|l| l["status"] == 200 && l["outcome"] == "completed"));
    assert!(owner_logs.as_array().unwrap().iter().any(|l| l["status"] == 404));
    assert!(!owner_logs.to_string().contains("not-for-logs"));
    assert_eq!(f.call("GET", "/api/resource-logs", &bob, None, None).await.1, json!([]));
    assert_eq!(f.call("GET", "/api/resource-logs", "", None, None).await.0, StatusCode::UNAUTHORIZED);
    let access = format!("/api/projects/{id}/project-versions/v1/platforms/Android/access");
    assert_eq!(f.call("PUT", &access, &alice, Some(json!({"enabled":false})), Some(3)).await.0, StatusCode::OK);
    assert_eq!(f.call("GET", "/res/v1/RollCall/Android/manifest.txt", "", None, None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(f.call("POST", &format!("/api/projects/{id}/members"), &alice, Some(json!({"user_id":"bob"})), Some(4)).await.0, StatusCode::OK);
    assert!(!f.call("GET", "/api/resource-logs", &bob, None, None).await.1.as_array().unwrap().is_empty());
    let event = f.state.resource_history.lock().unwrap().back().unwrap().clone();
    let bob_auth = auth::from_hash(&f.state, &auth::digest(&bob)).await.unwrap();
    assert!(resource_logs::allowed(&f.state, &bob_auth, &event).await);
    assert_eq!(f.call("DELETE", &format!("/api/projects/{id}/members/bob"), &alice, None, Some(5)).await.0, StatusCode::OK);
    assert!(!resource_logs::allowed(&f.state, &bob_auth, &event).await);
    assert_eq!(f.call("GET", "/api/resource-logs", &bob, None, None).await.1, json!([]));
    f.call("GET", "/res/unknown", "", None, None).await;
    let admin_logs = f.call("GET", "/api/resource-logs", &admin, None, None).await.1;
    assert!(admin_logs.as_array().unwrap().iter().any(|l| l["project_id"].is_null()));
    assert!(f.call("GET", "/api/resource-logs", &alice, None, None).await.1.as_array().unwrap().iter().all(|l| l["project_id"] == id));
    let mut renamed = input.clone(); renamed["project_name"] = json!("RollCallNew");
    assert_eq!(f.call("PUT", &format!("/api/projects/{id}"), &alice, Some(renamed), Some(6)).await.0, StatusCode::OK);
    assert_eq!(f.call("GET", "/res/v1/RollCall/Android/manifest.txt", "", None, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(f.call("GET", "/res/v1/RollCallNew/Android/manifest.txt", "", None, None).await.0, StatusCode::FORBIDDEN);
    // Legacy duplicates are retained on upgrade; a name can never pick a random owner.
    let mut duplicate = db::project(&f.state.db, id).await.unwrap(); duplicate.id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO projects(id, owner_id, config) VALUES(?, 'bob', ?)").bind(&duplicate.id).bind(serde_json::to_string(&duplicate).unwrap()).execute(&f.state.db).await.unwrap();
    assert_eq!(f.call("GET", "/res/v1/RollCallNew/Android/manifest.txt", "", None, None).await.0, StatusCode::CONFLICT);
    assert_eq!(f.call("GET", &format!("/res/{id}/v1/Android/manifest.txt"), "", None, None).await.0, StatusCode::FORBIDDEN);
    sqlx::query("UPDATE projects SET config = json_set(config, '$.project_name', 'v1')").execute(&f.state.db).await.unwrap();
    assert_eq!(f.call("GET", "/res/v1/v1/Android/manifest.txt", "", None, None).await.0, StatusCode::CONFLICT);
    assert_eq!(f.call("GET", &format!("/res/{id}/v1/Android/manifest.txt"), "", None, None).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn request_logs_report_aborted_and_truncated_streams_and_are_bounded() {
    let f = Fixture::new().await;
    let alice = f.login("alice").await;
    let p = f.project(&alice).await;
    let id = p["id"].as_str().unwrap();
    assert_eq!(f.upload(&alice, id, "r1", "file.bin", "payload").await, StatusCode::OK);
    f.call("PUT", &format!("/api/projects/{id}/project-versions/v1/versions/r1/activate?platform=Android"), &alice, Some(json!({})), Some(2)).await;
    let path = format!("/res/{id}/v1/Android/file.bin");
    let response = f.app.clone().oneshot(Request::builder().uri(&path).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    drop(response);
    assert_eq!(f.state.resource_history.lock().unwrap().back().unwrap().outcome, "interrupted");
    let response = f.app.clone().oneshot(Request::builder().uri(&path).body(Body::empty()).unwrap()).await.unwrap();
    std::fs::write(f.root.join("resources").join(id).join("v1/Android/_versions/r1/file.bin"), "").unwrap();
    let _ = to_bytes(response.into_body(), usize::MAX).await;
    let log = f.state.resource_history.lock().unwrap().back().unwrap().clone();
    assert_eq!(log.status, 200); // Headers were already sent; do not fabricate HTTP 500.
    assert_eq!(log.outcome, "failed");
    for _ in 0..510 { resource_logs::publish(&f.state, resource_logs::ResourceLog::new("GET", "/res/invalid"), 404, "failed", "无效请求"); }
    assert_eq!(f.state.resource_history.lock().unwrap().len(), 500);
}
