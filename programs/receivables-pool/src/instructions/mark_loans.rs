use anchor_lang::prelude::*;
use snapshot_roots::SnapshotRoots;

use crate::{
    error::PoolError,
    events::LoanMarked,
    risk_engine::{self, MarkOutcome},
    snapshot::{read_token_facts, ProvenSnapshot, Snapshot, TokenFacts},
    state::*,
};

#[derive(Accounts)]
pub struct MarkLoans<'info> {
    #[account(mut)]
    pub pool: AccountLoader<'info, Pool>,
    /// The roots of one of the pool's signers, which every snapshot passed
    /// is checked against.
    pub roots: Box<Account<'info, SnapshotRoots>>,
    // Remaining accounts: the pool's open loans to mark, writable.
}

/// Marks each loan passed as a remaining account from its token's facts,
/// one of `snapshots` proven against a root in `roots`: the risk engine
/// decides whether the loan stays performing or is written down. Anyone
/// may call it; each token's facts are checked once however many of its
/// loans are passed. Once every open loan is marked in the epoch,
/// `settle_epoch` pays withdrawals.
pub fn handle_mark_loans<'info>(
    ctx: Context<'info, MarkLoans<'info>>,
    snapshots: Vec<ProvenSnapshot>,
) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let pool_key = ctx.accounts.pool.key();
    let mut pool = ctx.accounts.pool.load_mut()?;
    pool.require_epoch_open(now)?;

    let mut read: Vec<(Pubkey, TokenFacts)> = Vec::with_capacity(snapshots.len());
    for info in ctx.remaining_accounts.iter() {
        let mut loan = Account::<Loan>::try_from(info)?;
        require_keys_eq!(loan.pool, pool_key, PoolError::NotALoan);
        let mint = loan.collateral_mint;
        if !read.iter().any(|(m, _)| *m == mint) {
            let proven = snapshots
                .iter()
                .find(|p| Snapshot::parse(&p.snapshot).is_ok_and(|s| s.mint == mint.to_bytes()))
                .ok_or_else(|| error!(PoolError::WrongMint))?;
            let facts = read_token_facts(&pool, &ctx.accounts.roots, proven, mint, now)?;
            read.push((mint, facts));
        }
        let (_, facts) = read
            .iter()
            .find(|(m, _)| *m == mint)
            .ok_or_else(|| error!(PoolError::WrongMint))?;
        let outcome = match pool.risk_model() {
            RiskModel::V1 => risk_engine::assess_mark(&facts.snapshot),
        };
        let is_written_down = matches!(outcome, MarkOutcome::WrittenDown);
        loan.mark(&mut pool, facts.root_as_of, is_written_down, now)?;
        loan.exit(&crate::ID)?;
        emit!(LoanMarked {
            pool: pool_key,
            loan: info.key(),
            is_written_down,
            is_stale: false,
        });
    }
    Ok(())
}
