use anchor_lang::prelude::*;

use crate::{instructions::initialize::validate_fees, state::Config, CONFIG_SEED};

/// Admin actions. Only config.authority may sign (has_one). The admin can
/// change fees and pause deposits/swaps, but cannot move any pool funds.
#[derive(Accounts)]
pub struct Admin<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        has_one = authority,
        seeds = [CONFIG_SEED, config.seed.to_le_bytes().as_ref()],
        bump = config.config_bump
    )]
    pub config: Account<'info, Config>,
}

impl<'info> Admin<'info> {
    pub fn update_fees(&mut self, fee_bps: u16, protocol_fee_bps: u16) -> Result<()> {
        validate_fees(fee_bps, protocol_fee_bps)?;
        self.config.fee_bps = fee_bps;
        self.config.protocol_fee_bps = protocol_fee_bps;
        Ok(())
    }

    pub fn set_locked(&mut self, locked: bool) -> Result<()> {
        self.config.locked = locked;
        Ok(())
    }
}
