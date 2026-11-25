//! The Android build: the board app in a native activity, with its settings
//! kept in the app's own storage, which no other app can read.
#![cfg(target_os = "android")]

use eframe::NativeOptions;
use winit::platform::android::activity::AndroidApp;

#[no_mangle]
fn android_main(app: AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info));
    let store = app.internal_data_path().map(|d| d.join("settings.json"));
    let settings = match store.as_deref().map(board_app::Settings::load) {
        Some(Ok(s)) => s.unwrap_or_default(),
        Some(Err(e)) => {
            log::error!("reading the saved settings: {e}");
            Default::default()
        }
        None => {
            log::error!("no app storage: settings can't be saved");
            Default::default()
        }
    };
    let keyboard = app.clone();
    let options = NativeOptions { android_app: Some(app), ..Default::default() };
    let made = move |_: &eframe::CreationContext<'_>| -> Result<Box<dyn eframe::App>, _> {
        let show = move |on| if on { keyboard.show_soft_input(true) } else { keyboard.hide_soft_input(false) };
        Ok(Box::new(board_app::BoardApp::new(settings, store).with_keyboard(show)))
    };
    if let Err(e) = eframe::run_native("SDLC board", options, Box::new(made)) {
        log::error!("running the app: {e}");
    }
}
