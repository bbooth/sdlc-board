use crate::board::{self, Issue};
use crate::cluster::RolloutInfo;
use crate::github::PrInfo;
use crate::metrics::Metrics;
use board_api::{Snapshot, SourceStatus};
use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const INTERVAL: Duration = Duration::from_secs(60);

/// Something the board reads on an interval.
pub trait Source: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn interval(&self) -> Duration {
        INTERVAL
    }
    fn read(&self) -> impl Future<Output = anyhow::Result<Read>> + Send;
}

/// What one read of a source returned.
#[derive(Default)]
pub struct Read {
    pub issues: Vec<Issue>,
    pub prs: Vec<PrInfo>,
    pub rollouts: Vec<RolloutInfo>,
    /// What failed in a read that still returned data, shown as the source's error.
    pub partial: Option<String>,
    /// When the oldest data in the read was read, if older than the read.
    pub as_of: Option<i64>,
}

impl Source for crate::linear::Linear {
    fn name(&self) -> &'static str {
        "linear"
    }
    async fn read(&self) -> anyhow::Result<Read> {
        let read = crate::linear::Linear::read(self).await;
        if let (0, [first, ..]) = (read.repos_read, read.errors.as_slice()) {
            anyhow::bail!("no repo's tickets could be read, first: {first}");
        }
        let partial = read.errors.first().map(|e| {
            format!("{} of {} repos failed, first: {e}", read.errors.len(), read.errors.len() + read.repos_read)
        });
        Ok(Read { issues: read.issues, partial, as_of: read.as_of, ..Read::default() })
    }
}

impl Source for crate::github::GitHub {
    fn name(&self) -> &'static str {
        "github"
    }
    fn interval(&self) -> Duration {
        self.interval()
    }
    async fn read(&self) -> anyhow::Result<Read> {
        let read = self.prs().await;
        // Nothing new was read, so the source's data is as old as its last good read.
        if let (0, [first, ..]) = (read.repos_read, read.errors.as_slice()) {
            anyhow::bail!("no repo's PRs could be read ({}), first: {first}", read.repos_failed.join(", "));
        }
        let repos = match read.repos_failed.as_slice() {
            [] => String::new(),
            names => format!("PR lists failed for {}; ", names.join(", ")),
        };
        let partial = read.errors.first().map(|e| format!("{repos}{} reads failed, first: {e}", read.errors.len()));
        let as_of = read.prs.iter().map(|p| p.read_at).min();
        Ok(Read { prs: read.prs, partial, as_of, ..Read::default() })
    }
}

impl Source for crate::cluster::Cluster {
    fn name(&self) -> &'static str {
        "cluster"
    }
    async fn read(&self) -> anyhow::Result<Read> {
        let read = crate::cluster::Cluster::read(self).await;
        if let (0, [first, ..]) = (read.namespaces_read, read.errors.as_slice()) {
            anyhow::bail!("no namespace's rollouts could be read, first: {first}");
        }
        let partial = read.errors.first().map(|e| format!("{} namespaces failed, first: {e}", read.errors.len()));
        Ok(Read { rollouts: read.rollouts, partial, ..Read::default() })
    }
}

/// What every source last returned, and how each read went.
pub struct State {
    data: RwLock<BTreeMap<&'static str, Part>>,
    pub metrics: Metrics,
    started: Instant,
}

#[derive(Default)]
struct Part {
    read: Read,
    last_good: Option<i64>,
    last_error: Option<String>,
}

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

impl State {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { data: RwLock::default(), metrics: Metrics::default(), started: Instant::now() })
    }

    pub fn snapshot(&self) -> Snapshot {
        let data = self.data.read().unwrap_or_else(|e| e.into_inner());
        let issues: Vec<Issue> = data.values().flat_map(|p| p.read.issues.iter().cloned()).collect();
        let prs: Vec<PrInfo> = data.values().flat_map(|p| p.read.prs.iter().cloned()).collect();
        let rollouts: Vec<RolloutInfo> = data.values().flat_map(|p| p.read.rollouts.iter().cloned()).collect();
        Snapshot {
            generated_at: now(),
            sources: data
                .iter()
                .map(|(n, p)| SourceStatus {
                    name: (*n).into(),
                    last_good: p.last_good,
                    last_error: p.last_error.clone(),
                })
                .collect(),
            columns: board::columns(&issues, &prs, &rollouts),
            needs_you: board::needs_you(&issues, &prs),
        }
    }

    /// Seconds since each source's last good read; since start for one that
    /// has never had one, so a source that never works still looks stale.
    pub fn ages(&self) -> Vec<(&'static str, f64)> {
        let data = self.data.read().unwrap_or_else(|e| e.into_inner());
        let since_start = self.started.elapsed().as_secs_f64();
        data.iter().map(|(n, p)| (*n, p.last_good.map(|t| (now() - t) as f64).unwrap_or(since_start))).collect()
    }
}

/// Reads a source once, retrying once on failure (a read is safe to
/// repeat). A failure keeps the source's earlier data.
pub async fn refresh<S: Source>(state: &State, source: &S) {
    let name = source.name();
    let started = Instant::now();
    let result = match source.read().await {
        Ok(v) => Ok(v),
        Err(e) => {
            tracing::warn!(source = name, error = %e, "source read failed, retrying");
            source.read().await
        }
    };
    let secs = started.elapsed().as_secs_f64();
    state.metrics.observe(name, secs, matches!(&result, Ok(r) if r.partial.is_none()));
    let mut data = state.data.write().unwrap_or_else(|e| e.into_inner());
    let part = data.entry(name).or_default();
    match result {
        Ok(read) => {
            let (issues, prs, rollouts) = (read.issues.len(), read.prs.len(), read.rollouts.len());
            tracing::info!(
                source = name,
                issues,
                prs,
                rollouts,
                partial = read.partial,
                duration_s = secs,
                "source read"
            );
            part.last_good = Some(read.as_of.map_or(now(), |t| t.min(now())));
            part.last_error = read.partial.clone();
            part.read = read;
        }
        Err(e) => {
            tracing::warn!(source = name, error = %e, duration_s = secs, "source read failed");
            part.last_error = Some(e.to_string());
        }
    }
}

/// Reads the source now and then after each interval, which can depend on the read's cost.
pub async fn run<S: Source>(state: Arc<State>, source: S) {
    loop {
        refresh(&state, &source).await;
        tokio::time::sleep(source.interval()).await;
    }
}
