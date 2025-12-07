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
