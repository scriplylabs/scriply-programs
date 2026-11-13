use anchor_lang::prelude::*;

use crate::{constants::*, error::SupplierPoolError, events::ListingCancelled, state::*};

#[derive(Accounts)]
pub struct CancelListing<'info> {
    #[account(mut)]
    pub anchor: Signer<'info>,
    #[account(
        mut,
        seeds = [POOL_SEED, pool.creator.as_ref(), pool.mint.as_ref()],
        bump = pool.bump,
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
}

/// Closes an unclaimed listing, returning its rent and deposit to the anchor.
pub fn handle_cancel_listing(ctx: Context<CancelListing>) -> Result<()> {
    let listing = &ctx.accounts.listing;
    require!(!listing.claimed, SupplierPoolError::AlreadyClaimed);
    let amount = listing.amount;
    ctx.accounts.anchor_account.owed -= amount;
    ctx.accounts.pool.listed -= amount;
    emit!(ListingCancelled {
        pool: ctx.accounts.pool.key(),
        listing: listing.key(),
    });
    Ok(())
}
