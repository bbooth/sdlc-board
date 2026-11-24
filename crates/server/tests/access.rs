// AC-9, server side: only a request carrying a live Access token for this
// application gets the snapshot or the web build. The key is a test-only key
// made for these tests (fixtures/access_test_key.pem).

use axum::{body::Body, http::Request, http::StatusCode, routing::get, Json, Router};
use board_server::access::{Access, Settings};
use board_server::{routes, sources};
use http_body_util::BodyExt;
use jsonwebtoken::{encode, EncodingKey, Header};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

const KEY: &[u8] = include_bytes!("fixtures/access_test_key.pem");
const CERTS: &str = include_str!("fixtures/access_test_certs.json");
const ISS: &str = "https://team.cloudflareaccess.com";
const AUD: &str = "board-aud";

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

fn token(kid: &str, aud: &str, iss: &str, exp: u64) -> String {
    let mut header = Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some(kid.into());
    let claims = json!({"aud": [aud], "iss": iss, "exp": exp, "iat": now(), "email": "owner@example.com"});
    encode(&header, &claims, &EncodingKey::from_rsa_pem(KEY).unwrap()).unwrap()
}

/// Serves the test key as Access's certs, counting reads.
async fn certs() -> (String, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    let r = reads.clone();
    let app = Router::new().route(
        "/cdn-cgi/access/certs",
        get(move || async move {
            r.fetch_add(1, Relaxed);
            Json(serde_json::from_str::<Value>(CERTS).unwrap())
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/cdn-cgi/access/certs"), reads)
}

async fn board() -> (Router, Arc<AtomicUsize>) {
    let (url, reads) = certs().await;
    let state = sources::State::new();
    let access = Access::with_certs(Settings { issuer: ISS.into(), aud: AUD.into() }, &url, state.clone()).unwrap();
    access.read_keys().await.unwrap();
    let web = std::env::temp_dir().join(format!("board-web-{}", std::process::id()));
    std::fs::create_dir_all(&web).unwrap();
    std::fs::write(web.join("index.html"), "<canvas id=board></canvas>").unwrap();
    (routes::router(state, Some(Arc::new(access)), Some(web)), reads)
}

async fn get_with(app: &Router, path: &str, token: Option<&str>) -> (StatusCode, String) {
    let mut req = Request::get(path);
    if let Some(t) = token {
        req = req.header("Cf-Access-Jwt-Assertion", t);
    }
    let resp = app.clone().oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
    let status = resp.status();
    (status, String::from_utf8(resp.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap())
}

#[tokio::test]
async fn without_a_live_access_token_nothing_is_served() {
    let (app, _) = board().await;
    let hour = now() + 3600;
    let refused = [
        ("missing", None),
        ("expired", Some(token("test-key", AUD, ISS, now() - 120))),
        ("wrong audience", Some(token("test-key", "another-app", ISS, hour))),
        ("wrong issuer", Some(token("test-key", AUD, "https://other.cloudflareaccess.com", hour))),
        ("garbage", Some("not.a.jwt".into())),
    ];
    for (why, t) in &refused {
        for path in ["/v1/snapshot", "/", "/index.html"] {
            let (status, body) = get_with(&app, path, t.as_deref()).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{why} on {path}");
            assert!(!body.contains("canvas") && !body.contains("columns"), "{why} on {path}: {body}");
        }
    }
    // /healthz is for the kubelet, and says nothing about the board.
    assert_eq!(get_with(&app, "/healthz", None).await, (StatusCode::OK, "ok".into()));
}

#[tokio::test]
async fn a_live_access_token_gets_the_snapshot_and_the_web_build() {
    let (app, _) = board().await;
    let t = token("test-key", AUD, ISS, now() + 3600);
    let (status, body) = get_with(&app, "/v1/snapshot", Some(&t)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(serde_json::from_str::<Value>(&body).unwrap()["columns"].is_array());
    let (status, body) = get_with(&app, "/", Some(&t)).await;
    assert_eq!((status, body.contains("<canvas id=board>")), (StatusCode::OK, true));
}

#[tokio::test]
async fn an_unknown_key_reads_the_keys_again_at_most_once_a_minute() {
    let (app, reads) = board().await;
    let t = token("rotated-key", AUD, ISS, now() + 3600);
    for _ in 0..3 {
        assert_eq!(get_with(&app, "/v1/snapshot", Some(&t)).await.0, StatusCode::UNAUTHORIZED);
    }
    // The first read was at start; the unknown key came within the minute, so no more.
    assert_eq!(reads.load(Relaxed), 1);
}

// When Cloudflare's keys can't be read, every request gets 401, so the
// reads are counted for an alert on their errors.
#[tokio::test]
async fn key_reads_are_counted_and_a_failed_one_is_an_error() {
    let state = sources::State::new();
    let settings = || Settings { issuer: ISS.into(), aud: AUD.into() };
    let (url, _) = certs().await;
    Access::with_certs(settings(), &url, state.clone()).unwrap().read_keys().await.unwrap();
    let gone = Access::with_certs(settings(), &url.replace("/certs", "/gone"), state.clone()).unwrap();
    assert!(gone.read_keys().await.is_err());
    let text = state.metrics.render(&[]);
    assert!(text.contains(r#"board_source_reads_total{source="access_keys"} 2"#), "{text}");
    assert!(text.contains(r#"board_source_read_errors_total{source="access_keys"} 1"#), "{text}");
}
