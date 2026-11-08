use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount};

use crate::{constants::*, events::LoanPaid, instructions::sync_loan::*, state::*, tokens};

#[derive(Accounts)]
pub struct Repay<'info> {
    /// Anyone may repay a loan; its units go back to its borrower.
    pub payer: Signer<'info>,
    /// The borrower repaying their own loan pays from the account that
    /// `loan_tokens.borrower_quote` also names, hence `dup`. Both change
    /// only through the token program, and neither is written back.
    #[account(mut, dup, token::mint = quote_mint, token::authority = payer)]
    pub payer_quote: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, has_one = quote_mint)]
    pub pool: AccountLoader<'info, Pool>,
    #[account(
        mut,
        seeds = [
            LOAN_SEED,
            pool.key().as_ref(),
            collateral_mint.key().as_ref(),
            loan.borrower.as_ref(),
        ],
        bump = loan.bump,
        has_one = pool,
        has_one = collateral_mint,
    )]
    pub loan: Box<Account<'info, Loan>>,
    #[account(
        mut,
        seeds = [EXPOSURE_SEED, pool.key().as_ref(), collateral_mint.key().as_ref()],
        bump = exposure.bump,
    )]
    pub exposure: Box<Account<'info, TokenExposure>>,
    pub loan_tokens: LoanTokens<'info>,
    pub quote_mint: Box<InterfaceAccount<'info, Mint>>,
    pub collateral_mint: Box<InterfaceAccount<'info, Mint>>,
}

/// Pays up to `amount` USDC towards the loan, never more than it owes, after
/// setting any USDC already paid for its units against it. Once nothing is
/// owed the loan closes and its units go back to the borrower.
pub fn handle_repay(ctx: Context<Repay>, amount: u64) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let pool_key = ctx.accounts.pool.key();
    let loan_key = ctx.accounts.loan.key();
    let a = &mut *ctx.accounts;
    a.loan_tokens
        .check(pool_key, &a.loan, a.quote_mint.key(), loan_key)?;
    let mut pool = a.pool.load_mut()?;
    pool.require_epoch_open(now)?;
    collect_proceeds(
        &mut pool,
        &mut a.loan,
        &mut a.exposure,
        &a.loan_tokens,
        &a.quote_mint,
        now,
    )?;
    record_units(&mut a.loan, &mut a.loan_tokens)?;

    if a.loan.is_open {
        let pay = amount.min(a.loan.owed(now)?);
        let payment = a.loan.apply_payment(&mut pool, pay, now)?;
        a.exposure.principal = a
            .exposure
            .principal
            .saturating_sub(payment.principal_repaid);
        tokens::transfer(
            &a.loan_tokens.quote_token_program,
            a.payer_quote.to_account_info(),
            &a.quote_mint,
            a.loan_tokens.quote_vault.to_account_info(),
            a.payer.to_account_info(),
            &[],
            payment.applied,
        )?;
        emit!(LoanPaid {
            pool: pool_key,
            collateral_mint: a.loan.collateral_mint,
            applied: payment.applied,
            excess: 0,
        });
    }
    close_if_repaid(&mut pool, &mut a.loan, &a.loan_tokens, &a.collateral_mint)?;
    Ok(())
}
