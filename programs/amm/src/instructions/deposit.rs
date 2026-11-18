use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{
        mint_to, transfer_checked, Mint, MintTo, TokenAccount, TokenInterface, TransferChecked,
    },
};

use crate::{curve, error::AmmError, state::Config, CONFIG_SEED, LP_SEED};

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        has_one = mint_x,
        has_one = mint_y,
        seeds = [CONFIG_SEED, config.seed.to_le_bytes().as_ref()],
        bump = config.config_bump
    )]
    pub config: Box<Account<'info, Config>>,

    pub mint_x: Box<InterfaceAccount<'info, Mint>>,
    pub mint_y: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        seeds = [LP_SEED, config.key().as_ref()],
        bump = config.lp_bump
    )]
    pub mint_lp: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        associated_token::mint = mint_x,
        associated_token::authority = config,
        associated_token::token_program = token_program
    )]
    pub vault_x: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        associated_token::mint = mint_y,
        associated_token::authority = config,
        associated_token::token_program = token_program
    )]
    pub vault_y: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = mint_x,
        associated_token::authority = user,
        associated_token::token_program = token_program
    )]
    pub user_x: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        associated_token::mint = mint_y,
        associated_token::authority = user,
        associated_token::token_program = token_program
    )]
    pub user_y: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = mint_lp,
        associated_token::authority = user,
        associated_token::token_program = token_program
    )]
    pub user_lp: Box<InterfaceAccount<'info, TokenAccount>>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> Deposit<'info> {
    /// `lp_amount`: LP tokens wanted. For the first deposit it is a minimum,
    /// because the LP amount is fixed by sqrt(max_x * max_y).
    /// `max_x`, `max_y`: the most the user is willing to pay (slippage limit).
    pub fn deposit(&mut self, lp_amount: u64, max_x: u64, max_y: u64) -> Result<()> {
        require!(!self.config.locked, AmmError::PoolLocked);

        let supply = self.mint_lp.supply;
        let (x_in, y_in, lp_out) = if supply == 0 {
            // First deposit sets the price: exactly max_x and max_y go in.
            require!(max_x > 0 && max_y > 0, AmmError::InvalidAmount);
            let lp = curve::initial_lp(max_x, max_y).ok_or(AmmError::MathOverflow)?;
            require!(lp > 0, AmmError::InvalidAmount);
            require!(lp >= lp_amount, AmmError::SlippageExceeded);
            (max_x, max_y, lp)
        } else {
            require!(lp_amount > 0, AmmError::InvalidAmount);
            let (x, y) = curve::deposit_amounts(
                self.vault_x.amount,
                self.vault_y.amount,
                supply,
                lp_amount,
            )
            .ok_or(AmmError::MathOverflow)?;
            require!(x <= max_x && y <= max_y, AmmError::SlippageExceeded);
            (x, y, lp_amount)
        };

        self.pay_in(true, x_in)?;
        self.pay_in(false, y_in)?;
        self.mint_lp_to_user(lp_out)
    }

    fn pay_in(&self, is_x: bool, amount: u64) -> Result<()> {
        let (from, to, mint, decimals) = if is_x {
            (&self.user_x, &self.vault_x, &self.mint_x, self.mint_x.decimals)
        } else {
            (&self.user_y, &self.vault_y, &self.mint_y, self.mint_y.decimals)
        };
        transfer_checked(
            CpiContext::new(
                self.token_program.key(),
                TransferChecked {
                    from: from.to_account_info(),
                    mint: mint.to_account_info(),
                    to: to.to_account_info(),
                    authority: self.user.to_account_info(),
                },
            ),
            amount,
            decimals,
        )
    }

    fn mint_lp_to_user(&self, amount: u64) -> Result<()> {
        let seed = self.config.seed.to_le_bytes();
        let seeds: &[&[u8]] = &[CONFIG_SEED, seed.as_ref(), &[self.config.config_bump]];
        mint_to(
            CpiContext::new_with_signer(
                self.token_program.key(),
                MintTo {
                    mint: self.mint_lp.to_account_info(),
                    to: self.user_lp.to_account_info(),
                    authority: self.config.to_account_info(),
                },
                &[seeds],
            ),
            amount,
        )
    }
}
