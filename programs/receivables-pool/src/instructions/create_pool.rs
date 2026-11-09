use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{Mint, TokenAccount, TokenInterface},
};

use crate::{constants::*, error::PoolError, events::PoolCreated, state::*};

#[derive(Accounts)]
#[instruction(seed: u64)]
pub struct CreatePool<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,
    #[account(
        init,
        payer = creator,
        space = 8 + std::mem::size_of::<Pool>(),
        seeds = [POOL_SEED, creator.key().as_ref(), &seed.to_le_bytes()],
        bump
    )]
    pub pool: AccountLoader<'info, Pool>,
    /// CHECK: a PDA with no data that signs for the pool's token accounts.
    #[account(seeds = [AUTHORITY_SEED, pool.key().as_ref()], bump)]
    pub pool_authority: UncheckedAccount<'info>,
    #[account(mint::token_program = quote_token_program)]
    pub quote_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        init,
        payer = creator,
        seeds = [SHARES_SEED, pool.key().as_ref()],
        bump,
        mint::decimals = quote_mint.decimals + SHARE_DECIMALS_OFFSET,
        mint::authority = pool_authority,
        mint::token_program = share_token_program,
    )]
    pub share_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        init,
        payer = creator,
        associated_token::mint = quote_mint,
        associated_token::authority = pool_authority,
        associated_token::token_program = quote_token_program,
    )]
    pub quote_vault: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        init,
        payer = creator,
        associated_token::mint = share_mint,
        associated_token::authority = pool_authority,
        associated_token::token_program = share_token_program,
    )]
    pub share_escrow: Box<InterfaceAccount<'info, TokenAccount>>,
    /// The quote token is a classic SPL token: a Token-2022 mint's transfer
    /// fee or permanent delegate would leave the vault holding less than
    /// the pool counts.
    #[account(address = anchor_spl::token::ID)]
    pub quote_token_program: Interface<'info, TokenInterface>,
    /// Shares are classic SPL tokens, which every wallet shows.
    #[account(address = anchor_spl::token::ID)]
    pub share_token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handle_create_pool(ctx: Context<CreatePool>, seed: u64, params: PoolParams) -> Result<()> {
    validate(&params, ctx.accounts.quote_mint.decimals)?;

    let now = Clock::get()?.unix_timestamp;
    let mut pool = ctx.accounts.pool.load_init()?;
    pool.version = ACCOUNT_VERSION;
    pool.creator = ctx.accounts.creator.key();
    pool.quote_mint = ctx.accounts.quote_mint.key();
    pool.share_mint = ctx.accounts.share_mint.key();
    for (slot, signer) in pool.signers.iter_mut().zip(params.signers.iter()) {
        *slot = *signer;
    }
    pool.signer_count = params.signers.len() as u8;
    if let Some(attestation) = params.depositor_attestation {
        pool.attestation_credential = attestation.credential;
        pool.attestation_schema = attestation.schema;
        pool.requires_attestation = 1;
    }
    pool.network[..params.network.len()].copy_from_slice(params.network.as_bytes());
    pool.seed = seed;
    pool.epoch_origin = now;
    pool.epoch_seconds = params.epoch_seconds;
    pool.mark_window_seconds = params.mark_window_seconds;
    pool.max_root_age = params.max_root_age;
    pool.remaining_factor.set(INDEX_SCALE);
    for (slot, maturity) in pool.maturities.iter_mut().zip(params.maturities.iter()) {
        *slot = *maturity;
    }
    pool.maturity_count = params.maturities.len() as u8;
    pool.risk = params.risk.into();
    pool.cap_per_token_bps = params.cap_per_token_bps;
    pool.impaired_share_bps = params.impaired_share_bps;
    pool.grace_epochs = params.grace_epochs;
    pool.late_spread_bps = params.late_spread_bps;
    pool.rate_mode = params.rate_mode.to_u8();
    pool.risk_model = params.risk_model.to_u8();
    pool.quote_decimals = ctx.accounts.quote_mint.decimals;
    pool.bump = ctx.bumps.pool;
    pool.authority_bump = ctx.bumps.pool_authority;

    emit!(PoolCreated {
        pool: ctx.accounts.pool.key(),
        creator: pool.creator
    });
    Ok(())
}

/// Fails unless `params` describe a pool this program can run.
fn validate(params: &PoolParams, quote_decimals: u8) -> Result<()> {
    require!(
        !params.signers.is_empty() && params.signers.len() <= MAX_SIGNERS,
        PoolError::InvalidSigners
    );
    require!(
        !params.maturities.is_empty()
            && params.maturities.len() <= MAX_MATURITIES
            && params.maturities.iter().all(|m| *m >= 1),
        PoolError::InvalidMaturities
    );
    let longest = *params.maturities.iter().max().unwrap_or(&0) as usize;
    require!(
        longest + (params.grace_epochs as usize) < BUCKETS,
        PoolError::MaturityTooLong
    );
    match params.rate_mode {
        RateMode::Fixed => {}
        RateMode::Variable => return err!(PoolError::RateModeNotSupported),
    }
    match params.risk_model {
        RiskModel::V1 => {}
    }
    let r = &params.risk;
    require!(
        r.base_advance_bps <= r.max_advance_bps && r.max_advance_bps as u64 <= BPS,
        PoolError::InvalidAdvance
    );
    require!(
        r.min_rate_bps <= r.base_rate_bps && r.base_rate_bps <= r.max_rate_bps,
        PoolError::InvalidRate
    );
    require!(
        [
            r.weight_31_to_60_bps,
            r.concentration_threshold_bps,
            params.impaired_share_bps,
            params.cap_per_token_bps,
        ]
        .iter()
        .all(|bps| *bps as u64 <= BPS)
            && (r.concentration_threshold_bps as u64) < BPS
            && params.cap_per_token_bps >= 1,
        PoolError::InvalidShare
    );
    require!(
        matches!(r.late_after_days, 30 | 60 | 90)
            && r.max_late_share_bps as u64 <= BPS
            && r.beyond_terms_cap_days >= 1,
        PoolError::InvalidLateRule
    );
    require!(params.epoch_seconds >= 60, PoolError::InvalidEpoch);
    require!(
        params.mark_window_seconds < params.epoch_seconds,
        PoolError::InvalidMarkWindow
    );
    require!(params.max_root_age >= 1, PoolError::InvalidRootAge);
    require!(quote_decimals <= 18, PoolError::InvalidDecimals);
    require!(
        params.network.len() <= 16 && params.network.is_ascii(),
        PoolError::WrongNetwork
    );
    Ok(())
}
