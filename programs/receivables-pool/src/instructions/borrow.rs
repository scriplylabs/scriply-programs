use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{Mint, TokenAccount, TokenInterface},
};

use snapshot_roots::SnapshotRoots;

use crate::{
    constants::*,
    error::PoolError,
    events::LoanOpened,
    loan::Opening,
    math::*,
    risk_engine,
    snapshot::{read_token_facts, ProvenSnapshot},
    state::*,
    tokens,
};

#[derive(Accounts)]
pub struct Borrow<'info> {
    #[account(mut)]
    pub borrower: Signer<'info>,
    #[account(mut, has_one = quote_mint)]
    pub pool: AccountLoader<'info, Pool>,
    /// CHECK: the PDA that signs for the pool's token accounts.
    #[account(seeds = [AUTHORITY_SEED, pool.key().as_ref()], bump)]
    pub pool_authority: UncheckedAccount<'info>,
    pub quote_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        associated_token::mint = quote_mint,
        associated_token::authority = pool_authority,
        associated_token::token_program = quote_token_program,
    )]
    pub quote_vault: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, token::mint = quote_mint, token::authority = borrower)]
    pub borrower_quote: Box<InterfaceAccount<'info, TokenAccount>>,
    /// The receivables token. The program never trusts the mint itself:
    /// it lends only against the mint a snapshot under its signers' root
    /// names.
    #[account(mint::token_program = collateral_token_program)]
    pub collateral_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        token::mint = collateral_mint,
        token::authority = borrower,
        token::token_program = collateral_token_program,
    )]
    pub borrower_collateral: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        init_if_needed,
        payer = borrower,
        space = 8 + Loan::INIT_SPACE,
        seeds = [
            LOAN_SEED,
            pool.key().as_ref(),
            collateral_mint.key().as_ref(),
            borrower.key().as_ref(),
        ],
        bump
    )]
    pub loan: Box<Account<'info, Loan>>,
    /// What the pool has lent against this token, across borrowers.
    #[account(
        init_if_needed,
        payer = borrower,
        space = 8 + TokenExposure::INIT_SPACE,
        seeds = [EXPOSURE_SEED, pool.key().as_ref(), collateral_mint.key().as_ref()],
        bump
    )]
    pub exposure: Box<Account<'info, TokenExposure>>,
    /// Holds the loan's units. The token's accounts start frozen, so the
    /// token's issuer must thaw this one before the first loan against it.
    #[account(
        init_if_needed,
        payer = borrower,
        associated_token::mint = collateral_mint,
        associated_token::authority = loan,
        associated_token::token_program = collateral_token_program,
    )]
    pub collateral_vault: Box<InterfaceAccount<'info, TokenAccount>>,
    /// Where USDC paid for the loan's units arrives (a buy-back, the
    /// agent's redemption), for `sync_loan` to set against the loan.
    #[account(
        init_if_needed,
        payer = borrower,
        associated_token::mint = quote_mint,
        associated_token::authority = loan,
        associated_token::token_program = quote_token_program,
    )]
    pub loan_proceeds: Box<InterfaceAccount<'info, TokenAccount>>,
    /// The roots of one of the pool's signers, which `proven` is checked
    /// against.
    pub roots: Box<Account<'info, SnapshotRoots>>,
    pub quote_token_program: Interface<'info, TokenInterface>,
    pub collateral_token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

/// Pledges `units` of a token and borrows `principal` USDC for
/// `maturity_epochs`, decided by the risk engine from the token's snapshot
/// proven against one of the pool's signers' roots.
pub fn handle_borrow(
    ctx: Context<Borrow>,
    units: u64,
    principal: u64,
    maturity_epochs: u16,
    proven: ProvenSnapshot,
) -> Result<()> {
    require!(units > 0 && principal > 0, PoolError::ZeroAmount);
    let now = Clock::get()?.unix_timestamp;
    let pool_key = ctx.accounts.pool.key();
    let mint = &ctx.accounts.collateral_mint;
    let mut pool = ctx.accounts.pool.load_mut()?;
    pool.require_settled(now)?;

    let facts = read_token_facts(&pool, &ctx.accounts.roots, &proven, mint.key(), now)?;
    // Burns since the snapshot only shrink what its facts describe; units
    // minted since are outside them.
    require!(
        mint.supply <= facts.snapshot.network_supply,
        PoolError::MintedSinceSnapshot
    );
    require!(
        pool.maturities().contains(&maturity_epochs),
        PoolError::MaturityNotOffered
    );
    let value = face_value(units, mint.decimals, ctx.accounts.quote_mint.decimals)?;
    let terms = match pool.risk_model() {
        RiskModel::V1 => risk_engine::assess_borrow(
            &pool.risk,
            &facts.snapshot,
            value,
            maturity_epochs as u64 * pool.epoch_seconds,
        )?,
    };
    require!(principal <= terms.max_principal, PoolError::ExceedsAdvance);
    let exposure = add(ctx.accounts.exposure.principal as u128, principal as u128)?;
    let nav = pool.nav(now)?;
    require!(
        exposure <= mul_div(nav as u128, pool.cap_per_token_bps as u128, BPS as u128)?,
        PoolError::ExceedsTokenCap
    );
    require!(principal <= pool.idle, PoolError::InsufficientLiquidity);

    let exposure_account = &mut ctx.accounts.exposure;
    if exposure_account.pool == Pubkey::default() {
        exposure_account.version = ACCOUNT_VERSION;
        exposure_account.pool = pool_key;
        exposure_account.collateral_mint = mint.key();
        exposure_account.bump = ctx.bumps.exposure;
    }
    exposure_account.principal = to_u64(exposure)?;

    let loan = &mut ctx.accounts.loan;
    if loan.pool == Pubkey::default() {
        loan.version = ACCOUNT_VERSION;
        loan.pool = pool_key;
        loan.collateral_mint = mint.key();
        loan.borrower = ctx.accounts.borrower.key();
        loan.bump = ctx.bumps.loan;
    }
    let maturity_epoch = pool.settled_epoch + maturity_epochs as u64;
    let rate_bps = match RateMode::try_from(pool.rate_mode)? {
        RateMode::Fixed => terms.rate_bps,
        RateMode::Variable => return err!(PoolError::RateModeNotSupported),
    };
    loan.open(
        &mut pool,
        Opening {
            principal,
            rate_bps,
            maturity_epoch,
            root_as_of: facts.root_as_of,
        },
        now,
    )?;
    let authority_bump = pool.authority_bump;
    drop(pool);

    let a = &ctx.accounts;
    // Units left from the borrower's last loan would be pledged unpriced.
    require!(a.collateral_vault.amount == 0, PoolError::UnitsNotWithdrawn);
    tokens::transfer(
        &a.collateral_token_program,
        a.borrower_collateral.to_account_info(),
        &a.collateral_mint,
        a.collateral_vault.to_account_info(),
        a.borrower.to_account_info(),
        &[],
        units,
    )?;
    tokens::transfer(
        &a.quote_token_program,
        a.quote_vault.to_account_info(),
        &a.quote_mint,
        a.borrower_quote.to_account_info(),
        a.pool_authority.to_account_info(),
        &[&[AUTHORITY_SEED, pool_key.as_ref(), &[authority_bump]]],
        principal,
    )?;
    ctx.accounts.collateral_vault.reload()?;
    ctx.accounts.loan.units = ctx.accounts.collateral_vault.amount;

    emit!(LoanOpened {
        pool: pool_key,
        collateral_mint: ctx.accounts.collateral_mint.key(),
        borrower: ctx.accounts.borrower.key(),
        units,
        principal,
        rate_bps,
        maturity_epoch,
    });
    Ok(())
}
