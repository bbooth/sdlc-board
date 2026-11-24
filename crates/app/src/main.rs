//! The board app. It only reads the server's snapshot, so no build of it
//! holds a Linear or GitHub token.

use board_api::Snapshot;
use board_app::Reads;
use eframe::egui;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn main() -> eframe::Result {
    let url = std::env::var("BOARD_URL").unwrap_or_else(|_| "http://localhost:8080".into());
    eframe::run_native(
        "SDLC board",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([2000.0, 700.0]),
            ..Default::default()
        },
        Box::new(move |_| Ok(Box::new(BoardApp { url, reads: Arc::default() }))),
    )
}

struct BoardApp {
    url: String,
    reads: Arc<Mutex<Reads>>,
}

impl BoardApp {
    /// Reads the snapshot if one is due, or now if `asked` and none is waiting.
    fn fetch(&self, ctx: &egui::Context, asked: bool) {
        let Some(n) = self.reads.lock().unwrap_or_else(|e| e.into_inner()).start(Instant::now(), asked) else {
            return;
        };
        let (reads, ctx) = (self.reads.clone(), ctx.clone());
        let req = ehttp::Request::get(format!("{}/v1/snapshot", self.url.trim_end_matches('/')));
        ehttp::fetch(req, move |res| {
            let parsed = res
                .and_then(|r| if r.ok { Ok(r) } else { Err(format!("{} {}", r.status, r.status_text)) })
                .and_then(|r| serde_json::from_slice::<Snapshot>(&r.bytes).map_err(|e| e.to_string()));
            reads.lock().unwrap_or_else(|e| e.into_inner()).finish(n, parsed);
            ctx.request_repaint();
        });
    }
}

impl eframe::App for BoardApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.fetch(&ctx, false);
        // Wake up to tick the ages and to read again when due.
        ctx.request_repaint_after(Duration::from_secs(1));
        let (snap, problem) = {
            let r = self.reads.lock().unwrap_or_else(|e| e.into_inner());
            (r.snap.clone(), r.problem(Instant::now()))
        };
        egui::Panel::top("bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("SDLC board");
                if ui.button("Refresh").clicked() {
                    self.fetch(&ctx, true);
                }
                if let Some(e) = &problem {
                    let msg = format!("Couldn't refresh from {}: {e}", self.url);
                    ui.colored_label(ui.visuals().error_fg_color, msg);
                }
            });
        });
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
        egui::CentralPanel::default().show(ui, |ui| match &snap {
            None => drop(ui.label(format!("Loading from {}…", self.url))),
            Some(snap) => board_app::board(ui, snap, now),
        });
    }
}
