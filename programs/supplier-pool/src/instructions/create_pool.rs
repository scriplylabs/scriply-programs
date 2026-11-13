use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{Mint, TokenAccount, TokenInterface},
};

use crate::{constants::*, error::SupplierPoolError, events::PoolCreated, state::*};

#[derive(Accounts)]
pub struct CreatePool<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,
    #[account(
        init,
        payer = creator,
        space = 8 + Pool::INIT_SPACE,
        seeds = [POOL_SEED, creator.key().as_ref(), mint.key().as_ref()],
        bump
    )]
    pub pool: Box<Account<'info, Pool>>,
    #[account(mint::token_program = token_program)]
    pub mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        init,
        payer = creator,
        seeds = [SHARES_SEED, pool.key().as_ref()],
        bump,
        mint::decimals = mint.decimals + SHARE_DECIMALS_OFFSET,
        mint::authority = pool,
        mint::token_program = token_program,
    )]
    pub share_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        init,
        payer = creator,
        associated_token::mint = mint,
        associated_token::authority = pool,
        associated_token::token_program = token_program,
    )]
    pub vault: Box<InterfaceAccount<'info, TokenAccount>>,
    /// The token is a classic SPL token: a Token-2022 mint's transfer fee
    /// or permanent delegate would leave the vault holding less than the
    /// pool counts. Shares are classic SPL tokens too.
    #[account(address = anchor_spl::token::ID)]
    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handle_create_pool(ctx: Context<CreatePool>, params: PoolParams) -> Result<()> {
    validate(&params, ctx.accounts.mint.decimals)?;
    let pool = &mut ctx.accounts.pool;
    pool.version = ACCOUNT_VERSION;
    pool.bump = ctx.bumps.pool;
    pool.creator = ctx.accounts.creator.key();
    pool.mint = ctx.accounts.mint.key();
    pool.token_decimals = ctx.accounts.mint.decimals;
    pool.share_mint = ctx.accounts.share_mint.key();
    pool.network = params.network;
    pool.currency = params.currency;
    pool.signers = params.signers;
    pool.anchors = params.anchors;
    pool.fee = params.fee;
    pool.rules = params.rules;
    pool.max_root_age = params.max_root_age;
    pool.cap_per_anchor_bps = params.cap_per_anchor_bps;
    pool.idle = 0;
    pool.outstanding = 0;
    pool.listed = 0;
    pool.reserved = [0; 128];
    emit!(PoolCreated {
        pool: pool.key(),
        creator: pool.creator,
        mint: pool.mint,
    });
    Ok(())
}

fn validate(params: &PoolParams, decimals: u8) -> Result<()> {
    require!(
        (1..=MAX_SIGNERS).contains(&params.signers.len()),
        SupplierPoolError::InvalidSigners
    );
    require!(
        params.anchors.len() <= MAX_ANCHORS,
        SupplierPoolError::TooManyAnchors
    );
    require!(params.fee.is_valid(), SupplierPoolError::InvalidFee);
    require!(
        (1..=BPS as u16).contains(&params.cap_per_anchor_bps),
        SupplierPoolError::InvalidCap
    );
    require!(
        params.max_root_age >= 1 && params.rules.max_term_days >= 1,
        SupplierPoolError::InvalidAge
    );
    require!(decimals <= 18, SupplierPoolError::InvalidDecimals);
    require!(
        params.rules.required_documents & !0b111 == 0
            && params.rules.min_vendor_tenure <= 4
            && params.rules.min_vendor_volume <= 5,
        SupplierPoolError::InvalidBand
    );
    Ok(())
}
