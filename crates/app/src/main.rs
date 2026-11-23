//! The board app. It only reads the server's snapshot, so no build of it
//! holds a Linear or GitHub token.

use board_api::{Snapshot, Ticket};
use eframe::egui;
use std::sync::{Arc, Mutex};

type Shared = Arc<Mutex<Option<Result<Snapshot, String>>>>;

fn main() -> eframe::Result {
    let url = std::env::var("BOARD_URL").unwrap_or_else(|_| "http://localhost:8080".into());
    eframe::run_native(
        "SDLC board",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([2000.0, 700.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            let app = BoardApp { url, latest: Shared::default() };
            app.fetch(cc.egui_ctx.clone());
            Ok(Box::new(app))
        }),
    )
}

struct BoardApp {
    url: String,
    latest: Shared,
}

impl BoardApp {
    fn fetch(&self, ctx: egui::Context) {
        let latest = self.latest.clone();
        let req = ehttp::Request::get(format!("{}/v1/snapshot", self.url.trim_end_matches('/')));
        ehttp::fetch(req, move |res| {
            let parsed = res
                .and_then(|r| if r.ok { Ok(r) } else { Err(format!("{} {}", r.status, r.status_text)) })
                .and_then(|r| serde_json::from_slice::<Snapshot>(&r.bytes).map_err(|e| e.to_string()));
            *latest.lock().unwrap_or_else(|e| e.into_inner()) = Some(parsed);
            ctx.request_repaint();
        });
    }
}

impl eframe::App for BoardApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let latest = self.latest.lock().unwrap_or_else(|e| e.into_inner()).clone();
        egui::Panel::top("bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("SDLC board");
                if ui.button("Refresh").clicked() {
                    self.fetch(ctx.clone());
                }
            });
        });
        egui::CentralPanel::default().show(ui, |ui| match &latest {
            None => {
                ui.label(format!("Loading from {}…", self.url));
            }
            Some(Err(e)) => {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("Couldn't load the board from {}: {e}", self.url),
                );
            }
            Some(Ok(snap)) => {
                egui::ScrollArea::both().show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        for col in &snap.columns {
                            ui.vertical(|ui| {
                                ui.set_width(170.0);
                                ui.strong(format!("{} ({})", col.stage, col.tickets.len()));
                                ui.separator();
                                for t in &col.tickets {
                                    card(ui, t);
                                }
                            });
                        }
                    });
                });
            }
        });
    }
}

fn card(ui: &mut egui::Ui, t: &Ticket) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(160.0);
        ui.hyperlink_to(egui::RichText::new(&t.id).strong(), &t.url);
        ui.label(&t.title);
        ui.small(&t.repo);
        for c in &t.children {
            ui.horizontal_wrapped(|ui| {
                ui.hyperlink_to(&c.id, &c.url);
                ui.small(format!("· {}", c.stage));
            });
        }
    });
}
