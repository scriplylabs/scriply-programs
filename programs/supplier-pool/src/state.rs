use anchor_lang::prelude::*;

use crate::{constants::*, error::SupplierPoolError};

/// What the pool keeps of each bill it pays early, in basis points of the
/// bill's amount.
#[derive(AnchorSerialize, AnchorDeserialize, InitSpace, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeeSchedule {
    /// The same whatever the term: 150 is 1.5%.
    Flat { bps: u16 },
    /// `base_bps` plus `bps_per_30_days` for each 30 days from the claim to
    /// the due date, pro rata by the day, at most `max_bps`.
    PerDay {
        base_bps: u16,
        bps_per_30_days: u16,
        max_bps: u16,
    },
}

impl FeeSchedule {
    /// The fee in basis points for a claim `days` before the due date
    /// (rounded up to whole days).
    ///
    /// ```ignore
    /// FeeSchedule::Flat { bps: 150 }.bps_for(45) // 150
    /// ```
    pub fn bps_for(&self, days: u64) -> u64 {
        match *self {
            FeeSchedule::Flat { bps } => bps as u64,
            FeeSchedule::PerDay {
                base_bps,
                bps_per_30_days,
                max_bps,
            } => {
                let by_days = (bps_per_30_days as u64).saturating_mul(days) / 30;
                (base_bps as u64)
                    .saturating_add(by_days)
                    .min(max_bps as u64)
            }
        }
    }

    pub fn is_valid(&self) -> bool {
        match *self {
            FeeSchedule::Flat { bps } => bps as u64 <= BPS,
            FeeSchedule::PerDay {
                base_bps, max_bps, ..
            } => base_bps <= max_bps && max_bps as u64 <= BPS,
        }
    }
}

/// What a pool refuses, from the bill leaf's facts.
#[derive(AnchorSerialize, AnchorDeserialize, InitSpace, Clone, Copy, Debug, PartialEq, Eq)]
pub struct BillRules {
    /// The documents an operator must have verified, as the bill leaf's
    /// bits (vendor's invoice 1, purchase order 2, proof of delivery 4);
    /// each one set here must be set there.
    pub required_documents: u8,
    /// The least `vendorTenure` band the pool takes (0 takes a new vendor).
    pub min_vendor_tenure: u8,
    /// The least `vendorVolume` band the pool takes (0 takes any).
    pub min_vendor_volume: u8,
    /// The most days from listing to the bill's repayment date.
    pub max_term_days: u16,
}

/// A pool's parameters, fixed when it is created.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct PoolParams {
    /// The network ID in ASCII, padded with zeros, that its bill leaves
    /// must name: `sol-devnet`.
    pub network: [u8; 16],
    /// The asset ID in ASCII, padded with zeros, that its bill leaves must
    /// be in: `FIAT_USD` for a pool of USDC.
    pub currency: [u8; 16],
    /// Whose posted roots it reads bills' facts from.
    pub signers: Vec<Pubkey>,
    /// The anchors' wallets it takes; empty takes any.
    pub anchors: Vec<Pubkey>,
    pub fee: FeeSchedule,
    pub rules: BillRules,
    /// The oldest root it reads, in seconds before now.
    pub max_root_age: u32,
    /// The most one anchor's wallet may owe the pool across its open
    /// listings, in the token's base units. A fixed amount, not a share of
    /// what the pool holds, so a deposit made for a listing and withdrawn
    /// after it can't raise it.
    pub max_owed_per_anchor: u64,
}

/// A pool at `[POOL_SEED, creator, mint]`. Its vault is its own associated
/// token account for `mint`, and it is the authority of its vault and its
/// share mint.
#[account]
#[derive(InitSpace)]
pub struct Pool {
    pub version: u8,
    pub bump: u8,
    pub creator: Pubkey,
    pub mint: Pubkey,
    pub token_decimals: u8,
    pub share_mint: Pubkey,
    pub network: [u8; 16],
    pub currency: [u8; 16],
    #[max_len(MAX_SIGNERS)]
    pub signers: Vec<Pubkey>,
    #[max_len(MAX_ANCHORS)]
    pub anchors: Vec<Pubkey>,
    pub fee: FeeSchedule,
    pub rules: BillRules,
    pub max_root_age: u32,
    pub max_owed_per_anchor: u64,
    /// The token in the vault that the pool counts: deposits and
    /// repayments in, claims and withdrawals out. Tokens sent to the vault
    /// any other way are not counted.
    pub idle: u64,
    /// What suppliers were paid early on and anchors haven't repaid yet, at
    /// the bills' face amounts.
    pub outstanding: u64,
    /// Open listings not yet claimed, at their amounts.
    pub listed: u64,
    /// Room for fields a later layout adds.
    pub reserved: [u8; 128],
}

impl Pool {
    /// What the pool holds: idle plus what anchors owe on paid bills.
    pub fn nav(&self) -> Result<u64> {
        self.idle
            .checked_add(self.outstanding)
            .ok_or_else(|| error!(SupplierPoolError::MathOverflow))
    }

    pub fn is_signer(&self, key: &Pubkey) -> bool {
        self.signers.contains(key)
    }

    pub fn takes_anchor(&self, wallet: &Pubkey) -> bool {
        self.anchors.is_empty() || self.anchors.contains(wallet)
    }

    /// The shares a deposit of `amount` buys, given the share mint's
    /// `supply`.
    pub fn shares_for_deposit(&self, amount: u64, supply: u64) -> Result<u64> {
        mul_div(
            amount,
            supply.saturating_add(VIRTUAL_SHARES),
            self.nav()?.saturating_add(VIRTUAL_ASSETS),
        )
    }

    /// What `shares` of `supply` are worth now.
    pub fn amount_for_shares(&self, shares: u64, supply: u64) -> Result<u64> {
        mul_div(
            shares,
            self.nav()?.saturating_add(VIRTUAL_ASSETS),
            supply.saturating_add(VIRTUAL_SHARES),
        )
    }

    /// A bill leaf's amount, in millionths of the currency, in the token's
    /// base units, or an error if it isn't a whole number of them.
    pub fn base_units(&self, millionths: u64) -> Result<u64> {
        if self.token_decimals >= BILL_AMOUNT_DECIMALS {
            let scale = 10u64.pow((self.token_decimals - BILL_AMOUNT_DECIMALS) as u32);
            return millionths
                .checked_mul(scale)
                .ok_or_else(|| error!(SupplierPoolError::MathOverflow));
        }
        let scale = 10u64.pow((BILL_AMOUNT_DECIMALS - self.token_decimals) as u32);
        require!(
            millionths % scale == 0,
            SupplierPoolError::AmountNotRepresentable
        );
        Ok(millionths / scale)
    }
}

/// What one anchor's wallet owes a pool, at `[ANCHOR_SEED, pool, wallet]`.
#[account]
#[derive(InitSpace)]
pub struct AnchorAccount {
    pub version: u8,
    pub bump: u8,
    pub pool: Pubkey,
    pub wallet: Pubkey,
    /// Its open listings' amounts, claimed or not.
    pub owed: u64,
}

/// One listed bill, at `[LISTING_SEED, pool, anchor wallet, commitment]`.
#[account]
#[derive(InitSpace)]
pub struct Listing {
    pub version: u8,
    pub bump: u8,
    pub pool: Pubkey,
    /// The anchor's wallet: it alone cancels or repays.
    pub anchor: Pubkey,
    /// `sha256(supplier's public key || salt)`.
    pub commitment: [u8; 32],
    /// The bill's amount, in the token's base units.
    pub amount: u64,
    /// When the anchor repays the pool: the bill leaf's `repayOn`.
    pub due_date: i64,
    /// Lamports above the account's rent that pay a claim's costs; what a
    /// claim doesn't take goes back to the anchor when the listing closes.
    pub fee_deposit: u64,
    pub claimed: bool,
    /// Set by the claim.
    pub supplier: Pubkey,
    pub claimed_at: i64,
    /// What the supplier was paid: the amount less the fee.
    pub paid: u64,
}

/// That a commitment was claimed, at `[CLAIMED_SEED, commitment]`: one per
/// program, so a bill's leaf pays its supplier once, whichever pool or
/// wallet lists it and however often. A claim reveals the salt, after which
/// anyone could list the leaf again and replay the supplier's claim.
#[account]
#[derive(InitSpace)]
pub struct ClaimRecord {
    pub version: u8,
    pub bump: u8,
    /// The listing the claim paid.
    pub listing: Pubkey,
    pub claimed_at: i64,
}

/// `a * b / c`, rounded down.
pub fn mul_div(a: u64, b: u64, c: u64) -> Result<u64> {
    require!(c > 0, SupplierPoolError::MathOverflow);
    u64::try_from((a as u128) * (b as u128) / (c as u128))
        .map_err(|_| error!(SupplierPoolError::MathOverflow))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flat_fee_ignores_days() {
        let fee = FeeSchedule::Flat { bps: 150 };
        assert_eq!(fee.bps_for(1), 150);
        assert_eq!(fee.bps_for(90), 150);
    }

    #[test]
    fn test_per_day_fee_grows_to_its_max() {
        let fee = FeeSchedule::PerDay {
            base_bps: 50,
            bps_per_30_days: 60,
            max_bps: 200,
        };
        assert_eq!(fee.bps_for(0), 50);
        assert_eq!(fee.bps_for(15), 80);
        assert_eq!(fee.bps_for(60), 170);
        assert_eq!(fee.bps_for(365), 200);
    }

    fn pool(decimals: u8) -> Pool {
        Pool {
            version: 1,
            bump: 0,
            creator: Pubkey::default(),
            mint: Pubkey::default(),
            token_decimals: decimals,
            share_mint: Pubkey::default(),
            network: [0; 16],
            currency: [0; 16],
            signers: vec![],
            anchors: vec![],
            fee: FeeSchedule::Flat { bps: 0 },
            rules: BillRules {
                required_documents: 0,
                min_vendor_tenure: 0,
                min_vendor_volume: 0,
                max_term_days: 1,
            },
            max_root_age: 1,
            max_owed_per_anchor: u64::MAX,
            idle: 0,
            outstanding: 0,
            listed: 0,
            reserved: [0; 128],
        }
    }

    #[test]
    fn test_base_units_scale_by_decimals() {
        assert_eq!(pool(6).base_units(1_250_000).unwrap(), 1_250_000);
        assert_eq!(pool(9).base_units(1_250_000).unwrap(), 1_250_000_000);
        assert_eq!(pool(2).base_units(1_250_000).unwrap(), 125);
        assert!(pool(2).base_units(1_250_001).is_err());
    }

    #[test]
    fn test_first_deposit_buys_at_the_virtual_price() {
        let p = pool(6);
        assert_eq!(p.shares_for_deposit(1_000_000, 0).unwrap(), 1_000_000_000);
    }
}
