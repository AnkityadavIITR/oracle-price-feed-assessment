//! Database Layer
//! 
//! Provides persistent storage for price history, health metrics, and analytics.
//! Uses PostgreSQL for reliable, queryable data storage.

use crate::{
    error::{OracleError, Result},
    types::{PriceData, PriceSource, OracleHealth},
};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, error};

/// Database client for price and metrics storage
pub struct Database {
    pool: PgPool,
}

impl Database {
    /// Create a new database client
    /// 
    /// # Arguments
    /// * `database_url` - PostgreSQL connection string
    ///
    /// # Example
    /// ```rust
    /// let db = Database::new("postgresql://user:pass@localhost/oracle_db").await?;
    /// ```
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await
            .map_err(|e| OracleError::DatabaseError(e))?;

        info!("Database connected successfully");

        Ok(Self { pool })
    }

    /// Run database migrations
    /// 
    /// Call this on application startup to ensure schema is up to date.
    pub async fn migrate(&self) -> Result<()> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(|e| OracleError::DatabaseError(e.into()))?;

        info!("Database migrations completed");
        Ok(())
    }

    // ========================================================================
    // PRICE HISTORY
    // ========================================================================

    /// Store a price update in history
    /// 
    /// # Example
    /// ```rust
    /// db.insert_price(&price_data).await?;
    /// ```
    pub async fn insert_price(&self, price: &PriceData) -> Result<i64> {
        let source_str = format!("{:?}", price.source);
        
        let row = sqlx::query(
            r#"
            INSERT INTO price_history (symbol, price, confidence, source, timestamp)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id
            "#
        )
        .bind(&price.symbol)
        .bind(&price.price)
        .bind(&price.confidence)
        .bind(&source_str)
        .bind(price.timestamp)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| OracleError::DatabaseError(e))?;

        let id: i64 = row.get("id");
        debug!("Inserted price history for {} with id {}", price.symbol, id);

        Ok(id)
    }

    /// Insert multiple prices in a batch (more efficient)
    pub async fn insert_prices(&self, prices: &[PriceData]) -> Result<()> {
        let mut tx = self.pool.begin().await
            .map_err(|e| OracleError::DatabaseError(e))?;

        for price in prices {
            let source_str = format!("{:?}", price.source);
            
            sqlx::query(
                r#"
                INSERT INTO price_history (symbol, price, confidence, source, timestamp)
                VALUES ($1, $2, $3, $4, $5)
                "#
            )
            .bind(&price.symbol)
            .bind(&price.price)
            .bind(&price.confidence)
            .bind(&source_str)
            .bind(price.timestamp)
            .execute(&mut *tx)
            .await
            .map_err(|e| OracleError::DatabaseError(e))?;
        }

        tx.commit().await.map_err(|e| OracleError::DatabaseError(e))?;
        debug!("Inserted {} prices in batch", prices.len());

        Ok(())
    }

    /// Get recent price history for a symbol
    /// 
    /// # Arguments
    /// * `symbol` - Trading pair
    /// * `limit` - Maximum number of records to return
    ///
    /// # Example
    /// ```rust
    /// let history = db.get_price_history("BTC/USD", 100).await?;
    /// for record in history {
    ///     println!("{}: ${}", record.timestamp, record.price);
    /// }
    /// ```
    pub async fn get_price_history(
        &self,
        symbol: &str,
        limit: i64,
    ) -> Result<Vec<PriceHistoryRecord>> {
        let rows = sqlx::query_as::<_, PriceHistoryRecord>(
            r#"
            SELECT id, symbol, price, confidence, source, timestamp, created_at
            FROM price_history
            WHERE symbol = $1
            ORDER BY timestamp DESC
            LIMIT $2
            "#
        )
        .bind(symbol)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| OracleError::DatabaseError(e))?;

        Ok(rows)
    }

    /// Get price history within a time range
    /// 
    /// # Example
    /// ```rust
    /// let now = chrono::Utc::now().timestamp();
    /// let one_hour_ago = now - 3600;
    /// let history = db.get_price_history_range("BTC/USD", one_hour_ago, now).await?;
    /// ```
    pub async fn get_price_history_range(
        &self,
        symbol: &str,
        start_timestamp: i64,
        end_timestamp: i64,
    ) -> Result<Vec<PriceHistoryRecord>> {
        let rows = sqlx::query_as::<_, PriceHistoryRecord>(
            r#"
            SELECT id, symbol, price, confidence, source, timestamp, created_at
            FROM price_history
            WHERE symbol = $1 AND timestamp >= $2 AND timestamp <= $3
            ORDER BY timestamp ASC
            "#
        )
        .bind(symbol)
        .bind(start_timestamp)
        .bind(end_timestamp)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| OracleError::DatabaseError(e))?;

        Ok(rows)
    }

    /// Get price statistics for a symbol
    /// 
    /// Returns min, max, average over a time period.
    pub async fn get_price_stats(
        &self,
        symbol: &str,
        start_timestamp: i64,
        end_timestamp: i64,
    ) -> Result<PriceStats> {
        let row = sqlx::query(
            r#"
            SELECT 
                MIN(price) as min_price,
                MAX(price) as max_price,
                AVG(price) as avg_price,
                STDDEV(price) as std_dev,
                COUNT(*) as count
            FROM price_history
            WHERE symbol = $1 AND timestamp >= $2 AND timestamp <= $3
            "#
        )
        .bind(symbol)
        .bind(start_timestamp)
        .bind(end_timestamp)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| OracleError::DatabaseError(e))?;

        Ok(PriceStats {
            symbol: symbol.to_string(),
            min_price: row.get("min_price"),
            max_price: row.get("max_price"),
            avg_price: row.get("avg_price"),
            std_dev: row.get("std_dev"),
            count: row.get("count"),
        })
    }

    // ========================================================================
    // ORACLE HEALTH TRACKING
    // ========================================================================

    /// Update oracle health status
    pub async fn update_oracle_health(&self, health: &OracleHealth) -> Result<()> {
        let source_str = format!("{:?}", health.source);
        
        sqlx::query(
            r#"
            INSERT INTO oracle_health (source, is_healthy, last_success_at, updated_at)
            VALUES ($1, $2, to_timestamp($3), NOW())
            ON CONFLICT (source) 
            DO UPDATE SET 
                is_healthy = $2,
                last_success_at = to_timestamp($3),
                updated_at = NOW(),
                consecutive_failures = CASE WHEN $2 THEN 0 ELSE oracle_health.consecutive_failures + 1 END,
                total_requests = oracle_health.total_requests + 1,
                total_failures = oracle_health.total_failures + CASE WHEN $2 THEN 0 ELSE 1 END
            "#
        )
        .bind(&source_str)
        .bind(health.is_healthy)
        .bind(health.last_update)
        .execute(&self.pool)
        .await
        .map_err(|e| OracleError::DatabaseError(e))?;

        debug!("Updated health for oracle {:?}: {}", health.source, health.is_healthy);
        Ok(())
    }

    /// Get oracle health status
    pub async fn get_oracle_health(&self, source: PriceSource) -> Result<Option<OracleHealthRecord>> {
        let source_str = format!("{:?}", source);
        
        let row = sqlx::query_as::<_, OracleHealthRecord>(
            r#"
            SELECT * FROM oracle_health WHERE source = $1
            "#
        )
        .bind(&source_str)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| OracleError::DatabaseError(e))?;

        Ok(row)
    }

    /// Get all oracle health statuses
    pub async fn get_all_oracle_health(&self) -> Result<Vec<OracleHealthRecord>> {
        let rows = sqlx::query_as::<_, OracleHealthRecord>(
            r#"
            SELECT * FROM oracle_health ORDER BY source
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| OracleError::DatabaseError(e))?;

        Ok(rows)
    }

    // ========================================================================
    // DEVIATION ALERTS
    // ========================================================================

    /// Record a price deviation alert
    pub async fn insert_deviation_alert(&self, alert: &DeviationAlert) -> Result<i64> {
        let source1_str = format!("{:?}", alert.source1);
        let source2_str = format!("{:?}", alert.source2);
        
        let row = sqlx::query(
            r#"
            INSERT INTO price_deviation_alerts 
            (symbol, source1, price1, source2, price2, deviation_bps, threshold_bps, timestamp)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING id
            "#
        )
        .bind(&alert.symbol)
        .bind(&source1_str)
        .bind(&alert.price1)
        .bind(&source2_str)
        .bind(&alert.price2)
        .bind(alert.deviation_bps as i64)
        .bind(alert.threshold_bps as i64)
        .bind(alert.timestamp)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| OracleError::DatabaseError(e))?;

        let id: i64 = row.get("id");
        debug!("Inserted deviation alert for {} with id {}", alert.symbol, id);

        Ok(id)
    }

    /// Get recent deviation alerts
    pub async fn get_deviation_alerts(&self, limit: i64) -> Result<Vec<DeviationAlertRecord>> {
        let rows = sqlx::query_as::<_, DeviationAlertRecord>(
            r#"
            SELECT * FROM price_deviation_alerts
            ORDER BY timestamp DESC
            LIMIT $1
            "#
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| OracleError::DatabaseError(e))?;

        Ok(rows)
    }

    // ========================================================================
    // CLEANUP & MAINTENANCE
    // ========================================================================

    /// Delete old price history (retention policy)
    /// 
    /// # Example
    /// ```rust
    /// // Keep last 30 days only
    /// let cutoff = chrono::Utc::now().timestamp() - (30 * 24 * 3600);
    /// db.cleanup_old_prices(cutoff).await?;
    /// ```
    pub async fn cleanup_old_prices(&self, before_timestamp: i64) -> Result<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM price_history WHERE timestamp < $1
            "#
        )
        .bind(before_timestamp)
        .execute(&self.pool)
        .await
        .map_err(|e| OracleError::DatabaseError(e))?;

        let deleted = result.rows_affected();
        info!("Cleaned up {} old price records", deleted);

        Ok(deleted)
    }

    /// Health check - verify database connectivity
    pub async fn health_check(&self) -> bool {
        sqlx::query("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .is_ok()
    }
}

// ============================================================================
// DATABASE RECORD TYPES
// ============================================================================

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct PriceHistoryRecord {
    pub id: i64,
    pub symbol: String,
    pub price: Decimal,
    pub confidence: Decimal,
    pub source: String,
    pub timestamp: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceStats {
    pub symbol: String,
    pub min_price: Decimal,
    pub max_price: Decimal,
    pub avg_price: Decimal,
    pub std_dev: Option<Decimal>,
    pub count: i64,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct OracleHealthRecord {
    pub id: i32,
    pub source: String,
    pub is_healthy: bool,
    pub last_success_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_failure_at: Option<chrono::DateTime<chrono::Utc>>,
    pub consecutive_failures: i32,
    pub total_requests: i64,
    pub total_failures: i64,
    pub average_response_time_ms: Option<i32>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviationAlert {
    pub symbol: String,
    pub source1: PriceSource,
    pub price1: Decimal,
    pub source2: PriceSource,
    pub price2: Decimal,
    pub deviation_bps: u64,
    pub threshold_bps: u64,
    pub timestamp: i64,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct DeviationAlertRecord {
    pub id: i64,
    pub symbol: String,
    pub source1: String,
    pub price1: Decimal,
    pub source2: String,
    pub price2: Decimal,
    pub deviation_bps: i64,
    pub threshold_bps: i64,
    pub timestamp: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}