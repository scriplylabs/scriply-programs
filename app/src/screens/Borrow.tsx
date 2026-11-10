import type { Address, TransactionSendingSigner } from '@solana/kit';
import { useState } from 'react';

import { buildBorrow, buildCreateVault, findLoanVault, type PoolAt } from '../actions';
import { requestLoanVaultThaw } from '../api';
import { getMint, getTokenAccount, listPoolMaturities, listToken2022Holdings, proveToken, rpc, sendInstructions } from '../chain';
import { assessBorrow, borrowRefusals, epochStart, faceValue, isSettled, nav } from '../engine';
import { fetchMaybeLoan, fetchMaybeTokenExposure, findExposurePda, type Pool } from '../generated/receivables-pool';
import { formatAmount, formatBps, formatDuration, formatTime, parseAmount, shortenAddress } from '../format';
import { useLoad, useNow } from '../hooks';
import { ActionButton, AmountField, ErrorNote, Loading, Stat } from '../ui';
import { WithSigner } from '../wallet';
import { EpochStatus, PoolTabs } from './Pool';

const THAW_WAIT_MS = 3 * 60_000;

/** Waits until the vault reads thawed on chain, or gives up after a few minutes. */
async function waitForThaw(vault: Address, onTick: (text: string) => void): Promise<void> {
  const started = Date.now();
  while (Date.now() - started < THAW_WAIT_MS) {
    const account = await getTokenAccount(vault);
    if (account && !account.isFrozen) {
      return;
    }
    onTick(`Waiting for the vault to be thawed on chain (${Math.round((Date.now() - started) / 1000)}s)…`);
    await new Promise((r) => setTimeout(r, 3_000));
  }
  throw new Error(
    "The vault is still frozen. The thaw rule may have refused it: the token must be one the signer issues, and your own account for it must be thawed. Try again in a few minutes.",
  );
}

function Facts({ pool, mint, signer, now }: { pool: PoolAt; mint: Address; signer: TransactionSendingSigner; now: bigint }) {
  const state = useLoad(async () => {
    const { loan, vault } = await findLoanVault(pool.address, mint, signer.address);
    const [exposureAddress] = await findExposurePda({ pool: pool.address, collateralMint: mint });
    const [mintFacts, proof, vaultAccount, loanAccount, exposure] = await Promise.all([
      getMint(mint),
      proveToken(pool.pool, mint),
      getTokenAccount(vault),
      fetchMaybeLoan(rpc, loan),
      fetchMaybeTokenExposure(rpc, exposureAddress),
    ]);
    return { mintFacts, proof, loanAddress: loan, vault, vaultAccount, loan: loanAccount.exists ? loanAccount.data : undefined, exposed: exposure.exists ? exposure.data.principal : 0n };
  }, [pool.address, mint, signer.address]);
  const [unitsText, setUnits] = useState('');
  const [principalText, setPrincipal] = useState('');
  const [maturity, setMaturity] = useState(listPoolMaturities(pool.pool)[0]);
  const [progress, setProgress] = useState<string>();

  if (!state.data) {
    return state.error ? <ErrorNote error={state.error} /> : <Loading what="the token's facts" />;
  }
  const { mintFacts, proof, loanAddress, vault, vaultAccount, loan, exposed } = state.data;
  const p = pool.pool;
  if ('missing' in proof) {
    return <p className="error">{proof.missing}</p>;
  }
  if (loan?.isOpen) {
    return (
      <p className="notice">
        You have an open loan against this token in this pool. <a href={`#/pools/${pool.address}/loans/${loanAddress}`}>Repay it</a> before borrowing again.
      </p>
    );
  }
  if (loan && vaultAccount && vaultAccount.amount > 0n) {
    return (
      <p className="notice">
        Units from your last loan wait in its vault. <a href={`#/pools/${pool.address}/loans/${loanAddress}`}>Withdraw them</a> before borrowing again.
      </p>
    );
  }

  const s = proof.proven.snapshot;
  const rootAge = now - proof.latest.asOf;
  const isRootTooOld = rootAge > p.maxRootAge;
  const isMintedSince = mintFacts.supply > s.networkSupply;
  const units = parseAmount(unitsText, mintFacts.decimals);
  // `borrow`'s term: from when the loan opens, partway into the epoch, to its maturity.
  const termFrom = (at: bigint) => epochStart(p, p.settledEpoch + BigInt(maturity)) - at;
  const termSeconds = termFrom(now);
  const quote = units ? assessBorrow(p.risk, s, faceValue(units, mintFacts.decimals, p.quoteDecimals), termSeconds) : undefined;
  const capLeft = (nav(p, now) * BigInt(p.capPerTokenBps)) / 10_000n - exposed;
  const limits = quote && 'terms' in quote ? [quote.terms.maxPrincipal, p.idle, capLeft > 0n ? capLeft : 0n] : [];
  const maxPrincipal = limits.length ? limits.reduce((a, b) => (a < b ? a : b)) : 0n;
  const principal = parseAmount(principalText, p.quoteDecimals) ?? (maxPrincipal > 0n ? maxPrincipal : undefined);
  const d = mintFacts.decimals;

  return (
    <>
      <h3>The token's facts</h3>
      <p className="muted">
        As its signer vouched for them on {formatTime(s.asOf)}, under the root posted {formatDuration(rootAge > 0n ? (rootAge / 60n) * 60n : 0n)} ago by{' '}
        {shortenAddress(proof.latest.signer)}.
      </p>
      <div className="stats">
        <Stat label="Outstanding" value={formatAmount(s.outstandingUnits, d, 0)} hint="units, every network" />
        <Stat label="Current" value={formatAmount(s.agingCurrent, d, 0)} hint="units not yet due" />
        <Stat label="1–30 days late" value={formatAmount(s.agingDays1To30, d, 0)} />
        <Stat label="31–60" value={formatAmount(s.agingDays31To60, d, 0)} />
        <Stat label="61–90" value={formatAmount(s.agingDays61To90, d, 0)} />
        <Stat label="Over 90" value={formatAmount(s.agingOver90, d, 0)} />
        <Stat label="Concentration" value={formatBps(s.concentrationIndexBps)} hint="HHI" />
        <Stat label="Disputed" value={formatBps(s.disputedShareBps)} />
        <Stat label="Days to due" value={s.weightedDaysToDue} hint="weighted by units" />
        <Stat label="Burn debt" value={s.burnDebtOverdue ? 'Overdue' : formatAmount(s.burnDebt, d, 0)} />
      </div>
      {isRootTooOld && <p className="error">The latest root is older than this pool reads ({formatDuration(p.maxRootAge)}). It lends again once the signer posts a newer one.</p>}
      {isMintedSince && <p className="error">Units were minted after this snapshot. The pool lends again once the next root covers them.</p>}

      <h3>Borrow</h3>
      <div className="panel-grid">
        <div className="panel">
          <AmountField label="Units to pledge" unit="units" value={unitsText} onChange={setUnits} />
          <label className="field">
            <span>Term</span>
            <select value={maturity} onChange={(e) => setMaturity(Number(e.target.value))}>
              {listPoolMaturities(p).map((m) => (
                <option key={m} value={m}>
                  {m} epochs ({formatDuration(BigInt(m) * p.epochSeconds)})
                </option>
              ))}
            </select>
          </label>
          <AmountField
            label="Borrow"
            unit="USDC"
            value={principalText}
            onChange={setPrincipal}
            hint={maxPrincipal > 0n ? `Up to ${formatAmount(maxPrincipal, p.quoteDecimals)} USDC` : undefined}
          />
        </div>
        <div className="panel">
          <h4>What the pool's rules decide</h4>
          {!quote && <p className="muted">Enter the units to pledge.</p>}
          {quote && 'refusal' in quote && <p className="error">{borrowRefusals[quote.refusal]}</p>}
          {quote && 'terms' in quote && (
            <dl className="facts">
              <dt>Units that count</dt>
              <dd>{formatBps(quote.terms.eligibleBps)}</dd>
              <dt>Advance</dt>
              <dd>{formatBps(quote.terms.advanceBps)} of what counts</dd>
              <dt>Most the units borrow</dt>
              <dd>{formatAmount(quote.terms.maxPrincipal, p.quoteDecimals)} USDC</dd>
              <dt>Rate, fixed for the loan</dt>
              <dd>{formatBps(quote.terms.rateBps)} a year, {formatBps(quote.terms.rateBps + p.lateSpreadBps)} past maturity</dd>
              <dt>Pool can lend</dt>
              <dd>{formatAmount(p.idle, p.quoteDecimals)} USDC idle, {formatAmount(capLeft > 0n ? capLeft : 0n, p.quoteDecimals)} USDC left under the cap for this token</dd>
            </dl>
          )}
          <p className="muted small">
            A preview of the program's rules. The program decides when you borrow, from the same facts.
          </p>
        </div>
      </div>
      {!vaultAccount && <p className="muted small">First loan here: this creates the loan's vault (you pay its rent), asks for it to be thawed, then borrows.</p>}
      {vaultAccount?.isFrozen && <p className="muted small">The loan's vault exists but is frozen: this asks for it to be thawed, then borrows.</p>}
      <ActionButton
        label="Borrow"
        disabled={!units || !principal || !quote || !('terms' in quote) || principal > maxPrincipal || isRootTooOld || isMintedSince}
        run={async () => {
          try {
            if (!(await getTokenAccount(vault))) {
              setProgress('Creating the loan’s vault…');
              await sendInstructions(signer, await buildCreateVault(signer, pool, mint));
            }
            if ((await getTokenAccount(vault))?.isFrozen) {
              setProgress('Asking for the vault to be thawed…');
              await requestLoanVaultThaw(pool.address, mint, signer.address);
              await waitForThaw(vault, setProgress);
            }
            setProgress('Borrowing…');
            const fresh = await proveToken(pool.pool, mint);
            if ('missing' in fresh) {
              throw new Error(fresh.missing);
            }
            // The program fixes the rate from the snapshot sent now, so a root posted
            // since the page loaded must not lend on worse terms than the ones shown.
            const shown = quote && 'terms' in quote ? quote.terms : undefined;
            const sentAt = BigInt(Math.floor(Date.now() / 1000));
            const current = assessBorrow(p.risk, fresh.proven.snapshot, faceValue(units!, mintFacts.decimals, p.quoteDecimals), termFrom(sentAt));
            if ('refusal' in current) {
              state.reload();
              throw new Error(`The signer posted new facts since this page loaded, and the pool now refuses: ${borrowRefusals[current.refusal]}`);
            }
            if (!shown || current.terms.rateBps > shown.rateBps || principal! > current.terms.maxPrincipal) {
              state.reload();
              throw new Error(
                `The signer posted new facts since this page loaded: the rate is now ${formatBps(current.terms.rateBps)} and the most the units borrow is ${formatAmount(current.terms.maxPrincipal, p.quoteDecimals)} USDC. Check the new terms and borrow again.`,
              );
            }
            return await sendInstructions(signer, await buildBorrow(signer, pool, { mint, units: units!, principal: principal!, maturityEpochs: maturity, proof: fresh }));
          } finally {
            setProgress(undefined);
          }
        }}
        onDone={state.reload}
      />
      {progress && <p className="muted">{progress}</p>}
    </>
  );
}

function Holdings({ signer, at, now }: { signer: TransactionSendingSigner; at: PoolAt; now: bigint }) {
  const holdings = useLoad(() => listToken2022Holdings(signer.address), [signer.address]);
  const [mint, setMint] = useState<Address>();
  if (!holdings.data) {
    return holdings.error ? <ErrorNote error={holdings.error} /> : <Loading what="your tokens" />;
  }
  if (holdings.data.length === 0) {
    return <p className="muted">This wallet holds no Token-2022 tokens, so it has no receivables token to pledge.</p>;
  }
  return (
    <>
      <label className="field">
        <span>Token to pledge</span>
        <select value={mint ?? ''} onChange={(e) => setMint((e.target.value || undefined) as Address | undefined)}>
          <option value="">Choose one of your tokens</option>
          {holdings.data.map((h) => (
            <option key={h.address} value={h.data.mint} disabled={h.data.isFrozen}>
              {h.data.mint}
              {h.data.isFrozen ? ' (frozen: not verified to hold it)' : ''}
            </option>
          ))}
        </select>
      </label>
      {mint && <Facts key={mint} pool={at} mint={mint} signer={signer} now={now} />}
    </>
  );
}

/** Borrowing against a receivables token the wallet holds. */
export function BorrowScreen({ poolAddress, pool }: { poolAddress: Address; pool: Pool }) {
  const now = useNow(5_000);
  return (
    <section>
      <PoolTabs poolAddress={poolAddress} active="borrow" />
      <header className="page-header">
        <h1>Borrow from {shortenAddress(poolAddress)}</h1>
        <p className="muted">
          Pledge units of a receivables token you hold. The pool's rules read the token's latest facts, proven against a root its
          signers posted, and decide how much it lends and at what rate. Nobody approves the loan.
        </p>
      </header>
      <EpochStatus pool={pool} now={now} />
      {!isSettled(pool, now) && <p className="muted">Borrowing waits until the epoch is settled: see Keep.</p>}
      <WithSigner prompt="Connect the wallet that holds the token.">{(signer) => <Holdings signer={signer} at={{ address: poolAddress, pool }} now={now} />}</WithSigner>
    </section>
  );
}
