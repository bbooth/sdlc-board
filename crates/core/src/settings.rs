//! Where a client reads the board, and the Cloudflare Access service token
//! it sends. The native app takes them from the environment.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::Path;

/// Whether `url` is this device, where plain http can't be overheard.
fn local(url: &str) -> bool {
    let host = url.strip_prefix("http://").unwrap_or("").split(['/', ':']).next();
    matches!(host, Some("localhost" | "127.0.0.1"))
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// The board's address, e.g. `https://board.trulinc.com`.
    pub url: String,
    pub client_id: String,
    pub client_secret: String,
}

// By hand, so a `{:?}` in a log can't print the Access secret.
impl fmt::Debug for Settings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let secret = if self.client_secret.is_empty() { "" } else { "<redacted>" };
        f.debug_struct("Settings")
            .field("url", &self.url)
            .field("client_id", &self.client_id)
            .field("client_secret", &secret)
            .finish()
    }
}

impl Settings {
    /// `BOARD_URL` (default `http://localhost:8080`), `BOARD_ACCESS_CLIENT_ID`
    /// and `BOARD_ACCESS_CLIENT_SECRET`.
    pub fn from_env() -> Settings {
        let var = |k| std::env::var(k).unwrap_or_default();
        let url = std::env::var("BOARD_URL").unwrap_or_else(|_| "http://localhost:8080".into());
        Settings { url, client_id: var("BOARD_ACCESS_CLIENT_ID"), client_secret: var("BOARD_ACCESS_CLIENT_SECRET") }
            .trimmed()
    }

    /// The same settings without the spaces a paste often brings.
    pub fn trimmed(&self) -> Settings {
        let t = |s: &str| s.trim().to_string();
        Settings { url: t(&self.url), client_id: t(&self.client_id), client_secret: t(&self.client_secret) }
    }

    /// Why these settings can't read a board, if they can't.
    pub fn check(&self) -> Result<(), String> {
        let url = self.url.trim();
        if url.is_empty() {
            return Err("set the board's address".into());
        }
        // ureq doesn't keep Android's cleartext rule, so the token's guard is here.
        if !self.headers()?.is_empty() && !url.starts_with("https://") && !local(url) {
            return Err("use an https:// address, so the Access token isn't sent in the clear".into());
        }
        Ok(())
    }

    /// The settings saved at `path`, or `None` when none are, or they don't read.
    pub fn load(path: &Path) -> Option<Settings> {
        serde_json::from_slice(&fs::read(path).ok()?).ok()
    }

    /// Saves the settings, trimmed, if they can read a board: whole or not
    /// at all, and readable by this user only, since they hold the secret.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let s = self.trimmed();
        s.check()?;
        let tmp = path.with_extension("tmp");
        let mut file = fs::OpenOptions::new();
        file.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut file, 0o600);
        let json = serde_json::to_vec(&s).map_err(|e| e.to_string())?;
        file.open(&tmp)
            .and_then(|mut f| f.write_all(&json).and_then(|()| f.sync_all()))
            .and_then(|()| fs::rename(&tmp, path))
            .map_err(|e| format!("couldn't save the settings: {e}"))
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
