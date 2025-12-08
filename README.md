# Oracle Integration & Price Feed System

A production-grade oracle integration system for perpetual futures DEX that aggregates price data from multiple sources (Pyth, Switchboard) with built-in validation and failover mechanisms.

## Features

- Multi-oracle integration (Pyth, Switchboard)
- Price validation and consensus
- Real-time price feeds
- Historical data storage
- 99.99% uptime design

## Architecture

Oracles → Smart Contract → Backend Service → Redis Cache → API
↓
PostgreSQL

## Tech Stack

- Solana/Anchor (Smart Contracts)
- Rust (Backend)
- PostgreSQL (Database)
- Redis (Cache)

  Prerequisites

  You'll need to install:

  1. Rust (latest stable version)
  2. PostgreSQL (database)
  3. Redis (caching)
  4. Solana CLI (optional, for interacting with Solana devnet)

  Setup Instructions

  1. Set up PostgreSQL

  # Install PostgreSQL (macOS)

  brew install postgresql@14
  brew services start postgresql@14

  # Create database

  createdb oracle_db

  2. Set up Redis

  # Install Redis (macOS)

  brew install redis
  brew services start redis

  # Verify Redis is running

  redis-cli ping # Should return "PONG"

  3. Configure Environment Variables

  The .env file already exists in the backend directory. Verify it has the correct settings:

  cd /Users/ankityadav/coding/oracle_price_feed/backend
  cat .env

  If needed, update it to match .env.example with your local settings.

  4. Run the Backend Service

  cd /Users/ankityadav/coding/oracle_price_feed/backend

  # Run the service

  cargo run

  The server will:

  - Connect to PostgreSQL and run migrations
  - Connect to Redis cache
  - Initialize oracle clients (Pyth & Switchboard)
  - Start the API server on http://0.0.0.0:8080

  API Endpoints to Test

  Once the server is running, you can test these endpoints:

  Price Endpoints

  1. Get single price:
     curl http://localhost:8080/api/v1/price/BTC-USD

  2. Get multiple prices:
     curl "http://localhost:8080/api/v1/prices?symbols=BTC-USD,ETH-USD,SOL-USD"

  3. Get price history:
     curl "http://localhost:8080/api/v1/price/BTC-USD/history?limit=10"

  4. Get price statistics:
     curl "http://localhost:8080/api/v1/price/BTC-USD/stats"

  Health Check Endpoints

  5. System health:
     curl http://localhost:8080/api/v1/health

  6. Oracle health details:
     curl http://localhost:8080/api/v1/health/oracles

  Admin Endpoints

  7. Clear cache:
     curl -X POST http://localhost:8080/api/v1/admin/cache/clear

  8. Cache statistics:
     curl http://localhost:8080/api/v1/admin/cache/stats

  Testing with Pretty Output

  For better formatted JSON responses, use jq:

  # Install jq (macOS)

  brew install jq

  # Test with formatted output

  curl -s http://localhost:8080/api/v1/price/BTC-USD | jq

  Available Trading Pairs

  The backend is currently configured with these trading pairs (backend/src/main.rs:56-72):

  - BTC/USD (BTC-USD)
  - ETH/USD (ETH-USD)
  - SOL/USD (SOL-USD)

  Troubleshooting

  If PostgreSQL connection fails:

  # Check DATABASE_URL in .env matches your setup

  # Default: postgresql://postgres:password@localhost/oracle_db

  If Redis connection fails:

  # Verify Redis is running

  brew services list | grep redis
  redis-cli ping

  If oracle data fails:

  - The project uses Solana devnet endpoints
  - Ensure you have internet connectivity to reach api.devnet.solana.com

  Expected Response Format

  {
  "success": true,
  "data": {
  "symbol": "BTC/USD",
  "price": "42500.50",
  "confidence": "10.25",
  "timestamp": 1702234567,
  "sources": ["Pyth", "Switchboard"]
  },
  "timestamp": 1702234567
  }
