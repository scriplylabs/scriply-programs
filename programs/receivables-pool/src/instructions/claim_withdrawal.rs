use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::{constants::*, error::PoolError, events::WithdrawalClaimed, state::*, tokens};

#[derive(Accounts)]
pub struct ClaimWithdrawal<'info> {
    /// CHECK: the request's owner, who is paid and gets the request's rent
    /// back when it closes; checked by the request's `has_one`. Anyone may
    /// claim for them, so a keeper can queue unpaid shares again.
    #[account(mut)]
    pub owner: UncheckedAccount<'info>,
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
    #[account(mut, token::mint = quote_mint, token::authority = owner)]
    pub owner_quote: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        seeds = [WITHDRAWAL_SEED, pool.key().as_ref(), owner.key().as_ref()],
        bump = request.bump,
        has_one = owner,
        has_one = pool,
    )]
    pub request: Account<'info, WithdrawalRequest>,
    /// The record of the request's generation, which exists once an
    /// epoch's start has settled it.
    #[account(
        seeds = [GENERATION_SEED, pool.key().as_ref(), &request.generation.to_le_bytes()],
        bump,
        constraint = generation_record.pool == pool.key() @ PoolError::WrongGenerationRecord,
    )]
    pub generation_record: Account<'info, WithdrawalGeneration>,
    pub quote_token_program: Interface<'info, TokenInterface>,
}

/// Pays the owner what the epoch start that settled their request set aside
/// for it. Shares it left unpaid are queued again, for the next epoch's
/// start; a request paid in full is closed.
pub fn handle_claim_withdrawal(ctx: Context<ClaimWithdrawal>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let pool_key = ctx.accounts.pool.key();
    let mut pool = ctx.accounts.pool.load_mut()?;
    let request = &mut ctx.accounts.request;
    let (amount, unpaid) = ctx.accounts.generation_record.owed_to(request.shares)?;
    // Claims round down, so what was set aside always covers them.
    pool.reserved = pool.reserved.saturating_sub(amount);
    if unpaid > 0 {
        // Queued like a new request, which waits for the epoch's start to
        // be settled.
        pool.require_settled(now)?;
        pool.queue_withdrawal(unpaid)?;
        request.generation = pool.withdrawal_generation;
        request.shares = unpaid;
    }
    let authority_bump = pool.authority_bump;
    drop(pool);

    let a = &ctx.accounts;
    tokens::transfer(
        &a.quote_token_program,
        a.quote_vault.to_account_info(),
        &a.quote_mint,
        a.owner_quote.to_account_info(),
        a.pool_authority.to_account_info(),
        &[&[AUTHORITY_SEED, pool_key.as_ref(), &[authority_bump]]],
        amount,
    )?;
    if unpaid == 0 {
        ctx.accounts
            .request
            .close(ctx.accounts.owner.to_account_info())?;
    }

    emit!(WithdrawalClaimed {
        pool: pool_key,
        owner: ctx.accounts.owner.key(),
        amount,
        shares_requeued: unpaid,
    });
    Ok(())
}
