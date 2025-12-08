//! Redis Cache Layer
//! 
//! This module provides high-performance caching for price data using Redis.
//! 
//! # Why Cache?
//! - Oracle calls are slow (~500ms via RPC)
//! - Redis lookups are fast (~1ms)
//! - Reduces load on Solana RPC
//! - Provides instant price queries for API
//!
//! # Architecture
//! ```text
//! Request → Check Cache → Hit? → Return (1ms)
//!              ↓
//!            Miss? → Fetch Oracle → Store Cache → Return (500ms)
//! ```
//!
//! # Cache Strategy
//! - TTL (Time To Live): 10 seconds
//! - Key format: "price:{symbol}"
//! - Stores JSON-serialized PriceData

use crate::{error::{OracleError, Result}, types::PriceData};
use redis::{aio::ConnectionManager, AsyncCommands};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, warn};

/// Default cache TTL (Time To Live) in seconds
const DEFAULT_CACHE_TTL: usize = 10;

/// Redis cache client
pub struct PriceCache {
    /// Redis connection manager (handles reconnection automatically)
    connection: ConnectionManager,
    
    /// Cache TTL in seconds
    ttl: usize,
}

impl PriceCache {
    /// Create a new price cache
    /// 
    /// # Arguments
    /// * `redis_url` - Redis connection string (e.g., "redis://127.0.0.1")
    ///
    /// # Example
    /// ```rust
    /// let cache = PriceCache::new("redis://127.0.0.1").await?;
    /// ```
    pub async fn new(redis_url: &str) -> Result<Self> {
        let client = redis::Client::open(redis_url)
            .map_err(|e| OracleError::RedisError(e))?;
        
        let connection = ConnectionManager::new(client)
            .await
            .map_err(|e| OracleError::RedisError(e))?;
        
        debug!("Redis cache connected to {}", redis_url);
        
        Ok(Self {
            connection,
            ttl: DEFAULT_CACHE_TTL,
        })
    }

    /// Set custom TTL
    /// 
    /// # Example
    /// ```rust
    /// cache.with_ttl(30); // 30 seconds
    /// ```
    pub fn with_ttl(mut self, ttl: usize) -> Self {
        self.ttl = ttl;
        self
    }

    /// Store a price in cache
    /// 
    /// # How it works:
    /// 1. Serialize PriceData to JSON
    /// 2. Store in Redis with key "price:{symbol}"
    /// 3. Set expiration (TTL)
    ///
    /// # Arguments
    /// * `price` - Price data to cache
    ///
    /// # Example
    /// ```rust
    /// let price = PriceData {
    ///     symbol: "BTC/USD".to_string(),
    ///     price: Decimal::from(50000),
    ///     // ...
    /// };
    /// cache.set_price(&price).await?;
    /// ```
    pub async fn set_price(&mut self, price: &PriceData) -> Result<()> {
        let key = self.make_key(&price.symbol);
        
        // Serialize to JSON
        let json = serde_json::to_string(price)
            .map_err(|e| OracleError::ParseError(format!("JSON serialize error: {}", e)))?;
        
        // Store in Redis with expiration
        self.connection
            .set_ex::<_, _, ()>(&key, json, self.ttl)
            .await
            .map_err(|e| OracleError::RedisError(e))?;
        
        debug!("Cached price for {} (TTL: {}s)", price.symbol, self.ttl);
        Ok(())
    }

    /// Retrieve a price from cache
    /// 
    /// # Returns
    /// - `Some(PriceData)` if cached and not expired
    /// - `None` if cache miss
    ///
    /// # Example
    /// ```rust
    /// if let Some(price) = cache.get_price("BTC/USD").await? {
    ///     println!("Cache hit! Price: ${}", price.price);
    /// } else {
    ///     println!("Cache miss, fetching from oracle...");
    /// }
    /// ```
    pub async fn get_price(&mut self, symbol: &str) -> Result<Option<PriceData>> {
        let key = self.make_key(symbol);
        
        // Try to get from Redis
        let result: Option<String> = self.connection
            .get(&key)
            .await
            .map_err(|e| OracleError::RedisError(e))?;
        
        match result {
            Some(json) => {
                // Cache hit - deserialize JSON
                let price: PriceData = serde_json::from_str(&json)
                    .map_err(|e| OracleError::ParseError(
                        format!("JSON deserialize error: {}", e)
                    ))?;
                
                debug!("Cache HIT for {}", symbol);
                Ok(Some(price))
            }
            None => {
                // Cache miss
                debug!("Cache MISS for {}", symbol);
                Ok(None)
            }
        }
    }

    /// Store multiple prices at once
    /// 
    /// More efficient than calling set_price() in a loop.
    ///
    /// # Example
    /// ```rust
    /// let prices = vec![btc_price, eth_price, sol_price];
    /// cache.set_prices(&prices).await?;
    /// ```
    pub async fn set_prices(&mut self, prices: &[PriceData]) -> Result<()> {
        for price in prices {
            // Note: Could be optimized with Redis pipeline
            self.set_price(price).await?;
        }
        Ok(())
    }

    /// Get multiple prices at once
    /// 
    /// # Returns
    /// Vector of Option<PriceData>, one for each symbol
    ///
    /// # Example
    /// ```rust
    /// let symbols = vec!["BTC/USD", "ETH/USD", "SOL/USD"];
    /// let prices = cache.get_prices(&symbols).await?;
    /// 
    /// for (symbol, price_opt) in symbols.iter().zip(prices.iter()) {
    ///     match price_opt {
    ///         Some(price) => println!("{}: ${}", symbol, price.price),
    ///         None => println!("{}: not cached", symbol),
    ///     }
    /// }
    /// ```
    pub async fn get_prices(&mut self, symbols: &[String]) -> Result<Vec<Option<PriceData>>> {
        let mut results = Vec::new();
        
        for symbol in symbols {
            let price = self.get_price(symbol).await?;
            results.push(price);
        }
        
        Ok(results)
    }

    /// Delete a price from cache
    /// 
    /// Useful when you know a price is stale and want to force refresh.
    ///
    /// # Example
    /// ```rust
    /// // Force refresh on next request
    /// cache.delete_price("BTC/USD").await?;
    /// ```
    pub async fn delete_price(&mut self, symbol: &str) -> Result<()> {
        let key = self.make_key(symbol);
        
        self.connection
            .del::<_, ()>(&key)
            .await
            .map_err(|e| OracleError::RedisError(e))?;
        
        debug!("Deleted cache for {}", symbol);
        Ok(())
    }

    /// Clear all cached prices
    /// 
    /// ⚠️ Use with caution - will cause cache misses for all symbols
    ///
    /// # Example
    /// ```rust
    /// // During maintenance or after config change
    /// cache.clear_all().await?;
    /// ```
    pub async fn clear_all(&mut self) -> Result<()> {
        // Get all price keys
        let pattern = "price:*";
        let keys: Vec<String> = self.connection
            .keys(pattern)
            .await
            .map_err(|e| OracleError::RedisError(e))?;
        
        if !keys.is_empty() {
            self.connection
                .del::<_, ()>(keys)
                .await
                .map_err(|e| OracleError::RedisError(e))?;
            
            debug!("Cleared all cached prices");
        }
        
        Ok(())
    }

    /// Get cache statistics
    /// 
    /// Provides metrics for monitoring cache performance.
    ///
    /// # Example
    /// ```rust
    /// let stats = cache.get_stats().await?;
    /// println!("Cached symbols: {}", stats.total_keys);
    /// println!("Memory used: {} bytes", stats.memory_usage);
    /// ```
    pub async fn get_stats(&mut self) -> Result<CacheStats> {
        // Count price keys
        let pattern = "price:*";
        let keys: Vec<String> = self.connection
            .keys(pattern)
            .await
            .map_err(|e| OracleError::RedisError(e))?;
        
        let total_keys = keys.len();
        
        // Get memory info (requires INFO command)
        let info: String = redis::cmd("INFO")
            .arg("memory")
            .query_async(&mut self.connection)
            .await
            .map_err(|e| OracleError::RedisError(e))?;
        
        // Parse used_memory from INFO output
        let memory_usage = info
            .lines()
            .find(|line| line.starts_with("used_memory:"))
            .and_then(|line| line.split(':').nth(1))
            .and_then(|val| val.parse::<usize>().ok())
            .unwrap_or(0);
        
        Ok(CacheStats {
            total_keys,
            memory_usage,
            ttl: self.ttl,
        })
    }

    /// Check if cache is healthy (can connect to Redis)
    /// 
    /// # Example
    /// ```rust
    /// if !cache.health_check().await {
    ///     alert!("Redis is down!");
    /// }
    /// ```
    pub async fn health_check(&mut self) -> bool {
        // Try a simple PING command
        match redis::cmd("PING")
            .query_async::<_, String>(&mut self.connection)
            .await
        {
            Ok(response) => {
                if response == "PONG" {
                    debug!("Redis health check passed");
                    true
                } else {
                    warn!("Redis health check failed: unexpected response");
                    false
                }
            }
            Err(e) => {
                warn!("Redis health check failed: {}", e);
                false
            }
        }
    }

    /// Generate cache key for a symbol
    /// 
    /// # Format
    /// "price:{symbol}"
    ///
    /// # Example
    /// "price:BTC/USD"
    fn make_key(&self, symbol: &str) -> String {
        format!("price:{}", symbol)
    }
}

// ============================================================================
// SUPPORTING TYPES
// ============================================================================

/// Cache statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    /// Number of cached price keys
    pub total_keys: usize,
    
    /// Redis memory usage in bytes
    pub memory_usage: usize,
    
    /// Cache TTL in seconds
    pub ttl: usize,
}

// ============================================================================
// CACHED PRICE FETCHER (High-level wrapper)
// ============================================================================

/// High-level price fetcher with automatic caching
/// 
/// This wraps the aggregator and cache to provide seamless caching.
///
/// # Usage Pattern
/// ```text
/// Request → Check Cache → Hit? Return cached
///              ↓
///           Miss? → Fetch from Aggregator → Store in Cache → Return
/// ```
pub struct CachedPriceFetcher {
    cache: PriceCache,
}

impl CachedPriceFetcher {
    /// Create a new cached price fetcher
    pub async fn new(redis_url: &str) -> Result<Self> {
        Ok(Self {
            cache: PriceCache::new(redis_url).await?,
        })
    }

    /// Get price with caching
    /// 
    /// This is the function you'd call in your API handlers.
    ///
    /// # How it works:
    /// 1. Check cache
    /// 2. If hit: return cached price (fast path - 1ms)
    /// 3. If miss: fetch from oracle, cache it, return (slow path - 500ms)
    ///
    /// # Arguments
    /// * `symbol` - Trading pair
    /// * `fetch_fn` - Async function to fetch from oracle (only called on cache miss)
    ///
    /// # Example
    /// ```rust
    /// let fetcher = CachedPriceFetcher::new("redis://127.0.0.1").await?;
    /// 
    /// let price = fetcher.get_price_with_cache(
    ///     "BTC/USD",
    ///     |symbol| async {
    ///         aggregator.get_consensus_price(symbol).await
    ///     }
    /// ).await?;
    /// ```
    pub async fn get_price_with_cache<F, Fut>(
        &mut self,
        symbol: &str,
        fetch_fn: F,
    ) -> Result<PriceData>
    where
        F: FnOnce(String) -> Fut,
        Fut: std::future::Future<Output = Result<PriceData>>,
    {
        // Step 1: Try cache
        if let Some(cached_price) = self.cache.get_price(symbol).await? {
            debug!("Serving {} from cache", symbol);
            return Ok(cached_price);
        }

        // Step 2: Cache miss - fetch from oracle
        debug!("Cache miss for {}, fetching from oracle", symbol);
        let price = fetch_fn(symbol.to_string()).await?;

        // Step 3: Store in cache for next time
        self.cache.set_price(&price).await?;

        Ok(price)
    }

    /// Get multiple prices with caching
    /// 
    /// Efficiently fetches multiple symbols, using cache when possible.
    ///
    /// # Example
    /// ```rust
    /// let symbols = vec!["BTC/USD", "ETH/USD", "SOL/USD"];
    /// let prices = fetcher.get_prices_with_cache(
    ///     &symbols,
    ///     |symbols_to_fetch| async {
    ///         // Only fetch uncached symbols
    ///         aggregator.get_prices(&symbols_to_fetch).await
    ///     }
    /// ).await?;
    /// ```
    pub async fn get_prices_with_cache<F, Fut>(
        &mut self,
        symbols: &[String],
        fetch_fn: F,
    ) -> Result<Vec<PriceData>>
    where
        F: FnOnce(Vec<String>) -> Fut,
        Fut: std::future::Future<Output = Result<Vec<PriceData>>>,
    {
        let mut results = Vec::new();
        let mut symbols_to_fetch = Vec::new();
        let mut fetch_indices = Vec::new();

        // Step 1: Check cache for each symbol
        for (idx, symbol) in symbols.iter().enumerate() {
            match self.cache.get_price(symbol).await? {
                Some(cached_price) => {
                    // Cache hit
                    results.push(Some(cached_price));
                }
                None => {
                    // Cache miss
                    results.push(None);
                    symbols_to_fetch.push(symbol.clone());
                    fetch_indices.push(idx);
                }
            }
        }

        // Step 2: Fetch uncached symbols
        if !symbols_to_fetch.is_empty() {
            debug!("Fetching {} symbols from oracle", symbols_to_fetch.len());
            let fetched_prices = fetch_fn(symbols_to_fetch).await?;

            // Step 3: Cache and insert fetched prices
            for (fetch_idx, price) in fetch_indices.iter().zip(fetched_prices.iter()) {
                self.cache.set_price(price).await?;
                results[*fetch_idx] = Some(price.clone());
            }
        }

        // Convert Option<PriceData> to PriceData (all should be Some now)
        Ok(results.into_iter().filter_map(|p| p).collect())
    }

    /// Get cache reference for direct access
    pub fn cache(&mut self) -> &mut PriceCache {
        &mut self.cache
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use crate::types::PriceSource;

    // Helper to create test price data
    fn create_test_price(symbol: &str, price: i64) -> PriceData {
        PriceData {
            symbol: symbol.to_string(),
            price: Decimal::from(price),
            confidence: Decimal::from(100),
            timestamp: chrono::Utc::now().timestamp(),
            source: PriceSource::Aggregate,
        }
    }

    #[tokio::test]
    async fn test_cache_key_format() {
        let cache = PriceCache::new("redis://127.0.0.1").await.unwrap();
        let key = cache.make_key("BTC/USD");
        assert_eq!(key, "price:BTC/USD");
    }

    // Note: The following tests require a running Redis instance
    // Run with: docker run -d -p 6379:6379 redis
    
    #[tokio::test]
    #[ignore] // Ignore by default, run with: cargo test -- --ignored
    async fn test_set_and_get_price() {
        let mut cache = PriceCache::new("redis://127.0.0.1").await.unwrap();
        
        let price = create_test_price("TEST/USD", 50000);
        cache.set_price(&price).await.unwrap();
        
        let retrieved = cache.get_price("TEST/USD").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().symbol, "TEST/USD");
    }

    #[tokio::test]
    #[ignore]
    async fn test_cache_expiration() {
        let mut cache = PriceCache::new("redis://127.0.0.1")
            .await
            .unwrap()
            .with_ttl(1); // 1 second TTL
        
        let price = create_test_price("TEST/USD", 50000);
        cache.set_price(&price).await.unwrap();
        
        // Should be in cache immediately
        let result = cache.get_price("TEST/USD").await.unwrap();
        assert!(result.is_some());
        
        // Wait for expiration
        tokio::time::sleep(Duration::from_secs(2)).await;
        
        // Should be expired
        let result = cache.get_price("TEST/USD").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    #[ignore]
    async fn test_cache_stats() {
        let mut cache = PriceCache::new("redis://127.0.0.1").await.unwrap();
        
        // Clear cache first
        cache.clear_all().await.unwrap();
        
        // Add some prices
        for i in 0..5 {
            let price = create_test_price(&format!("TEST{}/USD", i), 50000);
            cache.set_price(&price).await.unwrap();
        }
        
        let stats = cache.get_stats().await.unwrap();
        assert_eq!(stats.total_keys, 5);
        assert!(stats.memory_usage > 0);
    }

    #[tokio::test]
    #[ignore]
    async fn test_health_check() {
        let mut cache = PriceCache::new("redis://127.0.0.1").await.unwrap();
        let is_healthy = cache.health_check().await;
        assert!(is_healthy);
    }
}