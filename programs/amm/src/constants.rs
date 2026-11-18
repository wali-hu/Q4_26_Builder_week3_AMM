use anchor_lang::prelude::*;

#[constant]
pub const CONFIG_SEED: &[u8] = b"config";

#[constant]
pub const LP_SEED: &[u8] = b"lp";

/// Basis points: 10_000 bps = 100%.
pub const BPS: u64 = 10_000;

/// Highest swap fee the pool accepts: 1_000 bps = 10%.
pub const MAX_FEE_BPS: u16 = 1_000;

pub const LP_DECIMALS: u8 = 6;
