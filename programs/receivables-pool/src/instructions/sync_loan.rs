use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::{
    constants::*,
    events::{LoanClosed, LoanPaid},
    state::*,
    tokens,
};

#[derive(Accounts)]
pub struct SyncLoan<'info> {
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

/// The token accounts a loan settles through, shared by `sync_loan` and
/// `repay`.
#[derive(Accounts)]
pub struct LoanTokens<'info> {
    /// CHECK: the PDA that owns the pool's USDC vault; checked against the
    /// vault's owner below.
    pub pool_authority: UncheckedAccount<'info>,
    #[account(mut, token::authority = pool_authority, token::token_program = quote_token_program)]
    pub quote_vault: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, token::token_program = collateral_token_program)]
    pub collateral_vault: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, token::token_program = quote_token_program)]
    pub loan_proceeds: Box<InterfaceAccount<'info, TokenAccount>>,
    /// The borrower's USDC account, for what is left once nothing is owed.
    #[account(mut, token::token_program = quote_token_program)]
    pub borrower_quote: Box<InterfaceAccount<'info, TokenAccount>>,
    /// The borrower's account for the token, for units returned.
    #[account(mut, token::token_program = collateral_token_program)]
    pub borrower_collateral: Box<InterfaceAccount<'info, TokenAccount>>,
    pub quote_token_program: Interface<'info, TokenInterface>,
    pub collateral_token_program: Interface<'info, TokenInterface>,
}

impl<'info> LoanTokens<'info> {
    /// Fails unless every account is the one this loan in this pool
    /// settles through. Composite accounts can't name their parent's
    /// fields in constraints, so the parent calls this.
    pub fn check(
        &self,
        pool: Pubkey,
        loan: &Loan,
        quote_mint: Pubkey,
        loan_key: Pubkey,
    ) -> Result<()> {
        let (authority, _) =
            Pubkey::find_program_address(&[AUTHORITY_SEED, pool.as_ref()], &crate::ID);
        require_keys_eq!(
            self.pool_authority.key(),
            authority,
            ErrorCode::ConstraintSeeds
        );
        require_keys_eq!(
            self.quote_vault.mint,
            quote_mint,
            ErrorCode::ConstraintTokenMint
        );
        require_keys_eq!(
            self.collateral_vault.owner,
            loan_key,
            ErrorCode::ConstraintTokenOwner
        );
        require_keys_eq!(
            self.collateral_vault.mint,
            loan.collateral_mint,
            ErrorCode::ConstraintTokenMint
        );
        require_keys_eq!(
            self.loan_proceeds.owner,
            loan_key,
            ErrorCode::ConstraintTokenOwner
        );
        require_keys_eq!(
            self.loan_proceeds.mint,
            quote_mint,
            ErrorCode::ConstraintTokenMint
        );
        require_keys_eq!(
            self.borrower_quote.owner,
            loan.borrower,
            ErrorCode::ConstraintTokenOwner
        );
        require_keys_eq!(
            self.borrower_quote.mint,
            quote_mint,
            ErrorCode::ConstraintTokenMint
        );
        require_keys_eq!(
            self.borrower_collateral.owner,
            loan.borrower,
            ErrorCode::ConstraintTokenOwner
        );
        require_keys_eq!(
            self.borrower_collateral.mint,
            loan.collateral_mint,
            ErrorCode::ConstraintTokenMint
        );
        Ok(())
    }
}

/// Sets USDC paid for a loan's units against it and records the units left
/// in its vault, then closes it if nothing is owed. Anyone may call it:
/// units leave the vault and USDC arrives without the pool acting (a
/// buy-back, the agent's redemption, a last-resort burn), and each is one
/// fixed rule.
pub fn handle_sync_loan(ctx: Context<SyncLoan>) -> Result<()> {
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
    close_if_repaid(&mut pool, &mut a.loan, &a.loan_tokens, &a.collateral_mint)?;
    Ok(())
}

/// Sets the USDC waiting in the loan's proceeds account against the loan:
/// what pays its interest and principal goes to the pool, the rest to the
/// borrower.
pub fn collect_proceeds<'info>(
    pool: &mut Pool,
    loan: &mut Account<'info, Loan>,
    exposure: &mut Account<'info, TokenExposure>,
    t: &LoanTokens<'info>,
    quote_mint: &InterfaceAccount<'info, Mint>,
    now: i64,
) -> Result<()> {
    let amount = t.loan_proceeds.amount;
    if amount == 0 {
        return Ok(());
    }
    let payment = loan.apply_payment(pool, amount, now)?;
    exposure.principal = exposure.principal.saturating_sub(payment.principal_repaid);
    let seeds: &[&[u8]] = &[
        LOAN_SEED,
        loan.pool.as_ref(),
        loan.collateral_mint.as_ref(),
        loan.borrower.as_ref(),
        &[loan.bump],
    ];
    tokens::transfer(
        &t.quote_token_program,
        t.loan_proceeds.to_account_info(),
        quote_mint,
        t.quote_vault.to_account_info(),
        loan.to_account_info(),
        &[seeds],
        payment.applied,
    )?;
    tokens::transfer(
        &t.quote_token_program,
        t.loan_proceeds.to_account_info(),
        quote_mint,
        t.borrower_quote.to_account_info(),
        loan.to_account_info(),
        &[seeds],
        payment.excess,
    )?;
    emit!(LoanPaid {
        pool: loan.pool,
        collateral_mint: loan.collateral_mint,
        applied: payment.applied,
        excess: payment.excess,
    });
    Ok(())
}

/// Records the units the vault holds now. The vault's balance is the truth:
/// units can leave it without the pool acting.
pub fn record_units<'info>(
    loan: &mut Account<'info, Loan>,
    t: &mut LoanTokens<'info>,
) -> Result<()> {
    t.collateral_vault.reload()?;
    loan.units = t.collateral_vault.amount;
    Ok(())
}

/// Closes the loan once nothing is owed, returning its remaining units to
/// the borrower.
pub fn close_if_repaid<'info>(
    pool: &mut Pool,
    loan: &mut Account<'info, Loan>,
    t: &LoanTokens<'info>,
    collateral_mint: &InterfaceAccount<'info, Mint>,
) -> Result<()> {
    if !loan.is_open || !loan.is_repaid() {
        return Ok(());
    }
    let seeds: &[&[u8]] = &[
        LOAN_SEED,
        loan.pool.as_ref(),
        loan.collateral_mint.as_ref(),
        loan.borrower.as_ref(),
        &[loan.bump],
    ];
    tokens::transfer(
        &t.collateral_token_program,
        t.collateral_vault.to_account_info(),
        collateral_mint,
        t.borrower_collateral.to_account_info(),
        loan.to_account_info(),
        &[seeds],
        loan.units,
    )?;
    loan.units = 0;
    loan.close_repaid(pool)?;
    emit!(LoanClosed {
        pool: loan.pool,
        collateral_mint: loan.collateral_mint
    });
    Ok(())
}
