use anchor_lang::prelude::*;
use anchor_spl::token_interface::Mint;

use crate::{
    constants::*,
    error::PoolError,
    events::{LoanCollected, LoanPaid},
    instructions::sync_loan::*,
    state::*,
    tokens,
};

#[derive(Accounts)]
pub struct CollectFromBorrower<'info> {
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
    /// `borrower_quote` is the borrower's account the pool collects from:
    /// any of their USDC accounts that approves the pool's authority.
    pub loan_tokens: LoanTokens<'info>,
    pub quote_mint: Box<InterfaceAccount<'info, Mint>>,
    pub collateral_mint: Box<InterfaceAccount<'info, Mint>>,
}

/// Once the loan has matured, takes what it owes from the borrower's USDC
/// account, up to what the borrower approved the pool's authority for at
/// borrow and what the account holds, after setting any USDC already paid
/// for its units against it. Anyone may call it, a keeper above all. Once
/// nothing is owed the loan closes, and its units wait in its vault for the
/// borrower to withdraw. A borrower who revoked the approval or emptied the
/// account is refused, and the loan goes on accruing at its late rate.
pub fn handle_collect_from_borrower(ctx: Context<CollectFromBorrower>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let pool_key = ctx.accounts.pool.key();
    let loan_key = ctx.accounts.loan.key();
    let a = &mut *ctx.accounts;
    a.loan_tokens
        .check(pool_key, &a.loan, a.quote_mint.key(), loan_key)?;
    let mut pool = a.pool.load_mut()?;
    pool.require_epoch_open(now)?;
    require!(a.loan.is_open, PoolError::NotALoan);
    require!(now >= a.loan.maturity_time, PoolError::NotMatured);
    collect_proceeds(
        &mut pool,
        &mut a.loan,
        &mut a.exposure,
        &a.loan_tokens,
        &a.quote_mint,
        now,
    )?;
    record_units(&mut a.loan, &mut a.loan_tokens)?;

    // Proceeds may already have repaid it; then it only closes, as `repay`
    // would, and nothing is taken from the borrower.
    if a.loan.is_open && !a.loan.is_repaid() {
        a.loan_tokens.borrower_quote.reload()?;
        let source = &a.loan_tokens.borrower_quote;
        let approved = match source.delegate {
            anchor_lang::solana_program::program_option::COption::Some(d)
                if d == a.loan_tokens.pool_authority.key() =>
            {
                source.delegated_amount
            }
            _ => 0,
        };
        let pay = a.loan.owed(now)?.min(approved).min(source.amount);
        require!(pay > 0, PoolError::NothingToCollect);
        let payment = a.loan.apply_payment(&mut pool, pay, now)?;
        a.exposure.principal = a
            .exposure
            .principal
            .saturating_sub(payment.principal_repaid);
        let authority_bump = pool.authority_bump;
        tokens::transfer(
            &a.loan_tokens.quote_token_program,
            a.loan_tokens.borrower_quote.to_account_info(),
            &a.quote_mint,
            a.loan_tokens.quote_vault.to_account_info(),
            a.loan_tokens.pool_authority.to_account_info(),
            &[&[AUTHORITY_SEED, pool_key.as_ref(), &[authority_bump]]],
            payment.applied,
        )?;
        emit!(LoanPaid {
            pool: pool_key,
            collateral_mint: a.loan.collateral_mint,
            applied: payment.applied,
            excess: 0,
        });
        emit!(LoanCollected {
            pool: pool_key,
            collateral_mint: a.loan.collateral_mint,
            borrower: a.loan.borrower,
            amount: payment.applied,
        });
    }
    close_if_repaid(&mut pool, &mut a.loan)?;
    Ok(())
}
