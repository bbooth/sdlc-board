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
        let url = format!("{}/v1/snapshot", self.url.trim_end_matches('/'));
        // The whole call has a deadline, so a server that never answers can't
        // hold a thread and a socket open. The web build fetches with ehttp
        // (ADR 0001); natively ehttp has no deadline before the headers.
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(board_app::GIVE_UP)).build().into();
        std::thread::spawn(move || {
            let parsed = agent
                .get(&url)
                .call()
                .and_then(|r| r.into_body().read_to_vec())
                .map_err(|e| e.to_string())
                .and_then(|b| serde_json::from_slice::<Snapshot>(&b).map_err(|e| e.to_string()));
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
        let (snap, problem, waiting) = {
            let r = self.reads.lock().unwrap_or_else(|e| e.into_inner());
            (r.snap.clone(), r.problem(Instant::now()), r.waiting(Instant::now()))
        };
        egui::Panel::top("bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("SDLC board");
                let label = if waiting { "Reading…" } else { "Refresh" };
                if ui.add_enabled(!waiting, egui::Button::new(label)).clicked() {
                    self.fetch(&ctx, true);
                }
                if let Some(e) = &problem {
                    let msg = format!("Couldn't refresh from {}: {e}", self.url);
                    ui.colored_label(ui.visuals().error_fg_color, msg);
                }
            });
        });
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
        egui::CentralPanel::default().show(ui, |ui| match (&snap, &problem) {
            (Some(snap), _) => board_app::board(ui, snap, now),
            (None, Some(_)) => drop(ui.label("No board yet: the next read is due within 30 s.")),
            (None, None) => drop(ui.label(format!("Loading from {}…", self.url))),
        });
    }
}
