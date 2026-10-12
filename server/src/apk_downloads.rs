use crate::{auth::{self, Auth}, db::{self, ApiError, ApiResult}, resources::ProjectType, AppState};
use axum::{extract::{Extension, Path, State}, Json};
use serde::Serialize;
use sqlx::SqlitePool;
use std::{future::Future, pin::Pin, sync::Arc, task::{Context, Poll}};
use tokio::io::{AsyncRead, ReadBuf};

#[derive(Serialize, sqlx::FromRow)]
pub struct ReleaseCount {
    release: String,
    downloads: i64,
}

#[derive(Serialize)]
pub struct DownloadStats {
    total: i64,
    releases: Vec<ReleaseCount>,
}

pub async fn stats(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path((id, pver)): Path<(String, String)>,
) -> ApiResult<Json<DownloadStats>> {
    let _guard = state.mutation.lock().await;
    let user = auth::from_hash(&state, &auth.hash).await?;
    let (_, project) = db::authorized(&state, &user.user.id, &id, false).await?;
    if project.project_type != ProjectType::Apk || !project.project_versions.iter().any(|v| v.name == pver) {
        return Err(ApiError::not_found("APP 项目版本不存在"));
    }
    let releases: Vec<ReleaseCount> = sqlx::query_as(
        "SELECT c.release, c.downloads FROM apk_download_counts c JOIN apk_download_scopes s ON s.id = c.scope_id WHERE s.project_id = ? AND s.project_version = ? ORDER BY length(c.release) DESC, c.release DESC",
    ).bind(id).bind(pver).fetch_all(&state.db).await?;
    Ok(Json(DownloadStats { total: releases.iter().map(|r| r.downloads).sum(), releases }))
}

pub async fn scope(pool: &SqlitePool, project: &str, version: &str) -> Result<String, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM apk_download_scopes WHERE project_id = ? AND project_version = ?")
        .bind(project).bind(version).fetch_one(pool).await
}

pub async fn increment(pool: SqlitePool, scope: String, release: String) {
    // A deleted scope must never be recreated by an in-flight response.
    let result = sqlx::query("INSERT INTO apk_download_counts(scope_id, release, downloads) SELECT id, ?, 1 FROM apk_download_scopes WHERE id = ? ON CONFLICT(scope_id, release) DO UPDATE SET downloads = downloads + 1")
        .bind(&release).bind(&scope).execute(&pool).await;
    if let Err(error) = result {
        tracing::error!(%error, %scope, %release, "APK 下载计数保存失败");
    }
}

/// Retain only the final read chunk until its counter is persisted. No detached
/// tasks or unbounded event queue; an unsuccessful count does not fail delivery.
pub struct CountedReader<R> {
    reader: R,
    remaining: u64,
    count: Option<(SqlitePool, String, String)>,
    pending: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
    tail: Vec<u8>,
    tail_offset: usize,
}

impl<R> CountedReader<R> {
    pub fn new(reader: R, size: u64, pool: SqlitePool, scope: String, release: String) -> Self {
        Self { reader, remaining: size, count: Some((pool, scope, release)), pending: None, tail: Vec::new(), tail_offset: 0 }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for CountedReader<R> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        if buf.remaining() == 0 { return Poll::Ready(Ok(())); }
        if let Some(pending) = &mut this.pending {
            if pending.as_mut().poll(cx).is_pending() { return Poll::Pending; }
            this.pending = None;
        }
        if this.tail_offset < this.tail.len() {
            let end = (this.tail_offset + buf.remaining()).min(this.tail.len());
            buf.put_slice(&this.tail[this.tail_offset..end]);
            this.tail_offset = end;
            return Poll::Ready(Ok(()));
        }
        if this.remaining == 0 { return Poll::Ready(Ok(())); }
        let before = buf.filled().len();
        match Pin::new(&mut this.reader).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                let read = buf.filled().len() - before;
                this.remaining = this.remaining.saturating_sub(read as u64);
                if this.remaining == 0 {
                    this.tail.extend_from_slice(&buf.filled()[before..]);
                    buf.set_filled(before);
                    if let Some((pool, scope, release)) = this.count.take() {
                        this.pending = Some(Box::pin(increment(pool, scope, release)));
                    }
                    Pin::new(this).poll_read(cx, buf)
                } else {
                    Poll::Ready(Ok(()))
                }
            }
            result => result,
        }
    }
}
