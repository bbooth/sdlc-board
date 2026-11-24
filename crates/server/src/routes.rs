use crate::access::{self, Access};
use crate::sources::State;
use axum::extract::{MatchedPath, Request, State as S};
use axum::http::{header, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, Json, Router};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tower_http::services::ServeDir;

/// The public routes: the snapshot and the web build, behind Access when
/// `access` is set, and `/healthz`, which isn't.
pub fn router(state: Arc<State>, access: Option<Arc<Access>>, web: Option<PathBuf>) -> Router {
    let mut private = Router::new().route("/v1/snapshot", get(snapshot));
    if let Some(dir) = web {
        private = private.fallback_service(ServeDir::new(dir));
    }
    if let Some(access) = access {
        private = private.layer(from_fn_with_state(access, require_access));
    }
    private
        .route("/healthz", get(|| async { "ok" }))
        .layer(from_fn_with_state(state.clone(), observe))
        .with_state(state)
}

/// `/metrics`, served on its own port so Prometheus needn't pass Access and
/// the public address never serves it.
pub fn metrics_router(state: Arc<State>) -> Router {
    Router::new().route("/metrics", get(metrics)).with_state(state)
}

async fn snapshot(S(state): S<Arc<State>>) -> impl IntoResponse {
    Json(state.snapshot())
}

async fn metrics(S(state): S<Arc<State>>) -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], state.metrics.render(&state.ages()))
}

async fn require_access(S(access): S<Arc<Access>>, req: Request, next: Next) -> Response {
    let token = req.headers().get(access::HEADER).and_then(|v| v.to_str().ok());
    let verdict = match token {
        Some(t) => access.check(t).await,
        None => Err("no Access token".into()),
    };
    match verdict {
        Ok(()) => next.run(req).await,
        Err(why) => {
            tracing::info!(path = req.uri().path(), why, "refused a request without a valid Access token");
            (StatusCode::UNAUTHORIZED, "Log in through Cloudflare Access.\n").into_response()
        }
    }
}

/// Logs and counts every request by its route, not its raw path.
async fn observe(S(state): S<Arc<State>>, req: Request, next: Next) -> impl IntoResponse {
    let route = req.extensions().get::<MatchedPath>().map_or("unmatched", |m| m.as_str()).to_owned();
    let started = Instant::now();
    let resp = next.run(req).await;
    let (status, secs) = (resp.status().as_u16(), started.elapsed().as_secs_f64());
    state.metrics.request(&route, status, secs);
    tracing::info!(route, status, duration_s = secs, "request");
    resp
}
