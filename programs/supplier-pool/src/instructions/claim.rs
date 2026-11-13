use anchor_lang::{compat::solana_instructions_sysvar, prelude::*};
use anchor_spl::{
    associated_token::{self, get_associated_token_address_with_program_id, AssociatedToken},
    token_interface::{Mint, TokenAccount, TokenInterface},
};
use solana_sha256_hasher::hashv;

use crate::{
    claim_signature::require_claim_signature, constants::*, error::SupplierPoolError,
    events::BillClaimed, state::*, tokens,
};

#[derive(Accounts)]
pub struct Claim<'info> {
    /// Whoever submits the claim; the listing's deposit refunds it.
    #[account(mut)]
    pub fee_payer: Signer<'info>,
    #[account(
        mut,
        seeds = [POOL_SEED, pool.creator.as_ref(), pool.mint.as_ref()],
        bump = pool.bump,
        has_one = mint,
    )]
    pub pool: Box<Account<'info, Pool>>,
    #[account(
        mut,
        seeds = [LISTING_SEED, pool.key().as_ref(), listing.anchor.as_ref(), &listing.commitment],
        bump = listing.bump,
        has_one = pool,
    )]
    pub listing: Box<Account<'info, Listing>>,
    /// CHECK: the address the listing commits to, proven by the salt and by
    /// the ed25519 check of its signature, both in the handler.
    pub supplier: UncheckedAccount<'info>,
    /// CHECK: the supplier's associated token account for the pool's token,
    /// checked by address and created in the handler if it is missing.
    #[account(mut)]
    pub supplier_tokens: UncheckedAccount<'info>,
    pub mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = pool,
        associated_token::token_program = token_program,
    )]
    pub vault: Box<InterfaceAccount<'info, TokenAccount>>,
    /// CHECK: the instructions sysvar, by address.
    #[account(address = solana_instructions_sysvar::ID)]
    pub instructions: UncheckedAccount<'info>,
    #[account(address = anchor_spl::token::ID)]
    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

/// Pays the supplier who signed the claim message and whose address and
/// `salt` are the listing's commitment: the amount less the pool's fee for
/// the days left to the due date. Refunds the fee payer from the deposit.
pub fn handle_claim(ctx: Context<Claim>, salt: [u8; 32]) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let listing_key = ctx.accounts.listing.key();
    let supplier = ctx.accounts.supplier.key();
    let listing = &ctx.accounts.listing;
    require!(!listing.claimed, SupplierPoolError::AlreadyClaimed);
    require!(
        listing.due_date > now,
        SupplierPoolError::RepaymentDatePassed
    );
    require!(
        hashv(&[supplier.as_ref(), &salt]).to_bytes() == listing.commitment,
        SupplierPoolError::WrongSupplier
    );
    require_claim_signature(&ctx.accounts.instructions, &supplier, &listing_key)?;
    require_keys_eq!(
        ctx.accounts.supplier_tokens.key(),
        get_associated_token_address_with_program_id(
            &supplier,
            &ctx.accounts.mint.key(),
            &anchor_spl::token::ID
        ),
        SupplierPoolError::WrongSupplierAccount
    );

    let amount = listing.amount;
    let days = (listing.due_date - now + SECONDS_PER_DAY - 1) / SECONDS_PER_DAY;
    let fee = mul_div(amount, ctx.accounts.pool.fee.bps_for(days as u64), BPS)?;
    let paid = amount - fee;
    require!(paid <= ctx.accounts.pool.idle, SupplierPoolError::PoolShort);

    let mut refund = CLAIM_NETWORK_FEE_LAMPORTS;
    let a = &ctx.accounts;
    if a.supplier_tokens.data_is_empty() {
        associated_token::create(CpiContext::new(
            a.associated_token_program.key(),
            associated_token::Create {
                payer: a.fee_payer.to_account_info(),
                associated_token: a.supplier_tokens.to_account_info(),
                authority: a.supplier.to_account_info(),
                mint: a.mint.to_account_info(),
                system_program: a.system_program.to_account_info(),
                token_program: a.token_program.to_account_info(),
            },
        ))?;
        refund += Rent::get()?.minimum_balance(TOKEN_ACCOUNT_LEN);
    }
    let (creator, mint, bump) = (a.pool.creator, a.pool.mint, a.pool.bump);
    tokens::transfer(
        &a.token_program,
        a.vault.to_account_info(),
        &a.mint,
        a.supplier_tokens.to_account_info(),
        a.pool.to_account_info(),
        &[&[POOL_SEED, creator.as_ref(), mint.as_ref(), &[bump]]],
        paid,
    )?;

    // The listing is this program's account, so its lamports move directly.
    let refund = refund.min(a.listing.fee_deposit);
    a.listing.sub_lamports(refund)?;
    a.fee_payer.add_lamports(refund)?;

    let listing = &mut ctx.accounts.listing;
    listing.fee_deposit -= refund;
    listing.claimed = true;
    listing.supplier = supplier;
    listing.claimed_at = now;
    listing.paid = paid;

    let pool = &mut ctx.accounts.pool;
    pool.idle -= paid;
    pool.listed -= amount;
    pool.outstanding = pool
        .outstanding
        .checked_add(amount)
        .ok_or(SupplierPoolError::MathOverflow)?;
    emit!(BillClaimed {
        pool: pool.key(),
        listing: listing_key,
        supplier,
        paid,
        fee,
    });
    Ok(())
}
