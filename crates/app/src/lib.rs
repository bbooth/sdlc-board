//! What the app draws, kept apart from the window so tests can render it.

use board_api::{Checks, Snapshot, Ticket};
use eframe::egui;

/// Any source whose last read failed, what needs the owner, then a column
/// per stage, each ticket a card with its sub-tickets nested.
pub fn board(ui: &mut egui::Ui, snap: &Snapshot) {
    for s in &snap.sources {
        if let Some(err) = &s.last_error {
            // Without this the board would look current while showing old data.
            let age = match s.last_good {
                Some(t) => format!("data is {} min old", (snap.generated_at - t).max(0) / 60),
                None => "no data yet".into(),
            };
            ui.colored_label(ui.visuals().warn_fg_color, format!("{}: the last read failed, {age}: {err}", s.name));
        }
    }
    if !snap.needs_you.is_empty() {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.strong(format!("Needs you ({})", snap.needs_you.len()));
            for n in &snap.needs_you {
                let what = match n.kind.as_str() {
                    "approve" => "Approve the spec",
                    "merge" => "Merge",
                    _ => "Blocked",
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
        let checks = match pr.checks {
            Checks::Pass => "checks pass",
            Checks::Fail => "checks fail",
            Checks::Pending => "checks pending",
        };
        let state = if pr.merged { "merged" } else { checks };
        ui.hyperlink_to(format!("PR #{} {state}", pr.number), &pr.url);
    }
}
