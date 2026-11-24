//! What the app draws, kept apart from the window so tests can render it.

use board_api::{Snapshot, Ticket};
use eframe::egui;

/// A column per stage, each ticket a card with its sub-tickets nested.
pub fn board(ui: &mut egui::Ui, snap: &Snapshot) {
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
        children(ui, c, depth + 1);
    }
}
