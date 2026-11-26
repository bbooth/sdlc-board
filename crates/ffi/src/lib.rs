//! `board-core` for Kotlin, through UniFFI (ADR 0002). The Android app
//! holds a [`Board`], asks it to read when due, and lays out the
//! [`BoardView`] it returns: every line in it is already worded by
//! `board_core::words`, so the phone says what the desktop and web say.

use board_api::{Snapshot, Ticket};
use board_core::{words, Reads};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

uniffi::setup_scaffolding!();

/// Where to read the board and the Access service token to send.
#[derive(Clone, PartialEq, Eq, uniffi::Record)]
pub struct Settings {
    pub url: String,
    pub client_id: String,
    pub client_secret: String,
}

// The core's, so the secret stays out of logs here too.
impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        board_core::Settings::from(self.clone()).fmt(f)
    }
}

impl From<Settings> for board_core::Settings {
    fn from(s: Settings) -> Self {
        board_core::Settings { url: s.url, client_id: s.client_id, client_secret: s.client_secret }
    }
}

impl From<board_core::Settings> for Settings {
    fn from(s: board_core::Settings) -> Self {
        Settings { url: s.url, client_id: s.client_id, client_secret: s.client_secret }
    }
}

/// The settings without the spaces a paste often brings.
#[uniffi::export]
pub fn settings_trimmed(settings: Settings) -> Settings {
    board_core::Settings::from(settings).trimmed().into()
}

/// Why these settings can't read a board, or null if they can.
#[uniffi::export]
pub fn settings_problem(settings: Settings) -> Option<String> {
    board_core::Settings::from(settings).check().err()
}

/// The version of the core the app was built with, e.g. "0.1.0".
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").into()
}

/// The board as last read, and why it may be out of date.
#[derive(uniffi::Object)]
pub struct Board {
    settings: board_core::Settings,
    reads: Mutex<Reads>,
}

#[uniffi::export]
impl Board {
    #[uniffi::constructor]
    pub fn new(settings: Settings) -> Arc<Board> {
        Arc::new(Board { settings: board_core::Settings::from(settings).trimmed(), reads: Mutex::default() })
    }

    /// Reads the snapshot if a read is due, or now if `asked` and none is
    /// waiting, blocking up to `GIVE_UP`; call it off the main thread.
    /// Returns whether it read. A failed read keeps the last board.
    pub fn refresh(&self, asked: bool) -> bool {
        let Some(n) = self.reads().start(Instant::now(), asked) else {
            return false;
        };
        // Not under the lock: the view stays readable while this waits.
        let read = board_core::read(&self.settings, "android").map_err(|e| e.to_string());
        self.reads().finish(n, read);
        true
    }

    /// What to show now.
    pub fn view(&self) -> View {
        let reads = self.reads();
        let now = Instant::now();
        let board = reads.snap.as_ref().map(|s| board_view(s, reads.server_now(now).unwrap_or(s.generated_at)));
        View { board, problem: reads.problem(now), waiting: reads.waiting(now) }
    }
}

impl Board {
    fn reads(&self) -> MutexGuard<'_, Reads> {
        self.reads.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct View {
    /// The last board read, or null before the first good read.
    pub board: Option<BoardView>,
    /// Why the board may be out of date: the last read failed, or hasn't answered.
    pub problem: Option<String>,
    /// Whether a read is waiting on its answer.
    pub waiting: bool,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BoardView {
    /// "Last refreshed: linear 0 s ago, github 10 min ago".
    pub refreshed: String,
    /// One line per source whose last read failed.
    pub stale: Vec<String>,
    /// "Needs you (3)", or null when nothing does.
    pub needs_you_heading: Option<String>,
    pub needs_you: Vec<Link>,
    pub columns: Vec<ColumnView>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Link {
    pub text: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ColumnView {
    /// "Build (3)".
    pub title: String,
    pub cards: Vec<Card>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Card {
    pub id: String,
    pub title: String,
    pub url: String,
    pub repo: String,
    pub details: Details,
    /// Every sub-ticket under the card, depth first.
    pub sub_tickets: Vec<SubTicket>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SubTicket {
    /// 0 for the card's own sub-tickets, 1 for theirs, and so on.
    pub depth: u32,
    pub id: String,
    pub url: String,
    /// "· Verify".
    pub stage: String,
    /// Its repo when it isn't its parent's (repos share a Linear team).
    pub repo: Option<String>,
    pub details: Details,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Details {
    /// "Plan round 1: pass".
    pub gate: Option<String>,
    /// "PR #4 checks pending", linking to the PR.
    pub pr: Option<Link>,
    pub rollouts: Vec<RolloutLine>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RolloutLine {
    /// "web: step 2 of 5 at 20%, Paused, analysis Running".
    pub text: String,
    /// Whether to draw it as an error.
    pub failing: bool,
}

/// The board as the app lays it out. `now` is the server's time in Unix seconds.
pub fn board_view(snap: &Snapshot, now: i64) -> BoardView {
    let needs_you = &snap.needs_you;
    BoardView {
        refreshed: words::refreshed(&snap.sources, now),
        stale: snap.sources.iter().filter_map(|s| words::stale(s, now)).collect(),
        needs_you_heading: (!needs_you.is_empty()).then(|| words::needs_you_heading(needs_you)),
        needs_you: needs_you.iter().map(|n| Link { text: words::needs_you(n), url: n.url.clone() }).collect(),
        columns: snap
            .columns
            .iter()
            .map(|c| ColumnView {
                title: words::column(&c.stage, c.tickets.len()),
                cards: c.tickets.iter().map(card).collect(),
            })
            .collect(),
    }
}

fn card(t: &Ticket) -> Card {
    let mut sub_tickets = vec![];
    flatten(t, 0, &mut sub_tickets);
    Card {
        id: t.id.clone(),
        title: t.title.clone(),
        url: t.url.clone(),
        repo: t.repo.clone(),
        details: details(t),
        sub_tickets,
    }
}

fn flatten(parent: &Ticket, depth: u32, out: &mut Vec<SubTicket>) {
    for c in &parent.children {
        out.push(SubTicket {
            depth,
            id: c.id.clone(),
            url: c.url.clone(),
            stage: format!("· {}", c.stage),
            repo: (c.repo != parent.repo).then(|| c.repo.clone()),
            details: details(c),
        });
        flatten(c, depth + 1, out);
    }
}

fn details(t: &Ticket) -> Details {
    Details {
        gate: t.gate.as_ref().map(words::gate),
        pr: t.pr.as_ref().map(|p| Link { text: words::pr(p), url: p.url.clone() }),
        rollouts: t
            .rollouts
            .iter()
            .map(|r| RolloutLine { text: words::rollout(r), failing: words::rollout_failing(r) })
            .collect(),
    }
}
