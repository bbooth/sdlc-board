use board_api::{Checks, Pr, Snapshot};
use egui_kittest::{kittest::Queryable, Harness};

const SNAPSHOT: &str = r#"{"generated_at": 0, "sources": [], "columns": [{"stage": "Spec", "tickets": []}, {"stage": "Build",
  "tickets": [{"id": "TRU-8", "title": "One board", "url": "u", "repo": "r", "stage": "Build", "children": [
    {"id": "TRU-9", "title": "a", "url": "u", "repo": "r", "stage": "Verify", "children": [
      {"id": "TRU-10", "title": "b", "url": "u", "repo": "r", "stage": "Plan", "children": []}]}]}]}]}"#;

// AC-1, app side: each stage gets a column and a ticket's sub-tickets are
// drawn under it with their own stages.
#[test]
fn sub_tickets_are_drawn_under_their_ticket_with_their_stages() {
    let snap: Snapshot = serde_json::from_str(SNAPSHOT).unwrap();
    let harness = Harness::new_ui(|ui| board_app::board(ui, &snap));
    for text in ["Spec (0)", "Build (1)", "TRU-8", "One board", "TRU-9", "· Verify", "TRU-10", "· Plan"] {
        harness.get_by_label(text);
    }
}

const NEEDS_YOU: &str = r#"{"generated_at": 1000, "sources": [{"name": "linear", "last_good": 1000, "last_error": null},
    {"name": "github", "last_good": 400, "last_error": "GitHub /repos/b/s/pulls: 403 rate limited"}],
  "columns": [{"stage": "Build", "tickets": [
  {"id": "TRU-8", "title": "One board", "url": "u", "repo": "r", "stage": "Build",
   "gate": {"stage": "Plan", "round": 1, "pass": true}, "children": [
    {"id": "TRU-9", "title": "a", "url": "u", "repo": "r", "stage": "PR review", "children": [],
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
    let harness = Harness::new_ui(|ui| board_app::board(ui, &snap));
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
    let stale = "github: data is 10 min old: GitHub /repos/b/s/pulls: 403 rate limited";
    assert!(top(stale) < column);
    assert!(harness.query_by_label_contains("linear: ").is_none());
}

// A check state from a newer server reads as unknown rather than failing the whole snapshot.
#[test]
fn an_unknown_check_state_reads_as_unknown() {
    let pr: Pr = serde_json::from_str(r#"{"number": 1, "title": "t", "url": "u", "checks": "neutral"}"#).unwrap();
    assert_eq!(pr.checks, Checks::Unknown);
}
