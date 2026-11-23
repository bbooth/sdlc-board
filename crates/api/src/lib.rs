//! The board snapshot: what the server serves at `/v1/snapshot` and every
//! build of the app draws. Shared so the two can't drift apart.

use serde::{Deserialize, Serialize};

/// The pipeline's stages in board order, as named by the Stage label group.
pub const STAGES: [&str; 11] = [
    "Intake",
    "Spec",
    "Ready for approval",
    "Plan",
    "Build",
    "Verify",
    "Adversarial review",
    "PR review",
    "Rollout",
    "Outcome",
    "Blocked",
];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Unix seconds when the server built this snapshot.
    pub generated_at: i64,
    pub sources: Vec<SourceStatus>,
    /// One per entry of [`STAGES`], in that order.
    pub columns: Vec<Column>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceStatus {
    pub name: String,
    /// Unix seconds of the last read that succeeded, if any has.
    pub last_good: Option<i64>,
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
}
