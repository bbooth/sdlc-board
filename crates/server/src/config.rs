use serde::Deserialize;

/// `board.toml`: the repos the board covers.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_listen")]
    pub listen: String,
    pub repos: Vec<Repo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Repo {
    /// GitHub `owner/name`.
    pub name: String,
    /// The Linear team key (the ticket prefix) and project its tickets live in.
    pub linear_team: String,
    pub linear_project: String,
}

fn default_listen() -> String {
    "0.0.0.0:8080".into()
}

impl Config {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("reading board settings {path}: {e}"))?;
        toml::from_str(&text).map_err(|e| anyhow::anyhow!("parsing board settings {path}: {e}"))
    }
}
