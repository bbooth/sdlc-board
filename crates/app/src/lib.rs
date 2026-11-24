//! What the app draws, kept apart from the window so tests can render it.

use board_api::{Checks, Snapshot, Ticket};
use eframe::egui;
use std::time::Duration;

/// How often the app reads the snapshot again. The server reads Linear every
/// minute, so a change shows within two.
pub const REFRESH: Duration = Duration::from_secs(30);

/// Whether to read the snapshot now: none is in flight and the last read
/// started at least [`REFRESH`] ago, or there hasn't been one.
pub fn due(since_last: Option<Duration>, in_flight: bool) -> bool {
    !in_flight && since_last.is_none_or(|d| d >= REFRESH)
}

/// Takes in a read of the snapshot: a failed one keeps the board it had and
/// says why, so a server that's down doesn't blank it.
pub fn take(snap: &mut Option<Snapshot>, error: &mut Option<String>, read: Result<Snapshot, String>) {
    match read {
        Ok(s) => (*snap, *error) = (Some(s), None),
        Err(e) => *error = Some(e),
    }
}

fn ago(secs: i64) -> String {
    match secs.max(0) {
        s if s < 60 => format!("{s} s ago"),
        s if s < 3600 => format!("{} min ago", s / 60),
        s => format!("{} h ago", s / 3600),
    }
}

/// How old the oldest source's data is, each stale source with its last good
/// read and error, what needs the owner, then a column per stage of cards
/// with sub-tickets nested. `now` is Unix seconds.
pub fn board(ui: &mut egui::Ui, snap: &Snapshot, now: i64) {
    let oldest = snap.sources.iter().filter_map(|s| s.last_good).min().unwrap_or(snap.generated_at);
    ui.small(format!("Last refreshed {}", ago(now - oldest)));
    for s in &snap.sources {
        if let Some(err) = &s.last_error {
            let when = match s.last_good {
                Some(t) => format!("last good read {:02}:{:02} UTC, {}", t % 86400 / 3600, t % 3600 / 60, ago(now - t)),
                None => "no good read yet".into(),
            };
            ui.colored_label(ui.visuals().warn_fg_color, format!("{} is stale ({when}): {err}", s.name));
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
