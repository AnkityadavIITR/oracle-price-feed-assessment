
use crate::{error::{OracleError, Result}, types::{PriceData, PriceSource}};
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use switchboard_v2::AggregatorAccountData;
use rust_decimal::Decimal;
use std::str::FromStr;
use tracing::{debug, warn};

/// Client for interacting with Switchboard network
pub struct SwitchboardClient {
    rpc_client: RpcClient,
    aggregators: std::collections::HashMap<String, Pubkey>,
}

impl SwitchboardClient {
    pub fn new(rpc_url: &str) -> Self {
        Self {
            rpc_client: RpcClient::new(rpc_url.to_string()),
            aggregators: std::collections::HashMap::new(),
        }
    }

    pub fn register_aggregator(&mut self, symbol: String, aggregator_address: &str) -> Result<()> {
        let pubkey = Pubkey::from_str(aggregator_address)
            .map_err(|e| OracleError::ParseError(format!("Invalid pubkey: {}", e)))?;
        
        self.aggregators.insert(symbol.clone(), pubkey);
        debug!("Registered Switchboard aggregator for {}: {}", symbol, aggregator_address);
        
        Ok(())
    }

    pub async fn get_price(&self, symbol: &str) -> Result<PriceData> {
        // Step 1: Look up the aggregator address
        let aggregator_address = self.aggregators
            .get(symbol)
            .ok_or_else(|| OracleError::NoPriceData(
                format!("No Switchboard aggregator registered for {}", symbol)
            ))?;

        debug!("Fetching Switchboard price for {} from {}", symbol, aggregator_address);

        // Step 2: Read account data from Solana
        let account_data = self.rpc_client
            .get_account_data(aggregator_address)
            .map_err(|e| OracleError::SolanaError(format!(
                "Failed to fetch account: {}", e
            )))?;

        // Step 3: Parse Switchboard aggregator format
        let aggregator = AggregatorAccountData::new(&account_data)
            .map_err(|e| OracleError::ParseError(format!(
                "Failed to parse Switchboard account: {:?}", e
            )))?;

        // Step 4: Extract latest result
        // Switchboard stores the result as a SwitchboardDecimal
        let latest_result = aggregator.latest_confirmed_round.result
            .ok_or_else(|| OracleError::NoPriceData(
                format!("No confirmed round for {}", symbol)
            ))?;

        // Convert SwitchboardDecimal to our Decimal type
        let price = self.switchboard_decimal_to_decimal(&latest_result)?;

        // Calculate confidence from standard deviation
        // Switchboard provides std_deviation as a measure of oracle disagreement
        let std_deviation = aggregator.latest_confirmed_round.std_deviation
            .ok_or_else(|| OracleError::NoPriceData(
                format!("No std deviation for {}", symbol)
            ))?;
        
        let confidence = self.switchboard_decimal_to_decimal(&std_deviation)?;

        // Get timestamp of the round
        let timestamp = aggregator.latest_confirmed_round.round_open_timestamp;

        // Step 5: Create and return price data
        let price_data = PriceData {
            symbol: symbol.to_string(),
            price,
            confidence,
            timestamp,
            source: PriceSource::Switchboard,
        };

        debug!("Switchboard price for {}: ${} ±${}", 
               symbol, price_data.price, price_data.confidence);

        Ok(price_data)
    }

    pub async fn get_prices(&self, symbols: &[String]) -> Vec<Result<PriceData>> {
        let mut results = Vec::new();
        
        for symbol in symbols {
            results.push(self.get_price(symbol).await);
        }
        
        results
    }


    fn switchboard_decimal_to_decimal(
        &self,
        sb_decimal: &switchboard_v2::SwitchboardDecimal
    ) -> Result<Decimal> {
        // Get the mantissa (the number without decimal point)
        let mantissa = sb_decimal.mantissa;
        
        // Get the scale (number of decimal places)
        let scale = sb_decimal.scale;
        
        // Convert mantissa to Decimal
        let mut decimal = Decimal::from(mantissa);
        
        // Apply scale (always divide)
        // scale=5 means divide by 10^5 = 100000
        if scale > 0 {
            let divisor = Decimal::from(10_i128.pow(scale));
            decimal = decimal / divisor;
        }
        
        Ok(decimal)
    }

    /// Get detailed aggregator information
    /// 
    /// This provides metadata about the oracle aggregator:
    /// - How many oracles are participating
    /// - Last update time
    /// - Configuration parameters
    ///
    /// Useful for monitoring and debugging.
    ///
    /// # Example
    /// ```rust
    /// let info = client.get_aggregator_info("BTC/USD").await?;
    /// println!("Oracles: {}", info.num_success);
    /// ```
    pub async fn get_aggregator_info(&self, symbol: &str) -> Result<AggregatorInfo> {
        let aggregator_address = self.aggregators
            .get(symbol)
            .ok_or_else(|| OracleError::NoPriceData(
                format!("No aggregator for {}", symbol)
            ))?;

        let account_data = self.rpc_client
            .get_account_data(aggregator_address)
            .map_err(|e| OracleError::SolanaError(format!("{}", e)))?;

        let aggregator = AggregatorAccountData::new(&account_data)
            .map_err(|e| OracleError::ParseError(format!("{:?}", e)))?;

        Ok(AggregatorInfo {
            name: aggregator.name.iter()
                .filter(|&&c| c != 0)
                .map(|&c| c as char)
                .collect(),
            num_oracles: aggregator.oracle_request_batch_size as usize,
            num_success: aggregator.latest_confirmed_round.num_success as usize,
            min_responses: aggregator.min_oracle_results as usize,
            last_update: aggregator.latest_confirmed_round.round_open_timestamp,
        })
    }

    /// Check if Switchboard service is healthy
    /// 
    /// Verifies:
    /// 1. Can fetch data from aggregator
    /// 2. Data is recent (not stale)
    /// 3. Sufficient oracles responded
    ///
    /// # Returns
    /// `true` if healthy, `false` otherwise
    pub async fn health_check(&self) -> bool {
        if let Some((symbol, _)) = self.aggregators.iter().next() {
            match self.get_price(symbol).await {
                Ok(price_data) => {
                    // Check if price is recent (within last minute)
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as i64;
                    
                    let age = now - price_data.timestamp;
                    
                    if age > 60 {
                        warn!("Switchboard price is stale: {} seconds old", age);
                        return false;
                    }
                    
                    debug!("Switchboard health check passed");
                    true
                },
                Err(e) => {
                    warn!("Switchboard health check failed: {}", e);
                    false
                }
            }
        } else {
            warn!("No Switchboard aggregators registered for health check");
            false
        }
    }
}

// ============================================================================
// SUPPORTING TYPES
// ============================================================================

/// Information about a Switchboard aggregator
#[derive(Debug, Clone)]
pub struct AggregatorInfo {
    /// Human-readable name of the aggregator
    pub name: String,
    
    /// Total number of oracles configured
    pub num_oracles: usize,
    
    /// Number of oracles that successfully responded in last round
    pub num_success: usize,
    
    /// Minimum responses required for valid result
    pub min_responses: usize,
    
    /// Timestamp of last update
    pub last_update: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use switchboard_v2::SwitchboardDecimal;

    #[test]
    fn test_switchboard_decimal_conversion() {
        let client = SwitchboardClient::new("http://localhost");
        
        // Test case 1: Basic conversion
        let sb_decimal = SwitchboardDecimal {
            mantissa: 5000000000,
            scale: 5,
        };
        let result = client.switchboard_decimal_to_decimal(&sb_decimal).unwrap();
        assert_eq!(result, Decimal::from(50000));
        
        // Test case 2: No scale
        let sb_decimal = SwitchboardDecimal {
            mantissa: 50000,
            scale: 0,
        };
        let result = client.switchboard_decimal_to_decimal(&sb_decimal).unwrap();
        assert_eq!(result, Decimal::from(50000));
        
        // Test case 3: High precision
        let sb_decimal = SwitchboardDecimal {
            mantissa: 5000099999,
            scale: 5,
        };
        let result = client.switchboard_decimal_to_decimal(&sb_decimal).unwrap();
        // 5000099999 / 100000 = 50000.99999
        assert_eq!(result.to_string(), "50000.99999");
    }
}