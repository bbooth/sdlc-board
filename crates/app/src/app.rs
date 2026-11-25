//! The board app. It only reads the server's snapshot, so no build of it
//! holds a Linear or GitHub token. Natively and on Android it sends a
//! Cloudflare Access service token; in a browser, Access's login goes with the request.

use crate::{Reads, Settings, GIVE_UP};
use board_api::Snapshot;
use eframe::egui;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use web_time::Instant;

/// The native build, with its settings from the environment.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_native() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([2000.0, 700.0]),
        ..Default::default()
    };
    let settings = Settings::from_env();
    eframe::run_native("SDLC board", options, Box::new(move |_| Ok(Box::new(BoardApp::new(settings, None)))))
}

/// The web build: the server serves it, so it reads the snapshot from where it came from.
#[cfg(target_arch = "wasm32")]
pub fn run_web() {
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
            Ok(Box::new(BoardApp::new(Settings { url, ..Default::default() }, None)))
        });
        if let Err(e) = eframe::WebRunner::new().start(canvas, eframe::WebOptions::default(), app).await {
            web_sys::console::error_1(&e);
        }
    });
}

/// Reads `url` natively, within [`GIVE_UP`], with the Access service token in `settings`.
#[cfg(not(target_arch = "wasm32"))]
pub fn get(url: String, settings: &Settings, done: impl FnOnce(Result<Vec<u8>, String>) + Send + 'static) {
    // The whole call has a deadline, so a server that never answers can't
    // hold a thread and a socket open (ADR 0001).
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(GIVE_UP)).build().into();
    let headers = match settings.headers() {
        Ok(h) => h,
        Err(e) => return done(Err(e)),
    };
    std::thread::spawn(move || {
        let mut req = agent.get(&url);
        for (name, value) in &headers {
            req = req.header(*name, value);
        }
        done(req.call().and_then(|r| r.into_body().read_to_vec()).map_err(|e| e.to_string()));
    });
}

/// Reads `url` with the browser's fetch, aborted after [`GIVE_UP`].
#[cfg(target_arch = "wasm32")]
fn get(url: String, _: &Settings, done: impl FnOnce(Result<Vec<u8>, String>) + 'static) {
    use eframe::wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;
    let err = |e: JsValue| e.as_string().unwrap_or_else(|| format!("{e:?}"));
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
            text.as_string().map(String::into_bytes).ok_or_else(|| "an empty answer".to_string())
        };
        done(read.await);
    });
}

pub struct BoardApp {
    settings: Settings,
    /// Where settings are saved, on Android; elsewhere they can't be edited in the app.
    store: Option<PathBuf>,
    /// The settings being edited, while the settings form is open.
    draft: Option<Settings>,
    save_error: Option<String>,
    reads: Arc<Mutex<Reads>>,
    /// Shows or hides the soft keyboard: Android doesn't on its own when a text field takes focus.
    keyboard: Option<Box<dyn Fn(bool)>>,
    keyboard_shown: bool,
}

impl BoardApp {
    /// An app reading with `settings`. With a `store`, they can be changed
    /// in the app and saved there, and the form opens at once if no address is set.
    pub fn new(settings: Settings, store: Option<PathBuf>) -> BoardApp {
        let draft = (store.is_some() && settings.url.is_empty()).then(|| settings.clone());
        BoardApp {
            settings,
            store,
            draft,
            save_error: None,
            reads: Arc::default(),
            keyboard: None,
            keyboard_shown: false,
        }
    }

    pub fn with_keyboard(mut self, show: impl Fn(bool) + 'static) -> BoardApp {
        self.keyboard = Some(Box::new(show));
        self
    }

    /// Reads the snapshot if one is due, or now if `asked` and none is waiting.
    fn fetch(&self, ctx: &egui::Context, asked: bool) {
        if self.settings.url.is_empty() {
            return;
        }
        let Some(n) = self.reads.lock().unwrap_or_else(|e| e.into_inner()).start(Instant::now(), asked) else {
            return;
        };
        let (reads, ctx) = (self.reads.clone(), ctx.clone());
        get(format!("{}/v1/snapshot", self.settings.url.trim_end_matches('/')), &self.settings, move |body| {
            let parsed = body.and_then(|b| serde_json::from_slice::<Snapshot>(&b).map_err(|e| e.to_string()));
            reads.lock().unwrap_or_else(|e| e.into_inner()).finish(n, parsed);
            ctx.request_repaint();
        });
    }

    /// The settings form: the address and the Access service token.
    fn settings_form(&mut self, ui: &mut egui::Ui) {
        let (Some(draft), Some(store)) = (&mut self.draft, &self.store) else { return };
        ui.heading("Settings");
        egui::Grid::new("settings").num_columns(2).show(ui, |ui| {
            let fields = [
                ("Board address", &mut draft.url, false),
                ("Access client ID", &mut draft.client_id, false),
                ("Access client secret", &mut draft.client_secret, true),
            ];
            for (name, value, secret) in fields {
                let label = ui.label(name);
                ui.add(egui::TextEdit::singleline(value).password(secret)).labelled_by(label.id);
                ui.end_row();
            }
        });
        let has_board = !self.settings.url.is_empty();
        let (save, cancel) =
            ui.horizontal(|ui| (ui.button("Save").clicked(), has_board && ui.button("Cancel").clicked())).inner;
        if save {
            for field in [&mut draft.url, &mut draft.client_id, &mut draft.client_secret] {
                *field = field.trim().to_string();
            }
            let checked = match draft.url.as_str() {
                "" => Err("set the board's address".to_string()),
                _ => draft.headers().map(drop),
            };
            match checked.and_then(|_| draft.save(store).map_err(|e| e.to_string())) {
                Ok(()) => {
                    self.settings = self.draft.take().unwrap_or_default();
                    // A new address or token starts a new board, read at once.
                    self.reads = Arc::default();
                    self.save_error = None;
                }
                Err(e) => self.save_error = Some(format!("Couldn't save: {e}")),
            }
        } else if cancel {
            (self.draft, self.save_error) = (None, None);
        }
        if let Some(e) = &self.save_error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
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
            ui.horizontal_wrapped(|ui| {
                ui.heading("SDLC board");
                let label = if waiting { "Reading…" } else { "Refresh" };
                if ui.add_enabled(!waiting && self.draft.is_none(), egui::Button::new(label)).clicked() {
                    self.fetch(&ctx, true);
                }
                if self.store.is_some() && self.draft.is_none() && ui.button("Settings").clicked() {
                    self.draft = Some(self.settings.clone());
                }
                if let Some(e) = &problem {
                    let msg = format!("Couldn't refresh from {}: {e}", self.settings.url);
                    ui.colored_label(ui.visuals().error_fg_color, msg);
                }
            });
        });
        egui::CentralPanel::default().show(ui, |ui| match (&snap, &problem) {
            _ if self.draft.is_some() => self.settings_form(ui),
            (Some(snap), _) => crate::board(ui, snap, now),
            (None, Some(_)) => drop(ui.label("No board yet: the next read is due within 30 s.")),
            (None, None) => drop(ui.label(format!("Loading from {}…", self.settings.url))),
        });
        if let Some(show) = &self.keyboard {
            let wanted = ctx.egui_wants_keyboard_input();
            if wanted != self.keyboard_shown {
                show(wanted);
                self.keyboard_shown = wanted;
            }
        }
    }
}
