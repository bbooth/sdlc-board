//! `board-setup app` against a stub GitHub, with the test acting as the
//! owner's browser, a fake `gh` and the fake kubectl.

use axum::extract::{Form, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path as FsPath, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod common;

const KEY: &str = include_str!("../../server/tests/fixtures/app_test_key_pkcs1.pem");
const TOKEN: &str = "gho_test_owner_token";
const VALUES: &str = "hostnames:\n  - board.trulinc.com\nrepos:\n  - name: bbooth/booking\n    linearTeam: TRU\n  - name: bbooth/sdlc-board\n    linearTeam: TRU\ntokensSecret: x\n";

#[derive(Default)]
struct Rec {
    manifest: Option<Value>,
    conversions: usize,
    /// Installation 77's repos, as (id, full name).
    installed: Vec<(u64, String)>,
    changes: Vec<String>,
    bad_auth: usize,
    /// What the install page reports: (installation id, its App, its selection).
    installation: (u64, u64, &'static str),
}

type Shared = Arc<Mutex<Rec>>;

fn authed(rec: &Shared, headers: &HeaderMap) -> bool {
    let ok = headers.get("authorization").and_then(|v| v.to_str().ok()) == Some(&format!("Bearer {TOKEN}"));
    if !ok {
        rec.lock().unwrap().bad_auth += 1;
    }
    ok
}

/// GitHub's side: the new-App form, the code conversion, the install page
/// and the installation's repositories.
fn stub(rec: Shared) -> String {
    let app = Router::new()
        .route(
            "/settings/apps/new",
            post(
                |State(rec): State<Shared>,
                 Query(q): Query<HashMap<String, String>>,
                 Form(f): Form<HashMap<String, String>>| async move {
                    let m: Value = serde_json::from_str(&f["manifest"]).unwrap();
                    let to = format!("{}?code=abc123&state={}", m["redirect_url"].as_str().unwrap(), q["state"]);
                    rec.lock().unwrap().manifest = Some(m);
                    Redirect::to(&to)
                },
            ),
        )
        .route(
            "/app-manifests/{code}/conversions",
            post(|State(rec): State<Shared>, Path(code): Path<String>| async move {
                assert_eq!(code, "abc123");
                rec.lock().unwrap().conversions += 1;
                Json(json!({ "id": 4242, "slug": "sdlc-board-test", "pem": KEY, "owner": { "login": "bbooth" } }))
            }),
        )
        .route(
            "/apps/{slug}/installations/new",
            get(|State(rec): State<Shared>| async move {
                let setup = rec.lock().unwrap().manifest.as_ref().unwrap()["setup_url"].as_str().unwrap().to_string();
                let id = rec.lock().unwrap().installation.0;
                Redirect::to(&format!("{setup}?installation_id={id}&setup_action=install"))
            }),
        )
        .route(
            "/user/installations",
            get(|State(rec): State<Shared>, headers: HeaderMap| async move {
                if !authed(&rec, &headers) {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                let (id, app_id, selection) = rec.lock().unwrap().installation;
                let list = json!([{ "id": 5, "app_id": 1, "repository_selection": "all" },
                    { "id": id, "app_id": app_id, "repository_selection": selection }]);
                Json(json!({ "total_count": 2, "installations": list })).into_response()
            }),
        )
        .route(
            "/user/installations/77/repositories",
            get(|State(rec): State<Shared>, headers: HeaderMap| async move {
                if !authed(&rec, &headers) {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                let repos: Vec<Value> =
                    rec.lock().unwrap().installed.iter().map(|(id, n)| json!({ "id": id, "full_name": n })).collect();
                Json(json!({ "total_count": repos.len(), "repositories": repos })).into_response()
            }),
        )
        .route(
            "/repos/{owner}/{repo}",
            get(|Path((owner, repo)): Path<(String, String)>| async move {
                let id = match format!("{owner}/{repo}").as_str() {
                    "bbooth/booking" => 1,
                    "bbooth/sdlc-board" => 2,
                    _ => 99,
                };
                Json(json!({ "id": id }))
            }),
        )
        .route(
            "/user/installations/77/repositories/{id}",
            put(|State(rec): State<Shared>, headers: HeaderMap, Path(id): Path<u64>| async move {
                if !authed(&rec, &headers) {
                    return StatusCode::UNAUTHORIZED;
                }
                let mut r = rec.lock().unwrap();
                r.changes.push(format!("PUT {id}"));
                r.installed.push((id, if id == 2 { "bbooth/sdlc-board" } else { "bbooth/booking" }.into()));
                StatusCode::NO_CONTENT
            })
            .delete(|State(rec): State<Shared>, headers: HeaderMap, Path(id): Path<u64>| async move {
                if !authed(&rec, &headers) {
                    return StatusCode::UNAUTHORIZED;
                }
                let mut r = rec.lock().unwrap();
                r.changes.push(format!("DELETE {id}"));
                r.installed.retain(|(i, _)| *i != id);
                StatusCode::NO_CONTENT
            }),
        )
        .with_state(rec);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new().unwrap().block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(format!("http://{}", listener.local_addr().unwrap())).unwrap();
            axum::serve(listener, app).await.unwrap();
        })
    });
    rx.recv().unwrap()
}

fn script(dir: &FsPath, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, body).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

struct Run {
    code: i32,
    out: String,
    rec: Shared,
    dir: tempfile::TempDir,
    /// Every argument the setup gave kubectl, gh and the opener.
    args: String,
}

/// Runs `board-setup app`; `browse` plays the owner's browser from the URL
/// the setup opened.
fn run(installed: &[(u64, &str)], keys: &str, browse: impl FnOnce(&str, &str, &Shared)) -> Run {
    run_with(installed, keys, false, (77, 4242, "selected"), browse)
}

fn run_with(
    installed: &[(u64, &str)],
    keys: &str,
    gh_fails: bool,
    installation: (u64, u64, &'static str),
    browse: impl FnOnce(&str, &str, &Shared),
) -> Run {
    let rec: Shared = Arc::default();
    rec.lock().unwrap().installation = installation;
    rec.lock().unwrap().installed = installed.iter().map(|(i, n)| (*i, n.to_string())).collect();
    let web = stub(rec.clone());
    let dir = tempfile::tempdir().unwrap();
    let (rec_dir, work) = (dir.path().join("rec"), dir.path().join("work"));
    fs::create_dir_all(&rec_dir).unwrap();
    fs::create_dir_all(&work).unwrap();
    fs::write(dir.path().join("values.yaml"), VALUES).unwrap();
    let opened = dir.path().join("opened");
    let kubectl = script(dir.path(), "kubectl", common::FAKE);
    let gh = script(
        dir.path(),
        "gh",
        &format!(
            "#!/bin/bash\necho \"$@\" >> {0}/gh.args\n{1}echo {TOKEN}\n",
            rec_dir.display(),
            if gh_fails {
                {
                    "exit 1\n"
                }
            } else {
                {
                    ""
                }
            }
        ),
    );
    let open = script(dir.path(), "open", &format!("#!/bin/bash\necho \"$1\" > {}\n", opened.display()));
    let child = Command::new(env!("CARGO_BIN_EXE_board-setup"))
        .arg("app")
        .current_dir(&work)
        .env("TMPDIR", &work)
        .env("GITHUB_WEB", &web)
        .env("GITHUB_API", &web)
        .env("GH", &gh)
        .env("BOARD_SETUP_OPEN", &open)
        .env("BOARD_VALUES", dir.path().join("values.yaml"))
        .env("KUBECTL", &kubectl)
        .env("REC", &rec_dir)
        .env("FAKE_KEYS", keys)
        .env("FAKE_LOGS", r#"{"fields":{"message":"reading GitHub as the GitHub App"}}"#)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while !opened.exists() && start.elapsed() < Duration::from_secs(20) {
        if keys.contains("GITHUB_APP_ID") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if opened.exists() {
        // The opener may not have finished writing.
        std::thread::sleep(Duration::from_millis(50));
        browse(fs::read_to_string(&opened).unwrap().trim(), &web, &rec);
    }
    let output = child.wait_with_output().unwrap();
    let out = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let mut args = String::new();
    for e in fs::read_dir(&rec_dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "args") {
            args.push_str(&fs::read_to_string(p).unwrap());
        }
    }
    Run { code: output.status.code().unwrap_or(-1), out, rec, dir, args }
}

fn browser() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .into()
}

/// What a browser does with the setup's page: posts the form GitHub gets.
fn confirm_on_github(url: &str) {
    let b = browser();
    let page = b.get(url).call().unwrap().body_mut().read_to_string().unwrap();
    let attr = |name: &str| {
        let at = page.find(&format!("{name}=\"")).unwrap() + name.len() + 2;
        let v = &page[at..at + page[at..].find('"').unwrap()];
        v.replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
    };
    let (action, manifest) = (attr("action"), attr("value"));
    // GitHub redirects to the setup, which sends the browser to the install
    // page, which redirects back to the setup once installed.
    let mut resp = b.post(&action).send_form([("manifest", manifest.as_str())]).unwrap();
    assert_eq!(resp.status(), 200, "{}", resp.body_mut().read_to_string().unwrap());
}

fn key_lines() -> Vec<&'static str> {
    KEY.lines().filter(|l| !l.starts_with("-----") && l.len() > 16).collect()
}

/// AC-5: the key is in no output, no argument and no file the run leaves.
fn assert_no_leak(r: &Run) {
    fn files(dir: &FsPath) -> Vec<PathBuf> {
        fs::read_dir(dir)
            .unwrap()
            .flat_map(|e| {
                let p = e.unwrap().path();
                if p.is_dir() {
                    files(&p)
                } else {
                    vec![p]
                }
            })
            .collect()
    }
    for line in key_lines() {
        assert!(!r.out.contains(line), "the key is in the output:\n{}", r.out);
        assert!(!r.args.contains(line), "the key is in an argument");
        for f in files(&r.dir.path().join("work")) {
            assert!(!fs::read_to_string(&f).unwrap_or_default().contains(line), "the key is in {}", f.display());
        }
    }
    assert!(!r.out.contains(TOKEN) && !r.args.contains(TOKEN), "the gh token leaked:\n{}", r.out);
}

#[test]
fn creates_a_read_only_private_app_and_hands_its_key_to_the_cluster() {
    let r = run(&[(1, "bbooth/booking")], "LINEAR_API_KEY\n", |url, _, _| confirm_on_github(url));
    assert_eq!(r.code, 0, "{}", r.out);
    let rec = r.rec.lock().unwrap();
    let m = rec.manifest.as_ref().expect("the manifest reached GitHub");
    assert_eq!(m["public"], false);
    assert_eq!(m["hook_attributes"]["active"], false);
    assert_eq!(m["default_events"], json!([]));
    assert_eq!(
        m["default_permissions"],
        json!({ "checks": "read", "contents": "read", "metadata": "read", "pull_requests": "read" })
    );
    assert_eq!(rec.conversions, 1);
    // The cluster got the stub's App, with its key on stdin.
    assert!(r.out.contains("Done: the board reads GitHub as App 4242"), "{}", r.out);
    let stdin: String = fs::read_dir(r.dir.path().join("rec"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "stdin"))
        .map(|p| fs::read_to_string(p).unwrap())
        .collect();
    let patch: Value = serde_json::from_str(stdin.lines().find(|l| l.contains("stringData")).unwrap()).unwrap();
    assert_eq!(patch["stringData"]["GITHUB_APP_ID"], "4242");
    assert_eq!(patch["stringData"]["GITHUB_APP_PRIVATE_KEY"].as_str().unwrap().trim(), KEY.trim());
    drop(rec);
    assert_no_leak(&r);
}

#[test]
fn the_installation_ends_with_exactly_the_boards_repos() {
    let r = run(&[(1, "bbooth/booking"), (9, "bbooth/private-notes")], "LINEAR_API_KEY\n", |url, _, _| {
        confirm_on_github(url)
    });
    assert_eq!(r.code, 0, "{}", r.out);
    let rec = r.rec.lock().unwrap();
    assert_eq!(rec.changes, ["PUT 2", "DELETE 9"], "each added or removed once");
    let mut names: Vec<&str> = rec.installed.iter().map(|(_, n)| n.as_str()).collect();
    names.sort();
    assert_eq!(names, ["bbooth/booking", "bbooth/sdlc-board"]);
    assert_eq!(rec.bad_auth, 0, "every call used the owner's gh token");
    drop(rec);
    assert_no_leak(&r);
}

#[test]
fn a_redirect_with_the_wrong_state_is_refused() {
    let r = run(&[(1, "bbooth/booking"), (2, "bbooth/sdlc-board")], "LINEAR_API_KEY\n", |url, _, rec| {
        let base = url.trim_end_matches('/');
        let resp = browser().get(format!("{base}/created?code=abc123&state=not-the-setups")).call().unwrap();
        assert_eq!(resp.status(), 400);
        assert_eq!(rec.lock().unwrap().conversions, 0, "a forged redirect made the setup convert a code");
        confirm_on_github(url);
    });
    assert_eq!(r.code, 0, "{}", r.out);
    assert_eq!(r.rec.lock().unwrap().conversions, 1);
    assert_no_leak(&r);
}

#[test]
fn an_app_already_on_the_cluster_means_no_new_app() {
    let r = run(&[], "GITHUB_APP_ID\nGITHUB_APP_PRIVATE_KEY\n", |_, _, _| panic!("the browser was opened"));
    assert_eq!(r.code, 0, "{}", r.out);
    assert!(r.out.contains("already set up"), "{}", r.out);
    assert!(r.rec.lock().unwrap().manifest.is_none());
}

#[test]
fn reads_the_boards_repos_from_the_charts_values() {
    let repos = board_setup::app::board_repos(include_str!("../../../deploy/values.yaml"));
    assert_eq!(repos, ["bbooth/booking", "bbooth/sdlc-board"]);
}

#[test]
fn without_a_gh_sign_in_it_says_which_repos_to_choose_and_exits_non_zero() {
    let r = run_with(&[(1, "bbooth/booking")], "LINEAR_API_KEY\n", true, (77, 4242, "selected"), |url, _, _| {
        confirm_on_github(url)
    });
    // Not done until the repos are set, though the cluster step ran.
    assert_ne!(r.code, 0, "{}", r.out);
    assert!(r.out.contains("not finished"), "{}", r.out);
    assert!(r.out.contains("/settings/installations/77") && r.out.contains("bbooth/sdlc-board"), "{}", r.out);
    assert!(r.out.contains("gh auth login"), "{}", r.out);
    assert!(r.rec.lock().unwrap().changes.is_empty());
    assert!(r.out.contains("Done: the board reads GitHub as App 4242"), "{}", r.out);
    assert_no_leak(&r);
}

/// Both end like a failed gh sign-in: nothing changed on GitHub, the repos
/// to choose named, a non-zero exit.
fn assert_installation_refused(installation: (u64, u64, &'static str), why: &str) {
    let r =
        run_with(&[(1, "bbooth/booking")], "LINEAR_API_KEY\n", false, installation, |url, _, _| confirm_on_github(url));
    assert_ne!(r.code, 0, "{}", r.out);
    assert!(r.out.contains(why), "{}", r.out);
    assert!(r.rec.lock().unwrap().changes.is_empty(), "changed an installation it shouldn't");
    assert_no_leak(&r);
}

#[test]
fn an_installation_of_another_app_is_left_alone() {
    // A request to the setup's port that isn't GitHub's, naming installation 5.
    assert_installation_refused((5, 1, "all"), "belongs to another App");
}

#[test]
fn an_install_on_all_repositories_is_called_out() {
    assert_installation_refused((77, 4242, "all"), "installed on all repositories");
}
