//! How the board's lines are worded, so every client says the same thing.

use board_api::{Checks, Gate, NeedsYou, Pr, Rollout, SourceStatus};

/// `secs` as "12 s ago", "5 min ago" or "3 h ago".
pub fn ago(secs: i64) -> String {
    match secs.max(0) {
        s if s < 60 => format!("{s} s ago"),
        s if s < 3600 => format!("{} min ago", s / 60),
        s => format!("{} h ago", s / 3600),
    }
}

/// "Last refreshed: linear 0 s ago, github 10 min ago". `now` is Unix seconds.
pub fn refreshed(sources: &[SourceStatus], now: i64) -> String {
    let read = |s: &SourceStatus| format!("{} {}", s.name, s.last_good.map_or("never".into(), |t| ago(now - t)));
    format!("Last refreshed: {}", sources.iter().map(read).collect::<Vec<_>>().join(", "))
}

/// A failing source with its last good read and error, or `None` if it's fine.
pub fn stale(s: &SourceStatus, now: i64) -> Option<String> {
    let err = s.last_error.as_ref()?;
    let when = match s.last_good {
        Some(t) => format!("last good read {:02}:{:02} UTC, {}", t % 86400 / 3600, t % 3600 / 60, ago(now - t)),
        None => "no good read yet".into(),
    };
    Some(format!("{} is stale ({when}): {err}", s.name))
}

/// "Needs you (3)".
pub fn needs_you_heading(items: &[NeedsYou]) -> String {
    format!("Needs you ({})", items.len())
}

/// "Approve the spec: TRU-20 A spec".
pub fn needs_you(n: &NeedsYou) -> String {
    let what = match n.kind.as_str() {
        "approve" => "Approve the spec",
        "merge" => "Merge",
        "blocked" => "Blocked",
        // A kind from a newer server: show it as it is rather than mislabel it.
        other => other,
    };
    format!("{what}: {} {}", n.id, n.title)
}

/// "Build (3)".
pub fn column(stage: &str, tickets: usize) -> String {
    format!("{stage} ({tickets})")
}

/// "Plan round 1: pass".
pub fn gate(g: &Gate) -> String {
    format!("{} round {}: {}", g.stage, g.round, if g.pass { "pass" } else { "fail" })
}

/// "PR #4 checks pending", or "PR #2 merged".
pub fn pr(p: &Pr) -> String {
    let state = match (p.merged, p.checks) {
        (true, _) => "merged",
        (_, Checks::Pass) => "checks pass",
        (_, Checks::Fail) => "checks fail",
        (_, Checks::Pending) => "checks pending",
        (_, Checks::Unknown) => "checks unknown",
    };
    format!("PR #{} {state}", p.number)
}

/// "booking: step 2 of 5 at 20%, Paused, analysis Running".
pub fn rollout(r: &Rollout) -> String {
    let analysis = r.analysis.as_ref().map_or(String::new(), |a| format!(", analysis {a}"));
    format!("{}: step {} of {} at {}%, {}{analysis}", r.name, r.step, r.steps, r.weight, r.phase)
}

/// Whether a rollout is going wrong, so it's drawn as an error.
pub fn rollout_failing(r: &Rollout) -> bool {
    matches!(r.phase.as_str(), "Aborted" | "Degraded")
        || matches!(r.analysis.as_deref(), Some("Failed" | "Error" | "Inconclusive"))
}
