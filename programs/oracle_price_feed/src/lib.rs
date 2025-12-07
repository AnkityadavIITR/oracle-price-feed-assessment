use anchor_lang::prelude::*;

declare_id!("3Lrt5g6ef2RinghQRs3LVHeut4Rap81Z28wzigmqV3kF");

#[program]
pub mod oracle_price_feed {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        msg!("Greetings from: {:?}", ctx.program_id);
        Ok(())
    }
}

// ============================================================================
// DATA STRUCTURES
// ============================================================================

/// Represents price data from any oracle source
/// Think of this like a "Price Report" with all necessary info
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct PriceData {
    pub price: i64,
    pub confidence: u64,
    pub expo: i32,
    pub timestamp: i64,
    pub source: PriceSource,
}

/// Enum to identify which oracle provided the price
#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Debug)]
pub enum PriceSource {
    Pyth,        // From Pyth Network
    Switchboard, // From Switchboard
    Internal,    // Calculated internally (consensus)
}

/// Configuration for a trading symbol (e.g., BTC/USD)
/// This is stored on-chain and defines rules for that symbol
#[account]
pub struct OracleConfig {
    pub symbol: String,
    
    pub pyth_feed: Pubkey,
    
    pub switchboard_aggregator: Pubkey,

    pub max_staleness: i64,

    pub max_confidence: u64,
    
    pub max_deviation: u64,
    
    pub authority: Pubkey,

    pub bump: u8,
}

/// Custom error codes for our program
#[error_code]
pub enum OracleError {
    #[msg("Price data is too old (stale)")]
    StalePriceData,
    
    #[msg("Confidence interval too large (price unreliable)")]
    ConfidenceTooLarge,
    
    #[msg("Price sources disagree too much")]
    PriceDeviationTooLarge,
    
    #[msg("No valid price data available")]
    NoPriceData,
    
    #[msg("Invalid price feed account")]
    InvalidPriceFeed,
    
    #[msg("Math overflow in calculation")]
    MathOverflow,
}


#[derive(Accounts)]
pub struct Initialize {}
