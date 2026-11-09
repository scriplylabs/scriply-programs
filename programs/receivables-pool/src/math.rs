//! Pure arithmetic: interest, and checked operations that fail with
//! `MathOverflow` instead of wrapping.

use anchor_lang::prelude::*;

use crate::{constants::*, error::PoolError};

pub fn add(a: u128, b: u128) -> Result<u128> {
    a.checked_add(b)
        .ok_or_else(|| error!(PoolError::MathOverflow))
}

pub fn sub(a: u128, b: u128) -> Result<u128> {
    a.checked_sub(b)
        .ok_or_else(|| error!(PoolError::MathOverflow))
}

pub fn mul(a: u128, b: u128) -> Result<u128> {
    a.checked_mul(b)
        .ok_or_else(|| error!(PoolError::MathOverflow))
}

/// a × b / c, rounded down; fails if c is 0 or the result overflows.
pub fn mul_div(a: u128, b: u128, c: u128) -> Result<u128> {
    mul(a, b)?
        .checked_div(c)
        .ok_or_else(|| error!(PoolError::MathOverflow))
}

pub fn to_u64(v: u128) -> Result<u64> {
    u64::try_from(v).map_err(|_| error!(PoolError::MathOverflow))
}

/// Unix seconds as u128, for the sums in a maturity bucket. Times before
/// 1970 never occur on chain.
pub fn time(t: i64) -> u128 {
    t.max(0) as u128
}

/// The terms of a group of loans that share a maturity, summed: Σp, Σp·r,
/// Σp·r·min(s,m), Σp·l, Σp·l·max(s,m) and the interest carried, where each
/// loan has its own rate r and past-maturity rate l. One loan is a group of
/// one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Accrual {
    pub principal: u128,
    pub principal_rate: u128,
    pub principal_rate_start: u128,
    pub principal_late_rate: u128,
    pub principal_late_rate_start: u128,
    pub carried_interest: u128,
}

/// A loan's terms, for `Accrual::of_loan`.
#[derive(Clone, Copy, Debug)]
pub struct LoanTerms {
    pub principal: u64,
    pub rate_bps: u16,
    pub late_rate_bps: u16,
    /// When interest on `principal` started accruing.
    pub start: i64,
    pub maturity: i64,
    /// Interest unpaid from before `start`.
    pub carried: u64,
}

impl Accrual {
    pub fn of_loan(t: LoanTerms) -> Result<Self> {
        let p = t.principal as u128;
        let pr = mul(p, t.rate_bps as u128)?;
        let pl = mul(p, t.late_rate_bps as u128)?;
        Ok(Self {
            principal: p,
            principal_rate: pr,
            principal_rate_start: mul(pr, time(t.start.min(t.maturity)))?,
            principal_late_rate: pl,
            principal_late_rate_start: mul(pl, time(t.start.max(t.maturity)))?,
            carried_interest: t.carried as u128,
        })
    }

    /// The simple interest accrued from each loan's start to `now`, at its
    /// rate until `maturity` and its past-maturity rate after, rounded down.
    pub fn interest_to(&self, now: i64, maturity: i64) -> Result<u128> {
        let before = sub(
            mul(self.principal_rate, time(now.min(maturity)))?,
            self.principal_rate_start,
        )?;
        let after = sub(
            mul(self.principal_late_rate, time(now.max(maturity)))?,
            self.principal_late_rate_start,
        )?;
        Ok(add(before, after)? / (BPS as u128 * SECONDS_PER_YEAR as u128))
    }

    /// Principal, carried interest and interest accrued to `now`.
    pub fn owed_at(&self, now: i64, maturity: i64) -> Result<u128> {
        add(
            add(self.principal, self.carried_interest)?,
            self.interest_to(now, maturity)?,
        )
    }

    /// What was owed at maturity: principal, carried interest and interest
    /// at each loan's rate up to maturity. Interest a loan carries from a
    /// payment after maturity counts too, since it is part of what is owed.
    pub fn owed_at_maturity(&self, maturity: i64) -> Result<u128> {
        let before = sub(
            mul(self.principal_rate, time(maturity))?,
            self.principal_rate_start,
        )?;
        let interest = before / (BPS as u128 * SECONDS_PER_YEAR as u128);
        add(add(self.principal, self.carried_interest)?, interest)
    }

    pub fn checked_add(&self, other: &Self) -> Result<Self> {
        Ok(Self {
            principal: add(self.principal, other.principal)?,
            principal_rate: add(self.principal_rate, other.principal_rate)?,
            principal_rate_start: add(self.principal_rate_start, other.principal_rate_start)?,
            principal_late_rate: add(self.principal_late_rate, other.principal_late_rate)?,
            principal_late_rate_start: add(
                self.principal_late_rate_start,
                other.principal_late_rate_start,
            )?,
            carried_interest: add(self.carried_interest, other.carried_interest)?,
        })
    }

    pub fn checked_sub(&self, other: &Self) -> Result<Self> {
        Ok(Self {
            principal: sub(self.principal, other.principal)?,
            principal_rate: sub(self.principal_rate, other.principal_rate)?,
            principal_rate_start: sub(self.principal_rate_start, other.principal_rate_start)?,
            principal_late_rate: sub(self.principal_late_rate, other.principal_late_rate)?,
            principal_late_rate_start: sub(
                self.principal_late_rate_start,
                other.principal_late_rate_start,
            )?,
            carried_interest: sub(self.carried_interest, other.carried_interest)?,
        })
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The face value of `units` of a receivables token in the quote token's
/// base units: a unit is one dollar, so only the decimals differ.
pub fn face_value(units: u64, collateral_decimals: u8, quote_decimals: u8) -> Result<u128> {
    mul_div(
        units as u128,
        10u128.pow(quote_decimals as u32),
        10u128.pow(collateral_decimals as u32),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;
    const T0: i64 = 1_800_000_000;

    fn terms(principal: u64, rate: u16, late: u16, start: i64, maturity: i64) -> LoanTerms {
        LoanTerms {
            principal,
            rate_bps: rate,
            late_rate_bps: late,
            start,
            maturity,
            carried: 0,
        }
    }

    #[test]
    fn test_interest_matches_adr_example() {
        // 500 USDC at 12% a year owes 514.79 after 90 days.
        let m = T0 + 90 * DAY;
        let loan = Accrual::of_loan(terms(500_000_000, 1_200, 1_200, T0, m)).unwrap();
        assert_eq!(loan.owed_at(m, m).unwrap(), 514_794_520);
    }

    #[test]
    fn test_late_rate_applies_after_maturity() {
        let m = T0 + 365 * DAY;
        let loan = Accrual::of_loan(terms(1_000_000_000, 1_000, 2_000, T0, m)).unwrap();
        // A year at 10%, then half a year at 20%: 100 + 100.
        let interest = loan.interest_to(m + 365 * DAY / 2, m).unwrap();
        assert_eq!(interest, 200_000_000);
        assert_eq!(loan.owed_at_maturity(m).unwrap(), 1_100_000_000);
    }

    #[test]
    fn test_a_bucket_owes_what_its_loans_owe_at_their_own_rates() {
        let m = T0 + 60 * DAY;
        let a = Accrual::of_loan(terms(300_000_000, 600, 900, T0, m)).unwrap();
        let mut tb = terms(700_000_000, 1_400, 1_900, T0 + 10 * DAY, m);
        tb.carried = 5_000;
        let b = Accrual::of_loan(tb).unwrap();
        let bucket = a.checked_add(&b).unwrap();
        let now = T0 + 70 * DAY;
        let each = a.owed_at(now, m).unwrap() + b.owed_at(now, m).unwrap();
        let summed = bucket.owed_at(now, m).unwrap();
        // Rounding down once instead of per loan can only add a base unit per loan.
        assert!(summed >= each && summed - each <= 1);
        assert!(bucket
            .checked_sub(&a)
            .unwrap()
            .checked_sub(&b)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_face_value_converts_decimals() {
        assert_eq!(face_value(1_000_000, 6, 6).unwrap(), 1_000_000);
        assert_eq!(face_value(1_000_000_000, 9, 6).unwrap(), 1_000_000);
    }
}
