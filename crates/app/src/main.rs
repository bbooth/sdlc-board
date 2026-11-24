//! The board app. It only reads the server's snapshot, so no build of it
//! holds a Linear or GitHub token.

use board_api::Snapshot;
use eframe::egui;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The last snapshot read, kept when a later read fails, and that failure.
#[derive(Default)]
struct Latest {
    snap: Option<Snapshot>,
    error: Option<String>,
    in_flight: bool,
    started: Option<Instant>,
}

fn main() -> eframe::Result {
    let url = std::env::var("BOARD_URL").unwrap_or_else(|_| "http://localhost:8080".into());
    eframe::run_native(
        "SDLC board",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([2000.0, 700.0]),
            ..Default::default()
        },
        Box::new(move |_| Ok(Box::new(BoardApp { url, latest: Arc::default() }))),
    )
}

struct BoardApp {
    url: String,
    latest: Arc<Mutex<Latest>>,
}

impl BoardApp {
    fn fetch(&self, ctx: egui::Context) {
        {
            let mut l = self.latest.lock().unwrap_or_else(|e| e.into_inner());
            (l.in_flight, l.started) = (true, Some(Instant::now()));
        }
        let latest = self.latest.clone();
        let req = ehttp::Request::get(format!("{}/v1/snapshot", self.url.trim_end_matches('/')));
        ehttp::fetch(req, move |res| {
            let parsed = res
                .and_then(|r| if r.ok { Ok(r) } else { Err(format!("{} {}", r.status, r.status_text)) })
                .and_then(|r| serde_json::from_slice::<Snapshot>(&r.bytes).map_err(|e| e.to_string()));
            let mut l = latest.lock().unwrap_or_else(|e| e.into_inner());
            let Latest { snap, error, in_flight, .. } = &mut *l;
            *in_flight = false;
            board_app::take(snap, error, parsed);
            ctx.request_repaint();
        });
    }
}

impl eframe::App for BoardApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let (since, in_flight) = {
            let l = self.latest.lock().unwrap_or_else(|e| e.into_inner());
            (l.started.map(|t| t.elapsed()), l.in_flight)
        };
        if board_app::due(since, in_flight) {
            self.fetch(ctx.clone());
        }
        // Wake up to tick the "last refreshed" age and to read again when due.
        ctx.request_repaint_after(Duration::from_secs(1));
        let (snap, error) = {
            let l = self.latest.lock().unwrap_or_else(|e| e.into_inner());
            (l.snap.clone(), l.error.clone())
        };
        egui::Panel::top("bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("SDLC board");
                // One read at a time, so an older answer can't land after a newer one.
                if ui.add_enabled(!in_flight, egui::Button::new("Refresh")).clicked() {
                    self.fetch(ctx.clone());
                }
                if let Some(e) = &error {
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
