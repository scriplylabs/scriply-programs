//! The pool's rules on its own state: epochs and their settlement, NAV,
//! maturity buckets, impairment and the withdrawal queue. Nothing here moves tokens; the
//! instructions do that after these decide the amounts.

use anchor_lang::prelude::*;

use crate::{constants::*, error::PoolError, math::*, state::*};

impl MaturityBucket {
    pub fn accrual(&self) -> Accrual {
        Accrual {
            principal: self.principal.get(),
            principal_rate: self.principal_rate.get(),
            principal_rate_start: self.principal_rate_start.get(),
            principal_late_rate: self.principal_late_rate.get(),
            principal_late_rate_start: self.principal_late_rate_start.get(),
            carried_interest: self.carried_interest.get(),
        }
    }

    pub fn set_accrual(&mut self, a: &Accrual) {
        self.principal.set(a.principal);
        self.principal_rate.set(a.principal_rate);
        self.principal_rate_start.set(a.principal_rate_start);
        self.principal_late_rate.set(a.principal_late_rate);
        self.principal_late_rate_start
            .set(a.principal_late_rate_start);
        self.carried_interest.set(a.carried_interest);
    }
}

/// What an epoch's start paid the withdrawal queue.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WithdrawalSettlement {
    /// USDC set aside for the requests to claim.
    pub paid: u64,
    /// Shares to burn from escrow.
    pub shares_burned: u64,
    /// The generation that ended, with what it paid per normalized share,
    /// when every pending request was paid in full.
    pub ended_generation: Option<(u64, u128)>,
}

impl Pool {
    pub fn signers(&self) -> &[Pubkey] {
        &self.signers[..self.signer_count as usize]
    }

    pub fn maturities(&self) -> &[u16] {
        &self.maturities[..self.maturity_count as usize]
    }

    pub fn current_epoch(&self, now: i64) -> u64 {
        (now.saturating_sub(self.epoch_origin).max(0) as u64) / self.epoch_seconds
    }

    /// The start of `epoch` in unix seconds, which is also when loans
    /// maturing in it mature.
    pub fn epoch_start(&self, epoch: u64) -> i64 {
        self.epoch_origin + (epoch * self.epoch_seconds) as i64
    }

    /// Fails unless the current epoch is fully settled: its loans impaired
    /// by maturity, every loan marked from its token's facts, and its
    /// withdrawals paid. Deposits, loans and withdrawal requests wait for
    /// this, so no one acts on a NAV that hasn't counted what the epoch's
    /// start found.
    pub fn require_settled(&self, now: i64) -> Result<()> {
        let epoch = self.current_epoch(now);
        require!(
            self.settled_epoch == epoch && self.withdrawals_epoch == epoch,
            PoolError::EpochNotSettled
        );
        Ok(())
    }

    /// Fails unless the current epoch's start has been settled, though its
    /// marking may still be under way. Marks, repayments and syncs need
    /// only this: none of them moves NAV at another depositor's expense.
    pub fn require_epoch_open(&self, now: i64) -> Result<()> {
        require!(
            self.settled_epoch == self.current_epoch(now),
            PoolError::EpochNotSettled
        );
        Ok(())
    }

    /// The loans NAV counts as performing, which each epoch's marking
    /// covers. An impaired loan can't be marked up or down, so it isn't one.
    pub fn performing_loans(&self) -> u64 {
        self.buckets.iter().map(|b| b.loans).sum()
    }

    /// Whether every performing loan has been marked in the settled epoch.
    pub fn is_marked(&self) -> bool {
        self.marked_loans >= self.performing_loans()
    }

    /// Counts a new performing loan maturing in `maturity_epoch`, marked
    /// this epoch since its token was just read.
    pub fn add_performing_loan(&mut self, maturity_epoch: u64) {
        self.buckets[(maturity_epoch % BUCKETS as u64) as usize].loans += 1;
        self.marked_loans += 1;
    }

    /// Stops counting a performing loan maturing in `maturity_epoch`, once
    /// it is repaid or written down; `was_marked` says whether it was
    /// marked this epoch.
    pub fn remove_performing_loan(&mut self, maturity_epoch: u64, was_marked: bool) {
        let bucket = &mut self.buckets[(maturity_epoch % BUCKETS as u64) as usize];
        bucket.loans = bucket.loans.saturating_sub(1);
        if was_marked {
            self.marked_loans = self.marked_loans.saturating_sub(1);
        }
    }

    /// Starts settling `epoch`: impairs the loans past maturity and grace
    /// and begins its marking. Last epoch's stale discounts end here: a
    /// loan still unread is marked stale again once the window closes, and
    /// nothing waiting on the marks runs before then.
    pub fn open_epoch(&mut self, epoch: u64) -> Result<()> {
        self.settled_epoch = epoch;
        self.marked_loans = 0;
        self.stale_discount = 0;
        self.impair_due()
    }

    /// The signers' roots accounts this pool reads from.
    pub fn is_signer(&self, key: &Pubkey) -> bool {
        self.signers().contains(key)
    }

    pub fn risk_model(&self) -> RiskModel {
        // create_pool stores only models this program knows.
        RiskModel::V1
    }

    /// Whether loans maturing in `maturity_epoch` are impaired: their grace
    /// has passed by the last settled epoch's start.
    pub fn is_impaired(&self, maturity_epoch: u64) -> bool {
        maturity_epoch + self.grace_epochs as u64 <= self.settled_epoch
    }

    /// The pool's net asset value at `now`: idle USDC, every performing
    /// loan's principal and interest accrued, and what impaired loans count,
    /// less the discount on loans marked stale. USDC set aside for
    /// withdrawals is not the pool's any more.
    pub fn nav(&self, now: i64) -> Result<u64> {
        let mut total = add(self.idle as u128, self.impaired_value as u128)?;
        for bucket in self.buckets.iter() {
            let accrual = bucket.accrual();
            if accrual.is_empty() {
                continue;
            }
            let maturity = self.epoch_start(bucket.maturity_epoch);
            total = add(total, accrual.owed_at(now, maturity)?)?;
        }
        to_u64(total.saturating_sub(self.stale_discount as u128))
    }

    pub fn add_accrual(&mut self, maturity_epoch: u64, a: &Accrual) -> Result<()> {
        let bucket = &mut self.buckets[(maturity_epoch % BUCKETS as u64) as usize];
        if bucket.maturity_epoch != maturity_epoch {
            require!(
                bucket.accrual().is_empty() && bucket.loans == 0,
                PoolError::BucketInUse
            );
            bucket.maturity_epoch = maturity_epoch;
        }
        let sum = bucket.accrual().checked_add(a)?;
        bucket.set_accrual(&sum);
        Ok(())
    }

    pub fn remove_accrual(&mut self, maturity_epoch: u64, a: &Accrual) -> Result<()> {
        let bucket = &mut self.buckets[(maturity_epoch % BUCKETS as u64) as usize];
        require!(
            bucket.maturity_epoch == maturity_epoch,
            PoolError::BucketInUse
        );
        let rest = bucket.accrual().checked_sub(a)?;
        bucket.set_accrual(&rest);
        Ok(())
    }

    /// Impairs every bucket whose maturity and grace have passed by the
    /// settled epoch: NAV counts its loans at the impaired share of what
    /// they owed at maturity, they stop accruing into NAV, and marking no
    /// longer covers them.
    pub fn impair_due(&mut self) -> Result<()> {
        for i in 0..BUCKETS {
            let bucket = self.buckets[i];
            let accrual = bucket.accrual();
            if accrual.is_empty() || !self.is_impaired(bucket.maturity_epoch) {
                continue;
            }
            let owed = accrual.owed_at_maturity(self.epoch_start(bucket.maturity_epoch))?;
            let value = mul_div(owed, self.impaired_share_bps as u128, BPS as u128)?;
            self.impaired_value = to_u64(add(self.impaired_value as u128, value)?)?;
            self.buckets[i].set_accrual(&Accrual::default());
            self.buckets[i].loans = 0;
        }
        Ok(())
    }

    /// The shares `amount` of USDC buys at `now`, given the share supply.
    pub fn shares_for_deposit(&self, amount: u64, share_supply: u64, now: i64) -> Result<u64> {
        let nav = self.nav(now)?;
        to_u64(mul_div(
            amount as u128,
            share_supply as u128 + VIRTUAL_SHARES as u128,
            nav as u128 + VIRTUAL_ASSETS as u128,
        )?)
    }

    /// Adds `shares` to the withdrawal queue and returns the request's
    /// normalized shares.
    pub fn queue_withdrawal(&mut self, shares: u64) -> Result<u128> {
        let norm = mul_div(shares as u128, INDEX_SCALE, self.remaining_factor.get())?;
        self.pending_norm.set(add(self.pending_norm.get(), norm)?);
        self.pending_shares = to_u64(add(self.pending_shares as u128, shares as u128)?)?;
        Ok(norm)
    }

    /// Pays the withdrawal queue at the NAV per share at `now`, from idle
    /// USDC. `now` is no earlier than any loan's last payment. If idle USDC can't pay every request, each is paid the same
    /// share of what it asked and the rest waits for the next epoch.
    pub fn settle_withdrawals(
        &mut self,
        share_supply: u64,
        now: i64,
    ) -> Result<WithdrawalSettlement> {
        let pending = self.pending_shares;
        if pending == 0 {
            return Ok(WithdrawalSettlement::default());
        }
        let nav = self.nav(now)?;
        let needed = to_u64(mul_div(
            pending as u128,
            nav as u128 + VIRTUAL_ASSETS as u128,
            share_supply as u128 + VIRTUAL_SHARES as u128,
        )?)?;
        let paid = needed.min(self.idle);
        let is_full = paid == needed;
        let shares_burned = if is_full {
            pending
        } else {
            to_u64(mul_div(pending as u128, paid as u128, needed as u128)?)?
        };
        let paid_per_norm = add(
            self.paid_per_norm.get(),
            mul_div(paid as u128, INDEX_SCALE, self.pending_norm.get())?,
        )?;

        self.idle -= paid;
        self.reserved = to_u64(add(self.reserved as u128, paid as u128)?)?;
        self.pending_shares -= shares_burned;

        let ended_generation = if is_full {
            let ended = (self.withdrawal_generation, paid_per_norm);
            self.withdrawal_generation += 1;
            self.remaining_factor.set(INDEX_SCALE);
            self.paid_per_norm.set(0);
            self.pending_norm.set(0);
            Some(ended)
        } else {
            self.paid_per_norm.set(paid_per_norm);
            // Never 0: later requests divide by it.
            let remaining = mul_div(
                self.remaining_factor.get(),
                (pending - shares_burned) as u128,
                pending as u128,
            )?;
            self.remaining_factor.set(remaining.max(1));
            None
        };
        Ok(WithdrawalSettlement {
            paid,
            shares_burned,
            ended_generation,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytemuck::Zeroable;

    const EPOCH: u64 = 7 * 86_400;
    const T0: i64 = 1_800_000_000;

    fn pool() -> Box<Pool> {
        let mut p: Box<Pool> = Box::new(Pool::zeroed());
        p.epoch_origin = T0;
        p.epoch_seconds = EPOCH;
        p.grace_epochs = 2;
        p.remaining_factor.set(INDEX_SCALE);
        p
    }

    fn loan(principal: u64, maturity: i64) -> Accrual {
        Accrual::of_loan(LoanTerms {
            principal,
            rate_bps: 1_200,
            late_rate_bps: 1_200,
            start: T0,
            maturity,
            carried: 0,
        })
        .unwrap()
    }

    #[test]
    fn test_nav_accrues_loans_every_second() {
        let mut p = pool();
        p.idle = 500_000_000;
        let maturity_epoch = 13;
        let m = p.epoch_start(maturity_epoch);
        p.add_accrual(maturity_epoch, &loan(500_000_000, m))
            .unwrap();
        p.idle -= 500_000_000;
        assert_eq!(p.nav(T0).unwrap(), 500_000_000);
        assert_eq!(p.nav(T0 + 90 * 86_400).unwrap(), 514_794_520);
    }

    #[test]
    fn test_impairment_counts_the_share_of_what_was_owed_at_maturity() {
        let mut p = pool();
        p.impaired_share_bps = 5_000;
        let m = p.epoch_start(4);
        p.add_accrual(4, &loan(1_000_000_000, m)).unwrap();
        p.settled_epoch = 5;
        p.impair_due().unwrap();
        assert!(!p.buckets[4].accrual().is_empty(), "grace hasn't passed");
        p.settled_epoch = 6;
        p.impair_due().unwrap();
        assert!(p.buckets[4].accrual().is_empty());
        let owed = loan(1_000_000_000, m).owed_at_maturity(m).unwrap();
        assert_eq!(p.impaired_value as u128, owed / 2);
    }

    #[test]
    fn test_withdrawals_are_paid_in_full_when_idle_covers_them() {
        let mut p = pool();
        p.idle = 1_000_000;
        let norm = p.queue_withdrawal(400_000_000).unwrap();
        let s = p.settle_withdrawals(1_000_000_000, T0).unwrap();
        assert_eq!(s.shares_burned, 400_000_000);
        assert_eq!(s.paid, 400_000);
        let (generation, paid_per_norm) = s.ended_generation.unwrap();
        assert_eq!(generation, 0);
        assert_eq!(norm * paid_per_norm / INDEX_SCALE, 400_000);
        assert_eq!(p.withdrawal_generation, 1);
        assert_eq!((p.idle, p.reserved), (600_000, 400_000));
    }

    #[test]
    fn test_withdrawals_share_idle_when_it_falls_short() {
        let mut p = pool();
        p.idle = 100_000;
        // NAV is 1 USDC across 1000 shares, but only 0.1 USDC is idle.
        let m = p.epoch_start(4);
        p.add_accrual(4, &loan(900_000, m)).unwrap();
        let a = p.queue_withdrawal(300_000_000).unwrap();
        let b = p.queue_withdrawal(100_000_000).unwrap();
        let s = p.settle_withdrawals(1_000_000_000, T0).unwrap();
        assert_eq!(s.paid, 100_000);
        assert!(s.ended_generation.is_none());
        assert_eq!(s.shares_burned, 100_000_000);
        let c = p.paid_per_norm.get();
        assert_eq!(a * c / INDEX_SCALE, 75_000);
        assert_eq!(b * c / INDEX_SCALE, 25_000);
        // Three quarters of each request still waits.
        assert_eq!(a * p.remaining_factor.get() / INDEX_SCALE, 225_000_000);
    }
}
