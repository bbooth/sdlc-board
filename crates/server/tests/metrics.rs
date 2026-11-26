use axum::{body::Body, http::Request};
use board_server::{routes, sources};
use http_body_util::BodyExt;
use tower::ServiceExt;

struct Fake(&'static str, bool);

impl sources::Source for Fake {
    fn name(&self) -> &'static str {
        self.0
    }
    async fn read(&self) -> anyhow::Result<sources::Read> {
        if self.1 {
            Ok(sources::Read::default())
        } else {
            anyhow::bail!("unreachable")
        }
    }
}

async fn get(state: std::sync::Arc<sources::State>, path: &str) -> String {
    // /metrics has its own port; the rest is the public router.
    let app = if path == "/metrics" { routes::metrics_router(state) } else { routes::router(state, None, None) };
    let resp = app.oneshot(Request::get(path).body(Body::empty()).unwrap()).await.unwrap();
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
    let snap: board_api::Snapshot = serde_json::from_str(&get(state.clone(), "/v1/snapshot").await).unwrap();
    let gh = snap.sources.iter().find(|s| s.name == "github").unwrap();
    assert!(gh.last_good.is_none() && gh.last_error.as_deref().unwrap().contains("unreachable"));

    // Requests are counted by route, and by the client that sent them.
    let text = get(state, "/metrics").await;
    let line = r#"board_http_requests_total{client="other",route="/v1/snapshot",status="200"} 1"#;
    assert!(text.contains(line), "{text}");
}

// AC-6 of TRU-13: snapshot reads are counted by the client named in the User-Agent.
#[tokio::test]
async fn snapshot_reads_are_counted_by_client() {
    let state = sources::State::new();
    let agents = [
        "sdlc-board-android/0.1.0",
        "sdlc-board-native/0.1.0",
        "sdlc-board-native/0.2.0",
        "Mozilla/5.0 (X11; Linux x86_64) Chrome/140.0",
        "curl/8.5.0",
    ];
    for ua in agents {
        let req = Request::get("/v1/snapshot").header("user-agent", ua).body(Body::empty()).unwrap();
        let resp = routes::router(state.clone(), None, None).oneshot(req).await.unwrap();
        assert!(resp.status().is_success());
    }
    let text = get(state, "/metrics").await;
    for (client, n) in [("android", 1), ("native", 2), ("web", 1), ("other", 1)] {
        let line = format!(r#"board_http_requests_total{{client="{client}",route="/v1/snapshot",status="200"}} {n}"#);
        assert!(text.contains(&line), "no {line} in\n{text}");
    }
}
