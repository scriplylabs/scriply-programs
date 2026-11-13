use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{Mint, TokenAccount, TokenInterface},
};

use crate::{constants::*, error::SupplierPoolError, events::Deposited, state::*, tokens};

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub depositor: Signer<'info>,
    #[account(
        mut,
        seeds = [POOL_SEED, pool.creator.as_ref(), pool.mint.as_ref()],
        bump = pool.bump,
        has_one = mint,
        has_one = share_mint,
    )]
    pub pool: Box<Account<'info, Pool>>,
    pub mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(mut)]
    pub share_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = pool,
        associated_token::token_program = token_program,
    )]
    pub vault: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, token::mint = mint, token::authority = depositor)]
    pub depositor_tokens: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        init_if_needed,
        payer = depositor,
        associated_token::mint = share_mint,
        associated_token::authority = depositor,
        associated_token::token_program = token_program,
    )]
    pub depositor_shares: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(address = anchor_spl::token::ID)]
    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

/// Deposits `amount` of the pool's token for shares at its NAV per share.
pub fn handle_deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
    require!(amount > 0, SupplierPoolError::ZeroAmount);
    let shares = ctx
        .accounts
        .pool
        .shares_for_deposit(amount, ctx.accounts.share_mint.supply)?;
    require!(shares > 0, SupplierPoolError::DepositTooSmall);

    let a = &ctx.accounts;
    tokens::transfer(
        &a.token_program,
        a.depositor_tokens.to_account_info(),
        &a.mint,
        a.vault.to_account_info(),
        a.depositor.to_account_info(),
        &[],
        amount,
    )?;
    let (creator, mint, bump) = (a.pool.creator, a.pool.mint, a.pool.bump);
    tokens::mint_to(
        &a.token_program,
        &a.share_mint,
        a.depositor_shares.to_account_info(),
        a.pool.to_account_info(),
        &[&[POOL_SEED, creator.as_ref(), mint.as_ref(), &[bump]]],
        shares,
    )?;

    let pool = &mut ctx.accounts.pool;
    pool.idle = pool
        .idle
        .checked_add(amount)
        .ok_or(SupplierPoolError::MathOverflow)?;
    emit!(Deposited {
        pool: pool.key(),
        depositor: ctx.accounts.depositor.key(),
        amount,
        shares,
    });
    Ok(())
}
