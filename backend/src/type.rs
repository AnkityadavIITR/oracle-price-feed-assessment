use serde::{Deserialize, Serialize};
use rust_decimal::Decimal;

/// Represents a price from any oracle source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceData {
    /// Trading symbol (e.g., "BTC/USD")
    pub symbol: String,
    
    /// Price value
    pub price: Decimal,
    
    /// Confidence interval (± value)
    pub confidence: Decimal,
    
    /// Unix timestamp
    pub timestamp: i64,
    
    /// Oracle source
    pub source: PriceSource,
}

/// Oracle source identifier
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PriceSource {
    Pyth,
    Switchboard,
    Aggregate,
}

/// Health status of an oracle source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleHealth {
    pub source: PriceSource,
    pub is_healthy: bool,
    pub last_update: i64,
    pub error_count: u32,
}