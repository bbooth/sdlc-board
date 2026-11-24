//! Gate results, from `.sdlc/records/` file names or from the gate output
//! posted on a Linear ticket.

use board_api::Gate;

/// The gates in pipeline order: the record file's key and the name the gate
/// prints. The owner's decisions print as `Spec approval` and so on.
#[rustfmt::skip]
const GATES: [(&str, &str); 9] = [
    ("intake", "Intake"), ("spec", "Spec"), ("plan", "Plan"), ("build", "Build"), ("verify", "Verify"),
    ("adversarial_review", "Adversarial review"), ("pr_review", "PR review"),
    ("rollout", "Rollout"), ("outcome", "Outcome"),
];

/// An owner's approval comes right after the gate it approves.
pub fn order(stage: &str) -> usize {
    let (name, approval) = stage.strip_suffix(" approval").map_or((stage, 0), |s| (s, 1));
    GATES.iter().position(|(_, n)| *n == name).map_or(0, |i| 2 * i + approval)
}

fn known(stage: &str) -> bool {
    let name = stage.strip_suffix(" approval").unwrap_or(stage);
    GATES.iter().any(|(_, n)| *n == name)
}

/// The latest gate: the one recorded last, since a ticket sent back to an
/// earlier stage records gates there again. Gates with no time, or recorded in
/// the same comment, fall back to stage order, then round.
pub fn latest<'a>(gates: impl IntoIterator<Item = &'a Gate>) -> Option<Gate> {
    gates.into_iter().max_by_key(|g| (time(g.at.as_deref()), order(&g.stage), g.round)).cloned()
}

/// A UTC RFC 3339 time as sortable digits, so `07:10:14Z` and
/// `07:10:14.744Z` compare by value. None sorts first.
fn time(at: Option<&str>) -> Option<String> {
    at.map(|t| format!("{:0<17}", t.chars().filter(char::is_ascii_digit).take(17).collect::<String>()))
}

/// `verify-2.json` is round 2 of Verify.
pub fn from_file_name(name: &str) -> Option<(String, u32)> {
    let (key, round) = name.strip_suffix(".json")?.rsplit_once('-')?;
    let (_, stage) = GATES.iter().find(|(k, _)| *k == key)?;
    Some(((*stage).into(), round.parse().ok()?))
}

/// The gate headers in a comment, such as `### Gate: Verify (pass) · round 2`,
/// with the time the comment was posted.
pub fn from_comment(body: &str, at: Option<&str>) -> Vec<Gate> {
    let gate = |line: &str| {
        let (stage, rest) = line.trim().strip_prefix("### Gate: ")?.split_once(" (")?;
        let (result, rest) = rest.split_once(')')?;
        let round = rest.trim().strip_prefix("· round ")?.trim().parse().ok()?;
        known(stage).then(|| Gate { stage: stage.into(), round, pass: result == "pass", at: at.map(String::from) })
    };
    body.lines().filter_map(gate).collect()
}
