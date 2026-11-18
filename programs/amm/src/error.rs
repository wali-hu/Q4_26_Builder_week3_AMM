use anchor_lang::prelude::*;

#[error_code]
pub enum AmmError {
    // 6000
    #[msg("Amount must be greater than zero")]
    InvalidAmount,
    // 6001
    #[msg("Slippage limit exceeded")]
    SlippageExceeded,
    // 6002
    #[msg("The pool is locked")]
    PoolLocked,
    // 6003
    #[msg("Fee settings out of range")]
    InvalidFee,
    // 6004
    #[msg("Mint X and mint Y must be different")]
    IdenticalMints,
    // 6005
    #[msg("The pool has no liquidity")]
    NoLiquidity,
    // 6006
    #[msg("Math overflow")]
    MathOverflow,
    // 6007
    #[msg("Constant product invariant violated")]
    InvariantViolated,
}
