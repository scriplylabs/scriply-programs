//! A lending pool with no admin that lends USDC against receivables tokens.
//!
//! Anyone creates a pool with fixed parameters, including whose posted
//! roots of tokens' facts it trusts and its risk engine's weights; nothing
//! changes them afterwards. Depositors buy shares at the pool's NAV and
//! withdraw at an epoch's start. A borrower pledges a token's units with the
//! token's snapshot proven against a root, and the risk engine decides how
//! much to advance and at what rate. Every epoch, keepers mark each loan
//! from its token's latest facts before withdrawals are paid.

pub mod attestation;
pub mod constants;
pub mod error;
pub mod events;
pub mod instructions;
pub mod loan;
pub mod math;
pub mod pool;
pub mod risk_engine;
pub mod snapshot;
pub mod state;
pub mod tokens;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("A5yYgE96b4cPU98zAwXhZtcbG6pyjuRUemodPyqQpQFD");

#[program]
pub mod receivables_pool {
    use super::*;

    /// Creates a pool with `params`, which nothing changes afterwards.
    pub fn create_pool(ctx: Context<CreatePool>, seed: u64, params: PoolParams) -> Result<()> {
        instructions::create_pool::handle_create_pool(ctx, seed, params)
    }

    /// Moves the current epoch's settlement on as far as it can go: at a
    /// new epoch, impairs loans past maturity and begins marking; once
    /// every loan is marked, pays withdrawals. Otherwise does nothing, so
    /// clients send it first.
    pub fn settle_epoch(ctx: Context<SettleEpoch>) -> Result<()> {
        instructions::settle_epoch::handle_settle_epoch(ctx)
    }

    /// Marks the loans passed as remaining accounts from their tokens'
    /// snapshots, proven against one of the pool's signers' roots.
    pub fn mark_loans<'info>(
        ctx: Context<'info, MarkLoans<'info>>,
        snapshots: Vec<snapshot::ProvenSnapshot>,
    ) -> Result<()> {
        instructions::mark_loans::handle_mark_loans(ctx, snapshots)
    }

    /// Marks stale the loans passed as remaining accounts that weren't
    /// marked this epoch, once the marking window has closed.
    pub fn mark_stale<'info>(ctx: Context<'info, MarkStale<'info>>) -> Result<()> {
        instructions::mark_stale::handle_mark_stale(ctx)
    }

    /// Deposits USDC for shares at the current NAV per share.
    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        instructions::deposit::handle_deposit(ctx, amount)
    }

    /// Asks to withdraw shares, paid at the next epoch's start.
    pub fn request_withdrawal(ctx: Context<RequestWithdrawal>, shares: u64) -> Result<()> {
        instructions::request_withdrawal::handle_request_withdrawal(ctx, shares)
    }

    /// Pays what epoch starts have set aside for a withdrawal.
    pub fn claim_withdrawal(ctx: Context<ClaimWithdrawal>) -> Result<()> {
        instructions::claim_withdrawal::handle_claim_withdrawal(ctx)
    }

    /// Pledges a token's units and borrows USDC against them, decided by
    /// the risk engine from the token's snapshot proven against a root.
    pub fn borrow(
        ctx: Context<Borrow>,
        units: u64,
        principal: u64,
        maturity_epochs: u16,
        proven: snapshot::ProvenSnapshot,
    ) -> Result<()> {
        instructions::borrow::handle_borrow(ctx, units, principal, maturity_epochs, proven)
    }

    /// Pays towards a loan; once nothing is owed its units go back.
    pub fn repay(ctx: Context<Repay>, amount: u64) -> Result<()> {
        instructions::repay::handle_repay(ctx, amount)
    }

    /// Sets USDC paid for a loan's units against it, records the units its
    /// vault holds, and closes it if nothing is owed.
    pub fn sync_loan(ctx: Context<SyncLoan>) -> Result<()> {
        instructions::sync_loan::handle_sync_loan(ctx)
    }
}
