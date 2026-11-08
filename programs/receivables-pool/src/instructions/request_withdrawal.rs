use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::{constants::*, error::PoolError, events::WithdrawalRequested, state::*, tokens};

#[derive(Accounts)]
pub struct RequestWithdrawal<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(mut, has_one = share_mint)]
    pub pool: AccountLoader<'info, Pool>,
    /// CHECK: the PDA that owns the share escrow.
    #[account(seeds = [AUTHORITY_SEED, pool.key().as_ref()], bump)]
    pub pool_authority: UncheckedAccount<'info>,
    pub share_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(mut, token::mint = share_mint, token::authority = owner)]
    pub owner_shares: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        associated_token::mint = share_mint,
        associated_token::authority = pool_authority,
        associated_token::token_program = share_token_program,
    )]
    pub share_escrow: Box<InterfaceAccount<'info, TokenAccount>>,
    /// One request per wallet: a second fails until the first is paid in
    /// full and closed.
    #[account(
        init,
        payer = owner,
        space = 8 + WithdrawalRequest::INIT_SPACE,
        seeds = [WITHDRAWAL_SEED, pool.key().as_ref(), owner.key().as_ref()],
        bump
    )]
    pub request: Account<'info, WithdrawalRequest>,
    #[account(address = anchor_spl::token::ID)]
    pub share_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

/// Asks to withdraw `shares`. They move to escrow and are paid at the next
/// epoch's start, at that start's NAV per share; until paid they keep
/// sharing the pool's interest and losses.
pub fn handle_request_withdrawal(ctx: Context<RequestWithdrawal>, shares: u64) -> Result<()> {
    require!(shares > 0, PoolError::ZeroAmount);
    let now = Clock::get()?.unix_timestamp;
    let pool_key = ctx.accounts.pool.key();
    let owner = ctx.accounts.owner.key();
    let mut pool = ctx.accounts.pool.load_mut()?;
    pool.require_settled(now)?;
    let norm = pool.queue_withdrawal(shares)?;
    let request = &mut ctx.accounts.request;
    request.version = ACCOUNT_VERSION;
    request.pool = pool_key;
    request.owner = owner;
    request.bump = ctx.bumps.request;
    request.generation = pool.withdrawal_generation;
    request.norm = norm;
    request.paid_per_norm_claimed = pool.paid_per_norm.get();
    drop(pool);

    let a = &ctx.accounts;
    tokens::transfer(
        &a.share_token_program,
        a.owner_shares.to_account_info(),
        &a.share_mint,
        a.share_escrow.to_account_info(),
        a.owner.to_account_info(),
        &[],
        shares,
    )?;

    emit!(WithdrawalRequested {
        pool: pool_key,
        owner,
        shares
    });
    Ok(())
}
