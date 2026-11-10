use anchor_lang::prelude::*;

use crate::{constants::*, math::*};

/// A u128 stored as two u64s, so zero-copy accounts have the same layout on
/// the host (where u128 aligns to 16) as on-chain (where it aligns to 8).
#[zero_copy]
pub struct Wide {
    pub lo: u64,
    pub hi: u64,
}

impl Wide {
    pub fn get(&self) -> u128 {
        ((self.hi as u128) << 64) | self.lo as u128
    }

    pub fn set(&mut self, value: u128) {
        self.lo = value as u64;
        self.hi = (value >> 64) as u64;
    }
}

/// The loans that mature in one epoch, summed so the pool's NAV takes the
/// same few steps however many loans it has. Each loan has its own rate, so
/// with p a loan's principal, r its rate, l its past-maturity rate, s when
/// its interest last started accruing and m its maturity (unix seconds), a
/// bucket holds Σp, Σp·r, Σp·r·min(s,m), Σp·l, Σp·l·max(s,m) and the
/// interest its loans have accrued but not paid. A bucket whose loans are
/// impaired is emptied, and its NAV moves to the pool's `impaired_value`.
#[zero_copy]
pub struct MaturityBucket {
    pub maturity_epoch: u64,
    /// The bucket's performing loans: each must be marked every epoch.
    /// Impairing the bucket, or writing a loan down, takes them out.
    pub loans: u64,
    pub principal: Wide,
    pub principal_rate: Wide,
    pub principal_rate_start: Wide,
    pub principal_late_rate: Wide,
    pub principal_late_rate_start: Wide,
    pub carried_interest: Wide,
}

/// The risk engine's weights, all in basis points. `risk_engine.rs` says
/// what each decides.
#[zero_copy]
pub struct RiskParams {
    pub base_advance_bps: u16,
    pub max_advance_bps: u16,
    pub weight_31_to_60_bps: u16,
    pub concentration_threshold_bps: u16,
    pub concentration_penalty_bps: u16,
    pub base_rate_bps: u16,
    pub min_rate_bps: u16,
    pub max_rate_bps: u16,
    pub past_due_spread_bps: u16,
    pub concentration_spread_bps: u16,
    pub dispute_spread_bps: u16,
    /// Units past this many days (30, 60 or 90, an aging bucket's edge)
    /// count as late for `max_late_share_bps`.
    pub late_after_days: u16,
    /// Refuses a borrow when more of the token's units than this are late.
    pub max_late_share_bps: u16,
    /// Refuses a borrow when the token's burn debt was overdue for more
    /// than this many days in the year before its snapshot.
    pub max_days_burn_debt_overdue: u16,
    /// Priced in full when the customers pay `beyond_terms_cap_days` or
    /// more after the due date.
    pub beyond_terms_spread_bps: u16,
    pub beyond_terms_cap_days: u16,
    /// Priced in full when none of the customers' volume was paid on time.
    pub not_on_time_spread_bps: u16,
    /// Priced in full when no unit's customer has a payment history.
    pub no_history_spread_bps: u16,
}

/// How a loan's rate is set. Stored as a u8 in the pool.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateMode {
    /// Set by the risk engine when the loan opens, and never changed.
    Fixed,
    /// Moves with the pool's conditions. Declared so pools and clients
    /// name it; `create_pool` refuses it until it is built.
    Variable,
}

impl RateMode {
    pub fn to_u8(self) -> u8 {
        match self {
            Self::Fixed => 0,
            Self::Variable => 1,
        }
    }
}

impl TryFrom<u8> for RateMode {
    type Error = Error;

    fn try_from(v: u8) -> Result<Self> {
        match v {
            0 => Ok(Self::Fixed),
            1 => Ok(Self::Variable),
            _ => err!(crate::error::PoolError::RateModeNotSupported),
        }
    }
}

/// Which version of the risk engine's formula a pool decides by. A pool
/// keeps the model it was created with: a later program adds a model and
/// never changes an existing one, so no pool's rules change under it.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiskModel {
    V1,
}

impl RiskModel {
    pub fn to_u8(self) -> u8 {
        match self {
            Self::V1 => 1,
        }
    }
}

/// A pool: its fixed parameters, its running totals, and its loans by
/// maturity. Nothing changes its parameters after it is created.
#[account(zero_copy)]
pub struct Pool {
    /// Who created the pool. It holds no authority over it.
    pub creator: Pubkey,
    /// The token the pool lends and takes deposits in (USDC).
    pub quote_mint: Pubkey,
    /// The pool's share token.
    pub share_mint: Pubkey,
    /// The keys whose posted roots the pool reads tokens' facts from; the
    /// first `signer_count` are set.
    pub signers: [Pubkey; MAX_SIGNERS],
    /// The SAS credential and schema a depositor's wallet must hold an
    /// attestation of, when `requires_attestation` is 1.
    pub attestation_credential: Pubkey,
    pub attestation_schema: Pubkey,
    /// The network the pool lends on, as snapshots name it: a network ID
    /// ("sol", "sol-devnet") in ASCII, padded with zeros.
    pub network: [u8; 16],
    pub seed: u64,
    /// The start of epoch 0, the pool's creation.
    pub epoch_origin: i64,
    pub epoch_seconds: u64,
    /// The oldest root, in seconds since its `as_of`, the pool reads.
    pub max_root_age: u64,
    /// How long into an epoch keepers have to mark every loan before a
    /// loan left unmarked can be marked stale.
    pub mark_window_seconds: u64,
    /// The epoch whose start was last settled: its loans impaired by
    /// maturity and its marking begun.
    pub settled_epoch: u64,
    /// The epoch whose withdrawals were last paid, once every loan was
    /// marked. Deposits, loans and withdrawal requests wait for it to be
    /// the current epoch.
    pub withdrawals_epoch: u64,
    /// USDC the pool holds that is neither lent nor set aside for
    /// withdrawals.
    pub idle: u64,
    /// USDC set aside for withdrawals already settled but not yet claimed.
    pub reserved: u64,
    /// What NAV counts for impaired loans.
    pub impaired_value: u64,
    /// What NAV takes off for loans marked stale this epoch.
    pub stale_discount: u64,
    /// Shares held in escrow waiting to be paid at an epoch's start.
    pub pending_shares: u64,
    /// The withdrawal queue's generation, which ends at every epoch start
    /// that has withdrawals to pay.
    pub withdrawal_generation: u64,
    /// Performing loans marked in the settled epoch.
    pub marked_loans: u64,
    /// The loan terms allowed, in epochs; the first `maturity_count` are set.
    pub maturities: [u16; MAX_MATURITIES],
    pub risk: RiskParams,
    pub cap_per_token_bps: u16,
    pub impaired_share_bps: u16,
    pub grace_epochs: u16,
    /// Added to a loan's rate once it is past maturity.
    pub late_spread_bps: u16,
    pub signer_count: u8,
    pub maturity_count: u8,
    pub quote_decimals: u8,
    pub requires_attestation: u8,
    pub bump: u8,
    pub authority_bump: u8,
    pub version: u8,
    /// A RateMode.
    pub rate_mode: u8,
    /// A RiskModel.
    pub risk_model: u8,
    pub padding: [u8; 3],
    /// Room for fields a later layout version adds without moving any.
    pub spare: [u8; 256],
    pub buckets: [MaturityBucket; BUCKETS],
}

/// The parameters a pool is created with, which nothing changes afterwards.
/// `pools/*.toml` lists them with what each decides.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct PoolParams {
    pub signers: Vec<Pubkey>,
    /// A network ID in ASCII ("sol-devnet"), at most 16 bytes.
    pub network: String,
    pub epoch_seconds: u64,
    pub mark_window_seconds: u64,
    pub max_root_age: u64,
    pub maturities: Vec<u16>,
    pub grace_epochs: u16,
    pub impaired_share_bps: u16,
    pub cap_per_token_bps: u16,
    pub late_spread_bps: u16,
    pub rate_mode: RateMode,
    pub risk_model: RiskModel,
    pub risk: RiskConfig,
    /// The SAS credential and schema a depositor must hold an attestation
    /// of, if any.
    pub depositor_attestation: Option<DepositorAttestation>,
}

/// The risk engine's weights as a pool is created with them; stored as
/// RiskParams.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug)]
pub struct RiskConfig {
    pub base_advance_bps: u16,
    pub max_advance_bps: u16,
    pub weight_31_to_60_bps: u16,
    pub concentration_threshold_bps: u16,
    pub concentration_penalty_bps: u16,
    pub base_rate_bps: u16,
    pub min_rate_bps: u16,
    pub max_rate_bps: u16,
    pub past_due_spread_bps: u16,
    pub concentration_spread_bps: u16,
    pub dispute_spread_bps: u16,
    /// Units past this many days (30, 60 or 90, an aging bucket's edge)
    /// count as late for `max_late_share_bps`.
    pub late_after_days: u16,
    /// Refuses a borrow when more of the token's units than this are late.
    pub max_late_share_bps: u16,
    /// Refuses a borrow when the token's burn debt was overdue for more
    /// than this many days in the year before its snapshot.
    pub max_days_burn_debt_overdue: u16,
    /// Priced in full when the customers pay `beyond_terms_cap_days` or
    /// more after the due date.
    pub beyond_terms_spread_bps: u16,
    pub beyond_terms_cap_days: u16,
    /// Priced in full when none of the customers' volume was paid on time.
    pub not_on_time_spread_bps: u16,
    /// Priced in full when no unit's customer has a payment history.
    pub no_history_spread_bps: u16,
}

impl From<RiskConfig> for RiskParams {
    fn from(c: RiskConfig) -> Self {
        Self {
            base_advance_bps: c.base_advance_bps,
            max_advance_bps: c.max_advance_bps,
            weight_31_to_60_bps: c.weight_31_to_60_bps,
            concentration_threshold_bps: c.concentration_threshold_bps,
            concentration_penalty_bps: c.concentration_penalty_bps,
            base_rate_bps: c.base_rate_bps,
            min_rate_bps: c.min_rate_bps,
            max_rate_bps: c.max_rate_bps,
            past_due_spread_bps: c.past_due_spread_bps,
            concentration_spread_bps: c.concentration_spread_bps,
            dispute_spread_bps: c.dispute_spread_bps,
            late_after_days: c.late_after_days,
            max_late_share_bps: c.max_late_share_bps,
            max_days_burn_debt_overdue: c.max_days_burn_debt_overdue,
            beyond_terms_spread_bps: c.beyond_terms_spread_bps,
            beyond_terms_cap_days: c.beyond_terms_cap_days,
            not_on_time_spread_bps: c.not_on_time_spread_bps,
            no_history_spread_bps: c.no_history_spread_bps,
        }
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug)]
pub struct DepositorAttestation {
    pub credential: Pubkey,
    pub schema: Pubkey,
}

/// A borrower's loan against one token in a pool, at
/// [LOAN_SEED, pool, mint, borrower]. A borrower has one open loan per token
/// per pool, and the account is reused for the next once it closes, so the
/// vault holding the units is created and thawed only once.
#[account]
#[derive(InitSpace)]
pub struct Loan {
    pub version: u8,
    pub pool: Pubkey,
    pub collateral_mint: Pubkey,
    pub borrower: Pubkey,
    pub bump: u8,
    pub is_open: bool,
    /// The units in the vault when the loan was last synced. Units leave
    /// without the pool acting (a buy-back, the agent's redemption), so the
    /// vault's balance is the truth.
    pub units: u64,
    pub principal: u64,
    /// Interest accrued before `accrual_start` and not yet paid.
    pub carried_interest: u64,
    /// When interest on `principal` last started accruing.
    pub accrual_start: i64,
    pub maturity_epoch: u64,
    pub maturity_time: i64,
    /// The yearly rate, fixed when the loan opened.
    pub rate_bps: u16,
    /// The yearly rate past maturity: `rate_bps` plus the pool's spread.
    pub late_rate_bps: u16,
    /// Impaired because a mark found the token's facts bad, rather than by
    /// maturity and grace passing.
    pub is_written_down: bool,
    /// What NAV counts for this loan once impaired, set when a mark writes
    /// it down or the first time it is touched after its bucket is impaired.
    pub impaired_value: u64,
    pub is_impaired_value_set: bool,
    /// The epoch the loan was last marked in.
    pub marked_epoch: u64,
    /// The `as_of` of the newest root the loan's token was read from. A
    /// later read must use a root at least this new, so a keeper can't pick
    /// an older, kinder snapshot.
    pub last_root_as_of: i64,
    /// What NAV takes off for this loan while it is marked stale.
    pub stale_discount: u64,
    /// What the loan still owed when it was written off, its units gone,
    /// less what has been paid for it since. USDC that arrives for it
    /// later goes to the pool up to this, and only the rest to the
    /// borrower.
    pub written_off: u64,
}

/// What a pool has lent against one token, across its borrowers, at
/// [EXPOSURE_SEED, pool, mint], so the cap per token counts every loan.
#[account]
#[derive(InitSpace)]
pub struct TokenExposure {
    pub version: u8,
    pub pool: Pubkey,
    pub collateral_mint: Pubkey,
    pub bump: u8,
    /// The principal outstanding on the token's open loans.
    pub principal: u64,
}

/// A depositor's pending withdrawal. A wallet has one at a time; it is
/// closed once paid in full.
#[account]
#[derive(InitSpace)]
pub struct WithdrawalRequest {
    pub version: u8,
    pub pool: Pubkey,
    pub owner: Pubkey,
    pub bump: u8,
    /// The generation the shares are queued in.
    pub generation: u64,
    pub shares: u64,
}

/// What an ended generation of the withdrawal queue asked and paid, so its
/// requests can claim after it ends.
#[account]
#[derive(InitSpace)]
pub struct WithdrawalGeneration {
    pub version: u8,
    pub pool: Pubkey,
    pub generation: u64,
    /// Shares the generation's requests asked to withdraw.
    pub shares: u64,
    /// Of those, the shares paid for.
    pub shares_paid: u64,
    /// USDC set aside for them.
    pub paid: u64,
}

impl WithdrawalGeneration {
    /// What a request for `shares` in this generation is owed: the USDC it
    /// is paid, and its shares left unpaid. Both round down, so the
    /// generation's requests are never owed more than it set aside, nor
    /// given back more shares than it left in escrow.
    pub fn owed_to(&self, shares: u64) -> Result<(u64, u64)> {
        let usdc = to_u64(mul_div(
            shares as u128,
            self.paid as u128,
            self.shares as u128,
        )?)?;
        let unpaid = to_u64(mul_div(
            shares as u128,
            (self.shares - self.shares_paid) as u128,
            self.shares as u128,
        )?)?;
        Ok((usdc, unpaid))
    }
}
