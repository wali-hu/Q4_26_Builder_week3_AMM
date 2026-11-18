use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked},
};

use crate::{curve, error::AmmError, state::Config, CONFIG_SEED};

#[derive(Accounts)]
pub struct Swap<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        has_one = mint_x,
        has_one = mint_y,
        has_one = treasury,
        seeds = [CONFIG_SEED, config.seed.to_le_bytes().as_ref()],
        bump = config.config_bump
    )]
    pub config: Box<Account<'info, Config>>,

    /// CHECK: must equal config.treasury (has_one above); only its address is used.
    pub treasury: UncheckedAccount<'info>,

    pub mint_x: Box<InterfaceAccount<'info, Mint>>,
    pub mint_y: Box<InterfaceAccount<'info, Mint>>,

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

    // The user may not hold the output token yet: create the ATA if needed.
    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = mint_x,
        associated_token::authority = user,
        associated_token::token_program = token_program
    )]
    pub user_x: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = mint_y,
        associated_token::authority = user,
        associated_token::token_program = token_program
    )]
    pub user_y: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = mint_x,
        associated_token::authority = treasury,
        associated_token::token_program = token_program
    )]
    pub treasury_x: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        associated_token::mint = mint_y,
        associated_token::authority = treasury,
        associated_token::token_program = token_program
    )]
    pub treasury_y: Box<InterfaceAccount<'info, TokenAccount>>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> Swap<'info> {
    /// Exact-input swap. `x_to_y = true` sells X for Y, false sells Y for X.
    /// `min_out` is the least the user accepts (slippage limit).
    pub fn swap(&mut self, x_to_y: bool, amount_in: u64, min_out: u64) -> Result<()> {
        require!(!self.config.locked, AmmError::PoolLocked);
        require!(amount_in > 0, AmmError::InvalidAmount);

        let (reserve_in, reserve_out) = if x_to_y {
            (self.vault_x.amount, self.vault_y.amount)
        } else {
            (self.vault_y.amount, self.vault_x.amount)
        };
        require!(reserve_in > 0 && reserve_out > 0, AmmError::NoLiquidity);

        // 1. Fee comes off the input. LPs' part stays in the vault, the
        //    protocol part goes to the treasury.
        let (fee, protocol_fee) =
            curve::fee_split(amount_in, self.config.fee_bps, self.config.protocol_fee_bps)
                .ok_or(AmmError::MathOverflow)?;
        let in_after_fee = amount_in.checked_sub(fee).ok_or(AmmError::MathOverflow)?;

        // 2. Output from x * y = k on the amount after fee (rounded down).
        let amount_out =
            curve::swap_out(reserve_in, reserve_out, in_after_fee).ok_or(AmmError::MathOverflow)?;
        require!(amount_out > 0, AmmError::InvalidAmount);
        require!(amount_out >= min_out, AmmError::SlippageExceeded);

        // 3. Safety net: k must never go down. The vault receives everything
        //    except the protocol fee.
        let to_vault = amount_in.checked_sub(protocol_fee).ok_or(AmmError::MathOverflow)?;
        let k_before = (reserve_in as u128) * (reserve_out as u128);
        let k_after = (reserve_in as u128 + to_vault as u128) * ((reserve_out - amount_out) as u128);
        require!(k_after >= k_before, AmmError::InvariantViolated);

        // 4. Move the tokens.
        self.user_pays(x_to_y, to_vault, false)?;
        if protocol_fee > 0 {
            self.user_pays(x_to_y, protocol_fee, true)?;
        }
        self.pool_pays(x_to_y, amount_out)
    }

    /// User sends input tokens to the vault, or (to_treasury) to the treasury.
    fn user_pays(&self, x_to_y: bool, amount: u64, to_treasury: bool) -> Result<()> {
        let (from, mint, to) = match (x_to_y, to_treasury) {
            (true, false) => (&self.user_x, &self.mint_x, &self.vault_x),
            (true, true) => (&self.user_x, &self.mint_x, &self.treasury_x),
            (false, false) => (&self.user_y, &self.mint_y, &self.vault_y),
            (false, true) => (&self.user_y, &self.mint_y, &self.treasury_y),
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
            mint.decimals,
        )
    }

    /// Vault sends the output token to the user, signed by the config PDA.
    fn pool_pays(&self, x_to_y: bool, amount: u64) -> Result<()> {
        let (from, mint, to) = if x_to_y {
            (&self.vault_y, &self.mint_y, &self.user_y)
        } else {
            (&self.vault_x, &self.mint_x, &self.user_x)
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
