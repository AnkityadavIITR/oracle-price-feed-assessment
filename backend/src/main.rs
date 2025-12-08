use std::sync::Arc;
use tokio::sync::Mutex;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod types;
mod error;
mod pyth_client;
mod switchboard_client;
mod price_aggregator;
mod cache;
mod database;
mod api;

use config::Config;
use price_aggregator::PriceAggregator;
use cache::CachedPriceFetcher;
use database::Database;
use api::{AppState, create_router};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "oracle_backend=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("🚀 Starting Oracle Backend Service");

    // Load configuration
    dotenv::dotenv().ok();
    let config = Config::from_env()?;

    // Initialize database
    tracing::info!("📊 Connecting to database...");
    let db = Database::new(&config.database_url).await?;
    db.migrate().await?;

    // Initialize cache
    tracing::info!("💾 Connecting to Redis cache...");
    let cache = CachedPriceFetcher::new(&config.redis_url).await?;

    // Initialize price aggregator
    tracing::info!("🔗 Initializing oracle aggregator...");
    let mut aggregator = PriceAggregator::new(
        &config.solana_rpc_url,
        config.oracle_config.clone(),
    );

    // Register trading symbols
    // TODO: Load from config file or database
    aggregator.register_symbol(
        "BTC/USD",
        "GVXRSBjFk6e6J3NbVPXohDJetcTjaeeuykUpbQF8UoMU", // Pyth BTC/USD (devnet)
        "8SXvChNYFhRq4EZuZvnhjrB3jJRQCv4k3P4W6hesH3Ee", // Switchboard BTC/USD (devnet)
    )?;

    aggregator.register_symbol(
        "ETH/USD",
        "JBu1AL4obBcCMqKBBxhpWCNUt136ijcuMZLFvTP7iWdB", // Pyth ETH/USD (devnet)
        "GvDMxPzN1sCj7L26YDK2HnMRXEQmQ2aemov8YBtPS7vR", // Switchboard ETH/USD (devnet)
    )?;

    aggregator.register_symbol(
        "SOL/USD",
        "J83w4HKfqxwcq3BEMMkPFSppX3gqekLyLJBexebFVkix", // Pyth SOL/USD (devnet)
        "GvDMxPzN1sCj7L26YDK2HnMRXEQmQ2aemov8YBtPS7vR", // Switchboard SOL/USD (devnet)
    )?;

    // Create shared application state
    let state = AppState {
        aggregator: Arc::new(Mutex::new(aggregator)),
        cache: Arc::new(Mutex::new(cache)),
        db: Arc::new(db),
    };

    // Create API router
    let app = create_router(state.clone());

    // Start server
    let addr = format!("{}:{}", config.server_host, config.server_port);
    tracing::info!("🌐 Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}