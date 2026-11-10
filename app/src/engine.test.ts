// The Rust tests' cases (risk_engine.rs, math.rs, pool.rs), run against the
// port, so the preview can't drift from what the program decides.

import { describe, expect, it } from 'vitest';

import { assessBorrow, owedAt, rateBps, SECONDS_PER_DAY, type Accrual } from './engine';
import type { RiskParams } from './generated/receivables-pool';
import type { Snapshot } from './snapshot';

function params(): RiskParams {
  return {
    baseAdvanceBps: 8_000,
    maxAdvanceBps: 9_000,
    weight31To60Bps: 5_000,
    concentrationThresholdBps: 2_500,
    concentrationPenaltyBps: 2_000,
    baseRateBps: 800,
    minRateBps: 500,
    maxRateBps: 1_500,
    pastDueSpreadBps: 600,
    concentrationSpreadBps: 400,
    disputeSpreadBps: 1_000,
    lateAfterDays: 90,
    maxLateShareBps: 5_000,
    beyondTermsSpreadBps: 300,
    beyondTermsCapDays: 60,
    notOnTimeSpreadBps: 400,
    noHistorySpreadBps: 200,
  };
}

/** risk_engine.rs's `clean()`. */
function clean(): Snapshot {
  return {
    version: 1,
    network: '',
    mint: new Uint8Array(32),
    asOf: 0n,
    reportHash: new Uint8Array(32),
    networkSupply: 0n,
    outstandingUnits: 1_000n,
    burnDebt: 0n,
    burnDebtOverdue: false,
    agingCurrent: 1_000n,
    agingDays1To30: 0n,
    agingDays31To60: 0n,
    agingDays61To90: 0n,
    agingOver90: 0n,
    concentrationIndexBps: 2_000,
    disputedShareBps: 0,
    weightedDaysToDue: 20,
    weightedDaysToPay: 25,
    weightedDaysBeyondTerms: 0,
    paidOnTimeShareBps: 10_000,
    historyShareBps: 10_000,
  };
}

const TERM = 28n * SECONDS_PER_DAY;
const VALUE = 1_000_000_000n;

function terms(p: RiskParams, s: Snapshot, term = TERM) {
  const out = assessBorrow(p, s, VALUE, term);
  if (!('terms' in out)) {
    throw new Error(`refused: ${out.refusal}`);
  }
  return out.terms;
}

describe('assessBorrow, as risk_engine.rs tests it', () => {
  it('gives a clean token the base advance and rate', () => {
    expect(terms(params(), clean())).toEqual({ maxPrincipal: 800_000_000n, rateBps: 800, eligibleBps: 10_000, advanceBps: 8_000 });
  });

  it('shrinks the eligible share for aging and disputes', () => {
    const s = { ...clean(), agingCurrent: 600n, agingDays1To30: 200n, agingDays31To60: 100n, agingOver90: 100n, disputedShareBps: 500 };
    const t = terms(params(), s);
    expect(t.eligibleBps).toBe(8_000);
    expect(t.maxPrincipal).toBe(640_000_000n);
    expect(t.rateBps).toBe(800 + 240 + 50);
  });

  it('lowers the advance and raises the rate for concentration', () => {
    const t = terms(params(), { ...clean(), concentrationIndexBps: 6_250 });
    expect(t.advanceBps).toBe(7_000);
    expect(t.rateBps).toBe(1_000);
  });

  it('keeps the rate within its bounds', () => {
    const s = { ...clean(), agingCurrent: 0n, agingDays1To30: 1_000n, disputedShareBps: 10_000 };
    expect(rateBps({ ...params(), disputeSpreadBps: 9_000 }, s)).toBe(1_500);
    expect(rateBps({ ...params(), disputeSpreadBps: 0, baseRateBps: 100, pastDueSpreadBps: 0 }, s)).toBe(500);
  });

  it('caps spreads past u16 at the max rate', () => {
    const s = { ...clean(), agingCurrent: 0n, agingOver90: 1_000n, disputedShareBps: 10_000, concentrationIndexBps: 10_000 };
    expect(rateBps({ ...params(), baseRateBps: 65_535, pastDueSpreadBps: 65_535, maxRateBps: 1_500 }, s)).toBe(1_500);
  });

  it('may advance at face', () => {
    expect(terms({ ...params(), baseAdvanceBps: 10_000, maxAdvanceBps: 10_000 }, clean()).maxPrincipal).toBe(VALUE);
  });

  it('refuses overdue burn debt', () => {
    expect(assessBorrow(params(), { ...clean(), burnDebtOverdue: true }, VALUE, TERM)).toEqual({ refusal: 'BurnDebtOverdue' });
  });

  it('refuses too much late', () => {
    expect(assessBorrow(params(), { ...clean(), agingCurrent: 490n, agingOver90: 510n }, VALUE, TERM)).toEqual({ refusal: 'TooMuchLate' });
    const half = { ...clean(), agingCurrent: 500n, agingOver90: 500n };
    expect(assessBorrow(params(), half, VALUE, TERM)).toHaveProperty('terms');
    expect(assessBorrow({ ...params(), lateAfterDays: 60, maxLateShareBps: 4_000 }, half, VALUE, TERM)).toEqual({ refusal: 'TooMuchLate' });
  });

  it('refuses a term ending before collection', () => {
    const s = { ...clean(), weightedDaysBeyondTerms: 10 };
    expect(assessBorrow(params(), s, VALUE, TERM)).toEqual({ refusal: 'TermTooShort' });
    expect(assessBorrow(params(), s, VALUE, 30n * SECONDS_PER_DAY)).toHaveProperty('terms');
  });

  it('prices payment history', () => {
    const s = { ...clean(), weightedDaysBeyondTerms: 30, paidOnTimeShareBps: 6_000 };
    expect(terms(params(), s, 60n * SECONDS_PER_DAY).rateBps).toBe(800 + 150 + 160);
    expect(terms(params(), { ...s, historyShareBps: 5_000 }, 60n * SECONDS_PER_DAY).rateBps).toBe(800 + 75 + 80 + 100);
    expect(terms(params(), { ...clean(), historyShareBps: 0, paidOnTimeShareBps: 0 }).rateBps).toBe(800 + 200);
  });

  it('refuses a token with nothing eligible', () => {
    // Every unit late is refused as late first; a pool that allows it still finds none eligible.
    expect(assessBorrow({ ...params(), maxLateShareBps: 10_000 }, { ...clean(), agingCurrent: 0n, agingOver90: 1_000n }, VALUE, TERM)).toEqual({ refusal: 'NoEligibleUnits' });
    expect(assessBorrow(params(), { ...clean(), outstandingUnits: 0n }, VALUE, TERM)).toEqual({ refusal: 'NoEligibleUnits' });
  });
});

describe('owedAt', () => {
  const DAY = 86_400n;
  const T0 = 1_800_000_000n;

  function loan(principal: bigint, rate: bigint, late: bigint, start: bigint, maturity: bigint): Accrual {
    const pr = principal * rate;
    const pl = principal * late;
    return {
      principal,
      principalRate: pr,
      principalRateStart: pr * (start < maturity ? start : maturity),
      principalLateRate: pl,
      principalLateRateStart: pl * (start > maturity ? start : maturity),
      carriedInterest: 0n,
    };
  }

  it('accrues 500 USDC at 12% for 90 days to 14.79 USDC of interest', () => {
    const a = loan(500_000_000n, 1_200n, 1_200n, T0, T0 + 365n * DAY);
    expect(owedAt(a, T0 + 90n * DAY, T0 + 365n * DAY)).toBe(500_000_000n + 14_794_520n);
  });

  it('accrues at the late rate after maturity', () => {
    const maturity = T0 + 30n * DAY;
    const a = loan(1_000_000_000n, 1_000n, 2_000n, T0, maturity);
    const atMaturity = owedAt(a, maturity, maturity);
    expect(owedAt(a, maturity + 30n * DAY, maturity) - atMaturity).toBe(16_438_356n);
  });
});
