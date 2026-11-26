//! When the app reads the snapshot, and what it keeps between reads.

use board_api::Snapshot;
use std::time::Duration;
use web_time::Instant;

/// How often the app reads the snapshot again. The server reads Linear every
/// minute, so a change shows within two.
pub const REFRESH: Duration = Duration::from_secs(30);
/// A read's deadline: the app's request times out then, and a read still
/// unanswered doesn't hold up the next.
pub const GIVE_UP: Duration = Duration::from_secs(20);

/// The app's reads of the snapshot: the last board read, kept when a later
/// read fails, so a server that's down doesn't blank it.
#[derive(Default)]
pub struct Reads {
    pub snap: Option<Snapshot>,
    error: Option<String>,
    /// The latest read's number and start; `answered` is the last one that came back.
    started: Option<(u64, Instant)>,
    answered: u64,
    /// When `snap` came back, to tell the ages by the server's clock.
    got: Option<Instant>,
}

impl Reads {
    /// Starts a read if one is due: the first, [`REFRESH`] after the last
    /// started, or on `asked` when none is waiting on an answer. A read
    /// waiting longer than [`GIVE_UP`] doesn't hold up the next. Returns its number.
    pub fn start(&mut self, now: Instant, asked: bool) -> Option<u64> {
        let due = self.started.is_none_or(|(_, t)| now - t >= REFRESH) || asked;
        let n = self.started.map_or(1, |(n, _)| n + 1);
        (due && !self.waiting(now)).then(|| {
            self.started = Some((n, now));
            n
        })
    }

    /// Whether the latest read is still within its deadline with no answer.
    pub fn waiting(&self, now: Instant) -> bool {
        self.started.is_some_and(|(n, t)| n > self.answered && now - t < GIVE_UP)
    }

    /// Takes in read `n`'s answer, unless a later read has answered already,
    /// so a slow answer can't replace a newer board.
    pub fn finish(&mut self, n: u64, read: Result<Snapshot, String>) {
        if n > self.answered {
            self.answered = n;
            match read {
                Ok(s) => (self.snap, self.error, self.got) = (Some(s), None, Some(Instant::now())),
                Err(e) => self.error = Some(e),
            }
        }
    }

    /// The server's time now, in Unix seconds: the snapshot's time plus how
    /// long ago it came back, so a device clock that's off doesn't skew the ages.
    pub fn server_now(&self, now: Instant) -> Option<i64> {
        let (snap, got) = (self.snap.as_ref()?, self.got?);
        Some(snap.generated_at + now.saturating_duration_since(got).as_secs() as i64)
    }

    /// Why the board may be out of date: the last read failed, or hasn't answered.
    pub fn problem(&self, now: Instant) -> Option<String> {
        match self.started {
            Some((n, t)) if n > self.answered && now - t >= GIVE_UP => {
                Some(format!("no answer for {} s", (now - t).as_secs()))
            }
            _ => self.error.clone(),
        }
    }
}
