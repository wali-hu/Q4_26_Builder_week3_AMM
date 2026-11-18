pub mod constants;
pub mod curve;
pub mod error;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("BVaDvxuTZh9m2jTZjogYTLpiL6SUbSTM1iBsq6NFTRmj");

/// Constant product AMM (x * y = k) with a swap fee split between LPs and a
/// treasury, slippage limits on every user action, and an admin who can
/// change fees or pause deposits and swaps (never withdrawals).
#[program]
pub mod amm {
    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        seed: u64,
        fee_bps: u16,
        protocol_fee_bps: u16,
    ) -> Result<()> {
        ctx.accounts
            .initialize(seed, fee_bps, protocol_fee_bps, &ctx.bumps)
    }

    pub fn deposit(ctx: Context<Deposit>, lp_amount: u64, max_x: u64, max_y: u64) -> Result<()> {
        ctx.accounts.deposit(lp_amount, max_x, max_y)
    }

    pub fn swap(ctx: Context<Swap>, x_to_y: bool, amount_in: u64, min_out: u64) -> Result<()> {
        ctx.accounts.swap(x_to_y, amount_in, min_out)
    }

    pub fn withdraw(ctx: Context<Withdraw>, lp_amount: u64, min_x: u64, min_y: u64) -> Result<()> {
        ctx.accounts.withdraw(lp_amount, min_x, min_y)
    }

    pub fn update_fees(ctx: Context<Admin>, fee_bps: u16, protocol_fee_bps: u16) -> Result<()> {
        ctx.accounts.update_fees(fee_bps, protocol_fee_bps)
    }

    pub fn set_locked(ctx: Context<Admin>, locked: bool) -> Result<()> {
        ctx.accounts.set_locked(locked)
    }
}
