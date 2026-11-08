use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{Mint, TokenAccount, TokenInterface},
};

use crate::{
    attestation::require_attestation, constants::*, error::PoolError, events::Deposited, state::*,
    tokens,
};

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub depositor: Signer<'info>,
    #[account(mut, has_one = quote_mint, has_one = share_mint)]
    pub pool: AccountLoader<'info, Pool>,
    /// CHECK: the PDA that signs for the pool's token accounts.
    #[account(seeds = [AUTHORITY_SEED, pool.key().as_ref()], bump)]
    pub pool_authority: UncheckedAccount<'info>,
    pub quote_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(mut)]
    pub share_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        associated_token::mint = quote_mint,
        associated_token::authority = pool_authority,
        associated_token::token_program = quote_token_program,
    )]
    pub quote_vault: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, token::mint = quote_mint, token::authority = depositor)]
    pub depositor_quote: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        init_if_needed,
        payer = depositor,
        associated_token::mint = share_mint,
        associated_token::authority = depositor,
        associated_token::token_program = share_token_program,
    )]
    pub depositor_shares: Box<InterfaceAccount<'info, TokenAccount>>,
    /// CHECK: the depositor's SAS attestation, checked in the handler when
    /// the pool requires one.
    pub attestation: Option<UncheckedAccount<'info>>,
    pub quote_token_program: Interface<'info, TokenInterface>,
    #[account(address = anchor_spl::token::ID)]
    pub share_token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

/// Deposits `amount` of USDC for shares at the current NAV per share.
pub fn handle_deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
    require!(amount > 0, PoolError::ZeroAmount);
    let now = Clock::get()?.unix_timestamp;
    let pool_key = ctx.accounts.pool.key();
    let depositor = ctx.accounts.depositor.key();
    let mut pool = ctx.accounts.pool.load_mut()?;
    pool.require_settled(now)?;
    if pool.requires_attestation == 1 {
        let attestation = ctx
            .accounts
            .attestation
            .as_ref()
            .ok_or_else(|| error!(PoolError::AttestationRequired))?;
        require_attestation(
            attestation,
            &pool.attestation_credential,
            &pool.attestation_schema,
            &depositor,
            now,
        )?;
    }

    let shares = pool.shares_for_deposit(amount, ctx.accounts.share_mint.supply, now)?;
    require!(shares > 0, PoolError::DepositTooSmall);
    pool.idle = pool
        .idle
        .checked_add(amount)
        .ok_or(PoolError::MathOverflow)?;
    let authority_bump = pool.authority_bump;
    drop(pool);

    let a = &ctx.accounts;
    tokens::transfer(
        &a.quote_token_program,
        a.depositor_quote.to_account_info(),
        &a.quote_mint,
        a.quote_vault.to_account_info(),
        a.depositor.to_account_info(),
        &[],
        amount,
    )?;
    tokens::mint_to(
        &a.share_token_program,
        &a.share_mint,
        a.depositor_shares.to_account_info(),
        a.pool_authority.to_account_info(),
        &[&[AUTHORITY_SEED, pool_key.as_ref(), &[authority_bump]]],
        shares,
    )?;

    emit!(Deposited {
        pool: pool_key,
        depositor,
        amount,
        shares
    });
    Ok(())
}
