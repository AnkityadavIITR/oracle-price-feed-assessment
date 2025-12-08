use serde::Deserialize;

/// Application configuration
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Solana RPC endpoint
    pub solana_rpc_url: String,
    
    /// Solana WebSocket endpoint (for real-time updates)
    pub solana_ws_url: String,
    
    /// PostgreSQL connection string
    pub database_url: String,
    
    /// Redis connection string
    pub redis_url: String,
    
    /// Server host
    pub server_host: String,
    
    /// Server port
    pub server_port: u16,
    
    /// Oracle settings
    pub oracle_config: OracleConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OracleConfig {
    /// Maximum price age before considered stale (seconds)
    pub max_price_age_seconds: i64,
    
    /// Maximum confidence interval (basis points)
    pub max_confidence_bps: u64,
    
    /// Maximum price deviation between sources (basis points)
    pub max_deviation_bps: u64,
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Config {
            solana_rpc_url: std::env::var("SOLANA_RPC_URL")?,
            solana_ws_url: std::env::var("SOLANA_WS_URL")?,
            database_url: std::env::var("DATABASE_URL")?,
            redis_url: std::env::var("REDIS_URL")
                .unwrap_or_else(|_| "redis://127.0.0.1".to_string()),
            server_host: std::env::var("SERVER_HOST")
                .unwrap_or_else(|_| "0.0.0.0".to_string()),
            server_port: std::env::var("SERVER_PORT")
                .unwrap_or_else(|_| "8080".to_string())
                .parse()?,
            oracle_config: OracleConfig {
                max_price_age_seconds: std::env::var("MAX_PRICE_AGE_SECONDS")
                    .unwrap_or_else(|_| "30".to_string())
                    .parse()?,
                max_confidence_bps: std::env::var("MAX_CONFIDENCE_BPS")
                    .unwrap_or_else(|_| "100".to_string())
                    .parse()?,
                max_deviation_bps: std::env::var("MAX_DEVIATION_BPS")
                    .unwrap_or_else(|_| "100".to_string())
                    .parse()?,
            },
        })
    }
}