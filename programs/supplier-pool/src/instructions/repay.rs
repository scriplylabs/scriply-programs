use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::{constants::*, error::SupplierPoolError, events::ListingRepaid, state::*, tokens};

#[derive(Accounts)]
pub struct Repay<'info> {
    #[account(mut)]
    pub anchor: Signer<'info>,
    #[account(
        mut,
        seeds = [POOL_SEED, pool.creator.as_ref(), pool.mint.as_ref()],
        bump = pool.bump,
        has_one = mint,
    )]
    pub pool: Box<Account<'info, Pool>>,
    #[account(
        mut,
        seeds = [ANCHOR_SEED, pool.key().as_ref(), anchor.key().as_ref()],
        bump = anchor_account.bump,
    )]
    pub anchor_account: Box<Account<'info, AnchorAccount>>,
    #[account(
        mut,
        close = anchor,
        seeds = [LISTING_SEED, pool.key().as_ref(), anchor.key().as_ref(), &listing.commitment],
        bump = listing.bump,
        has_one = pool,
        has_one = anchor,
    )]
    pub listing: Box<Account<'info, Listing>>,
    pub mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = pool,
        associated_token::token_program = token_program,
    )]
    pub vault: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, token::mint = mint, token::authority = anchor)]
    pub anchor_tokens: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(address = anchor_spl::token::ID)]
    pub token_program: Interface<'info, TokenInterface>,
}

/// Pays a claimed listing's whole amount into the vault and closes it,
/// returning its rent and what is left of its deposit to the anchor.
pub fn handle_repay(ctx: Context<Repay>) -> Result<()> {
    let listing = &ctx.accounts.listing;
    require!(listing.claimed, SupplierPoolError::NotClaimed);
    let amount = listing.amount;
    let a = &ctx.accounts;
    tokens::transfer(
        &a.token_program,
        a.anchor_tokens.to_account_info(),
        &a.mint,
        a.vault.to_account_info(),
        a.anchor.to_account_info(),
        &[],
        amount,
    )?;
    ctx.accounts.anchor_account.owed -= amount;
    let pool = &mut ctx.accounts.pool;
    pool.outstanding -= amount;
    pool.idle = pool
        .idle
        .checked_add(amount)
        .ok_or(SupplierPoolError::MathOverflow)?;
    emit!(ListingRepaid {
        pool: pool.key(),
        listing: ctx.accounts.listing.key(),
        amount,
    });
    Ok(())
}
