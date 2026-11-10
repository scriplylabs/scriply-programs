// The pool program's arithmetic, ported so the app can show what the
// program will decide before anyone signs: a pool's NAV and share price,
// what a loan owes, and the risk engine's terms for a borrow.
//
// The program decides; these only preview. Each function names the Rust
// it follows, and engine.test.ts runs the Rust tests' cases against it, so
// a change to the program that isn't made here too fails those tests.

import type { Loan, MaturityBucket, Pool, RiskParams, Wide } from './generated/receivables-pool';
import type { Snapshot } from './snapshot';

export const BPS = 10_000n;
export const SECONDS_PER_YEAR = 31_536_000n;
export const SECONDS_PER_DAY = 86_400n;
export const VIRTUAL_SHARES = 1_000n;
export const VIRTUAL_ASSETS = 1n;
export const BUCKETS = 64n;

/** `math::mul_div`: a × b / c, rounded down; fails when c is 0. */
function mulDiv(a: bigint, b: bigint, c: bigint): bigint {
  if (c === 0n) {
    throw new Error('MathOverflow');
  }
  return (a * b) / c;
}

function min(a: bigint, b: bigint): bigint {
  return a < b ? a : b;
}

function max(a: bigint, b: bigint): bigint {
  return a > b ? a : b;
}

export function readWide(w: Wide): bigint {
  return (w.hi << 64n) | w.lo;
}

/** `math::time`: unix seconds, never before 1970. */
function time(t: bigint): bigint {
  return max(t, 0n);
}

/** `math::Accrual`: a group of loans that share a maturity, summed. */
export type Accrual = {
  principal: bigint;
  principalRate: bigint;
  principalRateStart: bigint;
  principalLateRate: bigint;
  principalLateRateStart: bigint;
  carriedInterest: bigint;
};

export function toBucketAccrual(b: MaturityBucket): Accrual {
  return {
    principal: readWide(b.principal),
    principalRate: readWide(b.principalRate),
    principalRateStart: readWide(b.principalRateStart),
    principalLateRate: readWide(b.principalLateRate),
    principalLateRateStart: readWide(b.principalLateRateStart),
    carriedInterest: readWide(b.carriedInterest),
  };
}

/** `Accrual::of_loan`, from a loan's own terms (`Loan::accrual`). */
export function toLoanAccrual(loan: Loan): Accrual {
  const p = loan.principal;
  const pr = p * BigInt(loan.rateBps);
  const pl = p * BigInt(loan.lateRateBps);
  return {
    principal: p,
    principalRate: pr,
    principalRateStart: pr * time(min(loan.accrualStart, loan.maturityTime)),
    principalLateRate: pl,
    principalLateRateStart: pl * time(max(loan.accrualStart, loan.maturityTime)),
    carriedInterest: loan.carriedInterest,
  };
}

function isEmpty(a: Accrual): boolean {
  return Object.values(a).every((v) => v === 0n);
}

/** `Accrual::interest_to`: simple interest to `now`, rounded down. */
function interestTo(a: Accrual, now: bigint, maturity: bigint): bigint {
  const before = a.principalRate * time(min(now, maturity)) - a.principalRateStart;
  const after = a.principalLateRate * time(max(now, maturity)) - a.principalLateRateStart;
  return (before + after) / (BPS * SECONDS_PER_YEAR);
}

/** `Accrual::owed_at`: principal, carried interest and interest to `now`. */
export function owedAt(a: Accrual, now: bigint, maturity: bigint): bigint {
  return a.principal + a.carriedInterest + interestTo(a, now, maturity);
}

/** `Loan::owed`: what a loan owes at `now`. */
export function owedOnLoan(loan: Loan, now: bigint): bigint {
  return owedAt(toLoanAccrual(loan), now, loan.maturityTime);
}

/** `Pool::current_epoch`. */
export function currentEpoch(pool: Pool, now: bigint): bigint {
  return time(now - pool.epochOrigin) / pool.epochSeconds;
}

/** `Pool::epoch_start`: also when loans maturing in `epoch` mature. */
export function epochStart(pool: Pool, epoch: bigint): bigint {
  return pool.epochOrigin + epoch * pool.epochSeconds;
}

/** `Pool::performing_loans`. */
export function performingLoans(pool: Pool): bigint {
  return pool.buckets.reduce((sum, b) => sum + b.loans, 0n);
}

/** `Pool::is_marked`: every performing loan marked in the settled epoch. */
export function isMarked(pool: Pool): boolean {
  return pool.markedLoans >= performingLoans(pool);
}

/** `Pool::require_settled`: deposits, borrows and withdrawal requests wait for it. */
export function isSettled(pool: Pool, now: bigint): boolean {
  const epoch = currentEpoch(pool, now);
  return pool.settledEpoch === epoch && pool.withdrawalsEpoch === epoch;
}

/** `Pool::is_impaired`: loans maturing in `maturityEpoch` are past grace. */
export function isBucketImpaired(pool: Pool, maturityEpoch: bigint): boolean {
  return maturityEpoch + BigInt(pool.graceEpochs) <= pool.settledEpoch;
}

/** `Loan::is_impaired`. */
export function isLoanImpaired(pool: Pool, loan: Loan): boolean {
  return loan.isWrittenDown || isBucketImpaired(pool, loan.maturityEpoch);
}

/** `Pool::nav`: the pool's net asset value at `now`, in quote base units. */
export function nav(pool: Pool, now: bigint): bigint {
  const net = navBeforeStaleDiscount(pool, now) - pool.staleDiscount;
  return net > 0n ? net : 0n;
}

/** `Pool::nav_before_stale_discount`: NAV counting stale loans as performing, what deposits are priced at. */
export function navBeforeStaleDiscount(pool: Pool, now: bigint): bigint {
  let total = pool.idle + pool.impairedValue;
  for (const bucket of pool.buckets) {
    const accrual = toBucketAccrual(bucket);
    if (isEmpty(accrual)) {
      continue;
    }
    total += owedAt(accrual, now, epochStart(pool, bucket.maturityEpoch));
  }
  return total;
}

/** `Pool::shares_for_deposit`: the shares `amount` buys. */
export function sharesForDeposit(pool: Pool, amount: bigint, shareSupply: bigint, now: bigint): bigint {
  return mulDiv(amount, shareSupply + VIRTUAL_SHARES, navBeforeStaleDiscount(pool, now) + VIRTUAL_ASSETS);
}

/** The quote base units `shares` are worth at today's NAV, before any queue. */
export function valueOfShares(pool: Pool, shares: bigint, shareSupply: bigint, now: bigint): bigint {
  return mulDiv(shares, nav(pool, now) + VIRTUAL_ASSETS, shareSupply + VIRTUAL_SHARES);
}

/** `math::face_value`: a unit is one dollar, so only the decimals differ. */
export function faceValue(units: bigint, collateralDecimals: number, quoteDecimals: number): bigint {
  return mulDiv(units, 10n ** BigInt(quoteDecimals), 10n ** BigInt(collateralDecimals));
}

/** `risk_engine::BorrowTerms`. */
export type BorrowTerms = {
  maxPrincipal: bigint;
  rateBps: number;
  eligibleBps: number;
  advanceBps: number;
};

/** Why the risk engine refuses a borrow, named as `PoolError` names it. */
export type BorrowRefusal = 'BurnDebtOverdue' | 'TooMuchLate' | 'TermTooShort' | 'NoEligibleUnits';

/** `risk_engine::share_bps`: part / whole in bps, at most 10000. */
function shareBps(part: bigint, whole: bigint): bigint {
  return min(mulDiv(part, BPS, whole), BPS);
}

/** `risk_engine::proportion`. */
function proportion(weight: number, share: bigint): bigint {
  return mulDiv(BigInt(weight), min(share, BPS), BPS);
}

/** `risk_engine::above_threshold`. */
function aboveThreshold(weight: number, value: number, threshold: number): bigint {
  const v = min(BigInt(value), BPS);
  const t = BigInt(threshold);
  if (v <= t) {
    return 0n;
  }
  return mulDiv(BigInt(weight), v - t, BPS - t);
}

function eligibleBps(p: RiskParams, s: Snapshot): bigint {
  if (s.outstandingUnits === 0n) {
    return 0n;
  }
  const weighted31To60 = mulDiv(s.agingDays31To60, BigInt(p.weight31To60Bps), BPS);
  const share = shareBps(s.agingCurrent + s.agingDays1To30 + weighted31To60, s.outstandingUnits);
  const disputed = min(BigInt(s.disputedShareBps), BPS);
  return share > disputed ? share - disputed : 0n;
}

function lateShareBps(p: RiskParams, s: Snapshot): bigint {
  let late: bigint;
  switch (p.lateAfterDays) {
    case 30:
      late = s.agingDays31To60 + s.agingDays61To90 + s.agingOver90;
      break;
    case 60:
      late = s.agingDays61To90 + s.agingOver90;
      break;
    default:
      late = s.agingOver90;
  }
  return s.outstandingUnits === 0n ? 0n : shareBps(late, s.outstandingUnits);
}

function advanceBps(p: RiskParams, s: Snapshot): bigint {
  const penalty = aboveThreshold(p.concentrationPenaltyBps, s.concentrationIndexBps, p.concentrationThresholdBps);
  const base = BigInt(p.baseAdvanceBps);
  return min(base > penalty ? base - penalty : 0n, BigInt(p.maxAdvanceBps));
}

function historySpreadBps(p: RiskParams, s: Snapshot): bigint {
  const history = min(BigInt(s.historyShareBps), BPS);
  const beyondTerms = proportion(
    p.beyondTermsSpreadBps,
    shareBps(BigInt(s.weightedDaysBeyondTerms), BigInt(p.beyondTermsCapDays)),
  );
  const notOnTime = proportion(p.notOnTimeSpreadBps, BPS - min(BigInt(s.paidOnTimeShareBps), BPS));
  const withHistory = mulDiv(beyondTerms + notOnTime, history, BPS);
  const withoutHistory = proportion(p.noHistorySpreadBps, BPS - history);
  return min(withHistory + withoutHistory, 65_535n);
}

/** `risk_engine::rate_bps`. */
export function rateBps(p: RiskParams, s: Snapshot): number {
  const pastDue = s.agingDays1To30 + s.agingDays31To60 + s.agingDays61To90 + s.agingOver90;
  const pastDueBps = s.outstandingUnits === 0n ? BPS : shareBps(pastDue, s.outstandingUnits);
  const rate =
    BigInt(p.baseRateBps) +
    proportion(p.pastDueSpreadBps, pastDueBps) +
    aboveThreshold(p.concentrationSpreadBps, s.concentrationIndexBps, p.concentrationThresholdBps) +
    proportion(p.disputeSpreadBps, BigInt(s.disputedShareBps)) +
    historySpreadBps(p, s);
  return Number(max(min(rate, BigInt(p.maxRateBps)), BigInt(p.minRateBps)));
}

/**
 * `risk_engine::assess_borrow`: the terms for units worth `value`, borrowed
 * for `termSeconds`, or why the engine refuses them.
 */
export function assessBorrow(
  p: RiskParams,
  s: Snapshot,
  value: bigint,
  termSeconds: bigint,
): { terms: BorrowTerms } | { refusal: BorrowRefusal } {
  if (s.burnDebtOverdue) {
    return { refusal: 'BurnDebtOverdue' };
  }
  if (lateShareBps(p, s) > BigInt(p.maxLateShareBps)) {
    return { refusal: 'TooMuchLate' };
  }
  const expectedDays = BigInt(s.weightedDaysToDue) + BigInt(s.weightedDaysBeyondTerms);
  if (termSeconds < expectedDays * SECONDS_PER_DAY) {
    return { refusal: 'TermTooShort' };
  }
  const eligible = eligibleBps(p, s);
  if (eligible === 0n) {
    return { refusal: 'NoEligibleUnits' };
  }
  const advance = advanceBps(p, s);
  const eligibleValue = mulDiv(value, eligible, BPS);
  return {
    terms: {
      maxPrincipal: mulDiv(eligibleValue, advance, BPS),
      rateBps: rateBps(p, s),
      eligibleBps: Number(eligible),
      advanceBps: Number(advance),
    },
  };
}

/** What a refusal means, for the borrower. */
export const borrowRefusals: Record<BorrowRefusal, string> = {
  BurnDebtOverdue: "The token's burn debt is overdue, so this pool won't lend against it.",
  TooMuchLate: 'Too many of the units are backed by late invoices for this pool.',
  TermTooShort: 'The term ends before the invoices are expected to be collected. Choose a longer one.',
  NoEligibleUnits: 'None of the units count: they are too far past due or disputed.',
};

/** The bucket a maturity epoch's loans are summed in. */
export function bucketFor(pool: Pool, maturityEpoch: bigint): MaturityBucket {
  return pool.buckets[Number(maturityEpoch % BUCKETS)];
}
