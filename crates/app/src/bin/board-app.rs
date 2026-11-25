//! The native and web builds. Android starts the app from `crates/android`.

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    board_app::run_native()
}

#[cfg(target_arch = "wasm32")]
fn main() {
    board_app::run_web()
}
