use crate::config::Repo;
use crate::gates;
use board_api::{Checks, Gate, Pr};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

pub const ENDPOINT: &str = "https://api.github.com";

/// Requests an hour to spend without a token, of the 60 GitHub allows. One
/// read never sends more, and reads are spaced to keep to it.
const UNAUTHENTICATED_BUDGET: u64 = 45;
/// How many recently closed PRs to look through for merged ones.
const RECENT: &str = "20";

/// An open or recently merged PR from the repo's own branches, tied to the
/// ticket its branch (or else its title) names.
#[derive(Debug, Clone, PartialEq)]
pub struct PrInfo {
    pub ticket: String,
    pub pr: Pr,
    pub draft: bool,
    /// The branch names the ticket, as pipeline branches do (`feat/tru-9-...`).
    /// A ticket with several PRs shows the one whose branch names it.
    pub by_branch: bool,
    /// The latest gate record at the PR's head, or at its merge.
    pub gate: Option<Gate>,
    /// The commit its checks and records were read at.
    pub at: String,
    /// The records at `at` were read (they never change, so once is enough).
    pub gate_read: bool,
    /// Unix seconds when its checks were last read; older when a failed read
    /// kept an earlier value.
    pub read_at: i64,
}

/// One read's PRs, and what failed in it.
#[derive(Debug, Default)]
pub struct PrRead {
    pub prs: Vec<PrInfo>,
    pub errors: Vec<String>,
    /// Repos whose PR lists were read; none means nothing in the read is new.
    pub repos_read: usize,
}

/// Reads each repo's PRs, their checks and their gate records.
pub struct GitHub {
    client: reqwest::Client,
    endpoint: String,
    token: Option<String>,
    repos: Vec<Repo>,
    /// Requests sent since the interval was last worked out, and in this read.
    requests: AtomicU64,
    this_read: AtomicU64,
    budget: u64,
    /// Each repo's PRs as last read, for the parts a read fails to get.
    last: Mutex<HashMap<String, Vec<PrInfo>>>,
}

fn list(v: &Option<Value>) -> impl Iterator<Item = &Value> {
    v.as_ref().and_then(Value::as_array).into_iter().flatten()
}

impl GitHub {
    pub fn new(endpoint: &str, token: Option<String>, repos: Vec<Repo>) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(20)).user_agent("sdlc-board").build()?;
        let (requests, this_read, last) = (AtomicU64::new(0), AtomicU64::new(0), Mutex::default());
        let budget = UNAUTHENTICATED_BUDGET;
        Ok(Self { client, endpoint: endpoint.into(), token, repos, requests, this_read, budget, last })
    }

    /// Spends at most `per_hour` requests an hour without a token.
    pub fn with_budget(self, per_hour: u64) -> Self {
        Self { budget: per_hour.max(1), ..self }
    }

    /// Without a token, long enough that reads like the ones since the last call fit the budget.
    pub fn interval(&self) -> Duration {
        let spent = self.requests.swap(0, Ordering::Relaxed);
        if self.token.is_some() {
            return crate::sources::INTERVAL;
        }
        crate::sources::INTERVAL.max(Duration::from_secs(spent * 3600 / self.budget))
    }

    /// GETs an API path; `None` when it doesn't exist. The raw media type
    /// returns a file's own contents and the usual JSON for everything else.
    async fn get(&self, path: &str, query: &[(&str, &str)]) -> anyhow::Result<Option<Value>> {
        if self.token.is_none() && self.this_read.fetch_add(1, Ordering::Relaxed) >= self.budget {
            anyhow::bail!("GitHub {path}: not sent, this read used its {} requests", self.budget);
        }
        self.requests.fetch_add(1, Ordering::Relaxed);
        let e = |err: reqwest::Error| anyhow::anyhow!("GitHub {path}: {err}");
        let mut req = self.client.get(format!("{}{path}", self.endpoint)).query(query);
        req = req.header("Accept", "application/vnd.github.raw+json");
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        let resp = req.send().await.map_err(e)?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        Ok(Some(resp.error_for_status().map_err(e)?.json().await.map_err(e)?))
    }

    pub async fn prs(&self) -> PrRead {
        let mut read = PrRead::default();
        self.this_read.store(0, Ordering::Relaxed);
        for repo in &self.repos {
            let last = self.last.lock().unwrap_or_else(|e| e.into_inner()).get(&repo.name).cloned();
            let prs = self.repo_prs(repo, last.as_deref().unwrap_or_default(), &mut read.errors).await;
            let prs = match prs {
                Ok(prs) => {
                    read.repos_read += 1;
                    prs
                }
                Err(e) => {
                    let kept = if last.is_some() { "kept its PRs from an earlier read" } else { "no earlier PRs" };
                    read.errors.push(format!("{e} ({kept})"));
                    last.unwrap_or_default()
                }
            };
            self.last.lock().unwrap_or_else(|e| e.into_inner()).insert(repo.name.clone(), prs.clone());
            read.prs.extend(prs);
        }
        read
    }

    async fn repo_prs(&self, repo: &Repo, last: &[PrInfo], errors: &mut Vec<String>) -> anyhow::Result<Vec<PrInfo>> {
        let pulls = format!("/repos/{}/pulls", repo.name);
        let open = self.get(&pulls, &[("state", "open"), ("per_page", "100")]).await?;
        let recent = [("state", "closed"), ("sort", "updated"), ("direction", "desc"), ("per_page", RECENT)];
        let closed = self.get(&pulls, &recent).await?;
        // A repo that isn't there (renamed, or private without a token) is an error, not an empty one.
        anyhow::ensure!(open.is_some() && closed.is_some(), "GitHub {pulls}: not found");
        let merged = list(&closed).filter(|p| !p["merged_at"].is_null());
        let mut out = Vec::new();
        for p in list(&open).chain(merged) {
            // Anyone can open a PR from a fork, so only the repo's own branches
            // count. GitHub names are case-insensitive.
            let from = p["head"]["repo"]["full_name"].as_str().unwrap_or_default();
            if from.eq_ignore_ascii_case(&repo.name) {
                out.extend(self.pr_info(repo, p, last, errors).await);
            }
        }
        Ok(out)
    }

    /// One PR's checks and gate. A failed checks read keeps the last read's
    /// value for the same commit, or is Unknown; it never fails the whole read.
    async fn pr_info(&self, repo: &Repo, p: &Value, last: &[PrInfo], errors: &mut Vec<String>) -> Option<PrInfo> {
        let base = format!("/repos/{}", repo.name);
        let title = p["title"].as_str().unwrap_or_default();
        let branch = p["head"]["ref"].as_str().unwrap_or_default();
        // A body only mentions tickets in passing, so it ties nothing.
        let on_branch = ticket_id(&branch.to_ascii_uppercase(), &repo.linear_team);
        let ticket = on_branch.clone().or_else(|| ticket_id(title, &repo.linear_team))?;
        let (number, merged) = (p["number"].as_u64().unwrap_or(0), !p["merged_at"].is_null());
        let at = if merged { &p["merge_commit_sha"] } else { &p["head"]["sha"] }.as_str().unwrap_or_default();
        // Only a read of the same commit is worth keeping: a new push has new checks.
        let before = last.iter().find(|l| l.pr.number == number && l.at == at);
        let mut failed = |what: &str, e: anyhow::Error, kept: bool| {
            tracing::warn!(repo = %repo.name, pr = number, error = %e, "reading {what} failed");
            errors
                .push(format!("PR #{number} {what}: {e} ({})", if kept { "kept an earlier read" } else { "unknown" }));
        };
        let mut read_at = crate::sources::now();
        // A merged PR's checks no longer matter; the card says it merged.
        let checks = if merged {
            Checks::Pass
        } else {
            match self.get(&format!("{base}/commits/{at}/check-runs"), &[("per_page", "100")]).await {
                Ok(runs) => checks(runs.as_ref().unwrap_or(&Value::Null)),
                Err(e) => {
                    failed("checks", e, before.is_some());
                    read_at = before.map_or(read_at, |b| b.read_at);
                    before.map_or(Checks::Unknown, |b| b.pr.checks)
                }
            }
        };
        let (gate, gate_read) = match before.filter(|b| b.gate_read) {
            Some(b) => (b.gate.clone(), true),
            None => match self.record(&base, &ticket, at).await {
                Ok(gate) => (gate, true),
                Err(e) => {
                    failed("gate records", e, false);
                    (None, false)
                }
            },
        };
        let url = p["html_url"].as_str().unwrap_or_default().into();
        let pr = Pr { number, title: title.into(), url, checks, merged };
        let (draft, by_branch, at) = (p["draft"] == true, on_branch.is_some(), at.into());
        Some(PrInfo { ticket, pr, draft, by_branch, gate, at, gate_read, read_at })
    }

    async fn record(&self, base: &str, ticket: &str, at: &str) -> anyhow::Result<Option<Gate>> {
        let dir = format!("{base}/contents/.sdlc/records/{ticket}");
        let Some(files) = self.get(&dir, &[("ref", at)]).await? else { return Ok(None) };
        let names = files.as_array().into_iter().flatten().filter_map(|f| f["name"].as_str());
        let latest =
            names.filter_map(|n| Some((n, gates::from_file_name(n)?))).max_by_key(|(_, (s, r))| (gates::order(s), *r));
        let Some((name, (stage, round))) = latest else { return Ok(None) };
        let record = self.get(&format!("{dir}/{name}"), &[("ref", at)]).await?;
        let result = record.as_ref().and_then(|r| r["result"].as_str());
        Ok(result.map(|r| Gate { stage, round, pass: r == "pass" }))
    }
}

/// Fail if any check failed, pending while any runs (or none has started), else pass.
pub fn checks(runs: &Value) -> Checks {
    let runs = runs["check_runs"].as_array().map(Vec::as_slice).unwrap_or_default();
    let failed = |r: &&Value| {
        matches!(r["conclusion"].as_str(), Some("failure" | "cancelled" | "timed_out" | "action_required"))
    };
    if runs.iter().any(|r| failed(&r)) {
        Checks::Fail
    } else if runs.is_empty() || runs.iter().any(|r| r["status"] != "completed") {
        Checks::Pending
    } else {
        Checks::Pass
    }
}

/// The first ticket ID with the team's key in the text: `TRU-9: Show tickets`,
/// `Show tickets (TRU-9)` and `FEAT/TRU-9-SHOW` all name TRU-9 when the key is `TRU`.
pub fn ticket_id(text: &str, team: &str) -> Option<String> {
    let prefix = format!("{team}-");
    let word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    text.match_indices(&prefix).find_map(|(i, _)| {
        let rest = &text[i + prefix.len()..];
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        let whole = !word(text[..i].chars().last()) && !word(rest[digits..].chars().next());
        (digits > 0 && whole).then(|| format!("{prefix}{}", &rest[..digits]))
    })
}
