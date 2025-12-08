-- Price History Table
-- Stores all price updates with full metadata
CREATE TABLE IF NOT EXISTS price_history (
    id BIGSERIAL PRIMARY KEY,
    symbol VARCHAR(50) NOT NULL,
    price DECIMAL(20, 8) NOT NULL,
    confidence DECIMAL(20, 8) NOT NULL,
    source VARCHAR(20) NOT NULL,
    timestamp BIGINT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    
    -- Indexes for fast queries
    CONSTRAINT price_history_price_check CHECK (price >= 0),
    CONSTRAINT price_history_confidence_check CHECK (confidence >= 0)
);

-- Index for symbol + timestamp queries (most common)
CREATE INDEX idx_price_history_symbol_timestamp 
ON price_history(symbol, timestamp DESC);

-- Index for time-range queries
CREATE INDEX idx_price_history_timestamp 
ON price_history(timestamp DESC);

-- Index for source-specific queries
CREATE INDEX idx_price_history_source 
ON price_history(source);

-- Oracle Health Table
-- Tracks health status of each oracle source
CREATE TABLE IF NOT EXISTS oracle_health (
    id SERIAL PRIMARY KEY,
    source VARCHAR(20) NOT NULL UNIQUE,
    is_healthy BOOLEAN NOT NULL DEFAULT true,
    last_success_at TIMESTAMP WITH TIME ZONE,
    last_failure_at TIMESTAMP WITH TIME ZONE,
    consecutive_failures INTEGER DEFAULT 0,
    total_requests BIGINT DEFAULT 0,
    total_failures BIGINT DEFAULT 0,
    average_response_time_ms INTEGER,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Price Deviation Alerts
-- Records when prices from different sources disagree significantly
CREATE TABLE IF NOT EXISTS price_deviation_alerts (
    id BIGSERIAL PRIMARY KEY,
    symbol VARCHAR(50) NOT NULL,
    source1 VARCHAR(20) NOT NULL,
    price1 DECIMAL(20, 8) NOT NULL,
    source2 VARCHAR(20) NOT NULL,
    price2 DECIMAL(20, 8) NOT NULL,
    deviation_bps BIGINT NOT NULL,
    threshold_bps BIGINT NOT NULL,
    timestamp BIGINT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

CREATE INDEX idx_deviation_alerts_symbol ON price_deviation_alerts(symbol);
CREATE INDEX idx_deviation_alerts_timestamp ON price_deviation_alerts(timestamp DESC);

-- Aggregated Statistics (for performance)
-- Pre-computed hourly statistics
CREATE TABLE IF NOT EXISTS price_stats_hourly (
    id BIGSERIAL PRIMARY KEY,
    symbol VARCHAR(50) NOT NULL,
    hour_timestamp TIMESTAMP WITH TIME ZONE NOT NULL,
    open_price DECIMAL(20, 8) NOT NULL,
    close_price DECIMAL(20, 8) NOT NULL,
    high_price DECIMAL(20, 8) NOT NULL,
    low_price DECIMAL(20, 8) NOT NULL,
    avg_price DECIMAL(20, 8) NOT NULL,
    update_count INTEGER NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    
    UNIQUE(symbol, hour_timestamp)
);

CREATE INDEX idx_price_stats_symbol_hour ON price_stats_hourly(symbol, hour_timestamp DESC);

-- System Metrics
-- General system health metrics
CREATE TABLE IF NOT EXISTS system_metrics (
    id BIGSERIAL PRIMARY KEY,
    metric_name VARCHAR(100) NOT NULL,
    metric_value DECIMAL(20, 4) NOT NULL,
    tags JSONB,
    timestamp BIGINT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

CREATE INDEX idx_system_metrics_name_timestamp ON system_metrics(metric_name, timestamp DESC);
CREATE INDEX idx_system_metrics_tags ON system_metrics USING GIN(tags);