use crate::config::Repo;
use crate::{board::Issue, gates};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

pub const ENDPOINT: &str = "https://api.linear.app/graphql";
const STAGE_GROUP: &str = "Stage";
// A project with more open tickets than this is beyond a one-person board.
const MAX_PAGES: usize = 10;

// Linear lists comments newest first, so a ticket's latest gate is in the first 50.
const QUERY: &str = r#"query Board($team: String!, $project: String!, $after: String) {
  issues(first: 100, after: $after, filter: {team: {key: {eq: $team}}, project: {name: {eq: $project}}, state: {type: {nin: ["completed", "canceled"]}}}) {
    pageInfo { hasNextPage endCursor }
    nodes { identifier title url state { type } parent { identifier } labels { nodes { name parent { name } } } comments(first: 50, orderBy: createdAt) { nodes { body createdAt } } }
  }
}"#;

/// Reads every repo's tickets from Linear's GraphQL API.
pub struct Linear {
    client: reqwest::Client,
    endpoint: String,
    key: String,
    repos: Vec<Repo>,
    /// Each repo's tickets and when they were read, kept when a later read of that repo fails.
    last: Mutex<HashMap<String, (i64, Vec<Issue>)>>,
}

/// One read's tickets, and the repos whose read failed.
#[derive(Debug, Default)]
pub struct IssueRead {
    pub issues: Vec<Issue>,
    pub errors: Vec<String>,
    pub repos_read: usize,
    /// When the oldest kept tickets were read, if any repo's read failed.
    pub as_of: Option<i64>,
}

impl Linear {
    pub fn new(endpoint: &str, key: &str, repos: Vec<Repo>) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(20)).build()?;
        Ok(Self { client, endpoint: endpoint.into(), key: key.into(), repos, last: Mutex::default() })
    }

    /// Every repo's tickets; fails only when no repo could be read.
    pub async fn issues(&self) -> anyhow::Result<Vec<Issue>> {
        let read = self.read().await;
        match (read.repos_read, read.errors.first()) {
            (0, Some(e)) => anyhow::bail!("{e}"),
            _ => Ok(read.issues),
        }
    }

    /// Reads each repo's tickets apart, so one failing project keeps its
    /// earlier tickets and doesn't stop the others updating.
    pub async fn read(&self) -> IssueRead {
        let mut read = IssueRead::default();
        for repo in &self.repos {
            let last = self.last.lock().unwrap_or_else(|e| e.into_inner()).get(&repo.name).cloned();
            let (read_at, issues) = match self.repo_issues(repo).await {
                Ok(issues) => {
                    read.repos_read += 1;
                    if issues.is_empty() {
                        // Also what a misspelt linear_project gets back.
                        tracing::warn!(repo = %repo.name, project = %repo.linear_project, "no open tickets in the Linear project");
                    }
                    (crate::sources::now(), issues)
                }
                Err(e) => {
                    tracing::warn!(repo = %repo.name, error = %e, "reading the repo's tickets failed");
                    let kept =
                        if last.is_some() { "kept its tickets from an earlier read" } else { "no earlier tickets" };
                    read.errors.push(format!("{e} ({kept})"));
                    // The source is only as fresh as the oldest tickets it kept.
                    if let Some((at, _)) = &last {
                        read.as_of = Some(read.as_of.map_or(*at, |a| a.min(*at)));
                    }
                    last.unwrap_or_default()
                }
            };
            self.last.lock().unwrap_or_else(|e| e.into_inner()).insert(repo.name.clone(), (read_at, issues.clone()));
            read.issues.extend(issues);
        }
        read
    }

    async fn repo_issues(&self, repo: &Repo) -> anyhow::Result<Vec<Issue>> {
        let mut out = Vec::new();
        let mut after: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let vars = json!({"team": repo.linear_team, "project": repo.linear_project, "after": after});
            let e = |err: reqwest::Error| anyhow::anyhow!("Linear issues for {}: {err}", repo.linear_project);
            let req = json!({"query": QUERY, "variables": vars});
            let resp = self.client.post(&self.endpoint).header("Authorization", &self.key).json(&req);
            let body = resp.send().await.map_err(e)?.error_for_status().map_err(e)?.text().await.map_err(e)?;
            let page = parse_page(&body, &repo.name)
                .map_err(|err| anyhow::anyhow!("Linear issues for {}: {err}", repo.linear_project))?;
            out.extend(page.issues);
            after = page.next;
            if after.is_none() {
                break;
            }
        }
        Ok(out)
    }
}

pub struct Page {
    pub issues: Vec<Issue>,
    pub next: Option<String>,
}

/// Parses one page of the board query's response.
pub fn parse_page(body: &str, repo: &str) -> anyhow::Result<Page> {
    let r: Value = serde_json::from_str(body).map_err(|e| anyhow::anyhow!("Linear response: {e}"))?;
    if let Some(errs) = r["errors"].as_array().filter(|e| !e.is_empty()) {
        let msgs: Vec<&str> = errs.iter().filter_map(|e| e["message"].as_str()).collect();
        anyhow::bail!("Linear: {}", msgs.join("; "));
    }
    let conn = &r["data"]["issues"];
    let nodes = conn["nodes"].as_array().ok_or_else(|| anyhow::anyhow!("Linear response has no issues"))?;
    let text = |v: &Value| v.as_str().map(String::from);
    let issues = nodes
        .iter()
        .map(|n| {
            let labels = n["labels"]["nodes"].as_array().map(Vec::as_slice).unwrap_or_default();
            Ok(Issue {
                id: text(&n["identifier"]).ok_or_else(|| anyhow::anyhow!("Linear issue without an identifier"))?,
                title: text(&n["title"]).unwrap_or_default(),
                url: text(&n["url"]).unwrap_or_default(),
                repo: repo.into(),
                stage: labels.iter().find(|l| l["parent"]["name"] == STAGE_GROUP).and_then(|l| text(&l["name"])),
                parent: text(&n["parent"]["identifier"]),
                open: !matches!(n["state"]["type"].as_str(), Some("completed" | "canceled")),
                gates: n["comments"]["nodes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|c| Some(gates::from_comment(c["body"].as_str()?, c["createdAt"].as_str())))
                    .flatten()
                    .collect(),
            })
        })
        .collect::<anyhow::Result<_>>()?;
    let next = if conn["pageInfo"]["hasNextPage"] == true { text(&conn["pageInfo"]["endCursor"]) } else { None };
    Ok(Page { issues, next })
}
