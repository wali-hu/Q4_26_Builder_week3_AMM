use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{Mint, TokenAccount, TokenInterface},
};

use crate::{
    constants::{BPS, LP_DECIMALS, MAX_FEE_BPS},
    error::AmmError,
    state::Config,
    CONFIG_SEED, LP_SEED,
};

#[derive(Accounts)]
#[instruction(seed: u64)]
pub struct Initialize<'info> {
    /// Pays for every account and becomes the pool's admin.
    #[account(mut)]
    pub authority: Signer<'info>,

    /// CHECK: only its address is used, as the owner of the treasury token accounts.
    pub treasury: UncheckedAccount<'info>,

    #[account(mint::token_program = token_program)]
    pub mint_x: Box<InterfaceAccount<'info, Mint>>,
    #[account(mint::token_program = token_program)]
    pub mint_y: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        init,
        payer = authority,
        seeds = [CONFIG_SEED, seed.to_le_bytes().as_ref()],
        bump,
        space = Config::DISCRIMINATOR.len() + Config::INIT_SPACE
    )]
    pub config: Box<Account<'info, Config>>,

    // LP mint: only the config PDA can mint, so LP tokens exist only against deposits.
    #[account(
        init,
        payer = authority,
        seeds = [LP_SEED, config.key().as_ref()],
        bump,
        mint::decimals = LP_DECIMALS,
        mint::authority = config,
        mint::token_program = token_program
    )]
    pub mint_lp: Box<InterfaceAccount<'info, Mint>>,

    // The two reserves. Their token authority is the config PDA.
    #[account(
        init,
        payer = authority,
        associated_token::mint = mint_x,
        associated_token::authority = config,
        associated_token::token_program = token_program
    )]
    pub vault_x: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        init,
        payer = authority,
        associated_token::mint = mint_y,
        associated_token::authority = config,
        associated_token::token_program = token_program
    )]
    pub vault_y: Box<InterfaceAccount<'info, TokenAccount>>,

    // Treasury accounts for both tokens (init_if_needed: one treasury can serve many pools).
    #[account(
        init_if_needed,
        payer = authority,
        associated_token::mint = mint_x,
        associated_token::authority = treasury,
        associated_token::token_program = token_program
    )]
    pub treasury_x: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        init_if_needed,
        payer = authority,
        associated_token::mint = mint_y,
        associated_token::authority = treasury,
        associated_token::token_program = token_program
    )]
    pub treasury_y: Box<InterfaceAccount<'info, TokenAccount>>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn validate_fees(fee_bps: u16, protocol_fee_bps: u16) -> Result<()> {
    require!(fee_bps <= MAX_FEE_BPS, AmmError::InvalidFee);
    require!(protocol_fee_bps as u64 <= BPS, AmmError::InvalidFee);
    Ok(())
}

impl<'info> Initialize<'info> {
    pub fn initialize(
        &mut self,
        seed: u64,
        fee_bps: u16,
        protocol_fee_bps: u16,
        bumps: &InitializeBumps,
    ) -> Result<()> {
        require_keys_neq!(self.mint_x.key(), self.mint_y.key(), AmmError::IdenticalMints);
        validate_fees(fee_bps, protocol_fee_bps)?;

        self.config.set_inner(Config {
            seed,
            authority: self.authority.key(),
            treasury: self.treasury.key(),
            mint_x: self.mint_x.key(),
            mint_y: self.mint_y.key(),
            fee_bps,
            protocol_fee_bps,
            locked: false,
            config_bump: bumps.config,
            lp_bump: bumps.mint_lp,
        });
        Ok(())
    }
}
