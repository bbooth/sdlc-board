//! Gate results, from `.sdlc/records/` file names or from the gate output
//! posted on a Linear ticket.

use board_api::Gate;

/// The gates in pipeline order: the record file's key and the name the gate prints.
#[rustfmt::skip]
const GATES: [(&str, &str); 8] = [
    ("spec", "Spec"), ("plan", "Plan"), ("build", "Build"), ("verify", "Verify"),
    ("adversarial_review", "Adversarial review"), ("pr_review", "PR review"),
    ("rollout", "Rollout"), ("outcome", "Outcome"),
];

pub fn order(stage: &str) -> usize {
    GATES.iter().position(|(_, name)| *name == stage).unwrap_or(0)
}

/// The latest gate: the furthest stage, then the highest round there.
pub fn latest<'a>(gates: impl IntoIterator<Item = &'a Gate>) -> Option<Gate> {
    gates.into_iter().max_by_key(|g| (order(&g.stage), g.round)).cloned()
}

/// `verify-2.json` is round 2 of Verify.
pub fn from_file_name(name: &str) -> Option<(String, u32)> {
    let (key, round) = name.strip_suffix(".json")?.rsplit_once('-')?;
    let (_, stage) = GATES.iter().find(|(k, _)| *k == key)?;
    Some(((*stage).into(), round.parse().ok()?))
}

/// The gate headers in a comment, such as `### Gate: Verify (pass) · round 2`.
pub fn from_comment(body: &str) -> Vec<Gate> {
    let gate = |line: &str| {
        let (stage, rest) = line.trim().strip_prefix("### Gate: ")?.split_once(" (")?;
        let (result, rest) = rest.split_once(')')?;
        let round = rest.trim().strip_prefix("· round ")?.trim().parse().ok()?;
        GATES.iter().any(|(_, n)| *n == stage).then(|| Gate { stage: stage.into(), round, pass: result == "pass" })
    };
    body.lines().filter_map(gate).collect()
}
