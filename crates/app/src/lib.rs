//! What the app draws, kept apart from the window so tests can render it,
//! and the app itself, which the native, web and Android builds all start.

mod app;
mod settings;

#[cfg(target_arch = "wasm32")]
pub use app::run_web;
pub use app::BoardApp;
#[cfg(not(target_arch = "wasm32"))]
pub use app::{get, run_native};
pub use settings::Settings;

use board_api::{Checks, Snapshot, Ticket};
use eframe::egui;
use std::time::Duration;
use web_time::Instant;

/// How often the app reads the snapshot again. The server reads Linear every
/// minute, so a change shows within two.
pub const REFRESH: Duration = Duration::from_secs(30);
/// A read's deadline: the app's request times out then, and a read still
/// unanswered doesn't hold up the next.
pub const GIVE_UP: Duration = Duration::from_secs(20);

/// The app's reads of the snapshot: the last board read, kept when a later
/// read fails, so a server that's down doesn't blank it.
#[derive(Default)]
pub struct Reads {
    pub snap: Option<Snapshot>,
    error: Option<String>,
    /// The latest read's number and start; `answered` is the last one that came back.
    started: Option<(u64, Instant)>,
    answered: u64,
    /// When `snap` came back, to tell the ages by the server's clock.
    got: Option<Instant>,
}

impl Reads {
    /// Starts a read if one is due: the first, [`REFRESH`] after the last
    /// started, or on `asked` when none is waiting on an answer. A read
    /// waiting longer than [`GIVE_UP`] doesn't hold up the next. Returns its number.
    pub fn start(&mut self, now: Instant, asked: bool) -> Option<u64> {
        let due = self.started.is_none_or(|(_, t)| now - t >= REFRESH) || asked;
        let n = self.started.map_or(1, |(n, _)| n + 1);
        (due && !self.waiting(now)).then(|| {
            self.started = Some((n, now));
            n
        })
    }

    /// Whether the latest read is still within its deadline with no answer.
    pub fn waiting(&self, now: Instant) -> bool {
        self.started.is_some_and(|(n, t)| n > self.answered && now - t < GIVE_UP)
    }

    /// Takes in read `n`'s answer, unless a later read has answered already,
    /// so a slow answer can't replace a newer board.
    pub fn finish(&mut self, n: u64, read: Result<Snapshot, String>) {
        if n > self.answered {
            self.answered = n;
            match read {
                Ok(s) => (self.snap, self.error, self.got) = (Some(s), None, Some(Instant::now())),
                Err(e) => self.error = Some(e),
            }
        }
    }

    /// The server's time now, in Unix seconds: the snapshot's time plus how
    /// long ago it came back, so a device clock that's off doesn't skew the ages.
    pub fn server_now(&self, now: Instant) -> Option<i64> {
        let (snap, got) = (self.snap.as_ref()?, self.got?);
        Some(snap.generated_at + now.saturating_duration_since(got).as_secs() as i64)
    }

    /// Why the board may be out of date: the last read failed, or hasn't answered.
    pub fn problem(&self, now: Instant) -> Option<String> {
        match self.started {
            Some((n, t)) if n > self.answered && now - t >= GIVE_UP => {
                Some(format!("no answer for {} s", (now - t).as_secs()))
            }
            _ => self.error.clone(),
        }
    }
}

fn ago(secs: i64) -> String {
    match secs.max(0) {
        s if s < 60 => format!("{s} s ago"),
        s if s < 3600 => format!("{} min ago", s / 60),
        s => format!("{} h ago", s / 3600),
    }
}

/// How old each source's data is, each stale source with its last good read
/// and error, what needs the owner, then a column per stage of cards with
/// sub-tickets nested. `now` is Unix seconds.
pub fn board(ui: &mut egui::Ui, snap: &Snapshot, now: i64) {
    let read =
        |s: &board_api::SourceStatus| format!("{} {}", s.name, s.last_good.map_or("never".into(), |t| ago(now - t)));
    let sources: Vec<String> = snap.sources.iter().map(read).collect();
    ui.small(format!("Last refreshed: {}", sources.join(", ")));
    for s in &snap.sources {
        if let Some(err) = &s.last_error {
            let when = match s.last_good {
                Some(t) => format!("last good read {:02}:{:02} UTC, {}", t % 86400 / 3600, t % 3600 / 60, ago(now - t)),
                None => "no good read yet".into(),
            };
            let text =
                egui::RichText::new(format!("{} is stale ({when}): {err}", s.name)).color(ui.visuals().warn_fg_color);
            ui.add(egui::Label::new(text).wrap());
        }
    }
    if !snap.needs_you.is_empty() {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.strong(format!("Needs you ({})", snap.needs_you.len()));
            for n in &snap.needs_you {
                let what = match n.kind.as_str() {
                    "approve" => "Approve the spec",
                    "merge" => "Merge",
                    "blocked" => "Blocked",
                    // A kind from a newer server: show it as it is rather than mislabel it.
                    other => other,
                };
                ui.hyperlink_to(format!("{what}: {} {}", n.id, n.title), &n.url);
            }
        });
    }
    egui::ScrollArea::both().show(ui, |ui| {
        ui.horizontal_top(|ui| {
            for col in &snap.columns {
                ui.vertical(|ui| {
                    ui.set_width(170.0);
                    ui.strong(format!("{} ({})", col.stage, col.tickets.len()));
                    ui.separator();
                    for t in &col.tickets {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_width(160.0);
                            ui.hyperlink_to(egui::RichText::new(&t.id).strong(), &t.url);
                            ui.label(&t.title);
                            ui.small(&t.repo);
                            details(ui, t);
                            children(ui, t, 0);
                        });
                    }
                });
            }
        });
    });
}

fn children(ui: &mut egui::Ui, t: &Ticket, depth: usize) {
    for c in &t.children {
        ui.horizontal_wrapped(|ui| {
            ui.add_space(12.0 * depth as f32);
            ui.hyperlink_to(&c.id, &c.url);
            ui.small(format!("· {}", c.stage));
            // Repos share a Linear team, so a sub-ticket elsewhere says where.
            if c.repo != t.repo {
                ui.small(&c.repo);
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.add_space(12.0 * (depth + 1) as f32);
            details(ui, c);
        });
        children(ui, c, depth + 1);
    }
}

/// The ticket's latest gate and its PR with the state of its checks.
fn details(ui: &mut egui::Ui, t: &Ticket) {
    if let Some(g) = &t.gate {
        ui.small(format!("{} round {}: {}", g.stage, g.round, if g.pass { "pass" } else { "fail" }));
    }
    if let Some(pr) = &t.pr {
        let state = match (pr.merged, pr.checks) {
            (true, _) => "merged",
            (_, Checks::Pass) => "checks pass",
            (_, Checks::Fail) => "checks fail",
            (_, Checks::Pending) => "checks pending",
            (_, Checks::Unknown) => "checks unknown",
        };
        ui.hyperlink_to(format!("PR #{} {state}", pr.number), &pr.url);
    }
}
