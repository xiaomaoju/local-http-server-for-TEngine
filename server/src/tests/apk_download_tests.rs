use super::*;
use axum::body::HttpBody;
use std::pin::Pin;

impl Fixture {
    async fn apk_stats(&self, token: &str, id: &str, version: &str) -> Value {
        let (status, stats) = self.call("GET", &format!("/api/projects/{id}/project-versions/{version}/download-stats"), token, None, None).await;
        assert_eq!(status, StatusCode::OK, "{stats}");
        stats
    }
    async fn apk_release(&self, token: &str, id: &str, release: &str, content: &str) {
        assert_eq!(self.upload_many(token, id, "Android", release, &[("catalog.json", "{}"), ("app.apk", content)]).await, StatusCode::OK);
        assert_eq!(self.publish(token, id, "Android", release, false).await, StatusCode::OK);
    }
    async fn edit_current(&self, token: &str, id: &str, method: &str, suffix: &str, body: Option<Value>) -> (StatusCode, Value) {
        let (_, project) = self.call("GET", &format!("/api/projects/{id}"), token, None, None).await;
        self.call(method, &format!("/api/projects/{id}{suffix}"), token, body, project["revision"].as_i64()).await
    }
}

#[tokio::test]
async fn apk_counts_are_per_release_and_project_with_atomic_concurrent_increments() {
    let f = Fixture::new().await;
    let alice = f.login("alice").await;
    let project = f.typed_project(&alice, "Counts", "apk").await;
    let id = project["id"].as_str().unwrap();
    f.apk_release(&alice, id, "69", "older").await;
    f.apk_release(&alice, id, "70", "newer").await;
    let other = f.typed_project(&alice, "Other", "apk").await;
    f.apk_release(&alice, other["id"].as_str().unwrap(), "69", "other").await;
    let revision = db::project(&f.state.db, id).await.unwrap().revision;
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..24 {
        let app = f.app.clone();
        tasks.spawn(async move {
            let res = app.oneshot(Request::builder().uri("/appversion/v1/Counts/releases/69/app.apk").body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(res.status(), StatusCode::OK);
            assert_eq!(to_bytes(res.into_body(), 4096).await.unwrap().as_ref(), b"older");
        });
    }
    while let Some(result) = tasks.join_next().await { result.unwrap(); }
    assert_eq!(f.download("GET", "/appversion/v1/Counts/releases/70/app.apk").await.0, StatusCode::OK);
    assert_eq!(f.apk_stats(&alice, id, "v1").await, json!({"total":25,"releases":[{"release":"70","downloads":1},{"release":"69","downloads":24}]}));
    assert_eq!(f.apk_stats(&alice, other["id"].as_str().unwrap(), "v1").await["total"], 0);
    assert_eq!(db::project(&f.state.db, id).await.unwrap().revision, revision);
}

#[tokio::test]
async fn apk_counts_exclude_non_apks_failed_and_interrupted_reads() {
    let f = Fixture::new().await;
    let alice = f.login("alice").await;
    let project = f.typed_project(&alice, "Counts", "apk").await;
    let id = project["id"].as_str().unwrap();
    f.apk_release(&alice, id, "69", &"a".repeat(100_000)).await;
    let path = "/appversion/v1/Counts/releases/69/app.apk";
    assert_eq!(f.download("HEAD", path).await.0, StatusCode::OK);
    assert_eq!(f.download("GET", "/appversion/v1/Counts/catalog.json").await.0, StatusCode::OK);
    assert_eq!(f.download("GET", "/appversion/v1/Counts/releases/70/app.apk").await.0, StatusCode::NOT_FOUND);
    assert_eq!(f.download("POST", path).await.0, StatusCode::METHOD_NOT_ALLOWED);
    let revision = db::project(&f.state.db, id).await.unwrap().revision;
    assert_eq!(f.attachment_upload(&alice, id, revision, &[("guide.html", "hello")], true).await.0, StatusCode::OK);
    assert_eq!(f.download("GET", "/appversion/v1/Counts/docs/guide.html").await.0, StatusCode::OK);
    // Drop once before reading and once after consuming a non-final chunk.
    let response = f.app.clone().oneshot(Request::builder().uri(path).body(Body::empty()).unwrap()).await.unwrap();
    drop(response);
    let response = f.app.clone().oneshot(Request::builder().uri(path).body(Body::empty()).unwrap()).await.unwrap();
    let mut body = response.into_body();
    let frame = std::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await.unwrap().unwrap();
    assert!(frame.data_ref().unwrap().len() < 100_000);
    drop(body);
    // Headers describe the original file, but it becomes truncated before reading.
    let response = f.app.clone().oneshot(Request::builder().uri(path).body(Body::empty()).unwrap()).await.unwrap();
    std::fs::write(f.root.join("resources").join(id).join("v1/Android/_versions/69/app.apk"), "short").unwrap();
    let _ = to_bytes(response.into_body(), usize::MAX).await;
    assert_eq!(f.apk_stats(&alice, id, "v1").await["total"], 0);
    assert_eq!(f.edit_current(&alice, id, "PUT", "/project-versions/v1/platforms/Android/access", Some(json!({"enabled":false}))).await.0, StatusCode::OK);
    assert_eq!(f.download("GET", path).await.0, StatusCode::FORBIDDEN);
    assert_eq!(f.apk_stats(&alice, id, "v1").await["total"], 0);
}

#[tokio::test]
async fn apk_counts_survive_rename_and_release_delete_but_not_scope_recreation() {
    let f = Fixture::new().await;
    let alice = f.login("alice").await;
    let project = f.typed_project(&alice, "Counts", "apk").await;
    let id = project["id"].as_str().unwrap();
    f.apk_release(&alice, id, "69", "payload").await;
    let scope = apk_downloads::scope(&f.state.db, id, "v1").await.unwrap();
    let response = f.app.clone().oneshot(Request::builder().uri("/appversion/v1/Counts/releases/69/app.apk").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(f.edit_current(&alice, id, "PUT", "/project-versions/v1", Some(json!({"name":"v2"}))).await.0, StatusCode::OK);
    assert_eq!(to_bytes(response.into_body(), 4096).await.unwrap().as_ref(), b"payload");
    assert_eq!(apk_downloads::scope(&f.state.db, id, "v2").await.unwrap(), scope);
    assert_eq!(f.apk_stats(&alice, id, "v2").await["total"], 1);
    // The same release number under a newly created project version starts at zero.
    assert_eq!(f.edit_current(&alice, id, "POST", "/project-versions", Some(json!({"name":"v1"}))).await.0, StatusCode::OK);
    f.apk_release(&alice, id, "69", "new scope").await;
    assert_eq!(f.apk_stats(&alice, id, "v1").await["total"], 0);
    assert_eq!(f.edit_current(&alice, id, "PUT", "", Some(json!({"project_name":"Renamed","platforms":["Android"]}))).await.0, StatusCode::OK);
    assert_eq!(f.download("GET", "/appversion/v2/Renamed/releases/69/app.apk").await.0, StatusCode::OK);
    assert_eq!(f.apk_stats(&alice, id, "v2").await["total"], 2);
    assert_eq!(f.edit_current(&alice, id, "PUT", "/project-versions/v2/versions/69/activate?platform=Android", Some(json!({"unpublish":true}))).await.0, StatusCode::OK);
    // Hold an old response across deletion and same-name recreation.
    let response = f.app.clone().oneshot(Request::builder().uri("/appversion/v2/Renamed/releases/69/app.apk").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(f.edit_current(&alice, id, "DELETE", "/project-versions/v2/versions/69?platform=Android", None).await.0, StatusCode::OK);
    assert_eq!(f.apk_stats(&alice, id, "v2").await["total"], 2);
    assert_eq!(f.edit_current(&alice, id, "DELETE", "/project-versions/v2", None).await.0, StatusCode::OK);
    assert_eq!(f.edit_current(&alice, id, "POST", "/project-versions", Some(json!({"name":"v2"}))).await.0, StatusCode::OK);
    assert_ne!(apk_downloads::scope(&f.state.db, id, "v2").await.unwrap(), scope);
    let _ = to_bytes(response.into_body(), 4096).await.unwrap();
    assert_eq!(f.apk_stats(&alice, id, "v2").await["total"], 0);
    assert_eq!(f.edit_current(&alice, id, "DELETE", "", None).await.0, StatusCode::OK);
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM apk_download_scopes").fetch_one(&f.state.db).await.unwrap();
    assert_eq!(rows, 0);
}

#[tokio::test]
async fn apk_stats_respect_session_and_project_membership() {
    let f = Fixture::new().await;
    let alice = f.login("alice").await;
    let bob = f.login("bob").await;
    let admin = f.login("admin").await;
    let p = f.typed_project(&alice, "Counts", "apk").await;
    let id = p["id"].as_str().unwrap();
    let path = format!("/api/projects/{id}/project-versions/v1/download-stats");
    assert_eq!(f.call("GET", &path, "", None, None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(f.call("GET", &path, &bob, None, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(f.call("GET", &path, &admin, None, None).await.0, StatusCode::OK);
    sqlx::query("INSERT INTO project_members(project_id, user_id) VALUES(?, 'bob')").bind(id).execute(&f.state.db).await.unwrap();
    assert_eq!(f.call("GET", &path, &bob, None, None).await.0, StatusCode::OK);
    sqlx::query("DELETE FROM project_members WHERE project_id = ?").bind(id).execute(&f.state.db).await.unwrap();
    assert_eq!(f.call("GET", &path, &bob, None, None).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn apk_stats_upgrade_existing_projects_and_persist_across_restart() {
    let f = Fixture::new().await;
    let mut config = f.state.server_config.clone();
    let pool = sqlx::sqlite::SqlitePoolOptions::new().max_connections(1).connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(config.data_dir.join("management.sqlite")).create_if_missing(true).foreign_keys(true),
    ).await.unwrap();
    let mut previous = sqlx::migrate!("./migrations");
    previous.migrations.to_mut().retain(|m| m.version < 4);
    previous.run(&pool).await.unwrap();
    sqlx::query("INSERT INTO users(id, username, password_hash, role, must_change_password) VALUES('owner', 'owner', 'preserved', 'admin', 0)").execute(&pool).await.unwrap();
    let project = json!({"id":"existing","project_name":"Existing","project_type":"apk","owner_id":"owner","project_versions":[{"name":"v1","platform_settings":{"Android":{"access_enabled":true,"active_bundle":"69","published_releases":["69"]}}}],"platforms":["Android"],"package_name":"","revision":1});
    sqlx::query("INSERT INTO projects(id, owner_id, config) VALUES('existing', 'owner', ?)").bind(project.to_string()).execute(&pool).await.unwrap();
    pool.close().await;
    config.require_existing_db = true;
    let upgraded = db::open(&config).await.unwrap();
    let scope = apk_downloads::scope(&upgraded, "existing", "v1").await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM apk_download_counts").fetch_one(&upgraded).await.unwrap();
    assert_eq!(count, 0); // Existing published releases do not backfill downloads.
    apk_downloads::increment(upgraded.clone(), scope.clone(), "69".into()).await;
    upgraded.close().await;
    let reopened = db::open(&config).await.unwrap();
    assert_eq!(apk_downloads::scope(&reopened, "existing", "v1").await.unwrap(), scope);
    let count: i64 = sqlx::query_scalar("SELECT downloads FROM apk_download_counts WHERE scope_id = ? AND release = '69'").bind(scope).fetch_one(&reopened).await.unwrap();
    assert_eq!(count, 1);
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = 'owner'").fetch_one(&reopened).await.unwrap();
    assert_eq!(hash, "preserved");
    assert_eq!(db::project(&reopened, "existing").await.unwrap().revision, 1);
    reopened.close().await;
}

#[tokio::test]
async fn apk_count_write_failure_does_not_break_delivery_and_rename_failure_rolls_back() {
    let f = Fixture::new().await;
    let alice = f.login("alice").await;
    let p = f.typed_project(&alice, "Counts", "apk").await;
    let id = p["id"].as_str().unwrap();
    f.apk_release(&alice, id, "69", "payload").await;
    let scope = apk_downloads::scope(&f.state.db, id, "v1").await.unwrap();
    sqlx::query("CREATE TRIGGER reject_count BEFORE INSERT ON apk_download_counts BEGIN SELECT RAISE(FAIL, 'test write failure'); END").execute(&f.state.db).await.unwrap();
    assert_eq!(f.download("GET", "/appversion/v1/Counts/releases/69/app.apk").await.2, b"payload");
    assert_eq!(f.apk_stats(&alice, id, "v1").await["total"], 0);
    sqlx::query("DROP TRIGGER reject_count").execute(&f.state.db).await.unwrap();
    assert_eq!(f.download("GET", "/appversion/v1/Counts/releases/69/app.apk").await.2, b"payload");
    let mut stale = db::project(&f.state.db, id).await.unwrap();
    stale.revision -= 1;
    stale.project_versions[0].name = "v2".into();
    assert!(db::save_renamed_version(&f.state.db, &mut stale, "v1", "v2").await.is_err());
    assert_eq!(apk_downloads::scope(&f.state.db, id, "v1").await.unwrap(), scope);
    assert!(apk_downloads::scope(&f.state.db, id, "v2").await.is_err());
    assert_eq!(f.apk_stats(&alice, id, "v1").await["total"], 1);
}
