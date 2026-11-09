use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::{constants::*, error::PoolError, events::UnitsWithdrawn, state::*, tokens};

#[derive(Accounts)]
pub struct WithdrawUnits<'info> {
    pub borrower: Signer<'info>,
    pub pool: AccountLoader<'info, Pool>,
    #[account(
        mut,
        seeds = [
            LOAN_SEED,
            pool.key().as_ref(),
            collateral_mint.key().as_ref(),
            borrower.key().as_ref(),
        ],
        bump = loan.bump,
        has_one = pool,
        has_one = collateral_mint,
        has_one = borrower,
        constraint = !loan.is_open @ PoolError::LoanOpen,
    )]
    pub loan: Box<Account<'info, Loan>>,
    #[account(
        mut,
        associated_token::mint = collateral_mint,
        associated_token::authority = loan,
        associated_token::token_program = collateral_token_program,
    )]
    pub collateral_vault: Box<InterfaceAccount<'info, TokenAccount>>,
    /// Any of the borrower's accounts for the token, thawed.
    #[account(
        mut,
        token::mint = collateral_mint,
        token::authority = borrower,
        token::token_program = collateral_token_program,
    )]
    pub borrower_collateral: Box<InterfaceAccount<'info, TokenAccount>>,
    pub collateral_mint: Box<InterfaceAccount<'info, Mint>>,
    pub collateral_token_program: Interface<'info, TokenInterface>,
}

/// Moves the units left in a closed loan's vault to the borrower's account.
/// The borrower may call it any time after the loan closes; it fails while
/// the loan is open, or while the account it names is frozen.
pub fn handle_withdraw_units(ctx: Context<WithdrawUnits>) -> Result<()> {
    let a = &mut *ctx.accounts;
    let units = a.collateral_vault.amount;
    let loan = &a.loan;
    let seeds: &[&[u8]] = &[
        LOAN_SEED,
        loan.pool.as_ref(),
        loan.collateral_mint.as_ref(),
        loan.borrower.as_ref(),
        &[loan.bump],
    ];
    tokens::transfer(
        &a.collateral_token_program,
        a.collateral_vault.to_account_info(),
        &a.collateral_mint,
        a.borrower_collateral.to_account_info(),
        a.loan.to_account_info(),
        &[seeds],
        units,
    )?;
    a.loan.units = 0;
    emit!(UnitsWithdrawn {
        pool: a.loan.pool,
        collateral_mint: a.loan.collateral_mint,
        borrower: a.loan.borrower,
        units,
    });
    Ok(())
}
