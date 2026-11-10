//! A loan's rules: what it owes, how a payment is set against it, and how
//! a mark from its token's facts changes what NAV counts for it.

use anchor_lang::prelude::*;

use crate::{constants::*, error::PoolError, math::*, state::*};

/// How a payment was set against a loan.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Payment {
    /// What went to the loan's interest and principal, and so to the pool.
    pub applied: u64,
    /// What was left once nothing was owed, which is the borrower's.
    pub excess: u64,
    /// The part of `applied` that repaid principal.
    pub principal_repaid: u64,
}

/// What a loan is opened with, once the risk engine has decided.
#[derive(Clone, Copy, Debug)]
pub struct Opening {
    pub principal: u64,
    pub rate_bps: u16,
    pub maturity_epoch: u64,
    /// The `as_of` of the root the token was read from.
    pub root_as_of: i64,
}

impl Loan {
    pub fn accrual(&self) -> Result<Accrual> {
        Accrual::of_loan(LoanTerms {
            principal: self.principal,
            rate_bps: self.rate_bps,
            late_rate_bps: self.late_rate_bps,
            start: self.accrual_start,
            maturity: self.maturity_time,
            carried: self.carried_interest,
        })
    }

    /// Principal and interest owed at `now`.
    pub fn owed(&self, now: i64) -> Result<u64> {
        to_u64(self.accrual()?.owed_at(now, self.maturity_time)?)
    }

    pub fn is_repaid(&self) -> bool {
        self.principal == 0 && self.carried_interest == 0
    }

    /// Whether NAV counts the loan as impaired: written down by a mark, or
    /// past maturity and grace.
    pub fn is_impaired(&self, pool: &Pool) -> bool {
        self.is_written_down || pool.is_impaired(self.maturity_epoch)
    }

    /// Opens the loan at `now` against the units pledged, on the terms the
    /// risk engine set. The token was just read, so the loan counts as
    /// marked this epoch.
    pub fn open(&mut self, pool: &mut Pool, o: Opening, now: i64) -> Result<()> {
        require!(!self.is_open, PoolError::LoanOpen);
        self.is_open = true;
        self.principal = o.principal;
        self.carried_interest = 0;
        self.accrual_start = now;
        self.maturity_epoch = o.maturity_epoch;
        self.maturity_time = pool.epoch_start(o.maturity_epoch);
        self.rate_bps = o.rate_bps;
        self.late_rate_bps = o.rate_bps.saturating_add(pool.late_spread_bps);
        self.is_written_down = false;
        self.impaired_value = 0;
        self.is_impaired_value_set = false;
        self.marked_epoch = pool.settled_epoch;
        self.last_root_as_of = o.root_as_of;
        self.stale_discount = 0;
        pool.add_accrual(o.maturity_epoch, &self.accrual()?)?;
        pool.idle = pool
            .idle
            .checked_sub(o.principal)
            .ok_or(PoolError::InsufficientLiquidity)?;
        pool.add_performing_loan(o.maturity_epoch);
        Ok(())
    }

    /// Sets `amount` against the loan at `now`: its interest first, then its
    /// principal; anything over is the borrower's. A closed loan owes
    /// nothing, so all of it is.
    pub fn apply_payment(&mut self, pool: &mut Pool, amount: u64, now: i64) -> Result<Payment> {
        if !self.is_open {
            return Ok(Payment {
                applied: 0,
                excess: amount,
                principal_repaid: 0,
            });
        }
        self.drop_old_stale(pool);
        let is_impaired = self.is_impaired(pool);
        if is_impaired {
            self.set_impaired_value(pool)?;
        }
        let before = self.accrual()?;
        let interest_due = to_u64(add(
            before.carried_interest,
            before.interest_to(now, self.maturity_time)?,
        )?)?;
        let to_interest = amount.min(interest_due);
        let to_principal = (amount - to_interest).min(self.principal);
        let applied = to_interest + to_principal;

        self.carried_interest = interest_due - to_interest;
        self.principal -= to_principal;
        self.accrual_start = now;
        if is_impaired {
            // What NAV counted for the loan becomes the USDC paid, up to it.
            let counted = to_u64(mul_div(
                applied as u128,
                pool.impaired_share_bps as u128,
                BPS as u128,
            )?)?
            .min(self.impaired_value);
            self.impaired_value -= counted;
            pool.impaired_value = pool.impaired_value.saturating_sub(counted);
        } else {
            pool.remove_accrual(self.maturity_epoch, &before)?;
            pool.add_accrual(self.maturity_epoch, &self.accrual()?)?;
            self.shrink_stale(pool, now)?;
        }
        pool.idle = to_u64(add(pool.idle as u128, applied as u128)?)?;
        Ok(Payment {
            applied,
            excess: amount - applied,
            principal_repaid: to_principal,
        })
    }

    /// Closes a repaid loan, so its borrower can borrow against the token
    /// again.
    pub fn close_repaid(&mut self, pool: &mut Pool) -> Result<()> {
        require!(self.is_open, PoolError::LoanClosed);
        require!(self.is_repaid(), PoolError::LoanOpen);
        self.release_stale(pool);
        if self.is_impaired(pool) {
            self.set_impaired_value(pool)?;
            pool.impaired_value = pool.impaired_value.saturating_sub(self.impaired_value);
        } else {
            pool.remove_performing_loan(self.maturity_epoch, self.is_marked(pool));
        }
        self.is_open = false;
        self.is_written_down = false;
        self.impaired_value = 0;
        self.is_impaired_value_set = false;
        Ok(())
    }

    /// Marks the loan from its token's facts, read from a root with
    /// `root_as_of`: if the facts are bad, writes the loan down at `now`.
    /// A stale discount set this epoch stays until the next epoch's start,
    /// so nobody can deposit at the discounted NAV and then lift it.
    /// Returns whether this was the loan's first mark of the epoch.
    pub fn mark(
        &mut self,
        pool: &mut Pool,
        root_as_of: i64,
        is_written_down: bool,
        now: i64,
    ) -> Result<bool> {
        require!(self.is_open, PoolError::NotALoan);
        require!(
            root_as_of >= self.last_root_as_of,
            PoolError::RootOlderThanLastRead
        );
        self.last_root_as_of = root_as_of;
        self.drop_old_stale(pool);
        if is_written_down {
            self.write_down(pool, now)?;
        }
        Ok(self.stamp(pool))
    }

    /// Marks the loan stale: its token couldn't be read this epoch, so NAV
    /// counts it at the pool's impaired share, as if written down, for the
    /// rest of the epoch. A payment only keeps the discount within what the
    /// loan still owes. A loan already impaired counts no
    /// lower. Returns whether this was the loan's first mark of the epoch.
    pub fn mark_stale(&mut self, pool: &mut Pool, now: i64) -> Result<bool> {
        require!(self.is_open, PoolError::NotALoan);
        require!(!self.is_marked(pool), PoolError::AlreadyMarked);
        self.drop_old_stale(pool);
        if !self.is_impaired(pool) {
            let owed = self.owed(now)? as u128;
            let kept = mul_div(owed, pool.impaired_share_bps as u128, BPS as u128)?;
            self.stale_discount = to_u64(owed - kept)?;
            pool.stale_discount = to_u64(add(
                pool.stale_discount as u128,
                self.stale_discount as u128,
            )?)?;
        }
        Ok(self.stamp(pool))
    }

    /// Records the loan as marked in the settled epoch; true the first time.
    /// Only a performing loan counts towards the epoch's marking.
    fn stamp(&mut self, pool: &mut Pool) -> bool {
        if self.is_marked(pool) {
            return false;
        }
        self.marked_epoch = pool.settled_epoch;
        if !self.is_impaired(pool) {
            pool.marked_loans += 1;
        }
        true
    }

    fn is_marked(&self, pool: &Pool) -> bool {
        self.marked_epoch == pool.settled_epoch
    }

    /// Whether the loan's stale discount is in the pool's: it was set this
    /// epoch. An epoch's start resets the pool's, so an older one isn't.
    fn is_stale(&self, pool: &Pool) -> bool {
        self.stale_discount > 0 && self.is_marked(pool)
    }

    /// Forgets a stale discount from an earlier epoch, which the pool's
    /// total no longer holds.
    fn drop_old_stale(&mut self, pool: &Pool) {
        if !self.is_stale(pool) {
            self.stale_discount = 0;
        }
    }

    /// Takes the loan's stale discount out of NAV's, once something else
    /// counts the loan: a write-down, or closing it repaid.
    fn release_stale(&mut self, pool: &mut Pool) {
        if self.is_stale(pool) {
            pool.stale_discount = pool.stale_discount.saturating_sub(self.stale_discount);
        }
        self.stale_discount = 0;
    }

    /// Keeps a stale discount within what the loan, paid down at `now`,
    /// still owes, so a payment's USDC isn't discounted again.
    fn shrink_stale(&mut self, pool: &mut Pool, now: i64) -> Result<()> {
        if !self.is_stale(pool) {
            return Ok(());
        }
        let owed = self.owed(now)? as u128;
        let kept = mul_div(owed, pool.impaired_share_bps as u128, BPS as u128)?;
        let cap = to_u64(owed - kept)?;
        if cap < self.stale_discount {
            pool.stale_discount = pool
                .stale_discount
                .saturating_sub(self.stale_discount - cap);
            self.stale_discount = cap;
        }
        Ok(())
    }

    /// Writes the loan down at `now`: NAV stops counting its accrual and
    /// counts the pool's impaired share of what it owes now instead. A loan
    /// already impaired stays as it is.
    fn write_down(&mut self, pool: &mut Pool, now: i64) -> Result<()> {
        if self.is_impaired(pool) {
            return Ok(());
        }
        // The write-down counts the loan at the impaired share, as the
        // stale discount did, so the discount goes.
        self.release_stale(pool);
        let accrual = self.accrual()?;
        pool.remove_accrual(self.maturity_epoch, &accrual)?;
        pool.remove_performing_loan(self.maturity_epoch, self.is_marked(pool));
        let owed = accrual.owed_at(now, self.maturity_time)?;
        self.impaired_value = to_u64(mul_div(owed, pool.impaired_share_bps as u128, BPS as u128)?)?;
        self.is_impaired_value_set = true;
        self.is_written_down = true;
        pool.impaired_value = to_u64(add(
            pool.impaired_value as u128,
            self.impaired_value as u128,
        )?)?;
        Ok(())
    }

    /// Works out the loan's part of its bucket's impaired value the first
    /// time it is touched after the bucket was impaired. The loan hasn't
    /// changed since, so this is the same sum the bucket counted.
    fn set_impaired_value(&mut self, pool: &Pool) -> Result<()> {
        if self.is_impaired_value_set {
            return Ok(());
        }
        let owed = self.accrual()?.owed_at_maturity(self.maturity_time)?;
        self.impaired_value = to_u64(mul_div(owed, pool.impaired_share_bps as u128, BPS as u128)?)?;
        self.is_impaired_value_set = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytemuck::Zeroable;

    const DAY: i64 = 86_400;
    const T0: i64 = 1_800_000_000;

    fn pool() -> Box<Pool> {
        let mut p: Box<Pool> = Box::new(Pool::zeroed());
        p.epoch_origin = T0;
        p.epoch_seconds = 7 * DAY as u64;
        p.grace_epochs = 1;
        p.idle = 1_000_000_000;
        p
    }

    fn loan() -> Loan {
        Loan {
            version: ACCOUNT_VERSION,
            pool: Pubkey::default(),
            collateral_mint: Pubkey::default(),
            borrower: Pubkey::default(),
            bump: 0,
            is_open: false,
            units: 0,
            principal: 0,
            carried_interest: 0,
            accrual_start: 0,
            maturity_epoch: 0,
            maturity_time: 0,
            rate_bps: 0,
            late_rate_bps: 0,
            is_written_down: false,
            impaired_value: 0,
            is_impaired_value_set: false,
            marked_epoch: 0,
            last_root_as_of: 0,
            stale_discount: 0,
        }
    }

    fn opening(principal: u64, maturity_epoch: u64) -> Opening {
        Opening {
            principal,
            rate_bps: 1_200,
            maturity_epoch,
            root_as_of: T0,
        }
    }

    fn opened(p: &mut Pool, principal: u64, maturity_epoch: u64) -> Loan {
        let mut l = loan();
        l.open(p, opening(principal, maturity_epoch), T0).unwrap();
        l
    }

    #[test]
    fn test_a_buy_back_repays_the_loan_and_returns_the_rest() {
        // 1000 units bought back at face repay 514.79 and return 485.21.
        let mut p = pool();
        let mut l = opened(&mut p, 500_000_000, 13);
        let payment = l
            .apply_payment(&mut p, 1_000_000_000, T0 + 90 * DAY)
            .unwrap();
        assert_eq!(
            payment,
            Payment {
                applied: 514_794_520,
                excess: 485_205_480,
                principal_repaid: 500_000_000,
            }
        );
        assert!(l.is_repaid());
        l.close_repaid(&mut p).unwrap();
        assert_eq!(p.nav(T0 + 90 * DAY).unwrap(), 1_014_794_520);
        assert!(p.buckets.iter().all(|b| b.accrual().is_empty()));
        assert_eq!((p.performing_loans(), p.marked_loans), (0, 0));
    }

    #[test]
    fn test_a_partial_payment_pays_interest_first() {
        let mut p = pool();
        let mut l = opened(&mut p, 500_000_000, 13);
        let payment = l.apply_payment(&mut p, 100_000_000, T0 + 90 * DAY).unwrap();
        assert_eq!(payment.excess, 0);
        assert_eq!(l.carried_interest, 0);
        assert_eq!(l.principal, 500_000_000 - (100_000_000 - 14_794_520));
        assert_eq!(l.owed(T0 + 90 * DAY).unwrap(), 414_794_520);
    }

    #[test]
    fn test_payments_on_an_impaired_loan_add_to_nav() {
        let mut p = pool();
        let mut l = opened(&mut p, 500_000_000, 2);
        p.open_epoch(3).unwrap();
        let now = p.epoch_start(3);
        assert_eq!(p.nav(now).unwrap(), 500_000_000, "impaired at a share of 0");
        let payment = l.apply_payment(&mut p, 600_000_000, now).unwrap();
        assert!(l.is_repaid());
        l.close_repaid(&mut p).unwrap();
        assert_eq!(p.nav(now).unwrap(), 500_000_000 + payment.applied);
        assert_eq!(p.impaired_value, 0);
    }

    #[test]
    fn test_a_bad_mark_writes_the_loan_down_before_maturity() {
        let mut p = pool();
        p.impaired_share_bps = 5_000;
        let mut l = opened(&mut p, 500_000_000, 13);
        p.open_epoch(1).unwrap();
        let now = p.epoch_start(1);
        let owed = l.owed(now).unwrap();
        assert!(l.mark(&mut p, T0 + 10, true, now).unwrap());
        assert!(l.is_written_down);
        assert_eq!(p.impaired_value, owed / 2);
        assert_eq!(p.nav(now).unwrap(), 500_000_000 + owed / 2);
        // Interest no longer accrues into NAV.
        assert_eq!(p.nav(now + 30 * DAY).unwrap(), 500_000_000 + owed / 2);
        assert!(p.is_marked());
    }

    #[test]
    fn test_a_mark_needs_a_root_at_least_as_new_as_the_last() {
        let mut p = pool();
        let mut l = opened(&mut p, 500_000_000, 13);
        p.open_epoch(1).unwrap();
        let now = p.epoch_start(1);
        assert!(l.mark(&mut p, T0 - 1, false, now).is_err());
        assert!(l.mark(&mut p, T0, false, now).unwrap());
        // A second mark in the epoch counts once.
        assert!(!l.mark(&mut p, T0 + 5, false, now).unwrap());
        assert_eq!(p.marked_loans, 1);
    }

    #[test]
    fn test_a_stale_loan_is_discounted_for_the_rest_of_the_epoch() {
        let mut p = pool();
        p.impaired_share_bps = 2_000;
        let mut l = opened(&mut p, 500_000_000, 13);
        p.open_epoch(1).unwrap();
        let now = p.epoch_start(1);
        let full = p.nav(now).unwrap();
        let owed = l.owed(now).unwrap();
        assert!(l.mark_stale(&mut p, now).unwrap());
        assert!(l.mark_stale(&mut p, now).is_err(), "already marked");
        let discounted = full - (owed - owed / 5);
        assert_eq!(p.nav(now).unwrap(), discounted);
        assert!(p.is_marked());

        // A fresh mark, or a payment of 1, in the same epoch lifts nothing.
        l.mark(&mut p, T0 + 20, false, now).unwrap();
        assert_eq!(p.nav(now).unwrap(), discounted);
        l.apply_payment(&mut p, 1, now).unwrap();
        assert!(p.nav(now).unwrap() <= discounted + 1);

        // The next epoch's start ends it, and a fresh mark keeps it ended.
        p.open_epoch(2).unwrap();
        let later = p.epoch_start(2);
        assert_eq!(p.stale_discount, 0);
        l.mark(&mut p, T0 + 30, false, later).unwrap();
        assert_eq!(l.stale_discount, 0);
        assert_eq!(p.nav(later).unwrap(), p.idle + l.owed(later).unwrap());
    }

    #[test]
    fn test_a_deposit_at_a_stale_nav_gains_nothing_from_a_late_mark() {
        // The reviewer's case: a 1M pool, one 100k loan left unmarked.
        let mut p = pool();
        p.idle = 1_000_000_000_000;
        let mut l = opened(&mut p, 100_000_000_000, 13);
        p.open_epoch(1).unwrap();
        let now = p.epoch_start(1) + DAY;
        l.mark_stale(&mut p, now).unwrap();
        let low = p.nav(now).unwrap();
        l.mark(&mut p, T0 + 20, false, now).unwrap();
        assert_eq!(p.nav(now).unwrap(), low);
    }

    #[test]
    fn test_a_payment_keeps_the_stale_discount_within_what_is_owed() {
        let mut p = pool();
        let mut l = opened(&mut p, 500_000_000, 13);
        p.open_epoch(1).unwrap();
        let now = p.epoch_start(1);
        l.mark_stale(&mut p, now).unwrap();
        let low = p.nav(now).unwrap();
        // Paying 90% leaves the loan counted at 0, not below it.
        let owed = l.owed(now).unwrap();
        l.apply_payment(&mut p, owed / 10 * 9, now).unwrap();
        assert_eq!(p.stale_discount, l.owed(now).unwrap());
        assert_eq!(p.nav(now).unwrap(), low + owed / 10 * 9);
    }

    #[test]
    fn test_impairing_a_stale_loan_counts_its_loss_once() {
        // The reviewer's case: maturity 4, grace 2, stale every epoch.
        let mut p = pool();
        p.grace_epochs = 2;
        p.idle = 900_000_000_000;
        let mut l = opened(&mut p, 100_000_000_000, 4);
        for epoch in 1..6 {
            p.open_epoch(epoch).unwrap();
            let now = p.epoch_start(epoch) + DAY;
            l.mark_stale(&mut p, now).unwrap();
        }
        p.open_epoch(6).unwrap();
        assert!(l.is_impaired(&p));
        // Impaired at a share of 0: NAV is the idle USDC, counted once.
        assert_eq!(p.nav(p.epoch_start(6)).unwrap(), p.idle);
        assert_eq!(p.idle, 800_000_000_000);
    }

    #[test]
    fn test_an_impaired_loan_needs_no_marking() {
        let mut p = pool();
        let mut l = opened(&mut p, 500_000_000, 1);
        // Maturity 1 and grace 1: impaired once epoch 2 opens.
        p.open_epoch(1).unwrap();
        assert!(!p.is_marked());
        p.open_epoch(2).unwrap();
        assert!(l.is_impaired(&p));
        assert_eq!(p.performing_loans(), 0);
        assert!(p.is_marked());
        // A stale mark of it counts for nothing.
        let now = p.epoch_start(2);
        l.mark_stale(&mut p, now).unwrap();
        assert_eq!(p.marked_loans, 0);
    }

    #[test]
    fn test_a_written_down_loan_leaves_the_marking() {
        let mut p = pool();
        let mut l = opened(&mut p, 500_000_000, 13);
        let _ = opened(&mut p, 100_000_000, 13);
        p.open_epoch(1).unwrap();
        let now = p.epoch_start(1) + 60;
        l.mark(&mut p, T0 + 10, true, now).unwrap();
        assert_eq!((p.performing_loans(), p.marked_loans), (1, 0));
        assert!(!p.is_marked());
        // Next epoch, only the other loan needs marking.
        p.open_epoch(2).unwrap();
        assert_eq!(p.performing_loans(), 1);
    }

    #[test]
    fn test_withdrawals_are_priced_after_a_repayment_during_marking() {
        // A loan alone in its bucket, repaid in part after the epoch opens.
        let mut p = pool();
        let mut l = opened(&mut p, 500_000_000, 13);
        p.open_epoch(1).unwrap();
        let now = p.epoch_start(1) + 3_600;
        l.apply_payment(&mut p, 1, now).unwrap();
        // Pricing at the epoch's start would count interest before the
        // payment's restart, and fail.
        assert!(p.nav(p.epoch_start(1)).is_err());
        assert!(p.nav(now).is_ok());
    }
}
