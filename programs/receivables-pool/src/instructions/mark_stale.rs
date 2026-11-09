use anchor_lang::prelude::*;

use crate::{error::PoolError, events::LoanMarked, state::*};

#[derive(Accounts)]
pub struct MarkStale<'info> {
    #[account(mut)]
    pub pool: AccountLoader<'info, Pool>,
    // Remaining accounts: the pool's open loans to mark stale, writable.
}

/// Marks stale each loan passed as a remaining account that no one marked
/// from its token's facts this epoch, once the pool's marking window has
/// closed: NAV counts it at the pool's impaired share until a fresh mark or
/// a payment clears it. Anyone may call it, so an epoch's withdrawals never
/// wait on a token whose facts nobody can prove.
pub fn handle_mark_stale<'info>(ctx: Context<'info, MarkStale<'info>>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let pool_key = ctx.accounts.pool.key();
    let mut pool = ctx.accounts.pool.load_mut()?;
    pool.require_epoch_open(now)?;
    let window_closes = pool
        .epoch_start(pool.settled_epoch)
        .saturating_add(pool.mark_window_seconds as i64);
    require!(now >= window_closes, PoolError::MarkWindowOpen);

    for info in ctx.remaining_accounts.iter() {
        let mut loan = Account::<Loan>::try_from(info)?;
        require_keys_eq!(loan.pool, pool_key, PoolError::NotALoan);
        loan.mark_stale(&mut pool, now)?;
        loan.exit(&crate::ID)?;
        emit!(LoanMarked {
            pool: pool_key,
            loan: info.key(),
            is_written_down: false,
            is_stale: true,
        });
    }
    Ok(())
}
