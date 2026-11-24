use axum::extract::Path;
use axum::{http::StatusCode, response::IntoResponse, routing::get, routing::post, Json, Router};
use board_api::{Checks, Snapshot, Ticket};
use board_server::{config::Repo, github::GitHub, linear::Linear, sources};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Arc;

fn repo(name: &str, project: &str) -> Repo {
    Repo { name: name.into(), linear_team: "TRU".into(), linear_project: project.into() }
}

fn ticket(id: &str, stage: &str) -> Value {
    json!({"identifier": id, "title": id, "url": format!("https://linear.app/t/{id}"), "state": {"type": "started"},
           "parent": null, "labels": {"nodes": [{"name": stage, "parent": {"name": "Stage"}}]}, "comments": {"nodes": []}})
}

fn pr(repo: &str, n: u64, title: &str, branch: &str) -> Value {
    json!({"number": n, "title": title, "html_url": format!("https://github.com/{repo}/pull/{n}"), "draft": false,
           "head": {"sha": format!("sha{n}"), "ref": branch, "repo": {"full_name": repo}}, "merged_at": null})
}

/// Linear and GitHub fakes for two repos, booking and sdlc-board, in one
/// Linear team. `moved` puts TRU-10 in Verify; `github_down` fails every GitHub call.
async fn fakes() -> (String, Arc<AtomicBool>, Arc<AtomicBool>) {
    let (moved, github_down) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let (m, down, down2) = (moved.clone(), github_down.clone(), github_down.clone());
    let linear = move |Json(req): Json<Value>| async move {
        let nodes = match req["variables"]["project"].as_str() {
            Some("Booking") => vec![ticket("TRU-6", "PR review")],
            Some("SDLC board") => vec![ticket("TRU-10", if m.load(Relaxed) { "Verify" } else { "Build" })],
            _ => vec![],
        };
        Json(json!({"data": {"issues": {"pageInfo": {"hasNextPage": false}, "nodes": nodes}}}))
    };
    let pulls = move |Path((owner, name)): Path<(String, String)>| async move {
        if down.load(Relaxed) {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        let repo = format!("{owner}/{name}");
        Json(match repo.as_str() {
            // booking's newer #3 names TRU-10 in its title too, but TRU-10 is sdlc-board's.
            "bbooth/booking" => {
                json!([pr(&repo, 1, "TRU-6: fields", "backend/tru-6-x"), pr(&repo, 3, "TRU-10 follow-up", "x")])
            }
            _ => json!([pr(&repo, 2, "TRU-10: board", "feat/board")]),
        })
        .into_response()
    };
    let runs = move || async move {
        if down2.load(Relaxed) {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        Json(json!({"check_runs": [{"status": "completed", "conclusion": "success"}]})).into_response()
    };
    let app = Router::new()
        .route("/graphql", post(linear))
        .route("/repos/{owner}/{name}/pulls", get(pulls))
        .route("/repos/{owner}/{name}/commits/{sha}/check-runs", get(runs))
        .route("/repos/{owner}/{name}/contents/{*path}", get(|| async { StatusCode::NOT_FOUND }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), moved, github_down)
}

fn card<'a>(snap: &'a Snapshot, id: &str) -> &'a Ticket {
    snap.columns.iter().flat_map(|c| &c.tickets).find(|t| t.id == id).unwrap()
}

// AC-5: two repos in the settings, each with its own Linear project: both
// repos' tickets and PRs show, each card naming its repo and showing only its
// own repo's PR. Each repo is listed even when the first one's PRs would use
// up a tokenless read's budget.
#[tokio::test]
async fn every_repo_in_the_settings_is_covered() {
    let (url, ..) = fakes().await;
    let repos = vec![repo("bbooth/booking", "Booking"), repo("bbooth/sdlc-board", "SDLC board")];
    let state = sources::State::new();
    sources::refresh(&state, &Linear::new(&format!("{url}/graphql"), "k", repos.clone()).unwrap()).await;
    sources::refresh(&state, &GitHub::new(&url, None, repos).unwrap().with_budget(4)).await;
    let snap = state.snapshot();

    let six = card(&snap, "TRU-6");
    assert_eq!(
        (six.repo.as_str(), six.pr.as_ref().unwrap().url.as_str()),
        ("bbooth/booking", "https://github.com/bbooth/booking/pull/1")
    );
    let ten = card(&snap, "TRU-10");
    let pr = ten.pr.as_ref().unwrap();
    assert_eq!(
        (ten.repo.as_str(), pr.url.as_str()),
        ("bbooth/sdlc-board", "https://github.com/bbooth/sdlc-board/pull/2")
    );
    // The 4 requests went to the two repos' lists, so no checks were read.
    assert_eq!(pr.checks, Checks::Unknown);
}

// AC-7: GitHub fails after one good read. The board keeps its PRs, marks
// GitHub stale with the time of its last good read and the error, and Linear
// keeps updating.
#[tokio::test]
async fn a_failing_source_keeps_its_data_and_the_others_keep_updating() {
    let (url, moved, github_down) = fakes().await;
    let repos = vec![repo("bbooth/booking", "Booking"), repo("bbooth/sdlc-board", "SDLC board")];
    let (linear, github) =
        (Linear::new(&format!("{url}/graphql"), "k", repos.clone()).unwrap(), GitHub::new(&url, None, repos).unwrap());
    let state = sources::State::new();
    sources::refresh(&state, &linear).await;
    sources::refresh(&state, &github).await;
    let good = state.snapshot();
    let github_good = good.sources.iter().find(|s| s.name == "github").unwrap().last_good.unwrap();

    github_down.store(true, Relaxed);
    moved.store(true, Relaxed);
    sources::refresh(&state, &linear).await;
    sources::refresh(&state, &github).await;
    let snap = state.snapshot();

    let status = |name: &str| snap.sources.iter().find(|s| s.name == name).unwrap().clone();
    let gh = status("github");
    assert_eq!(gh.last_good, Some(github_good));
    assert!(gh.last_error.unwrap().contains("500"));
    assert_eq!(card(&snap, "TRU-6").pr, card(&good, "TRU-6").pr);
    assert_eq!(card(&snap, "TRU-6").pr.as_ref().unwrap().checks, Checks::Pass);
    assert_eq!((status("linear").last_error, card(&snap, "TRU-10").stage.as_str()), (None, "Verify"));
}
