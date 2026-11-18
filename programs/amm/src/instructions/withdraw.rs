use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    burn, transfer_checked, Burn, Mint, TokenAccount, TokenInterface, TransferChecked,
};

use crate::{curve, error::AmmError, state::Config, CONFIG_SEED, LP_SEED};

/// Burn LP tokens and take back the pro-rata share of both reserves.
/// Deliberately NOT blocked by `locked`: LPs can always exit.
#[derive(Accounts)]
pub struct Withdraw<'info> {
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
        mut,
        associated_token::mint = mint_lp,
        associated_token::authority = user,
        associated_token::token_program = token_program
    )]
    pub user_lp: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
}

impl<'info> Withdraw<'info> {
    /// `min_x`, `min_y`: the least the user accepts back (slippage limit).
    pub fn withdraw(&mut self, lp_amount: u64, min_x: u64, min_y: u64) -> Result<()> {
        require!(lp_amount > 0, AmmError::InvalidAmount);

        let (x_out, y_out) = curve::withdraw_amounts(
            self.vault_x.amount,
            self.vault_y.amount,
            self.mint_lp.supply,
            lp_amount,
        )
        .ok_or(AmmError::MathOverflow)?;
        require!(x_out >= min_x && y_out >= min_y, AmmError::SlippageExceeded);
        require!(x_out > 0 || y_out > 0, AmmError::InvalidAmount);

        // The user's signature authorizes the burn. Burning more LP than the
        // user holds fails inside the Token program.
        burn(
            CpiContext::new(
                self.token_program.key(),
                Burn {
                    mint: self.mint_lp.to_account_info(),
                    from: self.user_lp.to_account_info(),
                    authority: self.user.to_account_info(),
                },
            ),
            lp_amount,
        )?;

        self.pool_pays(true, x_out)?;
        self.pool_pays(false, y_out)
    }

    fn pool_pays(&self, is_x: bool, amount: u64) -> Result<()> {
        if amount == 0 {
            return Ok(());
        }
        let (from, mint, to) = if is_x {
            (&self.vault_x, &self.mint_x, &self.user_x)
        } else {
            (&self.vault_y, &self.mint_y, &self.user_y)
        };
        let seed = self.config.seed.to_le_bytes();
        let seeds: &[&[u8]] = &[CONFIG_SEED, seed.as_ref(), &[self.config.config_bump]];
        transfer_checked(
            CpiContext::new_with_signer(
                self.token_program.key(),
                TransferChecked {
                    from: from.to_account_info(),
                    mint: mint.to_account_info(),
                    to: to.to_account_info(),
                    authority: self.config.to_account_info(),
                },
                &[seeds],
            ),
            amount,
            mint.decimals,
        )
    }
}
