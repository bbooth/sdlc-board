use crate::config::Repo;
use crate::gates;
use board_api::{Checks, Gate, Pr};
use serde_json::Value;
use std::time::Duration;

pub const ENDPOINT: &str = "https://api.github.com";

/// An open PR, tied to the ticket its branch (or else its title) names.
#[derive(Debug, Clone, PartialEq)]
pub struct PrInfo {
    pub ticket: String,
    pub pr: Pr,
    pub draft: bool,
    /// The branch names the ticket, as pipeline branches do (`feat/tru-9-...`).
    /// A ticket with several PRs shows the one whose branch names it.
    pub by_branch: bool,
    /// The latest gate record on the PR's branch.
    pub gate: Option<Gate>,
}

/// Reads each repo's open PRs, their checks and their gate records.
pub struct GitHub {
    client: reqwest::Client,
    endpoint: String,
    token: Option<String>,
    repos: Vec<Repo>,
}

impl GitHub {
    pub fn new(endpoint: &str, token: Option<String>, repos: Vec<Repo>) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(20)).user_agent("sdlc-board").build()?;
        Ok(Self { client, endpoint: endpoint.into(), token, repos })
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
            let pulls = self.get(&format!("{base}/pulls"), &[("state", "open"), ("per_page", "100")]).await?;
            for p in pulls.as_ref().and_then(Value::as_array).into_iter().flatten() {
                let title = p["title"].as_str().unwrap_or_default();
                let (sha, branch) = (p["head"]["sha"].as_str().unwrap_or(""), p["head"]["ref"].as_str().unwrap_or(""));
                // A body only mentions tickets in passing, so it ties nothing.
                let on_branch = ticket_id(&branch.to_ascii_uppercase(), &repo.linear_team);
                let Some(ticket) = on_branch.clone().or_else(|| ticket_id(title, &repo.linear_team)) else { continue };
                let runs = self.get(&format!("{base}/commits/{sha}/check-runs"), &[("per_page", "100")]).await?;
                let gate = self.record(&base, &ticket, branch).await?;
                let pr = Pr {
                    number: p["number"].as_u64().unwrap_or(0),
                    title: title.into(),
                    url: p["html_url"].as_str().unwrap_or_default().into(),
                    checks: checks(runs.as_ref().unwrap_or(&Value::Null)),
                };
                out.push(PrInfo { ticket, pr, draft: p["draft"] == true, by_branch: on_branch.is_some(), gate });
            }
        }
        Ok(out)
    }

    async fn record(&self, base: &str, ticket: &str, branch: &str) -> anyhow::Result<Option<Gate>> {
        let dir = format!("{base}/contents/.sdlc/records/{ticket}");
        let Some(files) = self.get(&dir, &[("ref", branch)]).await? else { return Ok(None) };
        let names = files.as_array().into_iter().flatten().filter_map(|f| f["name"].as_str());
        let latest =
            names.filter_map(|n| Some((n, gates::from_file_name(n)?))).max_by_key(|(_, (s, r))| (gates::order(s), *r));
        let Some((name, (stage, round))) = latest else { return Ok(None) };
        let record = self.get(&format!("{dir}/{name}"), &[("ref", branch)]).await?.unwrap_or_default();
        Ok(Some(Gate { stage, round, pass: record["result"] == "pass" }))
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
