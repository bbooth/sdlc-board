use axum::{body::Body, http::Request};
use board_server::{board::Issue, routes, sources};
use http_body_util::BodyExt;
use tower::ServiceExt;

struct Fake(&'static str, bool);

impl sources::Source for Fake {
    fn name(&self) -> &'static str {
        self.0
    }
    async fn read(&self) -> anyhow::Result<Vec<Issue>> {
        if self.1 {
            Ok(vec![])
        } else {
            anyhow::bail!("unreachable")
        }
    }
}

async fn get(state: std::sync::Arc<sources::State>, path: &str) -> String {
    let resp = routes::router(state).oneshot(Request::get(path).body(Body::empty()).unwrap()).await.unwrap();
    assert!(resp.status().is_success());
    String::from_utf8(resp.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap()
}

fn value(text: &str, metric: &str, source: &str) -> f64 {
    let prefix = format!("{metric}{{source=\"{source}\"}} ");
    let line = text.lines().find(|l| l.starts_with(&prefix)).unwrap_or_else(|| panic!("no {prefix} in\n{text}"));
    line[prefix.len()..].parse().unwrap()
}

#[tokio::test]
async fn reads_errors_duration_and_age_are_exported_per_source() {
    let state = sources::State::new();
    sources::refresh(&state, &Fake("linear", true)).await;
    sources::refresh(&state, &Fake("github", false)).await;
    let text = get(state.clone(), "/metrics").await;

    assert_eq!(value(&text, "board_source_reads_total", "linear"), 1.0);
    assert_eq!(value(&text, "board_source_read_errors_total", "linear"), 0.0);
    assert_eq!(value(&text, "board_source_reads_total", "github"), 1.0);
    assert_eq!(value(&text, "board_source_read_errors_total", "github"), 1.0);
    for s in ["linear", "github"] {
        assert_eq!(value(&text, "board_source_read_duration_seconds_count", s), 1.0);
        assert!(value(&text, "board_source_age_seconds", s) >= 0.0);
    }
    assert!(value(&text, "board_source_age_seconds", "linear") < 5.0);

    // The snapshot reports the failure too.
    let snap: board_api::Snapshot = serde_json::from_str(&get(state, "/v1/snapshot").await).unwrap();
    let gh = snap.sources.iter().find(|s| s.name == "github").unwrap();
    assert!(gh.last_good.is_none() && gh.last_error.as_deref().unwrap().contains("unreachable"));
}
