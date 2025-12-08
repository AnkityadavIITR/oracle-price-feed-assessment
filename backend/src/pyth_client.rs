
use crate::{error::{OracleError, Result}, types::{PriceData, PriceSource}};
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use pyth_sdk_solana::state::load_price_account;
use rust_decimal::Decimal;
use std::str::FromStr;
use tracing::{debug, warn};

pub struct PythClient {
    rpc_client: RpcClient,
    price_feeds: std::collections::HashMap<String, Pubkey>,
}

impl PythClient {
    pub fn new(rpc_url: &str) -> Self {
        Self {
            rpc_client: RpcClient::new(rpc_url.to_string()),
            price_feeds: std::collections::HashMap::new(),
        }
    }

    pub fn register_feed(&mut self, symbol: String, feed_address: &str) -> Result<()> {
        let pubkey = Pubkey::from_str(feed_address)
            .map_err(|e| OracleError::ParseError(format!("Invalid pubkey: {}", e)))?;
        
        self.price_feeds.insert(symbol.clone(), pubkey);
        debug!("Registered Pyth feed for {}: {}", symbol, feed_address);
        
        Ok(())
    }

    pub async fn get_price(&self, symbol: &str) -> Result<PriceData> {
        // Step 1: Look up the feed address
        let feed_address = self.price_feeds
            .get(symbol)
            .ok_or_else(|| OracleError::NoPriceData(
                format!("No Pyth feed registered for {}", symbol)
            ))?;

        debug!("Fetching Pyth price for {} from {}", symbol, feed_address);

        // Step 2: Read account data from Solana
        let account_data = self.rpc_client
            .get_account_data(feed_address)
            .map_err(|e| OracleError::SolanaError(format!(
                "Failed to fetch account: {}", e
            )))?;

        // Step 3: Parse Pyth price format
        let price_account = load_price_account(&account_data)
            .map_err(|e| OracleError::ParseError(format!(
                "Failed to parse Pyth account: {:?}", e
            )))?;

        let current_price = price_account.agg;

        // Step 4: Convert to decimal format
        let price = self.convert_to_decimal(current_price.price, price_account.expo)?;
        let confidence = self.convert_to_decimal(
            current_price.conf as i64,
            price_account.expo
        )?;

        let timestamp = current_price.pub_slot as i64;

        // Step 5: Create and return price data
        let price_data = PriceData {
            symbol: symbol.to_string(),
            price,
            confidence,
            timestamp,
            source: PriceSource::Pyth,
        };

        debug!("Pyth price for {}: ${} ±${}", 
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

    fn convert_to_decimal(&self, value: i64, expo: i32) -> Result<Decimal> {
        // Convert to Decimal
        let mut decimal = Decimal::from(value);
        if expo < 0 {
            // Example: expo=-2 means divide by 100
            let divisor = Decimal::from(10_i64.pow((-expo) as u32));
            decimal = decimal / divisor;
        } else {
            // Example: expo=2 means multiply by 100
            let multiplier = Decimal::from(10_i64.pow(expo as u32));
            decimal = decimal * multiplier;
        }
        
        Ok(decimal)
    }

    pub async fn health_check(&self) -> bool {
        if let Some((symbol, _)) = self.price_feeds.iter().next() {
            match self.get_price(symbol).await {
                Ok(_) => {
                    debug!("Pyth health check passed");
                    true
                },
                Err(e) => {
                    warn!("Pyth health check failed: {}", e);
                    false
                }
            }
        } else {
            warn!("No Pyth feeds registered for health check");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decimal_conversion() {
        let client = PythClient::new("http://localhost");
        
        // Test case 1: negative exponent
        let result = client.convert_to_decimal(5000000, -2).unwrap();
        assert_eq!(result, Decimal::from(50000));
        
        // Test case 2: positive exponent
        let result = client.convert_to_decimal(500, 2).unwrap();
        assert_eq!(result, Decimal::from(50000));
        
        // Test case 3: zero exponent
        let result = client.convert_to_decimal(50000, 0).unwrap();
        assert_eq!(result, Decimal::from(50000));
    }
}