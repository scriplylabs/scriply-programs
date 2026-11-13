use anchor_lang::prelude::*;

use crate::{constants::*, error::SupplierPoolError, events::ListingWrittenOff, state::*};

#[derive(Accounts)]
pub struct WriteOff<'info> {
    #[account(
        mut,
        seeds = [POOL_SEED, pool.creator.as_ref(), pool.mint.as_ref()],
        bump = pool.bump,
    )]
    pub pool: Box<Account<'info, Pool>>,
    #[account(
        mut,
        seeds = [LISTING_SEED, pool.key().as_ref(), listing.anchor.as_ref(), &listing.commitment],
        bump = listing.bump,
        has_one = pool,
    )]
    pub listing: Box<Account<'info, Listing>>,
}

/// Takes a claimed listing its anchor hasn't repaid out of NAV, once the
/// pool's days past its due date have passed. The anchor still owes it, and
/// a later repayment adds it back.
pub fn handle_write_off(ctx: Context<WriteOff>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let listing = &ctx.accounts.listing;
    require!(listing.claimed, SupplierPoolError::NotClaimed);
    require!(!listing.written_off, SupplierPoolError::AlreadyWrittenOff);
    let pool = &ctx.accounts.pool;
    let after = (pool.write_off_after_days as i64)
        .checked_mul(SECONDS_PER_DAY)
        .and_then(|s| listing.due_date.checked_add(s))
        .ok_or(SupplierPoolError::MathOverflow)?;
    require!(now >= after, SupplierPoolError::NotYetOverdue);

    let amount = listing.amount;
    let pool = &mut ctx.accounts.pool;
    pool.outstanding -= amount;
    pool.written_off = pool
        .written_off
        .checked_add(amount)
        .ok_or(SupplierPoolError::MathOverflow)?;
    ctx.accounts.listing.written_off = true;
    emit!(ListingWrittenOff {
        pool: pool.key(),
        listing: ctx.accounts.listing.key(),
        amount,
    });
    Ok(())
}
