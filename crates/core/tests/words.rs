//! The card wording every client shows. The egui render tests check it
//! drawn; these pin it for the clients that draw it themselves (Android).

use board_api::{Checks, Gate, NeedsYou, Pr, Rollout, SourceStatus};
use board_core::words;

#[test]
fn ages_read_in_seconds_minutes_or_hours() {
    assert_eq!(words::ago(-5), "0 s ago");
    assert_eq!(words::ago(59), "59 s ago");
    assert_eq!(words::ago(600), "10 min ago");
    assert_eq!(words::ago(7200), "2 h ago");
}

fn source(name: &str, last_good: Option<i64>, last_error: Option<&str>) -> SourceStatus {
    SourceStatus { name: name.into(), last_good, last_error: last_error.map(Into::into) }
}

#[test]
fn sources_say_how_old_their_data_is_and_why_they_are_stale() {
    let sources = [
        source("linear", Some(1000), None),
        source("github", Some(400), Some("403")),
        source("k8s", None, Some("down")),
    ];
    assert_eq!(words::refreshed(&sources, 1000), "Last refreshed: linear 0 s ago, github 10 min ago, k8s never");
    assert_eq!(words::stale(&sources[0], 1000), None);
    assert_eq!(words::stale(&sources[1], 1000).unwrap(), "github is stale (last good read 00:06 UTC, 10 min ago): 403");
    assert_eq!(words::stale(&sources[2], 1000).unwrap(), "k8s is stale (no good read yet): down");
}

#[test]
fn needs_you_lines_name_what_to_do() {
    let item = |kind: &str| NeedsYou { kind: kind.into(), id: "TRU-1".into(), title: "T".into(), url: "u".into() };
    let items: Vec<_> = ["approve", "merge", "blocked", "watch"].map(item).into();
    assert_eq!(words::needs_you_heading(&items), "Needs you (4)");
    let lines: Vec<_> = items.iter().map(words::needs_you).collect();
    assert_eq!(lines, ["Approve the spec: TRU-1 T", "Merge: TRU-1 T", "Blocked: TRU-1 T", "watch: TRU-1 T"]);
}

#[test]
fn columns_gates_and_prs_read_as_on_the_board() {
    assert_eq!(words::column("Build", 3), "Build (3)");
    assert_eq!(words::gate(&Gate { stage: "Plan".into(), round: 1, pass: true, at: None }), "Plan round 1: pass");
    assert_eq!(words::gate(&Gate { stage: "Verify".into(), round: 2, pass: false, at: None }), "Verify round 2: fail");
    let pr = |checks, merged| words::pr(&Pr { number: 4, title: "t".into(), url: "u".into(), checks, merged });
    assert_eq!(pr(Checks::Pass, false), "PR #4 checks pass");
    assert_eq!(pr(Checks::Fail, false), "PR #4 checks fail");
    assert_eq!(pr(Checks::Pending, false), "PR #4 checks pending");
    assert_eq!(pr(Checks::Unknown, false), "PR #4 checks unknown");
    assert_eq!(pr(Checks::Fail, true), "PR #4 merged");
}

#[test]
fn rollouts_show_their_step_and_whether_they_are_failing() {
    let r = |phase: &str, analysis: Option<&str>| Rollout {
        name: "web".into(),
        step: 2,
        steps: 5,
        weight: 20,
        phase: phase.into(),
        analysis: analysis.map(Into::into),
    };
    assert_eq!(words::rollout(&r("Paused", Some("Running"))), "web: step 2 of 5 at 20%, Paused, analysis Running");
    assert_eq!(words::rollout(&r("Aborted", None)), "web: step 2 of 5 at 20%, Aborted");
    assert!(!words::rollout_failing(&r("Paused", Some("Running"))));
    assert!(words::rollout_failing(&r("Aborted", None)));
    assert!(words::rollout_failing(&r("Degraded", None)));
    assert!(words::rollout_failing(&r("Progressing", Some("Failed"))));
}
