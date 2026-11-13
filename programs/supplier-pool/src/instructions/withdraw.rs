use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::{constants::*, error::SupplierPoolError, events::Withdrawn, state::*, tokens};

#[derive(Accounts)]
pub struct Withdraw<'info> {
    pub owner: Signer<'info>,
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
    #[account(mut, token::mint = share_mint, token::authority = owner)]
    pub owner_shares: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, token::mint = mint)]
    pub owner_tokens: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(address = anchor_spl::token::ID)]
    pub token_program: Interface<'info, TokenInterface>,
}

/// Burns `shares` for what they are worth at the pool's NAV, paid from what
/// it holds idle; refused if idle can't pay it all.
pub fn handle_withdraw(ctx: Context<Withdraw>, shares: u64) -> Result<()> {
    require!(shares > 0, SupplierPoolError::ZeroAmount);
    let amount = ctx
        .accounts
        .pool
        .amount_for_shares(shares, ctx.accounts.share_mint.supply)?;
    require!(
        amount <= ctx.accounts.pool.idle,
        SupplierPoolError::NotEnoughIdle
    );

    let a = &ctx.accounts;
    tokens::burn(
        &a.token_program,
        &a.share_mint,
        a.owner_shares.to_account_info(),
        a.owner.to_account_info(),
        shares,
    )?;
    let (creator, mint, bump) = (a.pool.creator, a.pool.mint, a.pool.bump);
    tokens::transfer(
        &a.token_program,
        a.vault.to_account_info(),
        &a.mint,
        a.owner_tokens.to_account_info(),
        a.pool.to_account_info(),
        &[&[POOL_SEED, creator.as_ref(), mint.as_ref(), &[bump]]],
        amount,
    )?;

    let pool = &mut ctx.accounts.pool;
    pool.idle -= amount;
    emit!(Withdrawn {
        pool: pool.key(),
        owner: ctx.accounts.owner.key(),
        shares,
        amount,
    });
    Ok(())
}
