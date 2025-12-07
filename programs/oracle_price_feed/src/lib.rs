use anchor_lang::prelude::*;
use anchor_lang::solana_program::clock::Clock;


declare_id!("3Lrt5g6ef2RinghQRs3LVHeut4Rap81Z28wzigmqV3kF");

#[program]
pub mod oracle_price_feed {
    use super::*;

        pub fn get_pyth_price(ctx: Context<GetPythPrice>,symbol:String) -> Result<PriceData> {
        // Get the Pyth price account (passed in via ctx.accounts)
        let price_feed = &ctx.accounts.price_feed;
        let clock = Clock::get()?;
        let current_time = clock.unix_timestamp;

        let price_account = price_feed.try_borrow_data()?;
        let price_feed_data = pyth_sdk_solana::load_price_feed_from_account(
            &price_account
        ).map_err(|_| OracleError::InvalidPriceFeed)?;

        // Get the current price from Pyth
        let current_price = price_feed_data
            .get_current_price()
            .ok_or(OracleError::NoPriceData)?;

        let price_age = current_time - current_price.publish_time;
        let config = &ctx.accounts.config;
        
        if price_age > config.max_staleness {
            return Err(OracleError::StalePriceData.into());
        }

        let confidence_bps = (current_price.conf as u128)
            .checked_mul(10000)
            .ok_or(OracleError::MathOverflow)?
            .checked_div(current_price.price.abs() as u128)
            .ok_or(OracleError::MathOverflow)? as u64;

        if confidence_bps > config.max_confidence {
            return Err(OracleError::ConfidenceTooLarge.into());
        }

        Ok(PriceData {
            price: current_price.price,
            confidence: current_price.conf,
            expo: current_price.expo,
            timestamp: current_price.publish_time,
            source: PriceSource::Pyth,
        })
    }

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        msg!("Greetings from: {:?}", ctx.program_id);
        Ok(())
    }


}


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

#[derive(Accounts)]
#[instruction(symbol: String)]
pub struct GetPythPrice<'info> {
    pub price_feed: AccountInfo<'info>,

    #[account(
        seeds = [b"oracle-config", symbol.as_bytes()],
        bump = config.bump,
    )]
    pub config: Account<'info, OracleConfig>,
}

