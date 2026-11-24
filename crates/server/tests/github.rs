use axum::extract::{Path, Query};
use axum::{http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use board_api::{Checks, Gate};
use board_server::{board, config::Repo, github::GitHub};
use serde_json::{json, Value};
use std::collections::HashMap;

// Recorded shapes of GitHub's REST responses, trimmed to the fields the board reads.
async fn fake_github() -> String {
    let pr = |n: u64, title: &str, branch: &str, body: &str| {
        json!({"number": n, "title": title, "html_url": format!("https://github.com/bbooth/booking/pull/{n}"),
               "draft": false, "head": {"sha": format!("sha{n}"), "ref": branch}, "body": body})
    };
    // GitHub lists the newest first. #5 only mentions TRU-6 in passing; #4
    // names no ticket (TRU-800X, XTRU-9 and TRU- aren't IDs); #6's title
    // names TRU-6 but its branch doesn't, so TRU-6's card keeps #1.
    let pulls = json!([
        pr(6, "Follow-up to TRU-6: docs", "docs/follow-up", ""),
        pr(5, "Run CI again on label changes", "ci/labels", "Found while opening TRU-6's PR."),
        pr(4, "Bump deps (TRU-800X)", "chore/XTRU-9", "TRU-"),
        pr(3, "One board", "feat/tru-8-one-board", ""),
        pr(2, "Drop the columns (TRU-7)", "backend/drop-columns", ""),
        pr(1, "TRU-6: Stop using the fields", "backend/tru-6-stop-using", ""),
    ]);
    let runs = |sha: String| {
        let run = |status: &str, conclusion: Value| json!({"status": status, "conclusion": conclusion});
        let runs = match sha.as_str() {
            "sha1" | "sha6" => vec![run("completed", json!("success")), run("completed", json!("skipped"))],
            "sha2" => vec![run("completed", json!("success")), run("completed", json!("failure"))],
            _ => vec![run("completed", json!("success")), run("in_progress", Value::Null)],
        };
        Json(json!({ "total_count": runs.len(), "check_runs": runs }))
    };
    let contents = |Path(path): Path<String>, Query(q): Query<HashMap<String, String>>| async move {
        let names = ["build-1.json", "verify-1.json", "verify-2.json", "adversarial_review-1.json", "notes.txt"];
        match (path.as_str(), q.get("ref").map(String::as_str)) {
            (".sdlc/records/TRU-6", Some("backend/tru-6-stop-using")) => {
                Json(json!(names.map(|n| json!({ "name": n })))).into_response()
            }
            (".sdlc/records/TRU-6/adversarial_review-1.json", Some("backend/tru-6-stop-using")) => {
                Json(json!({"stage": "adversarial_review", "round": 1, "result": "fail"})).into_response()
            }
            _ => StatusCode::NOT_FOUND.into_response(),
        }
    };
    let app = Router::new()
        .route("/repos/bbooth/booking/pulls", get(move || async move { Json(pulls) }))
        .route("/repos/bbooth/booking/commits/{sha}/check-runs", get(move |Path(sha)| async move { runs(sha) }))
        .route("/repos/bbooth/booking/contents/{*path}", get(contents));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn repo() -> Repo {
    Repo { name: "bbooth/booking".into(), linear_team: "TRU".into(), linear_project: "Booking".into() }
}

fn gate(stage: &str, round: u32, pass: bool) -> Gate {
    Gate { stage: stage.into(), round, pass }
}

#[tokio::test]
async fn checks_map_to_pass_fail_or_pending_and_the_latest_record_is_read() {
    let prs = GitHub::new(&fake_github().await, None, vec![repo()]).unwrap().prs().await.unwrap();

    let got: Vec<(&str, u64, Checks)> = prs.iter().map(|p| (p.ticket.as_str(), p.pr.number, p.pr.checks)).collect();
    // #5 (named only in its body) and #4 are left out.
    let tied = [
        ("TRU-6", 6, Checks::Pass),
        ("TRU-8", 3, Checks::Pending),
        ("TRU-7", 2, Checks::Fail),
        ("TRU-6", 1, Checks::Pass),
    ];
    assert_eq!(got, tied);
    let by_branch: Vec<bool> = prs.iter().map(|p| p.by_branch).collect();
    assert_eq!(by_branch, [false, true, false, true]);
    let prs: Vec<_> = prs.into_iter().rev().collect();
    assert_eq!(prs[0].pr.url, "https://github.com/bbooth/booking/pull/1");
    // A later stage beats a higher round of an earlier one; its result comes from the file.
    assert_eq!(prs[0].gate, Some(gate("Adversarial review", 1, false)));
    assert_eq!(prs[1].gate, None);
}

#[tokio::test]
async fn a_card_shows_the_latest_gate_from_its_comments_or_its_branch_and_its_pr() {
    let prs = GitHub::new(&fake_github().await, None, vec![repo()]).unwrap().prs().await.unwrap();
    let issue = |id: &str, comment: &str| board::Issue {
        id: id.into(),
        title: id.into(),
        url: format!("https://linear.app/t/{id}"),
        repo: "bbooth/booking".into(),
        stage: Some("PR review".into()),
        parent: None,
        open: true,
        gates: board_server::gates::from_comment(comment),
    };
    let issues = [
        issue("TRU-6", "### Gate: PR review (pass) · round 1\n- Checks: ..."),
        issue("TRU-7", "### Gate: Build (fail) · round 3"),
        issue("TRU-8", "No gate here"),
        // The owner sent the PR back after its review passed.
        issue("TRU-9", "### Gate: PR review (pass) · round 1\n\n### Gate: PR review approval (fail) · round 1"),
        issue("TRU-10", "### Gate: Spec approval (pass) · round 1\n### Gate: Spec (pass) · round 2"),
    ];
    let cols = board::columns(&issues, &prs);
    let tickets = &cols.iter().find(|c| c.stage == "PR review").unwrap().tickets;
    let card = |id: &str| tickets.iter().find(|t| t.id == id).unwrap();

    // TRU-6: its own PR, on the branch naming it, not #6 whose title mentions it;
    // and the comment's PR review beats the branch's Adversarial review.
    assert_eq!(card("TRU-6").gate, Some(gate("PR review", 1, true)));
    assert_eq!(card("TRU-6").pr.as_ref().map(|p| (p.number, p.checks)), Some((1, Checks::Pass)));
    assert_eq!(card("TRU-7").gate, Some(gate("Build", 3, false)));
    assert_eq!(card("TRU-8").gate, None);
    assert_eq!(card("TRU-8").pr.as_ref().unwrap().checks, Checks::Pending);
    // An owner's decision comes after the gate it decides on.
    assert_eq!(card("TRU-9").gate, Some(gate("PR review approval", 1, false)));
    assert_eq!(card("TRU-10").gate, Some(gate("Spec approval", 1, true)));
    // TRU-6 is in PR review with two green PRs; only its own is a merge to do.
    let merges: Vec<String> = board::needs_you(&issues, &prs).into_iter().map(|n| n.url).collect();
    assert_eq!(merges, ["https://github.com/bbooth/booking/pull/1"]);
}
