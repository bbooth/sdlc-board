//! The cluster source: Argo Rollouts in each repo's rollout namespace, read
//! from the Kubernetes API with the pod's read-only ServiceAccount.

use crate::config::Repo;
use board_api::Rollout;
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

/// Where Kubernetes mounts the pod's ServiceAccount token and the API's CA.
pub const SERVICE_ACCOUNT: &str = "/var/run/secrets/kubernetes.io/serviceaccount";

/// A rollout in flight in one of the board's repos.
#[derive(Debug, Clone, PartialEq)]
pub struct RolloutInfo {
    pub repo: String,
    pub rollout: Rollout,
}

/// One read's rollouts, and what failed in it.
#[derive(Debug, Default)]
pub struct RolloutRead {
    pub rollouts: Vec<RolloutInfo>,
    pub errors: Vec<String>,
    pub namespaces_read: usize,
}

pub struct Cluster {
    client: reqwest::Client,
    endpoint: String,
    /// Read before each request: Kubernetes rotates the token in place.
    token_file: Option<PathBuf>,
    /// Each repo and the namespace its rollouts run in.
    namespaces: Vec<(String, String)>,
}

impl Cluster {
    /// `ca` is the PEM the API's certificate is checked against, beside the usual roots.
    pub fn new(endpoint: &str, token_file: Option<PathBuf>, ca: Option<&[u8]>, repos: &[Repo]) -> anyhow::Result<Self> {
        let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(20)).user_agent("sdlc-board");
        if let Some(pem) = ca {
            builder = builder.add_root_certificate(reqwest::Certificate::from_pem(pem)?);
        }
        let namespaces =
            repos.iter().filter_map(|r| Some((r.name.clone(), r.rollout_namespace.clone()?))).collect::<Vec<_>>();
        Ok(Self { client: builder.build()?, endpoint: endpoint.trim_end_matches('/').into(), token_file, namespaces })
    }

    /// Reads the API the pod runs under, or `KUBE_ENDPOINT` without a token
    /// (e.g. `kubectl proxy`, for a local run). `None` outside a cluster.
    pub fn from_env(repos: &[Repo]) -> anyhow::Result<Option<Self>> {
        if let Ok(endpoint) = std::env::var("KUBE_ENDPOINT") {
            return Self::new(&endpoint, None, None, repos).map(Some);
        }
        let (Ok(host), Ok(port)) = (std::env::var("KUBERNETES_SERVICE_HOST"), std::env::var("KUBERNETES_SERVICE_PORT"))
        else {
            return Ok(None);
        };
        let dir = PathBuf::from(SERVICE_ACCOUNT);
        let ca = std::fs::read(dir.join("ca.crt")).map_err(|e| anyhow::anyhow!("reading the cluster's CA: {e}"))?;
        let host = if host.contains(':') { format!("[{host}]") } else { host };
        Self::new(&format!("https://{host}:{port}"), Some(dir.join("token")), Some(&ca), repos).map(Some)
    }

    pub fn has_namespaces(&self) -> bool {
        !self.namespaces.is_empty()
    }

    pub async fn read(&self) -> RolloutRead {
        let mut read = RolloutRead::default();
        for (repo, ns) in &self.namespaces {
            match self.list(ns).await {
                Ok(items) => {
                    read.namespaces_read += 1;
                    let rollouts = items.iter().filter_map(in_flight);
                    read.rollouts.extend(rollouts.map(|rollout| RolloutInfo { repo: repo.clone(), rollout }));
                }
                Err(e) => read.errors.push(e.to_string()),
            }
        }
        read
    }

    async fn list(&self, ns: &str) -> anyhow::Result<Vec<Value>> {
        let path = format!("/apis/argoproj.io/v1alpha1/namespaces/{ns}/rollouts");
        let e = |err: reqwest::Error| anyhow::anyhow!("listing rollouts in {ns}: {err}");
        let mut req = self.client.get(format!("{}{path}", self.endpoint));
        if let Some(file) = &self.token_file {
            let token =
                std::fs::read_to_string(file).map_err(|err| anyhow::anyhow!("reading {}: {err}", file.display()))?;
            req = req.bearer_auth(token.trim());
        }
        let list: Value = req.send().await.map_err(e)?.error_for_status().map_err(e)?.json().await.map_err(e)?;
        Ok(list["items"].as_array().cloned().unwrap_or_default())
    }
}

/// A canary Rollout whose new version isn't yet the stable one, with its
/// progress; `None` once it's complete or for any other strategy.
pub fn in_flight(obj: &Value) -> Option<Rollout> {
    let status = &obj["status"];
    let current = status["currentPodHash"].as_str()?;
    if status["stableRS"].as_str() == Some(current) {
        return None;
    }
    let steps = obj["spec"]["strategy"]["canary"]["steps"].as_array()?;
    let index = status["currentStepIndex"].as_u64().unwrap_or(0) as usize;
    let aborted = status["abort"].as_bool() == Some(true);
    // With traffic routing Argo reports the weight it set; without, it's the last setWeight reached.
    let reached = || steps.iter().take(index + 1).filter_map(|s| s["setWeight"].as_u64()).next_back();
    let weight = status["canary"]["weights"]["canary"]["weight"].as_u64().or_else(reached).unwrap_or(0);
    let analysis = ["currentStepAnalysisRunStatus", "currentBackgroundAnalysisRunStatus"]
        .iter()
        .find_map(|k| status["canary"][k]["status"].as_str())
        .map(Into::into);
    Some(Rollout {
        name: obj["metadata"]["name"].as_str().unwrap_or_default().into(),
        step: (index + 1).min(steps.len()) as u32,
        steps: steps.len() as u32,
        weight: if aborted { 0 } else { weight as u32 },
        phase: if aborted { "Aborted".into() } else { status["phase"].as_str().unwrap_or("Progressing").into() },
        analysis,
    })
}
