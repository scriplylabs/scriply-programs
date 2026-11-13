use anchor_lang::{prelude::*, system_program};
use snapshot_roots::{SnapshotRoots, ROOTS_SEED};

use crate::{
    constants::*,
    error::SupplierPoolError,
    events::BillListed,
    facts::{read_bill_leaf, ProvenLeaf},
    state::*,
};

#[derive(Accounts)]
#[instruction(commitment: [u8; 32])]
pub struct ListBill<'info> {
    /// The anchor's wallet: it pays every cost of the listing and alone
    /// cancels or repays it.
    #[account(mut)]
    pub anchor: Signer<'info>,
    #[account(
        mut,
        seeds = [POOL_SEED, pool.creator.as_ref(), pool.mint.as_ref()],
        bump = pool.bump,
    )]
    pub pool: Box<Account<'info, Pool>>,
    #[account(
        init_if_needed,
        payer = anchor,
        space = 8 + AnchorAccount::INIT_SPACE,
        seeds = [ANCHOR_SEED, pool.key().as_ref(), anchor.key().as_ref()],
        bump
    )]
    pub anchor_account: Box<Account<'info, AnchorAccount>>,
    /// Keyed by the anchor as well as the commitment, so nobody else can
    /// take a bill's listing address by listing its public leaf first.
    #[account(
        init,
        payer = anchor,
        space = 8 + Listing::INIT_SPACE,
        seeds = [LISTING_SEED, pool.key().as_ref(), anchor.key().as_ref(), &commitment],
        bump
    )]
    pub listing: Box<Account<'info, Listing>>,
    /// The signer's roots the leaf is proven against.
    #[account(
        seeds = [ROOTS_SEED, roots.signer.as_ref()],
        bump = roots.bump,
        seeds::program = snapshot_roots::ID,
    )]
    pub roots: Box<Account<'info, SnapshotRoots>>,
    pub system_program: Program<'info, System>,
}

/// Lists the bill whose leaf `proven` shows for `commitment`, at its amount
/// and repayment date, once the pool's rules pass; the error names the rule
/// that refused it.
pub fn handle_list_bill(
    ctx: Context<ListBill>,
    commitment: [u8; 32],
    proven: ProvenLeaf,
) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let anchor = ctx.accounts.anchor.key();
    let pool = &ctx.accounts.pool;
    require!(
        pool.takes_anchor(&anchor),
        SupplierPoolError::AnchorNotTaken
    );
    let bill = read_bill_leaf(pool, &ctx.accounts.roots, &proven, &commitment, now)?;

    let rules = pool.rules;
    require!(
        bill.documents_verified & rules.required_documents == rules.required_documents,
        SupplierPoolError::DocumentsNotVerified
    );
    require!(
        bill.vendor_tenure >= rules.min_vendor_tenure,
        SupplierPoolError::VendorTenureTooShort
    );
    require!(
        bill.vendor_volume >= rules.min_vendor_volume,
        SupplierPoolError::VendorVolumeTooLow
    );
    require!(bill.repay_on > now, SupplierPoolError::RepaymentDatePassed);
    let term_days = (bill.repay_on - now + SECONDS_PER_DAY - 1) / SECONDS_PER_DAY;
    require!(
        term_days <= rules.max_term_days as i64,
        SupplierPoolError::TermTooLong
    );

    let amount = pool.base_units(bill.amount)?;
    require!(amount > 0, SupplierPoolError::ZeroAmount);
    let owed = ctx
        .accounts
        .anchor_account
        .owed
        .checked_add(amount)
        .ok_or(SupplierPoolError::MathOverflow)?;
    require!(owed <= pool.anchor_cap()?, SupplierPoolError::OverAnchorCap);

    // The deposit pays a claim's network fee and the supplier's token
    // account if it has none, whoever submits the claim.
    let fee_deposit = CLAIM_NETWORK_FEE_LAMPORTS + Rent::get()?.minimum_balance(TOKEN_ACCOUNT_LEN);
    system_program::transfer(
        CpiContext::new(
            ctx.accounts.system_program.key(),
            system_program::Transfer {
                from: ctx.accounts.anchor.to_account_info(),
                to: ctx.accounts.listing.to_account_info(),
            },
        ),
        fee_deposit,
    )?;

    let pool_key = ctx.accounts.pool.key();
    let anchor_account = &mut ctx.accounts.anchor_account;
    if anchor_account.version == 0 {
        anchor_account.version = ACCOUNT_VERSION;
        anchor_account.bump = ctx.bumps.anchor_account;
        anchor_account.pool = pool_key;
        anchor_account.wallet = anchor;
    }
    anchor_account.owed = owed;

    let listing = &mut ctx.accounts.listing;
    listing.version = ACCOUNT_VERSION;
    listing.bump = ctx.bumps.listing;
    listing.pool = pool_key;
    listing.anchor = anchor;
    listing.commitment = commitment;
    listing.amount = amount;
    listing.due_date = bill.repay_on;
    listing.fee_deposit = fee_deposit;
    listing.claimed = false;

    let pool = &mut ctx.accounts.pool;
    pool.listed = pool
        .listed
        .checked_add(amount)
        .ok_or(SupplierPoolError::MathOverflow)?;
    emit!(BillListed {
        pool: pool_key,
        anchor,
        listing: listing.key(),
        commitment,
        amount,
        due_date: bill.repay_on,
    });
    Ok(())
}
