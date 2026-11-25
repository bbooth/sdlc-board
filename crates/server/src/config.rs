use serde::Deserialize;

/// `board.toml`: the repos the board covers.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Where to listen; 0.0.0.0:8080 if unset.
    pub listen: Option<String>,
    /// Where `/metrics` listens; 0.0.0.0:9091 if unset.
    pub metrics_listen: Option<String>,
    /// The Cloudflare Access application the server checks tokens against.
    pub access: Option<crate::access::Settings>,
    pub repos: Vec<Repo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Repo {
    /// GitHub `owner/name`.
    pub name: String,
    /// The Linear team key (the ticket prefix) and project its tickets live in.
    pub linear_team: String,
    pub linear_project: String,
    /// The namespace its Argo Rollouts run in, if it deploys any.
    #[serde(default)]
    pub rollout_namespace: Option<String>,
}

impl Config {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("reading board settings {path}: {e}"))?;
        toml::from_str(&text).map_err(|e| anyhow::anyhow!("parsing board settings {path}: {e}"))
    }
}
