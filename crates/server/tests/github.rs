use axum::extract::{Path, Query};
use axum::{http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use board_api::{Checks, Gate};
use board_server::{
    board,
    config::Repo,
    github::{GitHub, PrRead},
    sources,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering::Relaxed};
use std::sync::Arc;
use std::time::Duration;

/// The fake answers as recorded, or with any of: every check and record call
/// failing, the PR lists failing, #1 pushed to a new commit.
const OK: u8 = 0;
const PARTS_FAIL: u8 = 1;
const LISTS_FAIL: u8 = 2;
const MOVED: u8 = 4;

// Recorded shapes of GitHub's REST responses, trimmed to the fields the board reads.
async fn fake_github() -> (String, Arc<AtomicU8>) {
    let mode = Arc::new(AtomicU8::new(OK));
    let (m1, m2, m3) = (mode.clone(), mode.clone(), mode.clone());
    let pr = |n: u64, title: &str, branch: &str, body: &str| {
        json!({"number": n, "title": title, "html_url": format!("https://github.com/bbooth/booking/pull/{n}"),
               "draft": false, "head": {"sha": format!("sha{n}"), "ref": branch, "repo": {"full_name": "bbooth/booking"}},
               "body": body, "merged_at": null, "merge_commit_sha": format!("merge{n}")})
    };
    // GitHub lists the newest first. #8's checks and records fail to read;
    // #7 comes from a fork, so it doesn't count even though its branch names
    // TRU-6. #5 only mentions TRU-6 in passing; #4 names no ticket
    // (TRU-800X, XTRU-9 and TRU- aren't IDs); #6's title names TRU-6 but its
    // branch doesn't, so TRU-6's card keeps #1.
    let mut fork = pr(7, "TRU-6: take over", "feat/tru-6-mine", "");
    fork["head"]["repo"]["full_name"] = json!("someone/booking");
    let open = json!([
        pr(8, "TRU-9: Broken reads", "feat/tru-9-broken", ""),
        fork,
        pr(6, "Follow-up to TRU-6: docs", "docs/follow-up", ""),
        pr(5, "Run CI again on label changes", "ci/labels", "Found while opening TRU-6's PR."),
        pr(4, "Bump deps (TRU-800X)", "chore/XTRU-9", "TRU-"),
        pr(3, "One board", "feat/tru-8-one-board", ""),
        pr(2, "Drop the columns (TRU-7)", "backend/drop-columns", ""),
        pr(1, "TRU-6: Stop using the fields", "backend/tru-6-stop-using", ""),
    ]);
    // #10 closed without merging; #9 merged, and its records are read at its merge.
    let mut merged = pr(9, "TRU-11: Refresh", "feat/tru-11-refresh", "");
    merged["merged_at"] = json!("2026-11-24T00:00:00Z");
    let closed = json!([pr(10, "TRU-12: Abandoned", "feat/tru-12-old", ""), merged]);
    let pulls = move |Query(q): Query<HashMap<String, String>>| async move {
        let mode = m1.load(Relaxed);
        if mode & LISTS_FAIL != 0 {
            return StatusCode::FORBIDDEN.into_response();
        }
        let mut open = open;
        if mode & MOVED != 0 {
            open[7]["head"]["sha"] = json!("sha1b");
        }
        Json(if q.get("state").map(String::as_str) == Some("closed") { closed } else { open }).into_response()
    };
    let runs = move |sha: String| {
        if m2.load(Relaxed) & PARTS_FAIL != 0 {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        let run = |status: &str, conclusion: Value| json!({"status": status, "conclusion": conclusion});
        let runs = match sha.as_str() {
            "sha8" => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            "sha1" | "sha6" => vec![run("completed", json!("success")), run("completed", json!("skipped"))],
            "sha2" => vec![run("completed", json!("success")), run("completed", json!("failure"))],
            _ => vec![run("completed", json!("success")), run("in_progress", Value::Null)],
        };
        Json(json!({ "total_count": runs.len(), "check_runs": runs })).into_response()
    };
    let record = |result: &str, at: &str| Json(json!({"result": result, "recorded_at": at})).into_response();
    let contents = move |Path(path): Path<String>, Query(q): Query<HashMap<String, String>>| async move {
        if m3.load(Relaxed) & PARTS_FAIL != 0 {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        let names = ["build-1.json", "verify-1.json", "verify-2.json", "adversarial_review-1.json", "notes.txt"];
        match (path.as_str(), q.get("ref").map(String::as_str)) {
            (".sdlc/records/TRU-6", Some("sha1")) => Json(json!(names.map(|n| json!({ "name": n })))).into_response(),
            // The review failed, then Build and Verify passed again in the
            // same second: times compare by value, not as text.
            (".sdlc/records/TRU-6/build-1.json", Some("sha1")) => record("pass", "2026-11-24T03:00:00Z"),
            (".sdlc/records/TRU-6/verify-2.json", Some("sha1")) => record("pass", "2026-11-24T03:00:00.500Z"),
            (".sdlc/records/TRU-6/adversarial_review-1.json", Some("sha1")) => record("fail", "2026-11-24T02:00:00Z"),
            (".sdlc/records/TRU-11", Some("merge9")) => Json(json!([{ "name": "pr_review-1.json" }])).into_response(),
            (".sdlc/records/TRU-11/pr_review-1.json", Some("merge9")) => record("pass", "2026-11-24T04:00:00.000Z"),
            (_, Some("sha8")) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            _ => StatusCode::NOT_FOUND.into_response(),
        }
    };
    let app = Router::new()
        .route("/repos/bbooth/booking/pulls", get(pulls))
        .route("/repos/bbooth/booking/commits/{sha}/check-runs", get(move |Path(sha)| async move { runs(sha) }))
        .route("/repos/bbooth/booking/contents/{*path}", get(contents));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), mode)
}

fn repo() -> Repo {
    Repo {
        name: "bbooth/booking".into(),
        linear_team: "TRU".into(),
        linear_project: "Booking".into(),
        rollout_namespace: None,
    }
}

fn gate(stage: &str, round: u32, pass: bool, at: &str) -> Gate {
    Gate { stage: stage.into(), round, pass, at: Some(at.into()) }
}

#[tokio::test]
async fn checks_map_to_pass_fail_or_pending_and_the_latest_record_is_read() {
    let prs = GitHub::new(&fake_github().await.0, None, vec![repo()]).unwrap().prs().await.prs;

    let got: Vec<(&str, u64, Checks, bool)> =
        prs.iter().map(|p| (p.ticket.as_str(), p.pr.number, p.pr.checks, p.pr.merged)).collect();
    // The fork's #7, #5 (named only in its body), #4 and the unmerged #10 are
    // left out; #8 is kept though its reads failed.
    let tied = [
        ("TRU-9", 8, Checks::Unknown, false),
        ("TRU-6", 6, Checks::Pass, false),
        ("TRU-8", 3, Checks::Pending, false),
        ("TRU-7", 2, Checks::Fail, false),
        ("TRU-6", 1, Checks::Pass, false),
        ("TRU-11", 9, Checks::Pass, true),
    ];
    assert_eq!(got, tied);
    let by_branch: Vec<bool> = prs.iter().map(|p| p.by_branch).collect();
    assert_eq!(by_branch, [true, false, true, false, true, true]);
    let pr = |n: u64| prs.iter().find(|p| p.pr.number == n).unwrap();
    assert_eq!(pr(1).pr.url, "https://github.com/bbooth/booking/pull/1");
    // The gate recorded last wins, though an earlier stage: Verify round 2
    // passed after the review failed. Its result comes from the file at the PR's head.
    assert_eq!(pr(1).gate, Some(gate("Verify", 2, true, "2026-11-24T03:00:00.500Z")));
    assert_eq!(pr(3).gate, None);
    assert_eq!(pr(8).gate, None);
    assert_eq!(pr(9).gate, Some(gate("PR review", 1, true, "2026-11-24T04:00:00.000Z")));
}

#[tokio::test]
async fn a_card_shows_the_latest_gate_from_its_comments_or_its_branch_and_its_pr() {
    let prs = GitHub::new(&fake_github().await.0, None, vec![repo()]).unwrap().prs().await.prs;
    // Each comment is posted at hour h of 2026-11-24.
    let issue = |id: &str, comments: &[(u32, &str)]| board::Issue {
        id: id.into(),
        title: id.into(),
        url: format!("https://linear.app/t/{id}"),
        repo: "bbooth/booking".into(),
        stage: Some("PR review".into()),
        parent: None,
        open: true,
        gates: comments
            .iter()
            .flat_map(|(h, body)| {
                board_server::gates::from_comment(body, Some(&format!("2026-11-24T{h:02}:00:00.000Z")))
            })
            .collect(),
    };
    let issues = [
        issue("TRU-6", &[(0, "### Gate: Build (pass) · round 1\n- Checks: ...")]),
        // Two gates in one comment fall back to stage order.
        issue("TRU-7", &[(1, "### Gate: Build (pass) · round 3\n### Gate: Verify (fail) · round 3")]),
        issue("TRU-8", &[(1, "No gate here")]),
        // The owner sent the PR back after its review passed, and Build ran again.
        issue(
            "TRU-9",
            &[
                (5, "### Gate: PR review (pass) · round 1"),
                (6, "### Gate: PR review approval (fail) · round 1"),
                (7, "### Gate: Build (pass) · round 4"),
            ],
        ),
        // A rejected spec, revised.
        issue("TRU-10", &[(1, "### Gate: Spec approval (fail) · round 1"), (2, "### Gate: Spec (pass) · round 2")]),
        issue("TRU-12", &[(1, "### Gate: Spec (pass) · round 1"), (2, "### Gate: Spec approval (pass) · round 1")]),
    ];
    let cols = board::columns(&issues, &prs, &[]);
    let tickets = &cols.iter().find(|c| c.stage == "PR review").unwrap().tickets;
    let card = |id: &str| tickets.iter().find(|t| t.id == id).unwrap();

    let at = |h: u32| format!("2026-11-24T{h:02}:00:00.000Z");
    // TRU-6: its own PR, on the branch naming it, not #6 whose title mentions it;
    // and the branch's Verify, recorded after the comment's Build, wins.
    assert_eq!(card("TRU-6").gate, Some(gate("Verify", 2, true, "2026-11-24T03:00:00.500Z")));
    assert_eq!(card("TRU-6").pr.as_ref().map(|p| (p.number, p.checks)), Some((1, Checks::Pass)));
    assert_eq!(card("TRU-7").gate, Some(gate("Verify", 3, false, &at(1))));
    assert_eq!(card("TRU-8").gate, None);
    assert_eq!(card("TRU-8").pr.as_ref().unwrap().checks, Checks::Pending);
    assert_eq!(card("TRU-9").gate, Some(gate("Build", 4, true, &at(7))));
    assert_eq!(card("TRU-10").gate, Some(gate("Spec", 2, true, &at(2))));
    assert_eq!(card("TRU-12").gate, Some(gate("Spec approval", 1, true, &at(2))));
    // TRU-6 is in PR review with two green PRs; only its own is a merge to do.
    let merges: Vec<String> = board::needs_you(&issues, &prs).into_iter().map(|n| n.url).collect();
    assert_eq!(merges, ["https://github.com/bbooth/booking/pull/1"]);
}

// A failed checks read keeps the last value for the same commit, and the
// source reports it with the age of the oldest data it kept.
#[tokio::test]
async fn a_failed_part_keeps_its_earlier_value_and_marks_the_source() {
    let (url, mode) = fake_github().await;
    let github = GitHub::new(&url, None, vec![repo()]).unwrap();
    let pr = |read: &PrRead, n: u64| read.prs.iter().find(|p| p.pr.number == n).unwrap().clone();
    let first = github.prs().await;
    assert!(first.errors.iter().all(|e| e.starts_with("PR #8 ")), "{:?}", first.errors);
    tokio::time::sleep(Duration::from_millis(1100)).await;

    mode.store(PARTS_FAIL, Relaxed);
    let second = github.prs().await;
    // #1's records at its commit were read already, so they aren't read again.
    let pr1 = pr(&second, 1);
    assert_eq!(
        (pr1.pr.checks, pr1.gate.clone()),
        (Checks::Pass, Some(gate("Verify", 2, true, "2026-11-24T03:00:00.500Z")))
    );
    assert_eq!(pr1.read_at, pr(&first, 1).read_at);
    assert!(pr1.read_at < sources::now());
    // #8 had nothing to keep.
    assert_eq!((pr(&second, 8).pr.checks, pr(&second, 8).gate), (Checks::Unknown, None));
    let kept = |e: &String| e.starts_with("PR #1 checks: ") && e.ends_with("(kept an earlier read)");
    assert!(second.errors.iter().any(kept), "{:?}", second.errors);

    // After a push, the old commit's checks say nothing about the new one.
    mode.store(PARTS_FAIL | MOVED, Relaxed);
    let moved = github.prs().await;
    assert_eq!((pr(&moved, 1).pr.checks, pr(&moved, 1).gate), (Checks::Unknown, None));

    mode.store(LISTS_FAIL, Relaxed);
    let third = github.prs().await;
    assert_eq!(third.prs, moved.prs);
    assert!(third.errors[0].ends_with("(kept its PRs from an earlier read)"), "{:?}", third.errors);

    // With no PR list read the read fails, so the source's data doesn't look new.
    let state = sources::State::new();
    let status = || state.snapshot().sources.into_iter().find(|s| s.name == "github").unwrap();
    sources::refresh(&state, &github).await;
    assert_eq!(status().last_good, None);
    assert!(status().last_error.unwrap().starts_with("no repo's PRs could be read"));
    // A partial read is as old as the oldest data it kept: #6's checks from the first read.
    mode.store(PARTS_FAIL | MOVED, Relaxed);
    sources::refresh(&state, &github).await;
    assert_eq!(status().last_good, Some(pr(&first, 6).read_at));
    assert!(status().last_error.unwrap().contains("reads failed, first: "));

    // A repo that isn't there is an error, not a repo with no PRs.
    let gone = Repo { name: "bbooth/gone".into(), ..repo() };
    // Read twice: a failed read keeps nothing, so the second still has no earlier PRs.
    let gone = GitHub::new(&url, None, vec![gone]).unwrap();
    gone.prs().await;
    let read = gone.prs().await;
    let error = "GitHub /repos/bbooth/gone/pulls: not found (no earlier PRs)".to_string();
    assert_eq!((read.repos_read, read.errors.as_slice()), (0, [error].as_slice()));

    // Next to a working repo whose PRs also failed, the missing repo is still named.
    let both = GitHub::new(&url, None, vec![repo(), Repo { name: "bbooth/gone".into(), ..repo() }]).unwrap();
    let state = sources::State::new();
    sources::refresh(&state, &both).await;
    let github = state.snapshot().sources.into_iter().find(|s| s.name == "github").unwrap();
    let error = github.last_error.unwrap();
    assert!(error.starts_with("PR lists failed for bbooth/gone; "), "{error}");
}

// Without a token the next read waits long enough for reads like the last
// ones to stay within GitHub's 60 requests an hour.
#[tokio::test]
async fn without_a_token_reads_are_spaced_by_what_they_cost() {
    let url = fake_github().await.0;
    let github = GitHub::new(&url, None, vec![repo()]).unwrap();
    github.prs().await;
    let wait = github.interval();
    // 17 requests: two lists, checks for 5 tied open PRs, a records folder
    // for each of the 6 tied PRs, and the top round of each stage there (3 + 1).
    assert_eq!(wait, Duration::from_secs(17 * 3600 / 45));
    assert_eq!(github.interval(), sources::INTERVAL, "the count starts again after each interval");

    // Records at a commit never change, so the next read sends only the
    // lists, the checks, and #8's records, whose read failed.
    github.prs().await;
    assert_eq!(github.interval(), Duration::from_secs(8 * 3600 / 45));

    // One read never sends more than the budget; what it couldn't send is reported.
    let capped = GitHub::new(&url, None, vec![repo()]).unwrap().with_budget(5);
    let read = capped.prs().await;
    assert!(read.errors.iter().any(|e| e.contains("not sent, this read used its 5 requests")), "{:?}", read.errors);
    assert_eq!(capped.interval(), Duration::from_secs(3600));
}
