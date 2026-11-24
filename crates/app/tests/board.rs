use board_api::{Checks, Pr, Snapshot};
use egui_kittest::{kittest::Queryable, Harness};
use std::time::{Duration, Instant};

const SNAPSHOT: &str = r#"{"generated_at": 0, "sources": [], "columns": [{"stage": "Spec", "tickets": []}, {"stage": "Build",
  "tickets": [{"id": "TRU-8", "title": "One board", "url": "u", "repo": "r", "stage": "Build", "children": [
    {"id": "TRU-9", "title": "a", "url": "u", "repo": "r", "stage": "Verify", "children": [
      {"id": "TRU-10", "title": "b", "url": "u", "repo": "r", "stage": "Plan", "children": []}]}]}]}]}"#;

// AC-1, app side: each stage gets a column and a ticket's sub-tickets are
// drawn under it with their own stages.
#[test]
fn sub_tickets_are_drawn_under_their_ticket_with_their_stages() {
    let snap: Snapshot = serde_json::from_str(SNAPSHOT).unwrap();
    let harness = Harness::new_ui(|ui| board_app::board(ui, &snap, 1000));
    for text in ["Spec (0)", "Build (1)", "TRU-8", "One board", "TRU-9", "· Verify", "TRU-10", "· Plan"] {
        harness.get_by_label(text);
    }
}

const NEEDS_YOU: &str = r#"{"generated_at": 1000, "sources": [{"name": "linear", "last_good": 1000, "last_error": null},
    {"name": "github", "last_good": 400, "last_error": "GitHub /repos/b/s/pulls: 403 rate limited"}],
  "columns": [{"stage": "Build", "tickets": [
  {"id": "TRU-8", "title": "One board", "url": "u", "repo": "bbooth/sdlc-board", "stage": "Build",
   "gate": {"stage": "Plan", "round": 1, "pass": true}, "children": [
    {"id": "TRU-9", "title": "a", "url": "u", "repo": "bbooth/booking", "stage": "PR review", "children": [],
     "gate": {"stage": "Adversarial review", "round": 2, "pass": true},
     "pr": {"number": 4, "title": "TRU-9: a", "url": "https://github.com/b/s/pull/4", "checks": "pending"}},
    {"id": "TRU-7", "title": "c", "url": "u", "repo": "r", "stage": "Rollout", "children": [],
     "pr": {"number": 2, "title": "TRU-7: c", "url": "https://github.com/b/s/pull/2", "checks": "pass", "merged": true}}]}]}],
  "needs_you": [{"kind": "approve", "id": "TRU-20", "title": "A spec", "url": "https://linear.app/t/TRU-20"},
    {"kind": "merge", "id": "TRU-6", "title": "TRU-6: fields", "url": "https://github.com/b/s/pull/1"},
    {"kind": "blocked", "id": "TRU-11", "title": "Stuck", "url": "https://linear.app/t/TRU-11"},
    {"kind": "watch", "id": "TRU-30", "title": "Later", "url": "https://linear.app/t/TRU-30"}]}"#;

// AC-2 and AC-3, app side: what needs the owner is listed above the columns
// with links, a sub-ticket's row shows its latest gate and its PR, and a
// source whose read failed says so with the age of its data.
#[test]
fn needs_you_is_listed_first_and_cards_show_their_gate_and_pr() {
    let snap: Snapshot = serde_json::from_str(NEEDS_YOU).unwrap();
    let harness = Harness::new_ui(|ui| board_app::board(ui, &snap, 1000));
    let top = |text: &str| harness.get_by_label(text).rect().top();
    let column = top("Build (1)");
    let items = [
        "Approve the spec: TRU-20 A spec",
        "Merge: TRU-6 TRU-6: fields",
        "Blocked: TRU-11 Stuck",
        "watch: TRU-30 Later",
    ];
    for text in items {
        assert!(top(text) < column, "{text} is not above the columns");
    }
    harness.get_by_label("Plan round 1: pass");
    harness.get_by_label("Adversarial review round 2: pass");
    harness.get_by_label("PR #4 checks pending");
    harness.get_by_label("PR #2 merged");
    // AC-7, app side: a failing source is marked stale with when it was last read and why.
    let stale = "github is stale (last good read 00:06 UTC, 10 min ago): GitHub /repos/b/s/pulls: 403 rate limited";
    assert!(top(stale) < column);
    assert!(harness.query_by_label_contains("linear is stale").is_none());
    // AC-6, app side: when each source's data was last refreshed.
    harness.get_by_label("Last refreshed: linear 0 s ago, github 10 min ago");
    // AC-5, app side: the card names its repo, and a sub-ticket from another repo names its own.
    harness.get_by_label("bbooth/sdlc-board");
    harness.get_by_label("bbooth/booking");
}

// AC-7, app side: a failed read of the snapshot keeps the board it had.
#[test]
fn a_failed_read_keeps_the_board_it_had() {
    let (mut reads, t) = (board_app::Reads::default(), Instant::now());
    let n = reads.start(t, false).unwrap();
    reads.finish(n, Ok(serde_json::from_str(SNAPSHOT).unwrap()));
    let n = reads.start(t + secs(30), false).unwrap();
    reads.finish(n, Err("connection refused".into()));
    assert_eq!(reads.snap.as_ref().map(|s| s.columns.len()), Some(2));
    assert_eq!(reads.problem(t + secs(31)).as_deref(), Some("connection refused"));
}

// Ages are told by the server's clock, so a device clock that's off doesn't
// make stale data look fresh: the snapshot's time plus the time since it came back.
#[test]
fn ages_go_by_the_servers_clock() {
    let mut reads = board_app::Reads::default();
    assert_eq!(reads.server_now(Instant::now()), None);
    let n = reads.start(Instant::now(), false).unwrap();
    let mut snap: board_api::Snapshot = serde_json::from_str(SNAPSHOT).unwrap();
    snap.generated_at = 5000;
    reads.finish(n, Ok(snap));
    assert_eq!(reads.server_now(Instant::now() + secs(90)), Some(5090));
}

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

// AC-6, app side: the app reads the snapshot every 30 seconds, one at a
// time; a read the server never answers is given up on after 20, so the
// board can't freeze on it, and its late answer can't replace a newer board.
#[test]
fn the_board_refreshes_itself_every_30_seconds() {
    let (mut reads, t) = (board_app::Reads::default(), Instant::now());
    let first = reads.start(t, false).unwrap();
    assert!(reads.waiting(t + secs(10)));
    assert_eq!(reads.start(t + secs(10), true), None, "Refresh waits for the answer");
    reads.finish(first, Err("refused".into()));
    assert_eq!(reads.start(t + secs(20), false), None);
    let asked = reads.start(t + secs(21), true).expect("Refresh reads now once answered");
    reads.finish(asked, Err("refused".into()));
    let hung = reads.start(t + secs(51), false).unwrap();
    assert_eq!(reads.problem(t + secs(76)).as_deref(), Some("no answer for 25 s"));
    assert!(!reads.waiting(t + secs(76)));
    let next = reads.start(t + secs(81), false).unwrap();
    reads.finish(next, Ok(serde_json::from_str(SNAPSHOT).unwrap()));
    reads.finish(hung, Err("late".into()));
    assert_eq!((reads.snap.is_some(), reads.problem(t + secs(82))), (true, None));
    // A given-up read's answer still counts when nothing newer has answered.
    let slow = reads.start(t + secs(111), false).unwrap();
    let _hangs = reads.start(t + secs(141), false).unwrap();
    reads.finish(slow, Err("slow but real".into()));
    assert_eq!(reads.problem(t + secs(142)).as_deref(), Some("slow but real"));
}

// A check state from a newer server reads as unknown rather than failing the whole snapshot.
#[test]
fn an_unknown_check_state_reads_as_unknown() {
    let pr: Pr = serde_json::from_str(r#"{"number": 1, "title": "t", "url": "u", "checks": "neutral"}"#).unwrap();
    assert_eq!(pr.checks, Checks::Unknown);
}
