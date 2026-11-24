use board_server::{access::Access, config::Config, github, linear, routes, sources};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let path = std::env::var("BOARD_CONFIG").unwrap_or_else(|_| "board.toml".into());
    let config = Config::load(&path)?;
    // Without Access settings the board would be open to anyone who reaches
    // it, so that takes an explicit opt-out, for a local run only.
    let state = sources::State::new();
    let access = match (config.access.clone(), std::env::var("BOARD_NO_ACCESS").is_ok_and(|v| v == "1")) {
        (Some(settings), _) => Some(Arc::new(Access::new(settings)?.observed(state.clone()))),
        (None, true) => {
            tracing::warn!("BOARD_NO_ACCESS=1: serving without checking Cloudflare Access");
            None
        }
        (None, false) => {
            anyhow::bail!("{path} has no [access] settings; set BOARD_NO_ACCESS=1 to run without them locally")
        }
    };
    if let Some(a) = &access {
        tokio::spawn(a.clone().run());
    }
    let key = std::env::var("LINEAR_API_KEY").map_err(|_| anyhow::anyhow!("LINEAR_API_KEY is not set"))?;
    let endpoint = std::env::var("LINEAR_ENDPOINT").unwrap_or_else(|_| linear::ENDPOINT.into());
    let linear = linear::Linear::new(&endpoint, &key, config.repos.clone())?;
    tokio::spawn(sources::run(state.clone(), linear));
    let gh_endpoint = std::env::var("GITHUB_ENDPOINT").unwrap_or_else(|_| github::ENDPOINT.into());
    let token = std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.is_empty());
    if token.is_none() {
        tracing::warn!("GITHUB_TOKEN is not set: reading public repos only as often as 45 requests an hour allows");
    }
    let github = github::GitHub::new(&gh_endpoint, token, config.repos.clone())?;
    tokio::spawn(sources::run(state.clone(), github));
    let web = std::env::var("BOARD_WEB_DIR").ok().map(Into::into);
    let listen = config.listen.as_deref().unwrap_or("0.0.0.0:8080");
    let metrics_listen = config.metrics_listen.as_deref().unwrap_or("0.0.0.0:9091");
    let (listener, metrics) =
        (tokio::net::TcpListener::bind(listen).await?, tokio::net::TcpListener::bind(metrics_listen).await?);
    tracing::info!(listen, metrics_listen, repos = config.repos.len(), web = ?web, "serving");
    let metrics = async { axum::serve(metrics, routes::metrics_router(state.clone())).await };
    let public = async { axum::serve(listener, routes::router(state.clone(), access, web)).await };
    tokio::try_join!(public, metrics)?;
    Ok(())
}
