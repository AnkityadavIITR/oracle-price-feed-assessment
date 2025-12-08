
use std::sync::Arc;
use axum::{
    routing::{get, post},
    Router,
};
use tracing_subscriber;

mod config;
mod pyth_client;
mod switchboard_client;
mod price_aggregator;
mod cache;
mod api;
mod database;
mod error;
mod types;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging (print debug messages)
    tracing_subscriber::fmt::init();
    
    tracing::info!("🚀 Starting Oracle Backend Service...");

    // Load environment variables from .env file
    dotenv::dotenv().ok();

    // Initialize database connection
    tracing::info!("📊 Connecting to database...");
    let db_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await?;

    // Initialize Redis cache
    tracing::info!("💾 Connecting to Redis...");
    let redis_url = std::env::var("REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1".to_string());
    let redis_client = redis::Client::open(redis_url)?;

    // Initialize Solana RPC client
    tracing::info!("🔗 Connecting to Solana...");
    let rpc_url = std::env::var("SOLANA_RPC_URL")
        .unwrap_or_else(|_| "https://api.devnet.solana.com".to_string());

    // Application will be built in next commits
    tracing::info!("✅ All systems initialized!");
    tracing::info!("🌐 Server ready to start...");

    Ok(())
}