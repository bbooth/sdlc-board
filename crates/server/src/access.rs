//! Checks the JWT Cloudflare Access signs into `Cf-Access-Jwt-Assertion`
//! (ADR 0001), so a request that reaches the pod without passing Access,
//! from inside the cluster say, gets nothing.

use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use std::sync::RwLock;
use std::time::{Duration, Instant};

pub const HEADER: &str = "cf-access-jwt-assertion";
/// How often the signing keys are read again; Cloudflare rotates them every few weeks.
pub const REFRESH: Duration = Duration::from_secs(3600);
/// An unknown key reads the keys again at most this often, so bad tokens can't hammer Cloudflare.
const MIN_REREAD: Duration = Duration::from_secs(60);

/// The Access application's settings: neither is a secret.
#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    /// The team's Access URL, `https://<team>.cloudflareaccess.com`, the tokens' `iss`.
    pub issuer: String,
    /// The Access application's audience tag.
    pub aud: String,
}

pub struct Access {
    settings: Settings,
    certs: String,
    client: reqwest::Client,
    keys: RwLock<Vec<(String, DecodingKey)>>,
    last_read: RwLock<Option<Instant>>,
}

#[derive(Deserialize)]
struct Claims {}

impl Access {
    pub fn new(settings: Settings) -> anyhow::Result<Self> {
        Self::with_certs(settings.clone(), &format!("{}/cdn-cgi/access/certs", settings.issuer.trim_end_matches('/')))
    }

    /// Reads the signing keys from `certs` instead of Cloudflare, for tests.
    pub fn with_certs(settings: Settings, certs: &str) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build()?;
        Ok(Self { settings, certs: certs.into(), client, keys: RwLock::default(), last_read: RwLock::default() })
    }

    /// Reads the signing keys; on failure the earlier keys stay.
    pub async fn read_keys(&self) -> anyhow::Result<usize> {
        *self.last_read.write().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        let e = |err: reqwest::Error| anyhow::anyhow!("Access keys from {}: {err}", self.certs);
        let set: JwkSet = self.client.get(&self.certs).send().await.map_err(e)?.error_for_status().map_err(e)?.json().await.map_err(e)?;
        let keys: Vec<_> = set
            .keys
            .iter()
            .filter_map(|k| Some((k.common.key_id.clone()?, DecodingKey::from_jwk(k).ok()?)))
            .collect();
        anyhow::ensure!(!keys.is_empty(), "Access keys from {}: none usable", self.certs);
        let n = keys.len();
        *self.keys.write().unwrap_or_else(|e| e.into_inner()) = keys;
        Ok(n)
    }

    /// Reads the keys now and then every [`REFRESH`], or every minute until a read works.
    pub async fn run(self: std::sync::Arc<Self>) {
        loop {
            let wait = match self.read_keys().await {
                Ok(n) => {
                    tracing::info!(keys = n, "read the Access signing keys");
                    REFRESH
                }
                Err(e) => {
                    tracing::warn!(error = %e, "reading the Access signing keys failed");
                    MIN_REREAD
                }
            };
            tokio::time::sleep(wait).await;
        }
    }

    /// Whether `token` is a live Access token for this application.
    pub async fn check(&self, token: &str) -> Result<(), String> {
        let kid = decode_header(token).map_err(|e| format!("bad token: {e}"))?.kid.ok_or("token names no key")?;
        if self.key(&kid).is_none() && self.may_reread() {
            // A key Cloudflare rotated in since the last read.
            let _ = self.read_keys().await;
        }
        let key = self.key(&kid).ok_or_else(|| format!("unknown key {kid}"))?;
        let mut v = Validation::new(Algorithm::RS256);
        v.set_audience(&[&self.settings.aud]);
        v.set_issuer(&[self.settings.issuer.trim_end_matches('/')]);
        v.set_required_spec_claims(&["exp", "iss", "aud"]);
        decode::<Claims>(token, &key, &v).map(|_| ()).map_err(|e| format!("rejected token: {e}"))
    }

    fn key(&self, kid: &str) -> Option<DecodingKey> {
        self.keys.read().unwrap_or_else(|e| e.into_inner()).iter().find(|(k, _)| k == kid).map(|(_, d)| d.clone())
    }

    fn may_reread(&self) -> bool {
        self.last_read.read().unwrap_or_else(|e| e.into_inner()).is_none_or(|t| t.elapsed() >= MIN_REREAD)
    }
}
