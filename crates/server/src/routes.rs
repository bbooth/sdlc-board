use crate::sources::State;
use axum::extract::{MatchedPath, Request, State as S};
use axum::middleware::{from_fn_with_state, Next};
use axum::{http::header, response::IntoResponse, routing::get, Json, Router};
use std::sync::Arc;
use std::time::Instant;

pub fn router(state: Arc<State>) -> Router {
    Router::new()
        .route("/v1/snapshot", get(snapshot))
        .route("/metrics", get(metrics))
        .route("/healthz", get(|| async { "ok" }))
        .layer(from_fn_with_state(state.clone(), observe))
        .with_state(state)
}

async fn snapshot(S(state): S<Arc<State>>) -> impl IntoResponse {
    Json(state.snapshot())
}

async fn metrics(S(state): S<Arc<State>>) -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], state.metrics.render(&state.ages()))
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
