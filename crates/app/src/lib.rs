//! What the app draws, kept apart from the window so tests can render it.
//! What it says comes from `board_core::words`, shared with the other clients.

use board_api::{Snapshot, Ticket};
use board_core::words;
use eframe::egui;

/// How old each source's data is, each stale source with its last good read
/// and error, what needs the owner, then a column per stage of cards with
/// sub-tickets nested. `now` is Unix seconds.
pub fn board(ui: &mut egui::Ui, snap: &Snapshot, now: i64) {
    ui.small(words::refreshed(&snap.sources, now));
    for line in snap.sources.iter().filter_map(|s| words::stale(s, now)) {
        ui.add(egui::Label::new(egui::RichText::new(line).color(ui.visuals().warn_fg_color)).wrap());
    }
    if !snap.needs_you.is_empty() {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.strong(words::needs_you_heading(&snap.needs_you));
            for n in &snap.needs_you {
                ui.hyperlink_to(words::needs_you(n), &n.url);
            }
        });
    }
    egui::ScrollArea::both().show(ui, |ui| {
        ui.horizontal_top(|ui| {
            for col in &snap.columns {
                ui.vertical(|ui| {
                    ui.set_width(170.0);
                    ui.strong(words::column(&col.stage, col.tickets.len()));
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

/// The ticket's latest gate, its PR with the state of its checks, and its
/// rollouts in flight.
fn details(ui: &mut egui::Ui, t: &Ticket) {
    if let Some(g) = &t.gate {
        ui.small(words::gate(g));
    }
    if let Some(pr) = &t.pr {
        ui.hyperlink_to(words::pr(pr), &pr.url);
    }
    for r in &t.rollouts {
        let color = if words::rollout_failing(r) { ui.visuals().error_fg_color } else { ui.visuals().text_color() };
        ui.add(egui::Label::new(egui::RichText::new(words::rollout(r)).small().color(color)).wrap());
    }
}
