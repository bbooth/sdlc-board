//! Signs in to GitHub as a read-only GitHub App: a JWT signed with the App's
//! key buys an installation token, good for an hour, for the repos the App is
//! installed on. Fine-grained personal tokens can't read private repos' checks.

use crate::sources::{now, State};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// A token this close to expiring is replaced before it's used.
const MARGIN: i64 = 300;

/// A token or key that prints as `[redacted]`, so no log or error shows it.
#[derive(Clone)]
pub struct Secret(String);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

/// The App's ID and key, from `GITHUB_APP_ID` and `GITHUB_APP_PRIVATE_KEY`.
pub struct Settings {
    pub app_id: u64,
    key: EncodingKey,
}

impl Settings {
    /// None when neither is set. An error names the setting that's wrong and
    /// never quotes the key.
    pub fn from_values(id: Option<&str>, key: Option<&str>) -> anyhow::Result<Option<Self>> {
        let (id, key) = (id.filter(|v| !v.trim().is_empty()), key.filter(|v| !v.trim().is_empty()));
        let (id, key) = match (id, key) {
            (None, None) => return Ok(None),
            (Some(_), None) => anyhow::bail!("GITHUB_APP_ID is set but GITHUB_APP_PRIVATE_KEY isn't"),
            (None, Some(_)) => anyhow::bail!("GITHUB_APP_PRIVATE_KEY is set but GITHUB_APP_ID isn't"),
            (Some(id), Some(key)) => (id, key),
        };
        let app_id = id.trim().parse().map_err(|_| anyhow::anyhow!("GITHUB_APP_ID isn't a number"))?;
        let key = EncodingKey::from_rsa_pem(key.trim().as_bytes())
            .map_err(|_| anyhow::anyhow!("GITHUB_APP_PRIVATE_KEY isn't an RSA private key in PEM form"))?;
        // The PEM can parse yet hold no usable key; signing once proves it.
        let settings = Self { app_id, key };
        settings.jwt().map_err(|_| anyhow::anyhow!("GITHUB_APP_PRIVATE_KEY can't sign: is it the App's RSA key?"))?;
        Ok(Some(settings))
    }

    /// The App's own JWT, issued a minute back in case the clocks disagree.
    fn jwt(&self) -> jsonwebtoken::errors::Result<String> {
        let claims = serde_json::json!({ "iat": now() - 60, "exp": now() + 540, "iss": self.app_id.to_string() });
        encode(&Header::new(Algorithm::RS256), &claims, &self.key)
    }
}

/// Installation tokens, got and kept fresh for each repo the board reads.
pub struct AppAuth {
    settings: Settings,
    endpoint: String,
    client: reqwest::Client,
    state: Arc<State>,
    /// Each repo's installation; a repo the App isn't on is looked up again
    /// next time, so installing it later needs no restart.
    installations: Mutex<HashMap<String, u64>>,
    /// Each repo's own token: one scoped to a single repo, read-only, can
    /// reach nothing else if it leaks or a path names the wrong repo.
    tokens: Mutex<HashMap<String, (Secret, i64)>>,
}

impl AppAuth {
    /// `state` counts each attempt to get a token, in `board_github_app_tokens_total`.
    pub fn new(settings: Settings, endpoint: &str, state: Arc<State>) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(20)).user_agent("sdlc-board").build()?;
        let (installations, tokens) = (Mutex::default(), Mutex::default());
        Ok(Self { settings, endpoint: endpoint.into(), client, state, installations, tokens })
    }

    /// A live token for `repo` (`owner/name`). When a new one can't be got,
    /// the old one serves until it actually expires.
    pub async fn token(&self, repo: &str) -> anyhow::Result<Secret> {
        let mut tokens = self.tokens.lock().await;
        let cached = tokens.get(repo).cloned();
        if let Some((token, _)) = cached.as_ref().filter(|(_, expires)| *expires - MARGIN > now()) {
            return Ok(token.clone());
        }
        match self.fresh(repo).await {
            Ok((token, expires)) => {
                tracing::info!(repo, expires, "got a GitHub App installation token");
                tokens.insert(repo.into(), (token.clone(), expires));
                Ok(token)
            }
            Err(e) => match cached.filter(|(_, expires)| *expires > now()) {
                Some((token, expires)) => {
                    tracing::warn!(repo, expires, error = %e, "getting a new GitHub App token failed; using the old one");
                    Ok(token)
                }
                None => Err(e),
            },
        }
    }

    /// Drops `repo`'s token, after GitHub rejected it before it expired.
    pub async fn forget(&self, repo: &str) {
        self.tokens.lock().await.remove(repo);
    }

    /// Every failure to get a token counts as an error, except a repo the App isn't on.
    async fn fresh(&self, repo: &str) -> anyhow::Result<(Secret, i64)> {
        let got = match self.installation(repo).await {
            Ok(Some(installation)) => self.mint(repo, installation).await,
            Ok(None) => anyhow::bail!("the GitHub App isn't installed on {repo}"),
            Err(e) => Err(e),
        };
        self.state.metrics.app_token(got.is_ok());
        got
    }

    async fn installation(&self, repo: &str) -> anyhow::Result<Option<u64>> {
        if let Some(id) = self.installations.lock().await.get(repo) {
            return Ok(Some(*id));
        }
        let path = format!("/repos/{repo}/installation");
        let resp = self.app_request(self.client.get(format!("{}{path}", self.endpoint)), &path).await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let body: Value = resp.error_for_status().map_err(|e| anyhow::anyhow!("GitHub {path}: {e}"))?.json().await?;
        let id = body["id"].as_u64().ok_or_else(|| anyhow::anyhow!("GitHub {path}: no installation id"))?;
        self.installations.lock().await.insert(repo.into(), id);
        Ok(Some(id))
    }

    async fn mint(&self, repo: &str, installation: u64) -> anyhow::Result<(Secret, i64)> {
        let path = format!("/app/installations/{installation}/access_tokens");
        let name = repo.split_once('/').map_or(repo, |(_, name)| name);
        let read = "read";
        let scope = serde_json::json!({ "repositories": [name], "permissions":
            { "checks": read, "contents": read, "metadata": read, "pull_requests": read } });
        let req = self.client.post(format!("{}{path}", self.endpoint)).json(&scope);
        let resp = self.app_request(req, &path).await?;
        if matches!(resp.status().as_u16(), 401 | 404) {
            // Reinstalling the App gives it a new installation ID: look it up again next read.
            self.installations.lock().await.remove(repo);
            anyhow::bail!("GitHub {path}: {} (the installation is looked up again next read)", resp.status());
        }
        // The status says enough; the body isn't echoed in case it holds a token.
        let body: Value = resp.error_for_status().map_err(|e| anyhow::anyhow!("GitHub {path}: {e}"))?.json().await?;
        let token = body["token"].as_str().ok_or_else(|| anyhow::anyhow!("GitHub {path}: no token"))?;
        let expires = body["expires_at"].as_str().and_then(unix).unwrap_or_else(|| now() + 3600);
        Ok((Secret(token.into()), expires))
    }

    async fn app_request(&self, req: reqwest::RequestBuilder, path: &str) -> anyhow::Result<reqwest::Response> {
        let jwt = self.settings.jwt().map_err(|e| anyhow::anyhow!("signing the GitHub App JWT: {e}"))?;
        let req = req.bearer_auth(jwt).header("Accept", "application/vnd.github+json");
        req.send().await.map_err(|e| anyhow::anyhow!("GitHub {path}: {e}"))
    }
}

/// `2026-11-25T20:00:00Z` as Unix seconds; GitHub's times are always UTC.
pub fn unix(at: &str) -> Option<i64> {
    let n = |r: std::ops::Range<usize>| at.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (hh, mm, ss) = (n(11..13)?, n(14..16)?, n(17..19)?);
    // Days from 1970-01-01 to the date, by the proleptic Gregorian calendar.
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + d - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss)
}
