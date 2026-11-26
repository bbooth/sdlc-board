//! The board's logic, shared by every client: the native and web builds of
//! the egui app now, and the Android app from TRU-29. Clients only draw.

#[cfg(not(target_arch = "wasm32"))]
mod read;
mod reads;
mod settings;
pub mod words;

#[cfg(not(target_arch = "wasm32"))]
pub use read::{read, read_within, user_agent, ReadError};
pub use reads::{Reads, GIVE_UP, REFRESH};
pub use settings::Settings;
