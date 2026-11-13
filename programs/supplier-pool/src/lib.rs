//! A pool with no admin that pays an anchor's suppliers early.
//!
//! Anyone creates a pool with fixed parameters: its token, its fee, whose
//! posted roots it reads bills' facts from, which anchors it takes and its
//! rules over each bill's facts. Depositors buy shares at its NAV. An
//! anchor lists a bill with its bill leaf proven against a signer's latest
//! root; the supplier claims with a signature from the address the listing
//! commits to and is paid the amount less the fee; the anchor repays the
//! whole amount by the bill's repayment date. A listing left unrepaid past
//! the pool's grace is written off, out of NAV, by anyone.

pub mod claim_signature;
pub mod constants;
pub mod error;
pub mod events;
pub mod facts;
pub mod instructions;
pub mod state;
pub mod tokens;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("tpn1V8kAmBpxBdwTjUJF4Yh7Eui4NWX4NHiKjcrJW81");

#[program]
pub mod supplier_pool {
    use super::*;

    /// Creates a pool for `mint` with `params`, which nothing changes
    /// afterwards.
    pub fn create_pool(ctx: Context<CreatePool>, params: PoolParams) -> Result<()> {
        instructions::create_pool::handle_create_pool(ctx, params)
    }

    /// Deposits the pool's token for shares at its NAV per share.
    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        instructions::deposit::handle_deposit(ctx, amount)
    }

    /// Burns shares for what they are worth, paid from what the pool
    /// holds idle.
    pub fn withdraw(ctx: Context<Withdraw>, shares: u64) -> Result<()> {
        instructions::withdraw::handle_withdraw(ctx, shares)
    }

    /// Lists the bill whose leaf, proven against a signer's latest root,
    /// is for `commitment`, once the pool's rules pass.
    pub fn list_bill(
        ctx: Context<ListBill>,
        commitment: [u8; 32],
        proven: facts::ProvenLeaf,
    ) -> Result<()> {
        instructions::list_bill::handle_list_bill(ctx, commitment, proven)
    }

    /// Closes an unclaimed listing to its anchor.
    pub fn cancel_listing(ctx: Context<CancelListing>) -> Result<()> {
        instructions::cancel_listing::handle_cancel_listing(ctx)
    }

    /// Pays the supplier whose signature over the claim message is checked
    /// earlier in the transaction and whose address and `salt` are the
    /// listing's commitment.
    pub fn claim(ctx: Context<Claim>, salt: [u8; 32]) -> Result<()> {
        instructions::claim::handle_claim(ctx, salt)
    }

    /// Repays a claimed listing in full and closes it to its anchor.
    pub fn repay(ctx: Context<Repay>) -> Result<()> {
        instructions::repay::handle_repay(ctx)
    }

    /// Takes a claimed listing out of NAV once the pool's days past its due
    /// date have passed unrepaid. Anyone may send it.
    pub fn write_off(ctx: Context<WriteOff>) -> Result<()> {
        instructions::write_off::handle_write_off(ctx)
    }
}
