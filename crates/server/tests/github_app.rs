// TRU-18: the server reads private repos as a GitHub App. The fake answers a
// private repo only with an installation token it minted, and mints one only
// for a JWT signed by the App's key (the test-only key in
// fixtures/app_test_key_pkcs1.pem, the shape GitHub hands out).

use axum::extract::Path;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, routing::post, Json, Router};
use board_api::Checks;
use board_server::config::Repo;
use board_server::github::{Auth, GitHub};
use board_server::github_app::{unix, AppAuth, Settings};
use board_server::sources::{self, Source};
use jsonwebtoken::{decode, jwk::JwkSet, DecodingKey, Validation};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const KEY: &str = include_str!("fixtures/app_test_key_pkcs1.pem");
const PKCS8: &str = include_str!("fixtures/access_test_key.pem");
const CERTS: &str = include_str!("fixtures/access_test_certs.json");
const APP_ID: &str = "42";

/// Tokens minted expire long after the test, already, in 200 or 400
/// seconds; or minting fails, or the installation lookup does.
const LONG: u8 = 0;
const EXPIRED: u8 = 1;
const MINT_FAILS: u8 = 2;
const SOON: u8 = 3;
const LATER: u8 = 4;
const LOOKUP_FAILS: u8 = 5;
/// The personal token the fake accepts for the private repo.
const PAT: &str = "github_pat_real";

struct Fake {
    url: String,
    mode: Arc<AtomicU8>,
    mints: Arc<AtomicUsize>,
    /// The App's installation ID; reinstalling the App changes it.
    installation: Arc<AtomicU64>,
    /// Tokens GitHub has revoked before they expired.
    revoked: Arc<Mutex<Vec<String>>>,
}

/// Unix seconds as GitHub writes them, `2026-11-25T20:00:00Z`.
fn rfc3339(t: i64) -> String {
    let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", secs / 3600, secs / 60 % 60, secs % 60)
}

fn bearer(headers: &HeaderMap) -> String {
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default();
    auth.strip_prefix("Bearer ").unwrap_or_default().to_string()
}

fn app_jwt_ok(headers: &HeaderMap) -> bool {
    let set: JwkSet = serde_json::from_str(CERTS).unwrap();
    let key = DecodingKey::from_jwk(&set.keys[0]).unwrap();
    let mut v = Validation::new(jsonwebtoken::Algorithm::RS256);
    v.set_issuer(&[APP_ID]);
    v.set_required_spec_claims(&["exp", "iat", "iss"]);
    decode::<Value>(&bearer(headers), &key, &v).is_ok()
}

async fn fake() -> Fake {
    let (mode, mints) = (Arc::new(AtomicU8::new(LONG)), Arc::new(AtomicUsize::new(0)));
    let (installed, revoked) = (Arc::new(AtomicU64::new(7)), Arc::<Mutex<Vec<String>>>::default());
    let live = Arc::new(Mutex::new(vec![PAT.to_string()]));
    let (m, n, l) = (mode.clone(), mints.clone(), live.clone());
    let (m2, i1, i2, r) = (mode.clone(), installed.clone(), installed.clone(), revoked.clone());
    let installation = move |Path((owner, repo)): Path<(String, String)>, h: HeaderMap| async move {
        if !app_jwt_ok(&h) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        match (m2.load(Relaxed), owner.as_str(), repo.as_str()) {
            (LOOKUP_FAILS, ..) => StatusCode::BAD_GATEWAY.into_response(),
            (_, "bbooth", "secret") => Json(json!({ "id": i1.load(Relaxed) })).into_response(),
            _ => StatusCode::NOT_FOUND.into_response(),
        }
    };
    let mint = move |Path(id): Path<u64>, h: HeaderMap, Json(scope): Json<Value>| async move {
        if !app_jwt_ok(&h) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        if id != i2.load(Relaxed) {
            return StatusCode::NOT_FOUND.into_response();
        }
        // Each token is for the one repo, read-only.
        let read = json!({"checks": "read", "contents": "read", "metadata": "read", "pull_requests": "read"});
        if scope != json!({ "repositories": ["secret"], "permissions": read }) {
            return StatusCode::UNPROCESSABLE_ENTITY.into_response();
        }
        let expires = match m.load(Relaxed) {
            MINT_FAILS => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            EXPIRED => "2000-01-01T00:00:00Z".into(),
            SOON => rfc3339(sources::now() + 200),
            LATER => rfc3339(sources::now() + 400),
            _ => "2999-01-01T00:00:00Z".into(),
        };
        let token = format!("ghs_secret{}", n.fetch_add(1, Relaxed));
        l.lock().unwrap().push(token.clone());
        Json(json!({ "token": token, "expires_at": expires })).into_response()
    };
    // A private repo is "not found" to anyone without a live installation token.
    let private = move |h: &HeaderMap, ok: Value| -> Response {
        if r.lock().unwrap().contains(&bearer(h)) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        if !live.lock().unwrap().contains(&bearer(h)) {
            return StatusCode::NOT_FOUND.into_response();
        }
        Json(ok).into_response()
    };
    let (p1, p2, p3) = (private.clone(), private.clone(), private);
    let pr = json!([{"number": 1, "title": "TRU-5: Secret work", "html_url": "https://github.com/bbooth/secret/pull/1",
        "draft": false, "head": {"sha": "s1", "ref": "feat/tru-5-secret", "repo": {"full_name": "bbooth/secret"}},
        "merged_at": null, "merge_commit_sha": "m1"}]);
    let pulls = move |h: HeaderMap| async move { p1(&h, pr) };
    let runs = json!({"total_count": 1, "check_runs": [{"status": "completed", "conclusion": "success"}]});
    let checks = move |h: HeaderMap| async move { p2(&h, runs) };
    let contents = move |Path(path): Path<String>, h: HeaderMap| async move {
        match path.as_str() {
            ".sdlc/records/TRU-5" => p3(&h, json!([{ "name": "build-1.json" }])),
            ".sdlc/records/TRU-5/build-1.json" => {
                p3(&h, json!({"result": "pass", "recorded_at": "2026-11-25T00:00:00Z"}))
            }
            _ => StatusCode::NOT_FOUND.into_response(),
        }
    };
    let app = Router::new()
        .route("/repos/{owner}/{repo}/installation", get(installation))
        .route("/app/installations/{id}/access_tokens", post(mint))
        .route("/repos/bbooth/secret/pulls", get(pulls))
        .route("/repos/bbooth/secret/commits/{sha}/check-runs", get(checks))
        .route("/repos/bbooth/secret/contents/{*path}", get(contents));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Fake { url, mode, mints, installation: installed, revoked }
}

fn repo(name: &str) -> Repo {
    Repo { name: name.into(), linear_team: "TRU".into(), linear_project: "Board".into(), rollout_namespace: None }
}

fn settings() -> Settings {
    Settings::from_values(Some(APP_ID), Some(KEY)).unwrap().unwrap()
}

fn github(fake: &Fake, repos: &[&str], state: &Arc<sources::State>) -> GitHub {
    let app = AppAuth::new(settings(), &fake.url, state.clone()).unwrap();
    // A personal token alongside is ignored: the fake would refuse it.
    GitHub::new(&fake.url, Some("github_pat_ignored".into()), repos.iter().map(|r| repo(r)).collect())
        .unwrap()
        .with_app(app)
}

fn tokens(state: &sources::State, result: &str) -> String {
    let text = state.metrics.render(&[]);
    let prefix = format!("board_github_app_tokens_total{{result=\"{result}\"}} ");
    text.lines().find_map(|l| l.strip_prefix(&prefix)).unwrap_or("none").to_string()
}

#[tokio::test]
async fn a_private_repo_reads_as_the_app_every_minute() {
    let (fake, state) = (fake().await, sources::State::new());
    let github = github(&fake, &["bbooth/secret"], &state);
    assert!(matches!(github.auth(), Auth::App(_)), "the App wins over GITHUB_TOKEN");
    let read = github.prs().await;
    assert_eq!(read.errors, Vec::<String>::new());
    let pr = &read.prs[0];
    assert_eq!((pr.ticket.as_str(), pr.pr.number, pr.pr.checks), ("TRU-5", 1, Checks::Pass));
    assert_eq!(pr.gate.as_ref().map(|g| (g.stage.as_str(), g.pass)), Some(("Build", true)));
    assert_eq!(github.interval(), sources::INTERVAL);
    assert_eq!(Source::interval(&github), Duration::from_secs(60));
}

#[tokio::test]
async fn a_token_in_its_last_five_minutes_is_replaced_and_every_token_is_counted() {
    let (fake, state) = (fake().await, sources::State::new());
    let github = github(&fake, &["bbooth/secret"], &state);
    let read_and_mints = || async {
        let read = github.prs().await;
        assert!(read.errors.is_empty() && read.prs.len() == 1, "{:?}", read.errors);
        fake.mints.load(Relaxed)
    };
    // 400 s left is kept; 200 s left is inside the margin, so the next request replaces it.
    fake.mode.store(LATER, Relaxed);
    assert_eq!(read_and_mints().await, 1);
    assert_eq!(read_and_mints().await, 1, "a token with 400 s left is kept");
    let (fake2, state2) = (self::fake().await, sources::State::new());
    fake2.mode.store(SOON, Relaxed);
    let soon = self::github(&fake2, &["bbooth/secret"], &state2);
    soon.prs().await;
    let first = fake2.mints.load(Relaxed);
    fake2.mode.store(LONG, Relaxed);
    soon.prs().await;
    assert!(fake2.mints.load(Relaxed) > first, "a token with 200 s left is replaced");
    assert_eq!(tokens(&state2, "ok"), fake2.mints.load(Relaxed).to_string());
    let kept = fake2.mints.load(Relaxed);
    soon.prs().await;
    assert_eq!(fake2.mints.load(Relaxed), kept, "a live token is kept");
    assert_eq!(tokens(&state, "ok"), "1");

    // A failed mint with no live token fails that read and is counted; the next read tries again.
    let (fake, state) = (self::fake().await, sources::State::new());
    fake.mode.store(MINT_FAILS, Relaxed);
    let failing = self::github(&fake, &["bbooth/secret"], &state);
    assert_eq!(failing.prs().await.repos_failed, ["bbooth/secret"]);
    assert_eq!(tokens(&state, "error"), "1");
    fake.mode.store(SOON, Relaxed);
    assert!(failing.prs().await.errors.is_empty(), "the next read tries again");
    // A token inside its margin but not expired still serves while minting fails.
    fake.mode.store(MINT_FAILS, Relaxed);
    let read = failing.prs().await;
    assert!(read.errors.is_empty() && read.prs.len() == 1, "{:?}", read.errors);
    // and is tried once a minute, not once a request.
    failing.prs().await;
    assert_eq!(tokens(&state, "error"), "2");
}

#[tokio::test]
async fn a_failed_lookup_counts_and_a_reinstalled_or_revoked_app_recovers() {
    // A wrong App ID: GitHub rejects the JWT at the lookup, before any mint.
    let (fake, state) = (fake().await, sources::State::new());
    let wrong = Settings::from_values(Some("43"), Some(KEY)).unwrap().unwrap();
    let app = AppAuth::new(wrong, &fake.url, state.clone()).unwrap();
    let github = GitHub::new(&fake.url, None, vec![repo("bbooth/secret")]).unwrap().with_app(app);
    assert_eq!(github.prs().await.repos_failed, ["bbooth/secret"]);
    assert_eq!(tokens(&state, "error"), "1");
    // A lookup that fails on GitHub's side counts too; "not installed" doesn't.
    let (fake, state) = (self::fake().await, sources::State::new());
    let github = self::github(&fake, &["bbooth/secret", "bbooth/elsewhere"], &state);
    fake.mode.store(LOOKUP_FAILS, Relaxed);
    github.prs().await;
    assert_eq!(tokens(&state, "error"), "2");
    fake.mode.store(LONG, Relaxed);
    let read = github.prs().await;
    assert_eq!(
        (read.repos_failed.as_slice(), tokens(&state, "error")),
        (["bbooth/elsewhere".to_string()].as_slice(), "2".into())
    );

    // Reinstalled: the old installation's mint is 404, and the read after looks it up again.
    let (fake, state) = (self::fake().await, sources::State::new());
    fake.mode.store(EXPIRED, Relaxed);
    let github = self::github(&fake, &["bbooth/secret"], &state);
    github.prs().await;
    fake.installation.store(8, Relaxed);
    let read = github.prs().await;
    assert!(read.errors[0].contains("looked up again next read"), "{:?}", read.errors);
    fake.mode.store(LONG, Relaxed);
    assert!(github.prs().await.errors.is_empty(), "recovers without a restart");

    // Revoked before it expired: a 401 drops it, and the next read gets a new one.
    let before = fake.mints.load(Relaxed);
    let live = format!("ghs_secret{}", before - 1);
    fake.revoked.lock().unwrap().push(live);
    github.prs().await;
    let read = github.prs().await;
    assert!(read.errors.is_empty() && fake.mints.load(Relaxed) > before, "{:?}", read.errors);
}

#[tokio::test]
async fn without_an_app_the_personal_token_is_sent() {
    let fake = fake().await;
    let github = GitHub::new(&fake.url, Some(PAT.into()), vec![repo("bbooth/secret")]).unwrap();
    assert!(matches!(github.auth(), Auth::Token(_)));
    let read = github.prs().await;
    assert!(read.errors.is_empty() && read.prs.len() == 1, "{:?}", read.errors);
    assert_eq!(github.interval(), sources::INTERVAL);
    let tokenless = GitHub::new(&fake.url, None, vec![repo("bbooth/secret")]).unwrap();
    assert_eq!(tokenless.prs().await.repos_failed, ["bbooth/secret"], "a private repo is not found without one");
}

#[tokio::test]
async fn a_repo_the_app_isnt_on_fails_alone_and_no_secret_shows() {
    let (fake, state) = (fake().await, sources::State::new());
    let github = github(&fake, &["bbooth/elsewhere", "bbooth/secret"], &state);
    let read = github.prs().await;
    assert_eq!(read.repos_failed, ["bbooth/elsewhere"]);
    assert!(read.errors[0].contains("the GitHub App isn't installed on bbooth/elsewhere"), "{:?}", read.errors);
    assert_eq!(read.prs.iter().map(|p| p.repo.as_str()).collect::<Vec<_>>(), ["bbooth/secret"]);

    sources::refresh(&state, &github).await;
    let snapshot = serde_json::to_string(&state.snapshot()).unwrap();
    assert!(snapshot.contains("isn't installed on bbooth/elsewhere"), "{snapshot}");
    for secret in ["ghs_", "PRIVATE KEY", "github_pat"] {
        assert!(!snapshot.contains(secret) && !read.errors.concat().contains(secret), "{secret} leaked");
    }
}

#[test]
fn bad_app_settings_name_the_setting_and_never_the_key() {
    let err = |id: Option<&str>, key: Option<&str>| Settings::from_values(id, key).err().unwrap().to_string();
    assert!(Settings::from_values(None, None).unwrap().is_none());
    assert!(Settings::from_values(Some(" "), Some("")).unwrap().is_none(), "blank is unset");
    assert_eq!(err(Some("42"), None), "GITHUB_APP_ID is set but GITHUB_APP_PRIVATE_KEY isn't");
    assert_eq!(err(None, Some(KEY)), "GITHUB_APP_PRIVATE_KEY is set but GITHUB_APP_ID isn't");
    assert_eq!(err(Some("forty-two"), Some(KEY)), "GITHUB_APP_ID isn't a number");
    let broken = KEY.replace("MII", "XXX");
    for bad in ["not a key", broken.as_str()] {
        let e = err(Some("42"), Some(bad));
        assert!(e.starts_with("GITHUB_APP_PRIVATE_KEY "), "{e}");
        assert!(!e.contains("XXX") && !e.contains("not a key"), "{e}");
    }
    // Both PEM shapes work: GitHub's PKCS#1 and PKCS#8.
    assert_eq!(settings().app_id, 42);
    assert!(Settings::from_values(Some("42"), Some(PKCS8)).unwrap().is_some());
}

#[test]
fn github_times_read_as_unix_seconds() {
    assert_eq!(unix("2026-11-25T20:00:00Z"), Some(1_795_636_800));
    assert_eq!(rfc3339(951_868_799), "2000-02-29T23:59:59Z");
    assert_eq!(unix("2000-02-29T23:59:59Z"), Some(951_868_799));
    assert_eq!(unix("soon"), None);
}

#[tokio::test]
async fn a_personal_token_beside_the_app_is_ignored_with_a_warning() {
    #[derive(Clone, Default)]
    struct Logs(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Logs {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let (fake, state, logs) = (fake().await, sources::State::new(), Logs::default());
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt().with_writer(move || writer.clone()).finish();
    // Global, not scoped: other tests hit the same callsite from their own threads.
    tracing::subscriber::set_global_default(subscriber).unwrap();
    github(&fake, &["bbooth/secret"], &state);
    let text = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(text.contains("GITHUB_TOKEN is ignored: signing in as the GitHub App"), "{text}");
    assert!(!text.contains("github_pat"), "{text}");
}
