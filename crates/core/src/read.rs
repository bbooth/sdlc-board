//! Reading the snapshot natively. The web build fetches in the browser,
//! which sends its own User-Agent and Access login.

use crate::{Settings, GIVE_UP};
use board_api::Snapshot;
use std::fmt;

/// Why a read didn't give a board, named so the owner knows what to fix.
#[derive(Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The settings can't make a request (see [`Settings::check`]).
    Settings(String),
    /// Access or the server turned the request away: the status and its text.
    Refused(u16, String),
    /// The server couldn't be reached.
    Unreachable(String),
    /// No answer within [`GIVE_UP`].
    TimedOut,
    /// An answer that isn't a board, such as Access's login page.
    NotABoard(String),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::Settings(e) => write!(f, "check the settings: {e}"),
            ReadError::Refused(401 | 403, _) => f.write_str("Cloudflare Access refused the service token"),
            ReadError::Refused(status, text) => write!(f, "the server answered {status} {text}"),
            ReadError::Unreachable(e) => write!(f, "couldn't reach the board: {e}"),
            ReadError::TimedOut => write!(f, "no answer within {} s", GIVE_UP.as_secs()),
            ReadError::NotABoard(e) => write!(f, "the answer wasn't a board: {e}"),
        }
    }
}

/// The User-Agent a build sends, so the server can count reads by client.
pub fn user_agent(client: &str) -> String {
    format!("sdlc-board-{client}/{}", env!("CARGO_PKG_VERSION"))
}

/// Reads the board at `settings.url` within [`GIVE_UP`], sending the Access
/// service token and `client`'s User-Agent. Blocks: call it off the UI thread.
pub fn read(settings: &Settings, client: &str) -> Result<Snapshot, ReadError> {
    settings.check().map_err(ReadError::Settings)?;
    // The whole call has a deadline, so a server that never answers can't
    // hold a thread and a socket open (ADR 0001).
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(GIVE_UP))
        .http_status_as_error(false)
        // Access answers an expired token with a redirect to its login page.
        .max_redirects(0)
        .max_redirects_will_error(false)
        .user_agent(user_agent(client))
        .build()
        .into();
    let mut req = agent.get(format!("{}/v1/snapshot", settings.url.trim().trim_end_matches('/')));
    for (name, value) in settings.headers().map_err(ReadError::Settings)? {
        req = req.header(name, value);
    }
    let mut resp = req.call().map_err(|e| match e {
        ureq::Error::Timeout(_) => ReadError::TimedOut,
        e => ReadError::Unreachable(e.to_string()),
    })?;
    let status = resp.status();
    if status.is_redirection() {
        return Err(ReadError::Refused(401, "a redirect to the Access login".into()));
    } else if !status.is_success() {
        return Err(ReadError::Refused(status.as_u16(), status.canonical_reason().unwrap_or_default().into()));
    }
    let body = resp.body_mut().read_to_vec().map_err(|e| match e {
        ureq::Error::Timeout(_) => ReadError::TimedOut,
        e => ReadError::Unreachable(e.to_string()),
    })?;
    serde_json::from_slice(&body).map_err(|e| ReadError::NotABoard(e.to_string()))
}
