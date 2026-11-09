//! How a pool judges a token from its snapshot: whether to lend, how much
//! to advance on the units pledged, at what rate, and whether a loan
//! already made has gone bad. Every weight is the pool's, fixed when it was
//! created (`RiskParams`, listed with what each decides in `pools/*.toml`).
//! This is `RiskModel::V1`; a later model is added beside it, never in
//! place of it, so a pool keeps the rules it was created with.
//!
//! The protection behind a unit is the token's: the collateral agent
//! enforces each tokenized invoice at face value for every holder.
//! What a pool risks is the shortfall on the way there (disputes and credits
//! that shrink an invoice, customers who never pay, the agent's costs and
//! time), which the snapshot's aging, disputed share and concentration
//! predict. So the advance and the rate are set from them.

use anchor_lang::prelude::*;

use crate::{constants::*, error::PoolError, math::*, snapshot::Snapshot, state::RiskParams};

/// What the engine allows a borrower pledging some units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BorrowTerms {
    /// The most the units can borrow, in the quote token's base units.
    pub max_principal: u64,
    /// The yearly rate, fixed on the loan when it opens.
    pub rate_bps: u16,
    /// The share of the token's units that counts, after aging and disputes.
    pub eligible_bps: u16,
    /// The share of eligible face value advanced.
    pub advance_bps: u16,
}

/// What a mark makes of a loan from its token's latest snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkOutcome {
    Performing,
    /// Written down: NAV counts the loan at the pool's impaired share.
    WrittenDown,
}

/// The terms for borrowing against units whose face value is `face_value`,
/// or why the token can't be borrowed against.
///
/// - Refuses while the token's burn debt is overdue, when more units than
///   `max_late_share_bps` are past `late_after_days`, and when the term
///   (`term_seconds`) ends before the invoices are expected to be collected:
///   their days to due plus the days their customers pay beyond terms.
/// - Eligible share: units backed by invoices current or 1–30 days past
///   due count in full, 31–60 days at `weight_31_to_60_bps`, older ones not
///   at all, all over outstanding units, less the disputed share.
/// - Advance: `base_advance_bps`, less up to `concentration_penalty_bps` as
///   concentration rises from `concentration_threshold_bps` to 10000, and
///   at most `max_advance_bps`.
/// - Rate: `base_rate_bps`, plus each spread in proportion to what it
///   prices: units past due; concentration above the threshold; disputed
///   units; and the customers' payment history, priced on the units owed by
///   customers who have one (days beyond terms up to a cap, volume not paid
///   on time), with `no_history_spread_bps` on the rest. Kept within
///   `min_rate_bps` and `max_rate_bps`.
pub fn assess_borrow(
    p: &RiskParams,
    s: &Snapshot,
    face_value: u128,
    term_seconds: u64,
) -> Result<BorrowTerms> {
    require!(!s.burn_debt_overdue, PoolError::BurnDebtOverdue);
    require!(
        late_share_bps(p, s)? <= p.max_late_share_bps,
        PoolError::TooMuchLate
    );
    let expected_days = s.weighted_days_to_due as u64 + s.weighted_days_beyond_terms as u64;
    require!(
        term_seconds >= expected_days * SECONDS_PER_DAY,
        PoolError::TermTooShort
    );
    let eligible_bps = eligible_bps(p, s)?;
    require!(eligible_bps > 0, PoolError::NoEligibleUnits);
    let advance_bps = advance_bps(p, s)?;
    let eligible_value = mul_div(face_value, eligible_bps as u128, BPS as u128)?;
    let max_principal = to_u64(mul_div(eligible_value, advance_bps as u128, BPS as u128)?)?;
    Ok(BorrowTerms {
        max_principal,
        rate_bps: rate_bps(p, s)?,
        eligible_bps,
        advance_bps,
    })
}

/// Whether a loan stays performing given its token's latest snapshot.
pub fn assess_mark(s: &Snapshot) -> MarkOutcome {
    match s.burn_debt_overdue {
        true => MarkOutcome::WrittenDown,
        false => MarkOutcome::Performing,
    }
}

fn eligible_bps(p: &RiskParams, s: &Snapshot) -> Result<u16> {
    if s.outstanding_units == 0 {
        return Ok(0);
    }
    let weighted_31_to_60 = mul_div(
        s.aging_days_31_to_60 as u128,
        p.weight_31_to_60_bps as u128,
        BPS as u128,
    )?;
    let eligible_units = add(
        add(s.aging_current as u128, s.aging_days_1_to_30 as u128)?,
        weighted_31_to_60,
    )?;
    let share = share_bps(eligible_units, s.outstanding_units as u128)?;
    Ok(share.saturating_sub(s.disputed_share_bps.min(BPS as u16)))
}

/// The share of units past `late_after_days`.
fn late_share_bps(p: &RiskParams, s: &Snapshot) -> Result<u16> {
    let late_units = match p.late_after_days {
        30 => {
            s.aging_days_31_to_60 as u128 + s.aging_days_61_to_90 as u128 + s.aging_over_90 as u128
        }
        60 => s.aging_days_61_to_90 as u128 + s.aging_over_90 as u128,
        // create_pool allows only 30, 60 and 90.
        _ => s.aging_over_90 as u128,
    };
    match s.outstanding_units {
        0 => Ok(0),
        outstanding => share_bps(late_units, outstanding as u128),
    }
}

fn advance_bps(p: &RiskParams, s: &Snapshot) -> Result<u16> {
    let penalty = above_threshold(
        p.concentration_penalty_bps,
        s.concentration_index_bps,
        p.concentration_threshold_bps,
    )?;
    Ok(p.base_advance_bps
        .saturating_sub(penalty)
        .min(p.max_advance_bps))
}

fn rate_bps(p: &RiskParams, s: &Snapshot) -> Result<u16> {
    let past_due_units = [
        s.aging_days_1_to_30,
        s.aging_days_31_to_60,
        s.aging_days_61_to_90,
        s.aging_over_90,
    ]
    .iter()
    .try_fold(0u128, |sum, u| add(sum, *u as u128))?;
    let past_due_bps = match s.outstanding_units {
        0 => BPS as u16,
        outstanding => share_bps(past_due_units, outstanding as u128)?,
    };
    let past_due_spread = proportion(p.past_due_spread_bps, past_due_bps)?;
    let concentration_spread = above_threshold(
        p.concentration_spread_bps,
        s.concentration_index_bps,
        p.concentration_threshold_bps,
    )?;
    let dispute_spread = proportion(p.dispute_spread_bps, s.disputed_share_bps)?;
    let rate = p.base_rate_bps as u128
        + past_due_spread as u128
        + concentration_spread as u128
        + dispute_spread as u128
        + history_spread_bps(p, s)? as u128;
    // create_pool keeps min <= max.
    let capped = rate.min(p.max_rate_bps as u128) as u16;
    Ok(capped.max(p.min_rate_bps))
}

/// What the customers' payment history adds to the rate: on the share of
/// units whose customers have a history, the beyond-terms and not-on-time
/// spreads; on the rest, the no-history spread.
fn history_spread_bps(p: &RiskParams, s: &Snapshot) -> Result<u16> {
    let history = s.history_share_bps.min(BPS as u16);
    let beyond_terms_bps = share_bps(
        s.weighted_days_beyond_terms as u128,
        p.beyond_terms_cap_days as u128,
    )?;
    let beyond_terms = proportion(p.beyond_terms_spread_bps, beyond_terms_bps)?;
    let not_on_time = proportion(
        p.not_on_time_spread_bps,
        BPS as u16 - s.paid_on_time_share_bps.min(BPS as u16),
    )?;
    let with_history = mul_div(
        beyond_terms as u128 + not_on_time as u128,
        history as u128,
        BPS as u128,
    )?;
    let without_history = proportion(p.no_history_spread_bps, BPS as u16 - history)?;
    Ok((with_history + without_history as u128).min(u16::MAX as u128) as u16)
}

/// `part / whole` in bps, at most 10000.
fn share_bps(part: u128, whole: u128) -> Result<u16> {
    Ok(mul_div(part, BPS as u128, whole)?.min(BPS as u128) as u16)
}

/// `weight` scaled by `share_bps`, a share in bps at most 10000.
fn proportion(weight: u16, share_bps: u16) -> Result<u16> {
    let share = share_bps.min(BPS as u16) as u128;
    Ok(mul_div(weight as u128, share, BPS as u128)? as u16)
}

/// `weight` scaled by how far `value` is from `threshold` towards 10000:
/// 0 at or below the threshold, all of it at 10000.
fn above_threshold(weight: u16, value: u16, threshold: u16) -> Result<u16> {
    let value = value.min(BPS as u16);
    if value <= threshold {
        return Ok(0);
    }
    let range = BPS as u128 - threshold as u128;
    Ok(mul_div(weight as u128, (value - threshold) as u128, range)? as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> RiskParams {
        RiskParams {
            base_advance_bps: 8_000,
            max_advance_bps: 9_000,
            weight_31_to_60_bps: 5_000,
            concentration_threshold_bps: 2_500,
            concentration_penalty_bps: 2_000,
            base_rate_bps: 800,
            min_rate_bps: 500,
            max_rate_bps: 1_500,
            past_due_spread_bps: 600,
            concentration_spread_bps: 400,
            dispute_spread_bps: 1_000,
            late_after_days: 90,
            max_late_share_bps: 5_000,
            beyond_terms_spread_bps: 300,
            beyond_terms_cap_days: 60,
            not_on_time_spread_bps: 400,
            no_history_spread_bps: 200,
        }
    }

    const TERM: u64 = 28 * SECONDS_PER_DAY;

    /// 1000 units, all backed by invoices not yet due in 20 days, with no
    /// disputes, concentration under the threshold, and customers who all
    /// have a history of paying on time.
    fn clean() -> Snapshot {
        Snapshot {
            version: 1,
            outstanding_units: 1_000,
            aging_current: 1_000,
            concentration_index_bps: 2_000,
            weighted_days_to_due: 20,
            weighted_days_to_pay: 25,
            paid_on_time_share_bps: 10_000,
            history_share_bps: 10_000,
            ..Snapshot::default()
        }
    }

    #[test]
    fn test_a_clean_token_gets_the_base_advance_and_rate() {
        let t = assess_borrow(&params(), &clean(), 1_000_000_000, TERM).unwrap();
        assert_eq!(
            t,
            BorrowTerms {
                max_principal: 800_000_000,
                rate_bps: 800,
                eligible_bps: 10_000,
                advance_bps: 8_000,
            }
        );
    }

    #[test]
    fn test_aging_and_disputes_shrink_the_eligible_share() {
        let mut s = clean();
        // 600 current, 200 at 1–30 days, 100 at 31–60 (counted half),
        // 100 over 90 days (not counted), and 5% disputed.
        s.aging_current = 600;
        s.aging_days_1_to_30 = 200;
        s.aging_days_31_to_60 = 100;
        s.aging_over_90 = 100;
        s.disputed_share_bps = 500;
        let t = assess_borrow(&params(), &s, 1_000_000_000, TERM).unwrap();
        assert_eq!(t.eligible_bps, 8_500 - 500);
        assert_eq!(t.max_principal, 640_000_000);
        // 40% past due prices 240 bps, the 5% disputed 50 bps.
        assert_eq!(t.rate_bps, 800 + 240 + 50);
    }

    #[test]
    fn test_concentration_lowers_the_advance_and_raises_the_rate() {
        let mut s = clean();
        // Halfway from the threshold (2500) to 10000.
        s.concentration_index_bps = 6_250;
        let t = assess_borrow(&params(), &s, 1_000_000_000, TERM).unwrap();
        assert_eq!(t.advance_bps, 8_000 - 1_000);
        assert_eq!(t.rate_bps, 800 + 200);
    }

    #[test]
    fn test_the_rate_stays_within_its_bounds() {
        let mut s = clean();
        s.aging_current = 0;
        s.aging_days_1_to_30 = 1_000;
        s.disputed_share_bps = 10_000;
        let mut p = params();
        p.dispute_spread_bps = 9_000;
        assert_eq!(rate_bps(&p, &s).unwrap(), 1_500);
        p.base_rate_bps = 100;
        p.past_due_spread_bps = 0;
        p.dispute_spread_bps = 0;
        assert_eq!(rate_bps(&p, &s).unwrap(), 500);
    }

    #[test]
    fn test_spreads_past_u16_still_cap_at_the_max_rate() {
        let mut s = clean();
        s.aging_current = 0;
        s.aging_over_90 = 1_000;
        s.disputed_share_bps = 10_000;
        s.concentration_index_bps = 10_000;
        let mut p = params();
        p.base_rate_bps = u16::MAX;
        p.past_due_spread_bps = u16::MAX;
        p.max_rate_bps = 1_500;
        assert_eq!(rate_bps(&p, &s).unwrap(), 1_500);
    }

    #[test]
    fn test_a_pool_may_advance_at_face() {
        let mut p = params();
        p.base_advance_bps = 10_000;
        p.max_advance_bps = 10_000;
        let t = assess_borrow(&p, &clean(), 1_000_000_000, TERM).unwrap();
        assert_eq!(t.max_principal, 1_000_000_000);
    }

    #[test]
    fn test_overdue_burn_debt_refuses_and_writes_down() {
        let mut s = clean();
        s.burn_debt_overdue = true;
        assert!(assess_borrow(&params(), &s, 1_000_000_000, TERM).is_err());
        assert_eq!(assess_mark(&s), MarkOutcome::WrittenDown);
        assert_eq!(assess_mark(&clean()), MarkOutcome::Performing);
    }

    #[test]
    fn test_too_much_late_is_refused() {
        let mut s = clean();
        s.aging_current = 490;
        s.aging_over_90 = 510;
        assert!(assess_borrow(&params(), &s, 1_000_000_000, TERM).is_err());
        s.aging_current = 500;
        s.aging_over_90 = 500;
        assert!(assess_borrow(&params(), &s, 1_000_000_000, TERM).is_ok());
        // Counted past 60 days, the same token is refused at 50% too.
        let mut p = params();
        p.late_after_days = 60;
        p.max_late_share_bps = 4_000;
        assert!(assess_borrow(&p, &s, 1_000_000_000, TERM).is_err());
    }

    #[test]
    fn test_a_term_ending_before_collection_is_refused() {
        let mut s = clean();
        // Due in 20 days, paid 10 days late: collected in about 30.
        s.weighted_days_beyond_terms = 10;
        assert!(assess_borrow(&params(), &s, 1_000_000_000, TERM).is_err());
        assert!(assess_borrow(&params(), &s, 1_000_000_000, 30 * SECONDS_PER_DAY).is_ok());
    }

    #[test]
    fn test_payment_history_prices_the_rate() {
        let mut s = clean();
        // Paid 30 days beyond terms (half the cap) and 60% on time.
        s.weighted_days_beyond_terms = 30;
        s.paid_on_time_share_bps = 6_000;
        let rate = assess_borrow(&params(), &s, 1_000_000_000, 60 * SECONDS_PER_DAY)
            .unwrap()
            .rate_bps;
        assert_eq!(rate, 800 + 150 + 160);
        // Only half the units' customers have a history: half the history
        // spreads, and half the no-history spread.
        s.history_share_bps = 5_000;
        let rate = assess_borrow(&params(), &s, 1_000_000_000, 60 * SECONDS_PER_DAY)
            .unwrap()
            .rate_bps;
        assert_eq!(rate, 800 + 75 + 80 + 100);
        // Before a signer can vouch history, every unit has none.
        let mut none = clean();
        none.history_share_bps = 0;
        none.paid_on_time_share_bps = 0;
        let rate = assess_borrow(&params(), &none, 1_000_000_000, TERM)
            .unwrap()
            .rate_bps;
        assert_eq!(rate, 800 + 200);
    }

    #[test]
    fn test_a_token_with_nothing_eligible_is_refused() {
        let mut s = clean();
        s.aging_current = 0;
        s.aging_over_90 = 1_000;
        assert!(assess_borrow(&params(), &s, 1_000_000_000, TERM).is_err());
        let mut empty = clean();
        empty.outstanding_units = 0;
        assert!(assess_borrow(&params(), &empty, 1_000_000_000, TERM).is_err());
    }
}
