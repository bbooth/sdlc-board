use board_server::{config::Config, linear, routes, sources};

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

    let listener = tokio::net::TcpListener::bind(&config.listen).await?;
    tracing::info!(listen = %config.listen, repos = config.repos.len(), "serving");
    axum::serve(listener, routes::router(state)).await?;
    Ok(())
}
