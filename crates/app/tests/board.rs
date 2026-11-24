use board_api::Snapshot;
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

const NEEDS_YOU: &str = r#"{"generated_at": 0, "sources": [], "columns": [{"stage": "Build", "tickets": [
  {"id": "TRU-8", "title": "One board", "url": "u", "repo": "r", "stage": "Build", "children": [
    {"id": "TRU-9", "title": "a", "url": "u", "repo": "r", "stage": "PR review", "children": [],
     "gate": {"stage": "Adversarial review", "round": 2, "pass": true},
     "pr": {"number": 4, "title": "TRU-9: a", "url": "https://github.com/b/s/pull/4", "checks": "pending"}}]}]}],
  "needs_you": [{"kind": "approve", "id": "TRU-20", "title": "A spec", "url": "https://linear.app/t/TRU-20"},
    {"kind": "merge", "id": "TRU-6", "title": "TRU-6: fields", "url": "https://github.com/b/s/pull/1"},
    {"kind": "blocked", "id": "TRU-11", "title": "Stuck", "url": "https://linear.app/t/TRU-11"}]}"#;

// AC-2 and AC-3, app side: what needs the owner is listed above the columns
// with links, and a sub-ticket's row shows its latest gate and its PR.
#[test]
fn needs_you_is_listed_first_and_cards_show_their_gate_and_pr() {
    let snap: Snapshot = serde_json::from_str(NEEDS_YOU).unwrap();
    let harness = Harness::new_ui(|ui| board_app::board(ui, &snap));
    let top = |text: &str| harness.get_by_label(text).rect().top();
    let column = top("Build (1)");
    for text in ["Approve the spec: TRU-20 A spec", "Merge: TRU-6 TRU-6: fields", "Blocked: TRU-11 Stuck"] {
        assert!(top(text) < column, "{text} is not above the columns");
    }
    harness.get_by_label("Adversarial review round 2: pass");
    harness.get_by_label("PR #4 checks pending");
}
