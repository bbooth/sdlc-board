//! The board app. It only reads the server's snapshot, so no build of it
//! holds a Linear or GitHub token. Natively it sends a Cloudflare Access
//! service token; in a browser, Access's login goes with the request.

use board_api::Snapshot;
#[cfg(target_arch = "wasm32")]
use board_core::GIVE_UP;
use board_core::{Reads, Settings};
use eframe::egui;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use web_time::Instant;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    let settings = Settings::from_env();
    eframe::run_native(
        "SDLC board",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([2000.0, 700.0]),
            ..Default::default()
        },
        Box::new(move |_| Ok(Box::new(BoardApp { settings, reads: Arc::default() }))),
    )
}

/// The web build: the server serves it, so it reads the snapshot from where it came from.
#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast;
    let window = web_sys::window().expect("a browser window");
    let url = window.location().origin().unwrap_or_default();
    let canvas = window
        .document()
        .and_then(|d| d.get_element_by_id("board"))
        .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        .expect("index.html has a canvas#board");
    wasm_bindgen_futures::spawn_local(async move {
        let app = Box::new(|_: &eframe::CreationContext<'_>| -> Result<Box<dyn eframe::App>, _> {
            Ok(Box::new(BoardApp { settings: Settings { url, ..Default::default() }, reads: Arc::default() }))
        });
        if let Err(e) = eframe::WebRunner::new().start(canvas, eframe::WebOptions::default(), app).await {
            web_sys::console::error_1(&e);
        }
    });
}

/// Reads the board natively through `board_core`, on a thread of its own.
#[cfg(not(target_arch = "wasm32"))]
fn get(settings: &Settings, done: impl FnOnce(Result<Snapshot, String>) + Send + 'static) {
    let settings = settings.clone();
    std::thread::spawn(move || done(board_core::read(&settings, "native").map_err(|e| e.to_string())));
}

/// Reads the board with the browser's fetch, aborted after [`GIVE_UP`].
#[cfg(target_arch = "wasm32")]
fn get(settings: &Settings, done: impl FnOnce(Result<Snapshot, String>) + 'static) {
    use eframe::wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;
    let err = |e: JsValue| e.as_string().unwrap_or_else(|| format!("{e:?}"));
    let url = format!("{}/v1/snapshot", settings.url.trim_end_matches('/'));
    wasm_bindgen_futures::spawn_local(async move {
        let read = async {
            let init = web_sys::RequestInit::new();
            init.set_signal(Some(&web_sys::AbortSignal::timeout_with_u32(GIVE_UP.as_millis() as u32)));
            // An expired Access login redirects to Cloudflare, which fetch can't follow cross-origin.
            init.set_redirect(web_sys::RequestRedirect::Manual);
            let window = web_sys::window().ok_or("no window")?;
            let resp: web_sys::Response =
                JsFuture::from(window.fetch_with_str_and_init(&url, &init)).await.map_err(err)?.unchecked_into();
            if resp.type_() == web_sys::ResponseType::Opaqueredirect {
                return Err("your Cloudflare Access login has expired: reload the page".into());
            } else if !resp.ok() {
                return Err(format!("{} {}", resp.status(), resp.status_text()));
            }
            let text = JsFuture::from(resp.text().map_err(err)?).await.map_err(err)?;
            let text = text.as_string().ok_or_else(|| "an empty answer".to_string())?;
            serde_json::from_str::<Snapshot>(&text).map_err(|e| format!("the answer wasn't a board: {e}"))
        };
        done(read.await);
    });
}

struct BoardApp {
    settings: Settings,
    reads: Arc<Mutex<Reads>>,
}

impl BoardApp {
    /// Reads the snapshot if one is due, or now if `asked` and none is waiting.
    fn fetch(&self, ctx: &egui::Context, asked: bool) {
        let Some(n) = self.reads.lock().unwrap_or_else(|e| e.into_inner()).start(Instant::now(), asked) else {
            return;
        };
        let (reads, ctx) = (self.reads.clone(), ctx.clone());
        get(&self.settings, move |read| {
            reads.lock().unwrap_or_else(|e| e.into_inner()).finish(n, read);
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
        let (snap, problem, waiting, now) = {
            let r = self.reads.lock().unwrap_or_else(|e| e.into_inner());
            let t = Instant::now();
            (r.snap.clone(), r.problem(t), r.waiting(t), r.server_now(t).unwrap_or(0))
        };
        egui::Panel::top("bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("SDLC board");
                let label = if waiting { "Reading…" } else { "Refresh" };
                if ui.add_enabled(!waiting, egui::Button::new(label)).clicked() {
                    self.fetch(&ctx, true);
                }
                if let Some(e) = &problem {
                    let msg = format!("Couldn't refresh from {}: {e}", self.settings.url);
                    ui.colored_label(ui.visuals().error_fg_color, msg);
                }
            });
        });
        egui::CentralPanel::default().show(ui, |ui| match (&snap, &problem) {
            (Some(snap), _) => board_app::board(ui, snap, now),
            (None, Some(_)) => drop(ui.label("No board yet: the next read is due within 30 s.")),
            (None, None) => drop(ui.label(format!("Loading from {}…", self.settings.url))),
        });
    }
}
