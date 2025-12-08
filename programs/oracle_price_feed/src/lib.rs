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

        pub fn validate_price_consensus(
        ctx: Context<ValidatePrice>,
        prices: Vec<PriceData>,
    ) -> Result<PriceData> {
        // Need at least 1 price to validate
        require!(!prices.is_empty(), OracleError::NoPriceData);

        let config = &ctx.accounts.config;
        let clock = Clock::get()?;
        let current_time = clock.unix_timestamp;

        // ============================================================
        // STEP 1: Validate each individual price
        // ============================================================
        for price_data in &prices {
            // Check staleness
            let age = current_time - price_data.timestamp;
            if age > config.max_staleness {
                return Err(OracleError::StalePriceData.into());
            }

            // Check confidence (convert to basis points)
            let confidence_bps = (price_data.confidence as u128)
                .checked_mul(10000)
                .ok_or(OracleError::MathOverflow)?
                .checked_div(price_data.price.abs() as u128)
                .ok_or(OracleError::MathOverflow)? as u64;

            if confidence_bps > config.max_confidence {
                return Err(OracleError::ConfidenceTooLarge.into());
            }
        }

        // ============================================================
        // STEP 2: Calculate median price (most reliable)
        // ============================================================
        let median_price = calculate_median(&prices)?;

        // ============================================================
        // STEP 3: Check price deviation (are all sources agreeing?)
        // ============================================================
        for price_data in &prices {
            let deviation = calculate_deviation(
                price_data.price,
                median_price.price
            )?;

            // If any price deviates too much from median, reject all
            if deviation > config.max_deviation {
                return Err(OracleError::PriceDeviationTooLarge.into());
            }
        }

        // All validations passed! Return consensus price
        Ok(median_price)
    }

    // ============================================================================
    // HELPER FUNCTIONS
    // ============================================================================

    /// Calculates median price from multiple sources
    /// 
    /// # Why median instead of average?
    /// Median is resistant to outliers:
    /// - Average of [50000, 50100, 100000] = 66,700 (wrong!)
    /// - Median of [50000, 50100, 100000] = 50,100 (correct!)
    ///
    /// The outlier (100000) doesn't skew the median.
    fn calculate_median(prices: &Vec<PriceData>) -> Result<PriceData> {
        require!(!prices.is_empty(), OracleError::NoPriceData);

        // Create a copy and sort by price
        let mut sorted_prices = prices.clone();
        sorted_prices.sort_by_key(|p| p.price);

        // Get middle element(s)
        let len = sorted_prices.len();
        
        if len % 2 == 1 {
            // Odd number of prices: return middle one
            // Example: [100, 200, 300] → return 200
            Ok(sorted_prices[len / 2].clone())
        } else {
            // Even number of prices: average the two middle ones
            // Example: [100, 200, 300, 400] → average 200 and 300 = 250
            let mid1 = &sorted_prices[len / 2 - 1];
            let mid2 = &sorted_prices[len / 2];
            
            let avg_price = (mid1.price + mid2.price) / 2;
            let avg_confidence = (mid1.confidence + mid2.confidence) / 2;
            
            Ok(PriceData {
                price: avg_price,
                confidence: avg_confidence,
                expo: mid1.expo, // Assume same exponent
                timestamp: mid1.timestamp.max(mid2.timestamp), // Use most recent
                source: PriceSource::Internal, // This is a calculated price
            })
        }
    }

    /// Calculates deviation between two prices in basis points
    /// 
    /// # Formula:
    /// deviation = |price1 - price2| / price2 × 10000
    ///
    /// # Example:
    /// price1 = $50,000
    /// price2 = $50,500
    /// deviation = |50000 - 50500| / 50500 × 10000 = 99 bps (0.99%)
    fn calculate_deviation(price1: i64, price2: i64) -> Result<u64> {
        let diff = (price1 - price2).abs() as u128;
        let base = price2.abs() as u128;
        
        // Calculate: (difference / base_price) × 10000
        let deviation = diff
            .checked_mul(10000)
            .ok_or(OracleError::MathOverflow)?
            .checked_div(base)
            .ok_or(OracleError::MathOverflow)? as u64;
        
        Ok(deviation)
    }

    /// Initialize oracle configuration for a trading symbol
    ///
    /// # Purpose:
    /// Set up rules for a new trading pair (e.g., BTC/USD)
    ///
    /// # Parameters:
    /// - symbol: Trading pair name (e.g., "BTC/USD")
    /// - pyth_feed: Address of Pyth price feed
    /// - switchboard_aggregator: Address of Switchboard aggregator
    /// - max_staleness: Max age in seconds (e.g., 30)
    /// - max_confidence: Max uncertainty in bps (e.g., 100 = 1%)
    /// - max_deviation: Max price difference in bps (e.g., 100 = 1%)
    pub fn initialize_config(
        ctx: Context<InitializeConfig>,
        symbol: String,
        pyth_feed: Pubkey,
        switchboard_aggregator: Pubkey,
        max_staleness: i64,
        max_confidence: u64,
        max_deviation: u64,
    ) -> Result<()> {
        let config = &mut ctx.accounts.config;
        
        config.symbol = symbol;
        config.pyth_feed = pyth_feed;
        config.switchboard_aggregator = switchboard_aggregator;
        config.max_staleness = max_staleness;
        config.max_confidence = max_confidence;
        config.max_deviation = max_deviation;
        config.authority = ctx.accounts.authority.key();
        config.bump = ctx.bumps.config;
        
        Ok(())
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

#[derive(Accounts)]
pub struct ValidatePrice<'info> {
    #[account(
        seeds = [b"oracle-config", config.symbol.as_bytes()],
        bump = config.bump,
    )]
    pub config: Account<'info, OracleConfig>,
}

#[derive(Accounts)]
#[instruction(symbol: String)]
pub struct InitializeConfig<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + // Discriminator
                32 + // symbol (String max 32 bytes)
                32 + // pyth_feed (Pubkey)
                32 + // switchboard_aggregator (Pubkey)
                8 +  // max_staleness (i64)
                8 +  // max_confidence (u64)
                8 +  // max_deviation (u64)
                32 + // authority (Pubkey)
                1,   // bump (u8)
        seeds = [b"oracle-config", symbol.as_bytes()],
        bump
    )]
    pub config: Account<'info, OracleConfig>,
    
    #[account(mut)]
    pub authority: Signer<'info>,
    
    pub system_program: Program<'info, System>,
}


