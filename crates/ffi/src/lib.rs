//! `board-core` for Kotlin, through UniFFI (ADR 0002). The Android app
//! holds a [`Board`], asks it to read when due, and lays out the
//! [`BoardView`] it returns: every line in it is already worded by
//! `board_core::words`, so the phone says what the desktop and web say.

use board_api::{Snapshot, Ticket};
use board_core::{words, Reads};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

uniffi::setup_scaffolding!();

pub use board_core::Settings;

/// Where to read the board and the Access service token to send: the
/// core's own type. Rust's Debug hides the secret; Kotlin's toString doesn't.
#[uniffi::remote(Record)]
pub struct Settings {
    pub url: String,
    pub client_id: String,
    pub client_secret: String,
}

/// The settings without the spaces a paste often brings.
#[uniffi::export]
pub fn settings_trimmed(settings: Settings) -> Settings {
    settings.trimmed()
}

/// Why these settings can't read a board, or null if they can.
#[uniffi::export]
pub fn settings_problem(settings: Settings) -> Option<String> {
    settings.check().err()
}

/// The settings saved at `path`, or null when none are (first start).
#[uniffi::export]
pub fn settings_load(path: String) -> Option<Settings> {
    Settings::load(Path::new(&path))
}

/// Saves the settings at `path`, trimmed and readable by the app only, or
/// says why it didn't: they can't read a board, or the write failed.
#[uniffi::export]
pub fn settings_save(settings: Settings, path: String) -> Option<String> {
    settings.save(Path::new(&path)).err()
}

/// The version of the core the app was built with, e.g. "0.1.0".
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").into()
}

/// The board as last read, and why it may be out of date.
#[derive(uniffi::Object)]
pub struct Board {
    settings: Settings,
    reads: Mutex<Reads>,
    /// Where the app's clock reads 0: see [`Board::refresh`].
    origin: Instant,
}

#[uniffi::export]
impl Board {
    #[uniffi::constructor]
    pub fn new(settings: Settings) -> Arc<Board> {
        Arc::new(Board { settings: settings.trimmed(), reads: Mutex::default(), origin: Instant::now() })
    }

    /// Reads the snapshot if a read is due, or now if `asked` and none is
    /// waiting, blocking up to `GIVE_UP`; call it off the main thread.
    /// Returns whether it read. A failed read keeps the last board.
    ///
    /// `now_ms` is a clock that keeps counting while the phone sleeps
    /// (`SystemClock.elapsedRealtime()`): Rust's `Instant` stops then, so a
    /// board read before a night asleep would still look fresh.
    pub fn refresh(&self, asked: bool, now_ms: u64) -> bool {
        let Some(n) = self.reads().start(self.at(now_ms), asked) else {
            return false;
        };
        // Not under the lock: the view stays readable while this waits.
        let took = Instant::now();
        let read = board_core::read(&self.settings, "android").map_err(|e| e.to_string());
        self.reads().finish(n, read, self.at(now_ms) + took.elapsed());
        true
    }

    /// What to show at `now_ms`, on the same clock as [`Board::refresh`].
    pub fn view(&self, now_ms: u64) -> View {
        let reads = self.reads();
        let now = self.at(now_ms);
        let board = reads.snap.as_ref().map(|s| board_view(s, reads.server_now(now).unwrap_or(s.generated_at)));
        View { board, problem: reads.problem(now), waiting: reads.waiting(now) }
    }
}

impl Board {
    fn at(&self, now_ms: u64) -> Instant {
        self.origin + Duration::from_millis(now_ms)
    }

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
