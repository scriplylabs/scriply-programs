use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::{constants::*, error::PoolError, events::WithdrawalClaimed, math::*, state::*, tokens};

#[derive(Accounts)]
pub struct ClaimWithdrawal<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
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
    /// The record of the request's generation, needed once it has ended.
    #[account(
        seeds = [GENERATION_SEED, pool.key().as_ref(), &request.generation.to_le_bytes()],
        bump,
    )]
    pub generation_record: Option<Account<'info, WithdrawalGeneration>>,
    pub quote_token_program: Interface<'info, TokenInterface>,
}

/// Pays the owner what epoch starts have set aside for their request so
/// far. A request whose generation has ended is paid in full and closed.
pub fn handle_claim_withdrawal(ctx: Context<ClaimWithdrawal>) -> Result<()> {
    let pool_key = ctx.accounts.pool.key();
    let mut pool = ctx.accounts.pool.load_mut()?;
    let request = &mut ctx.accounts.request;
    let is_generation_over = request.generation < pool.withdrawal_generation;
    let paid_per_norm = if is_generation_over {
        let record = ctx
            .accounts
            .generation_record
            .as_ref()
            .ok_or_else(|| error!(PoolError::GenerationRecordRequired))?;
        require!(
            record.pool == pool_key && record.generation == request.generation,
            PoolError::WrongGenerationRecord
        );
        record.final_paid_per_norm
    } else {
        pool.paid_per_norm.get()
    };
    let amount = to_u64(mul_div(
        request.norm,
        sub(paid_per_norm, request.paid_per_norm_claimed)?,
        INDEX_SCALE,
    )?)?;
    request.paid_per_norm_claimed = paid_per_norm;
    // Claims round down, so what was set aside always covers them.
    pool.reserved = pool.reserved.saturating_sub(amount);
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
    if is_generation_over {
        ctx.accounts
            .request
            .close(ctx.accounts.owner.to_account_info())?;
    }

    emit!(WithdrawalClaimed {
        pool: pool_key,
        owner: ctx.accounts.owner.key(),
        amount
    });
    Ok(())
}
