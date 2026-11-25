//! Where the app reads the board, and the Cloudflare Access service token it
//! sends. Natively they come from the environment; on Android they're
//! entered once in the app and kept in its own storage.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// The board's address, e.g. `https://board.trulinc.com`.
    pub url: String,
    pub client_id: String,
    pub client_secret: String,
}

impl Settings {
    /// `BOARD_URL` (default `http://localhost:8080`), `BOARD_ACCESS_CLIENT_ID`
    /// and `BOARD_ACCESS_CLIENT_SECRET`.
    pub fn from_env() -> Settings {
        let var = |k| std::env::var(k).unwrap_or_default();
        let url = std::env::var("BOARD_URL").unwrap_or_else(|_| "http://localhost:8080".into());
        Settings { url, client_id: var("BOARD_ACCESS_CLIENT_ID"), client_secret: var("BOARD_ACCESS_CLIENT_SECRET") }
    }

    /// The settings saved at `path`, or `None` if none have been saved.
    pub fn load(path: &Path) -> io::Result<Option<Settings>> {
        match std::fs::read(path) {
            Ok(b) => serde_json::from_slice(&b).map(Some).map_err(io::Error::other),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Saves to `path` whole or not at all: a crash mid-write leaves the old
    /// settings, not a torn file.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec(self).map_err(io::Error::other)?)?;
        std::fs::rename(&tmp, path)
    }

    /// The Access headers to send: both, or none when neither is set. One
    /// without the other is a mistake that Access would only answer with a login page.
    pub fn headers(&self) -> Result<Vec<(&'static str, String)>, String> {
        let (id, secret) = (self.client_id.trim(), self.client_secret.trim());
        match (id.is_empty(), secret.is_empty()) {
            (true, true) => Ok(vec![]),
            (false, false) => {
                Ok(vec![("CF-Access-Client-Id", id.to_string()), ("CF-Access-Client-Secret", secret.to_string())])
            }
            _ => Err("set both the Access client ID and secret, or neither".into()),
        }
    }
}
