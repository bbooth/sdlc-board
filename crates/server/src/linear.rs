use crate::config::Repo;
use crate::{board::Issue, gates};
use serde_json::{json, Value};
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
}

impl Linear {
    pub fn new(endpoint: &str, key: &str, repos: Vec<Repo>) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(20)).build()?;
        Ok(Self { client, endpoint: endpoint.into(), key: key.into(), repos })
    }

    pub async fn issues(&self) -> anyhow::Result<Vec<Issue>> {
        let mut out = Vec::new();
        for repo in &self.repos {
            let mut after: Option<String> = None;
            for _ in 0..MAX_PAGES {
                let vars = json!({"team": repo.linear_team, "project": repo.linear_project, "after": after});
                let e = |err: reqwest::Error| anyhow::anyhow!("Linear issues for {}: {err}", repo.linear_project);
                let req = json!({"query": QUERY, "variables": vars});
                let resp = self.client.post(&self.endpoint).header("Authorization", &self.key).json(&req);
                let body = resp.send().await.map_err(e)?.error_for_status().map_err(e)?.text().await.map_err(e)?;
                let page = parse_page(&body, &repo.name)?;
                out.extend(page.issues);
                after = page.next;
                if after.is_none() {
                    break;
                }
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
