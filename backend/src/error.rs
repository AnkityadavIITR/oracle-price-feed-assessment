
use thiserror::Error;

#[derive(Error, Debug)]
pub enum OracleError {
    #[error("Price data is stale: {0}")]
    StalePrice(String),
    
    #[error("Confidence interval too large: {0}")]
    HighConfidence(String),
    
    #[error("Price sources disagree: {0}")]
    PriceDeviation(String),
    
    #[error("No price data available for symbol: {0}")]
    NoPriceData(String),
    
    #[error("Solana RPC error: {0}")]
    SolanaError(String),
    
    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),
    
    #[error("Redis error: {0}")]
    RedisError(#[from] redis::RedisError),
    
    #[error("Parse error: {0}")]
    ParseError(String),
}

pub type Result<T> = std::result::Result<T, OracleError>;