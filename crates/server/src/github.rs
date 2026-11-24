use crate::config::Repo;
use crate::gates;
use board_api::{Checks, Gate, Pr};
use serde_json::Value;
use std::time::Duration;

pub const ENDPOINT: &str = "https://api.github.com";

/// How often to read GitHub without a token: the unauthenticated limit is 60
/// requests an hour, and one read costs a few per PR.
pub const UNAUTHENTICATED_INTERVAL: Duration = Duration::from_secs(30 * 60);
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
}

/// Reads each repo's PRs, their checks and their gate records.
pub struct GitHub {
    client: reqwest::Client,
    endpoint: String,
    token: Option<String>,
    repos: Vec<Repo>,
}

fn list(v: &Option<Value>) -> impl Iterator<Item = &Value> {
    v.as_ref().and_then(Value::as_array).into_iter().flatten()
}

impl GitHub {
    pub fn new(endpoint: &str, token: Option<String>, repos: Vec<Repo>) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(20)).user_agent("sdlc-board").build()?;
        Ok(Self { client, endpoint: endpoint.into(), token, repos })
    }

    pub fn interval(&self) -> Duration {
        if self.token.is_some() {
            crate::sources::INTERVAL
        } else {
            UNAUTHENTICATED_INTERVAL
        }
    }

    /// GETs an API path; `None` when it doesn't exist. The raw media type
    /// returns a file's own contents and the usual JSON for everything else.
    async fn get(&self, path: &str, query: &[(&str, &str)]) -> anyhow::Result<Option<Value>> {
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

    pub async fn prs(&self) -> anyhow::Result<Vec<PrInfo>> {
        let mut out = Vec::new();
        for repo in &self.repos {
            let base = format!("/repos/{}", repo.name);
            let pulls = format!("{base}/pulls");
            let open = self.get(&pulls, &[("state", "open"), ("per_page", "100")]).await?;
            let recent = [("state", "closed"), ("sort", "updated"), ("direction", "desc"), ("per_page", RECENT)];
            let closed = self.get(&pulls, &recent).await?;
            let merged = list(&closed).filter(|p| !p["merged_at"].is_null());
            for p in list(&open).chain(merged) {
                // Anyone can open a PR from a fork, so only the repo's own branches count.
                if p["head"]["repo"]["full_name"] == repo.name.as_str() {
                    out.extend(self.pr_info(&base, repo, p).await);
                }
            }
        }
        Ok(out)
    }

    /// One PR's checks and gate; a part that fails to read is logged and
    /// left unknown, so one bad PR doesn't fail the whole read.
    async fn pr_info(&self, base: &str, repo: &Repo, p: &Value) -> Option<PrInfo> {
        let title = p["title"].as_str().unwrap_or_default();
        let branch = p["head"]["ref"].as_str().unwrap_or_default();
        // A body only mentions tickets in passing, so it ties nothing.
        let on_branch = ticket_id(&branch.to_ascii_uppercase(), &repo.linear_team);
        let ticket = on_branch.clone().or_else(|| ticket_id(title, &repo.linear_team))?;
        let (number, merged) = (p["number"].as_u64().unwrap_or(0), !p["merged_at"].is_null());
        let at = if merged { &p["merge_commit_sha"] } else { &p["head"]["sha"] }.as_str().unwrap_or_default();
        let warn = |what: &str, e: anyhow::Error| tracing::warn!(repo = %repo.name, pr = number, error = %e, "reading {what} failed");
        // A merged PR's checks no longer matter; the card says it merged.
        let checks = if merged {
            Checks::Pass
        } else {
            match self.get(&format!("{base}/commits/{at}/check-runs"), &[("per_page", "100")]).await {
                Ok(runs) => checks(runs.as_ref().unwrap_or(&Value::Null)),
                Err(e) => {
                    warn("checks", e);
                    Checks::Pending
                }
            }
        };
        let gate = self.record(base, &ticket, at).await.unwrap_or_else(|e| {
            warn("gate records", e);
            None
        });
        let url = p["html_url"].as_str().unwrap_or_default().into();
        let pr = Pr { number, title: title.into(), url, checks, merged };
        Some(PrInfo { ticket, pr, draft: p["draft"] == true, by_branch: on_branch.is_some(), gate })
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
