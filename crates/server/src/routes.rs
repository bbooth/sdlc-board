use crate::sources::State;
use axum::{extract::State as S, http::header, response::IntoResponse, routing::get, Json, Router};
use std::sync::Arc;

pub fn router(state: Arc<State>) -> Router {
    Router::new()
        .route("/v1/snapshot", get(snapshot))
        .route("/metrics", get(metrics))
        .route("/healthz", get(|| async { "ok" }))
        .with_state(state)
}

async fn snapshot(S(state): S<Arc<State>>) -> impl IntoResponse {
    Json(state.snapshot())
}

async fn metrics(S(state): S<Arc<State>>) -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], state.metrics.render(&state.ages()))
}
