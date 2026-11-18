use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
pub struct Config {
    /// Lets one program host many pools: part of the config PDA seeds.
    pub seed: u64,
    /// Admin: may change fees and lock the pool. Cannot touch the vaults.
    pub authority: Pubkey,
    /// Wallet that owns the treasury token accounts (protocol fee receiver).
    pub treasury: Pubkey,
    pub mint_x: Pubkey,
    pub mint_y: Pubkey,
    /// Total swap fee in basis points, taken from the input amount.
    pub fee_bps: u16,
    /// Share of that fee sent to the treasury, in basis points of the fee.
    pub protocol_fee_bps: u16,
    /// Locked: no deposits or swaps. Withdrawals always stay open.
    pub locked: bool,
    pub config_bump: u8,
    pub lp_bump: u8,
}
