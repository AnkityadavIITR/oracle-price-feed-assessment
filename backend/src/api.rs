//! REST API
//! 
//! Provides HTTP endpoints for accessing price data, health status, and analytics.

use crate::{
    error::{OracleError, Result},
    types::PriceData,
    price_aggregator::PriceAggregator,
    cache::CachedPriceFetcher,
    database::Database,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response, Json},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tracing::info;

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub aggregator: Arc<Mutex<PriceAggregator>>,
    pub cache: Arc<Mutex<CachedPriceFetcher>>,
    pub db: Arc<Database>,
}

/// Create the API router
pub fn create_router(state: AppState) -> Router {
    Router::new()
        // Price endpoints
        .route("/api/v1/price/:symbol", get(get_price))
        .route("/api/v1/prices", get(get_all_prices))
        .route("/api/v1/price/:symbol/history", get(get_price_history))
        .route("/api/v1/price/:symbol/stats", get(get_price_stats))
        
        // Health endpoints
        .route("/api/v1/health", get(health_check))
        .route("/api/v1/health/oracles", get(oracle_health))
        
        // Admin endpoints
        .route("/api/v1/admin/cache/clear", post(clear_cache))
        .route("/api/v1/admin/cache/stats", get(cache_stats))
        
        .layer(CorsLayer::permissive())
        .with_state(state)
}

// ============================================================================
// PRICE ENDPOINTS
// ============================================================================

/// GET /api/v1/price/:symbol
/// 
/// Get current price for a symbol
/// 
/// Example: GET /api/v1/price/BTC-USD
#[axum::debug_handler]
async fn get_price(
    State(state): State<AppState>,
    Path(symbol): Path<String>,
) -> Result<Json<PriceResponse>> {
    let symbol = symbol.replace("-", "/");
    
    info!("Fetching price for {}", symbol);
    
    let price = {
        let mut cache = state.cache.lock().await;
        let aggregator = state.aggregator.lock().await;
        
        cache.get_price_with_cache(
            &symbol,
            |s| {
                let agg = aggregator.clone();
                async move {
                    agg.get_consensus_price(&s).await
                }
            }
        ).await?
    };
    
    // Store in database for history
    state.db.insert_price(&price).await?;
    
    Ok(Json(PriceResponse {
        success: true,
        data: price,
        timestamp: chrono::Utc::now().timestamp(),
    }))
}

/// GET /api/v1/prices
/// 
/// Get current prices for multiple symbols
/// 
/// Query params: ?symbols=BTC-USD,ETH-USD,SOL-USD
async fn get_all_prices(
    State(state): State<AppState>,
    Query(params): Query<MultiPriceQuery>,
) -> Result<Json<MultiPriceResponse>> {
    let symbols: Vec<String> = params.symbols
        .split(',')
        .map(|s| s.trim().replace("-", "/").to_string())
        .collect();
    
    info!("Fetching prices for {} symbols", symbols.len());
    
    let mut prices = Vec::new();
    
    for symbol in symbols {
        match get_price_internal(&state, &symbol).await {
            Ok(price) => prices.push(price),
            Err(e) => {
                tracing::warn!("Failed to fetch {}: {}", symbol, e);
            }
        }
    }
    
    Ok(Json(MultiPriceResponse {
        success: true,
        data: prices,
        count: prices.len(),
        timestamp: chrono::Utc::now().timestamp(),
    }))
}

/// GET /api/v1/price/:symbol/history
/// 
/// Get price history for a symbol
/// 
/// Query params: ?limit=100&start=<timestamp>&end=<timestamp>
async fn get_price_history(
    State(state): State<AppState>,
    Path(symbol): Path<String>,
    Query(params): Query<HistoryQuery>,
) -> Result<Json<HistoryResponse>> {
    let symbol = symbol.replace("-", "/");
    
    let history = if let (Some(start), Some(end)) = (params.start, params.end) {
        state.db.get_price_history_range(&symbol, start, end).await?
    } else {
        let limit = params.limit.unwrap_or(100);
        state.db.get_price_history(&symbol, limit).await?
    };
    
    Ok(Json(HistoryResponse {
        success: true,
        data: history,
        count: history.len(),
    }))
}

/// GET /api/v1/price/:symbol/stats
/// 
/// Get price statistics for a symbol
/// 
/// Query params: ?start=<timestamp>&end=<timestamp>
async fn get_price_stats(
    State(state): State<AppState>,
    Path(symbol): Path<String>,
    Query(params): Query<StatsQuery>,
) -> Result<Json<StatsResponse>> {
    let symbol = symbol.replace("-", "/");
    
    let now = chrono::Utc::now().timestamp();
    let start = params.start.unwrap_or(now - 3600); // Default: last hour
    let end = params.end.unwrap_or(now);
    
    let stats = state.db.get_price_stats(&symbol, start, end).await?;
    
    Ok(Json(StatsResponse {
        success: true,
        data: stats,
    }))
}

// ============================================================================
// HEALTH ENDPOINTS
// ============================================================================

/// GET /api/v1/health
/// 
/// System health check
async fn health_check(State(state): State<AppState>) -> Json<HealthResponse> {
    let db_healthy = state.db.health_check().await;
    let cache_healthy = state.cache.lock().await.cache().health_check().await;
    
    let mut aggregator = state.aggregator.lock().await;
    let oracle_health = aggregator.health_check().await;
    
    let all_healthy = db_healthy && cache_healthy && 
        oracle_health.values().all(|h| h.is_healthy);
    
    Json(HealthResponse {
        success: all_healthy,
        database: db_healthy,
        cache: cache_healthy,
        oracles: oracle_health.into_iter()
            .map(|(source, health)| (format!("{:?}", source), health.is_healthy))
            .collect(),
        timestamp: chrono::Utc::now().timestamp(),
    })
}

/// GET /api/v1/health/oracles
/// 
/// Detailed oracle health information
async fn oracle_health(State(state): State<AppState>) -> Result<Json<OracleHealthResponse>> {
    let health_records = state.db.get_all_oracle_health().await?;
    
    Ok(Json(OracleHealthResponse {
        success: true,
        data: health_records,
    }))
}

// ============================================================================
// ADMIN ENDPOINTS
// ============================================================================

/// POST /api/v1/admin/cache/clear
/// 
/// Clear all cached prices
async fn clear_cache(State(state): State<AppState>) -> Result<Json<AdminResponse>> {
    state.cache.lock().await.cache().clear_all().await?;
    
    Ok(Json(AdminResponse {
        success: true,
        message: "Cache cleared successfully".to_string(),
    }))
}

/// GET /api/v1/admin/cache/stats
/// 
/// Get cache statistics
async fn cache_stats(State(state): State<AppState>) -> Result<Json<CacheStatsResponse>> {
    let stats = state.cache.lock().await.cache().get_stats().await?;
    
    Ok(Json(CacheStatsResponse {
        success: true,
        data: stats,
    }))
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

async fn get_price_internal(state: &AppState, symbol: &str) -> Result<PriceData> {
    let mut cache = state.cache.lock().await;
    let aggregator = state.aggregator.lock().await;
    
    let price = cache.get_price_with_cache(
        symbol,
        |s| {
            let agg = aggregator.clone();
            async move {
                agg.get_consensus_price(&s).await
            }
        }
    ).await?;
    
    state.db.insert_price(&price).await?;
    
    Ok(price)
}

// ============================================================================
// REQUEST/RESPONSE TYPES
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct MultiPriceQuery {
    pub symbols: String,
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<i64>,
    pub start: Option<i64>,
    pub end: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct StatsQuery {
    pub start: Option<i64>,
    pub end: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct PriceResponse {
    pub success: bool,
    pub data: PriceData,
    pub timestamp: i64,
}

#[derive(Debug, Serialize)]
pub struct MultiPriceResponse {
    pub success: bool,
    pub data: Vec<PriceData>,
    pub count: usize,
    pub timestamp: i64,
}

#[derive(Debug, Serialize)]
pub struct HistoryResponse {
    pub success: bool,
    pub data: Vec<crate::database::PriceHistoryRecord>,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct StatsResponse {
    pub success: bool,
    pub data: crate::database::PriceStats,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub success: bool,
    pub database: bool,
    pub cache: bool,
    pub oracles: std::collections::HashMap<String, bool>,
    pub timestamp: i64,
}

#[derive(Debug, Serialize)]
pub struct OracleHealthResponse {
    pub success: bool,
    pub data: Vec<crate::database::OracleHealthRecord>,
}

#[derive(Debug, Serialize)]
pub struct AdminResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct CacheStatsResponse {
    pub success: bool,
    pub data: crate::cache::CacheStats,
}

// ============================================================================
// ERROR HANDLING
// ============================================================================

impl IntoResponse for OracleError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            OracleError::NoPriceData(msg) => (StatusCode::NOT_FOUND, msg),
            OracleError::StalePrice(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg),
            OracleError::PriceDeviation(msg) => (StatusCode::CONFLICT, msg),
            OracleError::DatabaseError(e) => {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e))
            }
            OracleError::RedisError(e) => {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Cache error: {}", e))
            }
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = Json(serde_json::json!({
            "success": false,
            "error": message,
        }));

        (status, body).into_response()
    }
}