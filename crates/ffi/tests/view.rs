//! What the Android app is handed to lay out, from a recorded snapshot: the
//! same snapshot and lines the egui render tests in crates/app check.

use board_api::Snapshot;
use board_ffi::{board_view, settings_problem, settings_trimmed, Link, RolloutLine, Settings, SubTicket};

const NEEDS_YOU: &str = r#"{"generated_at": 1000, "sources": [{"name": "linear", "last_good": 1000, "last_error": null},
    {"name": "github", "last_good": 400, "last_error": "GitHub /repos/b/s/pulls: 403 rate limited"}],
  "columns": [{"stage": "Spec", "tickets": []}, {"stage": "Build", "tickets": [
  {"id": "TRU-8", "title": "One board", "url": "u8", "repo": "bbooth/sdlc-board", "stage": "Build",
   "gate": {"stage": "Plan", "round": 1, "pass": true}, "children": [
    {"id": "TRU-9", "title": "a", "url": "u9", "repo": "bbooth/booking", "stage": "PR review", "children": [
      {"id": "TRU-10", "title": "b", "url": "u10", "repo": "bbooth/booking", "stage": "Plan", "children": [],
       "rollouts": [{"name": "web", "step": 2, "steps": 5, "weight": 20, "phase": "Aborted"}]}],
     "gate": {"stage": "Adversarial review", "round": 2, "pass": true},
     "pr": {"number": 4, "title": "TRU-9: a", "url": "https://github.com/b/s/pull/4", "checks": "pending"}},
    {"id": "TRU-7", "title": "c", "url": "u7", "repo": "bbooth/sdlc-board", "stage": "Rollout", "children": [],
     "pr": {"number": 2, "title": "TRU-7: c", "url": "https://github.com/b/s/pull/2", "checks": "pass", "merged": true}}]}]}],
  "needs_you": [{"kind": "approve", "id": "TRU-20", "title": "A spec", "url": "https://linear.app/t/TRU-20"},
    {"kind": "merge", "id": "TRU-6", "title": "TRU-6: fields", "url": "https://github.com/b/s/pull/1"}]}"#;

#[test]
fn the_view_carries_the_boards_lines_already_worded() {
    let snap: Snapshot = serde_json::from_str(NEEDS_YOU).unwrap();
    let v = board_view(&snap, 1000);
    assert_eq!(v.refreshed, "Last refreshed: linear 0 s ago, github 10 min ago");
    assert_eq!(
        v.stale,
        ["github is stale (last good read 00:06 UTC, 10 min ago): GitHub /repos/b/s/pulls: 403 rate limited"]
    );
    assert_eq!(v.needs_you_heading.as_deref(), Some("Needs you (2)"));
    assert_eq!(
        v.needs_you[0],
        Link { text: "Approve the spec: TRU-20 A spec".into(), url: "https://linear.app/t/TRU-20".into() }
    );
    let titles: Vec<_> = v.columns.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, ["Spec (0)", "Build (1)"]);

    let card = &v.columns[1].cards[0];
    assert_eq!((card.id.as_str(), card.repo.as_str()), ("TRU-8", "bbooth/sdlc-board"));
    assert_eq!(card.details.gate.as_deref(), Some("Plan round 1: pass"));
    let subs: Vec<_> = card.sub_tickets.iter().map(|s| (s.depth, s.id.as_str(), s.stage.as_str())).collect();
    assert_eq!(subs, [(0, "TRU-9", "· PR review"), (1, "TRU-10", "· Plan"), (0, "TRU-7", "· Rollout")]);
}

#[test]
fn a_sub_ticket_names_its_repo_only_when_it_differs_from_its_parents() {
    let snap: Snapshot = serde_json::from_str(NEEDS_YOU).unwrap();
    let subs: Vec<SubTicket> = board_view(&snap, 1000).columns[1].cards[0].sub_tickets.clone();
    // TRU-9 is in booking under an sdlc-board card; TRU-10 is in booking under TRU-9.
    let repos: Vec<_> = subs.iter().map(|s| s.repo.as_deref()).collect();
    assert_eq!(repos, [Some("bbooth/booking"), None, None]);
    assert_eq!(subs[0].details.gate.as_deref(), Some("Adversarial review round 2: pass"));
    let pr = subs[0].details.pr.as_ref().unwrap();
    assert_eq!((pr.text.as_str(), pr.url.as_str()), ("PR #4 checks pending", "https://github.com/b/s/pull/4"));
    assert_eq!(subs[2].details.pr.as_ref().unwrap().text, "PR #2 merged");
    assert_eq!(
        subs[1].details.rollouts,
        [RolloutLine { text: "web: step 2 of 5 at 20%, Aborted".into(), failing: true }]
    );
}

#[test]
fn nothing_needs_you_gives_no_heading() {
    let snap: Snapshot = serde_json::from_str(r#"{"generated_at": 0, "sources": [], "columns": []}"#).unwrap();
    let v = board_view(&snap, 0);
    assert_eq!((v.needs_you_heading, v.needs_you.len()), (None, 0));
}

#[test]
fn settings_are_trimmed_and_checked_by_the_core() {
    let s = |url: &str, id: &str, secret: &str| Settings {
        url: url.into(),
        client_id: id.into(),
        client_secret: secret.into(),
    };
    assert_eq!(settings_trimmed(s(" https://b \n", " id", "sec ")), s("https://b", "id", "sec"));
    assert_eq!(
        settings_problem(s("https://b", "id", "")).as_deref(),
        Some("set both the Access client ID and secret, or neither")
    );
    assert_eq!(settings_problem(s("", "", "")).as_deref(), Some("set the board's address"));
    assert_eq!(settings_problem(s("https://b", "id", "sec")), None);
}
