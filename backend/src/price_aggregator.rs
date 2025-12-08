//! Price Aggregator
//! 
//! This module combines prices from multiple oracle sources to produce
//! a single, reliable consensus price. It implements manipulation resistance
//! through median calculation and outlier detection.
//!
//! # Architecture
//! ```text
//! Pyth → $50,000
//!         ↓
//! Switchboard → $50,100  →  [Aggregator]  →  Consensus: $50,050
//!         ↓                      ↓
//! (Future oracle)           Validates, detects outliers
//! ```

use crate::{
    error::{OracleError, Result},
    types::{PriceData, PriceSource, OracleHealth},
    pyth_client::PythClient,
    switchboard_client::SwitchboardClient,
    config::OracleConfig,
};
use rust_decimal::Decimal;
use std::collections::HashMap;
use tracing::{debug, warn, error};

/// Price aggregator that combines multiple oracle sources
pub struct PriceAggregator {
    /// Pyth client
    pyth: PythClient,
    
    /// Switchboard client
    switchboard: SwitchboardClient,
    
    /// Configuration
    config: OracleConfig,
    
    /// Health status of each oracle
    oracle_health: HashMap<PriceSource, OracleHealth>,
}

impl PriceAggregator {
    /// Create a new price aggregator
    /// 
    /// # Arguments
    /// * `rpc_url` - Solana RPC endpoint
    /// * `config` - Oracle configuration
    ///
    /// # Example
    /// ```rust
    /// let aggregator = PriceAggregator::new(
    ///     "https://api.devnet.solana.com",
    ///     config
    /// );
    /// ```
    pub fn new(rpc_url: &str, config: OracleConfig) -> Self {
        Self {
            pyth: PythClient::new(rpc_url),
            switchboard: SwitchboardClient::new(rpc_url),
            config,
            oracle_health: HashMap::new(),
        }
    }

    /// Register a price feed for a symbol across all oracles
    /// 
    /// This sets up both Pyth and Switchboard for a trading pair.
    ///
    /// # Arguments
    /// * `symbol` - Trading pair (e.g., "BTC/USD")
    /// * `pyth_feed` - Pyth price feed address
    /// * `switchboard_aggregator` - Switchboard aggregator address
    ///
    /// # Example
    /// ```rust
    /// aggregator.register_symbol(
    ///     "BTC/USD",
    ///     "J83w4HKfqxwcq3BEMMkPFSppX3gqekLyLJBexebFVkix",
    ///     "8SXvChNYFhRq4EZuZvnhjrB3jJRQCv4k3P4W6hesH3Ee"
    /// )?;
    /// ```
    pub fn register_symbol(
        &mut self,
        symbol: &str,
        pyth_feed: &str,
        switchboard_aggregator: &str,
    ) -> Result<()> {
        self.pyth.register_feed(symbol.to_string(), pyth_feed)?;
        self.switchboard.register_aggregator(symbol.to_string(), switchboard_aggregator)?;
        
        debug!("Registered symbol {} with all oracles", symbol);
        Ok(())
    }

    /// Get consensus price for a symbol
    /// 
    /// This is the main function that combines multiple oracle sources.
    /// 
    /// # Algorithm:
    /// 1. Fetch prices from all available oracles (Pyth, Switchboard)
    /// 2. Validate each price individually (freshness, confidence)
    /// 3. Calculate median price (resistant to manipulation)
    /// 4. Check for outliers (prices too far from median)
    /// 5. Return consensus price or error if validation fails
    ///
    /// # Why Median?
    /// Median is manipulation-resistant:
    /// - If one oracle is hacked to report $100,000
    /// - But others report $50,000
    /// - Median = $50,000 (ignores the outlier)
    ///
    /// Average would give: ($50,000 + $50,000 + $100,000) / 3 = $66,667 ❌
    ///
    /// # Arguments
    /// * `symbol` - Trading pair (e.g., "BTC/USD")
    ///
    /// # Returns
    /// Consensus `PriceData` with source set to `Aggregate`
    ///
    /// # Errors
    /// * `NoPriceData` - No oracles available
    /// * `StalePrice` - All prices are too old
    /// * `PriceDeviation` - Sources disagree too much
    ///
    /// # Example
    /// ```rust
    /// let price = aggregator.get_consensus_price("BTC/USD").await?;
    /// println!("Consensus price: ${}", price.price);
    /// ```
    pub async fn get_consensus_price(&self, symbol: &str) -> Result<PriceData> {
        debug!("Fetching consensus price for {}", symbol);

        // Step 1: Fetch prices from all oracles
        let mut prices = Vec::new();
        let mut errors = Vec::new();

        // Try Pyth
        match self.pyth.get_price(symbol).await {
            Ok(price) => {
                debug!("Pyth price for {}: ${}", symbol, price.price);
                prices.push(price);
            }
            Err(e) => {
                warn!("Pyth error for {}: {}", symbol, e);
                errors.push(("Pyth", e));
            }
        }

        // Try Switchboard
        match self.switchboard.get_price(symbol).await {
            Ok(price) => {
                debug!("Switchboard price for {}: ${}", symbol, price.price);
                prices.push(price);
            }
            Err(e) => {
                warn!("Switchboard error for {}: {}", symbol, e);
                errors.push(("Switchboard", e));
            }
        }

        // Step 2: Check if we have any prices
        if prices.is_empty() {
            error!("No oracle prices available for {}", symbol);
            return Err(OracleError::NoPriceData(
                format!("All oracles failed for {}: {:?}", symbol, errors)
            ));
        }

        // Step 3: Validate individual prices
        let valid_prices = self.validate_prices(&prices)?;

        if valid_prices.is_empty() {
            return Err(OracleError::NoPriceData(
                format!("No valid prices after validation for {}", symbol)
            ));
        }

        // Step 4: Calculate median (consensus price)
        let consensus = self.calculate_consensus(&valid_prices)?;

        // Step 5: Validate consensus (check for outliers)
        self.validate_consensus(&valid_prices, &consensus)?;

        debug!(
            "Consensus price for {}: ${} (from {} sources)",
            symbol,
            consensus.price,
            valid_prices.len()
        );

        Ok(consensus)
    }

    /// Validate individual prices
    /// 
    /// Checks each price for:
    /// - Staleness (age < max_price_age_seconds)
    /// - Confidence (uncertainty < max_confidence_bps)
    ///
    /// # Arguments
    /// * `prices` - Raw prices from oracles
    ///
    /// # Returns
    /// Vector of valid prices
    fn validate_prices(&self, prices: &[PriceData]) -> Result<Vec<PriceData>> {
        let mut valid_prices = Vec::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        for price in prices {
            // Check staleness
            let age = now - price.timestamp;
            if age > self.config.max_price_age_seconds {
                warn!(
                    "Rejecting stale price from {:?}: {} seconds old",
                    price.source, age
                );
                continue;
            }

            // Check confidence
            let confidence_bps = self.calculate_confidence_bps(price)?;
            if confidence_bps > self.config.max_confidence_bps {
                warn!(
                    "Rejecting high-confidence price from {:?}: {} bps",
                    price.source, confidence_bps
                );
                continue;
            }

            valid_prices.push(price.clone());
        }

        Ok(valid_prices)
    }

    /// Calculate confidence in basis points
    /// 
    /// Converts absolute confidence to percentage.
    /// 
    /// # Formula
    /// ```text
    /// confidence_bps = (confidence / price) × 10000
    /// ```
    ///
    /// # Example
    /// ```text
    /// Price: $50,000
    /// Confidence: $500
    /// 
    /// confidence_bps = (500 / 50000) × 10000 = 100 bps (1%)
    /// ```
    fn calculate_confidence_bps(&self, price: &PriceData) -> Result<u64> {
        if price.price.is_zero() {
            return Ok(0);
        }

        let confidence_ratio = price.confidence / price.price;
        let confidence_bps = (confidence_ratio * Decimal::from(10000))
            .to_u64()
            .ok_or_else(|| OracleError::ParseError(
                "Failed to convert confidence to u64".to_string()
            ))?;

        Ok(confidence_bps)
    }

    /// Calculate consensus price (median)
    /// 
    /// # Why Median?
    /// 
    /// Median is the middle value when sorted:
    /// - Resistant to outliers
    /// - Doesn't get skewed by extreme values
    /// - Industry standard for combining oracle data
    ///
    /// # Algorithm
    /// 1. Sort prices by value
    /// 2. If odd number of prices: return middle one
    /// 3. If even number: return average of two middle ones
    ///
    /// # Example
    /// ```text
    /// Prices: [$50,000, $50,100, $49,900]
    /// Sorted: [$49,900, $50,000, $50,100]
    /// Median: $50,000 (middle value)
    /// ```
    ///
    /// # Arguments
    /// * `prices` - Valid prices from oracles
    ///
    /// # Returns
    /// Consensus price with source set to `Aggregate`
    fn calculate_consensus(&self, prices: &[PriceData]) -> Result<PriceData> {
        if prices.is_empty() {
            return Err(OracleError::NoPriceData("No prices to aggregate".to_string()));
        }

        // Sort prices by value
        let mut sorted_prices = prices.to_vec();
        sorted_prices.sort_by(|a, b| a.price.cmp(&b.price));

        let len = sorted_prices.len();
        let (consensus_price, consensus_confidence) = if len % 2 == 1 {
            // Odd number: return middle price
            let mid = &sorted_prices[len / 2];
            (mid.price, mid.confidence)
        } else {
            // Even number: average the two middle prices
            let mid1 = &sorted_prices[len / 2 - 1];
            let mid2 = &sorted_prices[len / 2];
            (
                (mid1.price + mid2.price) / Decimal::from(2),
                (mid1.confidence + mid2.confidence) / Decimal::from(2),
            )
        };

        // Use the most recent timestamp
        let latest_timestamp = prices.iter()
            .map(|p| p.timestamp)
            .max()
            .unwrap_or(0);

        Ok(PriceData {
            symbol: prices[0].symbol.clone(),
            price: consensus_price,
            confidence: consensus_confidence,
            timestamp: latest_timestamp,
            source: PriceSource::Aggregate,
        })
    }

    /// Validate consensus against individual prices
    /// 
    /// Ensures all oracle prices are close to the consensus.
    /// If any price deviates too much, reject the entire result.
    ///
    /// # Why?
    /// Large deviations indicate:
    /// - One oracle might be compromised
    /// - Network issues causing stale data
    /// - Market disruption events
    ///
    /// Better to reject than risk using bad prices.
    ///
    /// # Algorithm
    /// For each oracle price:
    ///   Calculate deviation from consensus
    ///   If deviation > threshold → Error
    ///
    /// # Formula
    /// ```text
    /// deviation = |oracle_price - consensus| / consensus × 10000
    /// ```
    ///
    /// # Example
    /// ```text
    /// Consensus: $50,000
    /// Oracle 1: $50,100
    /// 
    /// deviation = |50100 - 50000| / 50000 × 10000
    ///          = 100 / 50000 × 10000
    ///          = 20 bps (0.2%)
    /// 
    /// If max_deviation = 100 bps (1%) → ✅ Valid
    /// ```
    fn validate_consensus(&self, prices: &[PriceData], consensus: &PriceData) -> Result<()> {
        for price in prices {
            let deviation = self.calculate_deviation(price.price, consensus.price)?;

            if deviation > self.config.max_deviation_bps {
                return Err(OracleError::PriceDeviation(format!(
                    "Price from {:?} deviates {} bps from consensus (max: {})",
                    price.source,
                    deviation,
                    self.config.max_deviation_bps
                )));
            }

            debug!(
                "{:?} deviation from consensus: {} bps",
                price.source, deviation
            );
        }

        Ok(())
    }

    /// Calculate price deviation in basis points
    /// 
    /// # Formula
    /// ```text
    /// deviation = |price1 - price2| / price2 × 10000
    /// ```
    fn calculate_deviation(&self, price1: Decimal, price2: Decimal) -> Result<u64> {
        if price2.is_zero() {
            return Ok(0);
        }

        let diff = (price1 - price2).abs();
        let ratio = diff / price2;
        let deviation_bps = (ratio * Decimal::from(10000))
            .to_u64()
            .ok_or_else(|| OracleError::ParseError(
                "Failed to convert deviation to u64".to_string()
            ))?;

        Ok(deviation_bps)
    }

    /// Perform health check on all oracles
    /// 
    /// # Returns
    /// Map of oracle source to health status
    ///
    /// # Example
    /// ```rust
    /// let health = aggregator.health_check().await;
    /// if !health[&PriceSource::Pyth].is_healthy {
    ///     alert!("Pyth is down!");
    /// }
    /// ```
    pub async fn health_check(&mut self) -> HashMap<PriceSource, OracleHealth> {
        let mut health = HashMap::new();

        // Check Pyth
        let pyth_healthy = self.pyth.health_check().await;
        health.insert(
            PriceSource::Pyth,
            OracleHealth {
                source: PriceSource::Pyth,
                is_healthy: pyth_healthy,
                last_update: chrono::Utc::now().timestamp(),
                error_count: if pyth_healthy { 0 } else { 1 },
            },
        );

        // Check Switchboard
        let sb_healthy = self.switchboard.health_check().await;
        health.insert(
            PriceSource::Switchboard,
            OracleHealth {
                source: PriceSource::Switchboard,
                is_healthy: sb_healthy,
                last_update: chrono::Utc::now().timestamp(),
                error_count: if sb_healthy { 0 } else { 1 },
            },
        );

        self.oracle_health = health.clone();
        health
    }

    /// Get health status for a specific oracle
    pub fn get_oracle_health(&self, source: &PriceSource) -> Option<&OracleHealth> {
        self.oracle_health.get(source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_median_odd_count() {
        // Test with 3 prices
        let prices = vec![
            PriceData {
                symbol: "TEST".to_string(),
                price: Decimal::from(100),
                confidence: Decimal::from(1),
                timestamp: 0,
                source: PriceSource::Pyth,
            },
            PriceData {
                symbol: "TEST".to_string(),
                price: Decimal::from(200),
                confidence: Decimal::from(1),
                timestamp: 0,
                source: PriceSource::Switchboard,
            },
            PriceData {
                symbol: "TEST".to_string(),
                price: Decimal::from(150),
                confidence: Decimal::from(1),
                timestamp: 0,
                source: PriceSource::Pyth,
            },
        ];

        let config = OracleConfig {
            max_price_age_seconds: 30,
            max_confidence_bps: 100,
            max_deviation_bps: 100,
        };

        let aggregator = PriceAggregator::new("http://localhost", config);
        let consensus = aggregator.calculate_consensus(&prices).unwrap();

        // Median of [100, 150, 200] = 150
        assert_eq!(consensus.price, Decimal::from(150));
    }

    #[test]
    fn test_median_even_count() {
        let prices = vec![
            PriceData {
                symbol: "TEST".to_string(),
                price: Decimal::from(100),
                confidence: Decimal::from(1),
                timestamp: 0,
                source: PriceSource::Pyth,
            },
            PriceData {
                symbol: "TEST".to_string(),
                price: Decimal::from(200),
                confidence: Decimal::from(1),
                timestamp: 0,
                source: PriceSource::Switchboard,
            },
        ];

        let config = OracleConfig {
            max_price_age_seconds: 30,
            max_confidence_bps: 100,
            max_deviation_bps: 100,
        };

        let aggregator = PriceAggregator::new("http://localhost", config);
        let consensus = aggregator.calculate_consensus(&prices).unwrap();

        // Median of [100, 200] = (100 + 200) / 2 = 150
        assert_eq!(consensus.price, Decimal::from(150));
    }

    #[test]
    fn test_deviation_calculation() {
        let config = OracleConfig {
            max_price_age_seconds: 30,
            max_confidence_bps: 100,
            max_deviation_bps: 100,
        };

        let aggregator = PriceAggregator::new("http://localhost", config);

        // Test 1% deviation
        let deviation = aggregator
            .calculate_deviation(Decimal::from(50500), Decimal::from(50000))
            .unwrap();
        
        // |50500 - 50000| / 50000 × 10000 = 500 / 50000 × 10000 = 100 bps
        assert_eq!(deviation, 100);
    }
}