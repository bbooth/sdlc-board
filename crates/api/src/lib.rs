//! The board snapshot: what the server serves at `/v1/snapshot` and every
//! build of the app draws. Shared so the two can't drift apart.

use serde::{Deserialize, Serialize};

/// The pipeline's stages in board order, as named by the Stage label group.
#[rustfmt::skip]
pub const STAGES: [&str; 11] = [
    "Intake", "Spec", "Ready for approval", "Plan", "Build", "Verify",
    "Adversarial review", "PR review", "Rollout", "Outcome", "Blocked",
];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Unix seconds when the server built this snapshot.
    pub generated_at: i64,
    pub sources: Vec<SourceStatus>,
    /// One per entry of [`STAGES`], in that order.
    pub columns: Vec<Column>,
    /// What waits on the owner, shown above the columns.
    #[serde(default)]
    pub needs_you: Vec<NeedsYou>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceStatus {
    pub name: String,
    /// Unix seconds when the oldest data shown was read: the last read that
    /// succeeded, or older parts of it kept after their reads failed.
    pub last_good: Option<i64>,
    /// What failed in the last read, in whole or in part.
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Column {
    pub stage: String,
    pub tickets: Vec<Ticket>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ticket {
    /// The tracker's identifier, e.g. `TRU-8`.
    pub id: String,
    pub title: String,
    pub url: String,
    /// The repo from the board's settings this ticket belongs to.
    pub repo: String,
    pub stage: String,
    pub children: Vec<Ticket>,
    #[serde(default)]
    pub gate: Option<Gate>,
    #[serde(default)]
    pub pr: Option<Pr>,
}

// Fields added to /v1 later carry #[serde(default)], so an app can read an
// older server.

/// A gate record's result: `stage` is a gate name such as `Verify`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gate {
    pub stage: String,
    pub round: u32,
    pub pass: bool,
    /// When it was recorded (RFC 3339, UTC), if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pr {
    pub number: u64,
    pub title: String,
    pub url: String,
    /// Its checks while open; meaningless once `merged`.
    pub checks: Checks,
    #[serde(default)]
    pub merged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Checks {
    Pass,
    Fail,
    Pending,
    /// Not read (its read failed with nothing earlier to keep), or a state
    /// from a newer server.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NeedsYou {
    /// `approve` (a spec), `merge` (a green PR) or `blocked`.
    pub kind: String,
    pub id: String,
    pub title: String,
    pub url: String,
}
