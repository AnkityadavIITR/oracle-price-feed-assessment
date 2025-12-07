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

#[derive(Accounts)]
pub struct Initialize {}
