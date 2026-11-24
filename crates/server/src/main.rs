use board_server::{config::Config, github, linear, routes, sources};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let path = std::env::var("BOARD_CONFIG").unwrap_or_else(|_| "board.toml".into());
    let config = Config::load(&path)?;
    let key = std::env::var("LINEAR_API_KEY").map_err(|_| anyhow::anyhow!("LINEAR_API_KEY is not set"))?;
    let endpoint = std::env::var("LINEAR_ENDPOINT").unwrap_or_else(|_| linear::ENDPOINT.into());
    let state = sources::State::new();
    let linear = linear::Linear::new(&endpoint, &key, config.repos.clone())?;
    tokio::spawn(sources::run(state.clone(), linear));
    // A token is optional for public repos, but unauthenticated reads are rate limited.
    let gh_endpoint = std::env::var("GITHUB_ENDPOINT").unwrap_or_else(|_| github::ENDPOINT.into());
    let github = github::GitHub::new(&gh_endpoint, std::env::var("GITHUB_TOKEN").ok(), config.repos.clone())?;
    tokio::spawn(sources::run(state.clone(), github));
    let listen = config.listen.as_deref().unwrap_or("0.0.0.0:8080");
    let listener = tokio::net::TcpListener::bind(listen).await?;
    tracing::info!(listen, repos = config.repos.len(), "serving");
    axum::serve(listener, routes::router(state)).await?;
    Ok(())
}
