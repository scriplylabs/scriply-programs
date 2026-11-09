import type { Address, TransactionSendingSigner } from '@solana/kit';
import { TOKEN_PROGRAM_ADDRESS } from '@solana-program/token';
import { useState } from 'react';

import { buildClaimWithdrawal, buildDeposit, buildRequestWithdrawal, type PoolAt } from '../actions';
import { findTokenAccount, getMint, getTokenAccount, listLoans, listPoolMaturities, listPoolSigners, rpc, sendInstructions, type Found } from '../chain';
import {
  currentEpoch,
  epochStart,
  isLoanImpaired,
  isMarked,
  isSettled,
  nav,
  owedOnLoan,
  performingLoans,
  sharesForDeposit,
  valueOfShares,
} from '../engine';
import { fetchMaybeWithdrawalRequest, findRequestPda, type Loan, type Pool } from '../generated/receivables-pool';
import { formatAmount, formatBps, formatDuration, formatTime, parseAmount, shortenAddress } from '../format';
import { useLoad, useNow } from '../hooks';
import { ActionButton, AddressLink, AmountField, ErrorNote, Loading, Stat } from '../ui';
import { useWalletAccount, WithSigner } from '../wallet';

/** Shares carry three more decimals than the quote token (`SHARE_DECIMALS_OFFSET`). */
const SHARE_DECIMALS_OFFSET = 3;

/** Where a loan stands in the pool's epoch. */
export function describeLoan(pool: Pool, loan: Loan): string {
  if (!loan.isOpen) {
    return 'Closed';
  }
  if (isLoanImpaired(pool, loan)) {
    return loan.isWrittenDown ? 'Written down' : 'Impaired';
  }
  if (loan.staleDiscount > 0n) {
    return 'Marked stale';
  }
  return loan.markedEpoch === pool.settledEpoch ? 'Marked' : 'Awaiting its mark';
}

export function PoolTabs({ poolAddress, active }: { poolAddress: string; active: 'overview' | 'borrow' | 'keep' }) {
  const tabs = [
    ['overview', 'Lend', `#/pools/${poolAddress}`],
    ['borrow', 'Borrow', `#/pools/${poolAddress}/borrow`],
    ['keep', 'Keep', `#/pools/${poolAddress}/keep`],
  ] as const;
  return (
    <nav className="tabs">
      <a href="#/">All pools</a>
      {tabs.map(([id, label, href]) => (
        <a key={id} href={href} className={id === active ? 'active' : ''}>
          {label}
        </a>
      ))}
    </nav>
  );
}

export function EpochStatus({ pool, now }: { pool: Pool; now: bigint }) {
  const epoch = currentEpoch(pool, now);
  const performing = performingLoans(pool);
  let status: string;
  if (pool.settledEpoch !== epoch) {
    status = `Epoch ${epoch} has started and isn't settled yet: anyone's next transaction settles it.`;
  } else if (!isMarked(pool)) {
    status = `Marking: ${pool.markedLoans} of ${performing} loans read from their tokens' latest facts. Deposits, borrows and withdrawal requests wait until every loan is marked.`;
  } else if (!isSettled(pool, now)) {
    status = "Every loan is marked; the next transaction pays this epoch's withdrawals.";
  } else {
    status = 'Settled: deposits, borrows and withdrawal requests are open.';
  }
  return (
    <div className="notice">
      <strong>Epoch {epoch.toString()}</strong> · ends {formatTime(epochStart(pool, epoch + 1n))}
      <p>{status}</p>
    </div>
  );
}

function PoolFacts({ address, pool, shareSupply, now }: { address: Address; pool: Pool; shareSupply: bigint; now: bigint }) {
  const d = pool.quoteDecimals;
  const unit = 10n ** BigInt(d + SHARE_DECIMALS_OFFSET);
  return (
    <>
      <div className="stats">
        <Stat label="Net asset value" value={`${formatAmount(nav(pool, now), d)} USDC`} />
        <Stat label="Available to lend" value={`${formatAmount(pool.idle, d)} USDC`} />
        <Stat label="Share price" value={`${formatAmount(valueOfShares(pool, unit, shareSupply, now), d, 6)} USDC`} hint="per share" />
        <Stat label="Loans" value={performingLoans(pool).toString()} hint="performing" />
        <Stat label="Impaired" value={`${formatAmount(pool.impairedValue, d)} USDC`} hint="as NAV counts them" />
        <Stat label="Set aside" value={`${formatAmount(pool.reserved, d)} USDC`} hint="for withdrawals paid, not yet claimed" />
      </div>
      <details className="params">
        <summary>The pool's rules, fixed when it was created</summary>
        <dl className="facts">
          <dt>Pool</dt>
          <dd><AddressLink address={address} /></dd>
          <dt>Signers it reads facts from</dt>
          <dd>{listPoolSigners(pool).map((s) => <AddressLink key={s} address={s} />)}</dd>
          <dt>Oldest root it reads</dt>
          <dd>{formatDuration(pool.maxRootAge)}</dd>
          <dt>Epoch</dt>
          <dd>{formatDuration(pool.epochSeconds)}, marked within {formatDuration(pool.markWindowSeconds)}</dd>
          <dt>Terms</dt>
          <dd>{listPoolMaturities(pool).map((m) => `${m} epochs`).join(', ')}, then {pool.graceEpochs} epochs' grace</dd>
          <dt>Cap per token</dt>
          <dd>{formatBps(pool.capPerTokenBps)} of NAV</dd>
          <dt>Impaired loans count at</dt>
          <dd>{formatBps(pool.impairedShareBps)} of what they owed</dd>
          <dt>Advance</dt>
          <dd>
            {formatBps(pool.risk.baseAdvanceBps)} of eligible face value, at most {formatBps(pool.risk.maxAdvanceBps)}, less up to{' '}
            {formatBps(pool.risk.concentrationPenaltyBps)} as concentration passes {formatBps(pool.risk.concentrationThresholdBps)}
          </dd>
          <dt>Units 31–60 days past due count at</dt>
          <dd>{formatBps(pool.risk.weight31To60Bps)}</dd>
          <dt>Refuses when late</dt>
          <dd>more than {formatBps(pool.risk.maxLateShareBps)} of units past {pool.risk.lateAfterDays} days</dd>
          <dt>Rate</dt>
          <dd>
            {formatBps(pool.risk.baseRateBps)} base, within {formatBps(pool.risk.minRateBps)}–{formatBps(pool.risk.maxRateBps)}; spreads up to{' '}
            {formatBps(pool.risk.pastDueSpreadBps)} past due, {formatBps(pool.risk.concentrationSpreadBps)} concentration,{' '}
            {formatBps(pool.risk.disputeSpreadBps)} disputes, {formatBps(pool.risk.beyondTermsSpreadBps)} days beyond terms (full at{' '}
            {pool.risk.beyondTermsCapDays} days), {formatBps(pool.risk.notOnTimeSpreadBps)} not paid on time,{' '}
            {formatBps(pool.risk.noHistorySpreadBps)} no payment history; {formatBps(pool.lateSpreadBps)} more past maturity
          </dd>
          <dt>Depositors</dt>
          <dd>{pool.requiresAttestation === 1 ? <>must hold an attestation of credential <AddressLink address={pool.attestationCredential} /></> : 'anyone'}</dd>
        </dl>
      </details>
    </>
  );
}

function Position({ signer, at, shareSupply, now, onDone }: { signer: TransactionSendingSigner; at: PoolAt; shareSupply: bigint; now: bigint; onDone: () => void }) {
  const { pool } = at;
  const d = pool.quoteDecimals;
  const shareDecimals = d + SHARE_DECIMALS_OFFSET;
  const holdings = useLoad(async () => {
    const [request] = await findRequestPda({ pool: at.address, owner: signer.address });
    const [shares, usdc, pending] = await Promise.all([
      findTokenAccount(signer.address, pool.shareMint, TOKEN_PROGRAM_ADDRESS).then(getTokenAccount),
      findTokenAccount(signer.address, pool.quoteMint, TOKEN_PROGRAM_ADDRESS).then(getTokenAccount),
      fetchMaybeWithdrawalRequest(rpc, request),
    ]);
    return { shares: shares?.amount ?? 0n, usdc: usdc?.amount ?? 0n, request: pending.exists ? pending.data : undefined };
  }, [signer.address, at.address, pool]);
  const [deposit, setDeposit] = useState('');
  const [withdraw, setWithdraw] = useState('');
  const depositAmount = parseAmount(deposit, d);
  const withdrawShares = parseAmount(withdraw, shareDecimals);
  const done = () => {
    holdings.reload();
    onDone();
  };
  if (!holdings.data) {
    return holdings.error ? <ErrorNote error={holdings.error} /> : <Loading what="your position" />;
  }
  const h = holdings.data;
  return (
    <div className="panel-grid">
      <div className="panel">
        <h3>Deposit</h3>
        <p className="muted">USDC for shares at today's NAV. You hold {formatAmount(h.usdc, d)} USDC.</p>
        <AmountField
          label="Amount"
          unit="USDC"
          value={deposit}
          onChange={setDeposit}
          hint={depositAmount ? `≈ ${formatAmount(sharesForDeposit(pool, depositAmount, shareSupply, now), shareDecimals)} shares` : undefined}
        />
        <ActionButton
          label="Deposit"
          disabled={!depositAmount || depositAmount > h.usdc}
          run={async () => sendInstructions(signer, await buildDeposit(signer, at, depositAmount!))}
          onDone={done}
        />
      </div>
      <div className="panel">
        <h3>Withdraw</h3>
        <p className="muted">
          You hold {formatAmount(h.shares, shareDecimals, 4)} shares, worth {formatAmount(valueOfShares(pool, h.shares, shareSupply, now), d)} USDC today. A
          request is paid at the next epoch's start, at the NAV then; when idle USDC can't pay every request, each is paid the same share and the rest
          waits.
        </p>
        <AmountField label="Shares" unit="shares" value={withdraw} onChange={setWithdraw} />
        <ActionButton
          label="Request withdrawal"
          disabled={!withdrawShares || withdrawShares > h.shares || !!h.request}
          run={async () => sendInstructions(signer, await buildRequestWithdrawal(signer, at, withdrawShares!))}
          onDone={done}
        />
        {h.request && (
          <div className="notice">
            <p>You have a withdrawal request in generation {h.request.generation.toString()}. Claim what has been paid towards it.</p>
            <ActionButton
              label="Claim"
              run={async () => sendInstructions(signer, await buildClaimWithdrawal(signer, at, h.request!.generation))}
              onDone={done}
            />
          </div>
        )}
      </div>
    </div>
  );
}

function LoanList({ poolAddress, pool, loans, now }: { poolAddress: Address; pool: Pool; loans: Found<Loan>[]; now: bigint }) {
  const account = useWalletAccount();
  const open = loans.filter((l) => l.data.isOpen);
  if (loans.length === 0) {
    return <p className="muted">No loans yet.</p>;
  }
  return (
    <div className="table-wrap">
      <table>
        <thead>
          <tr>
            <th>Token</th>
            <th>Borrower</th>
            <th className="num">Owes</th>
            <th className="num">Rate</th>
            <th>Matures</th>
            <th>Status</th>
          </tr>
        </thead>
        <tbody>
          {[...open, ...loans.filter((l) => !l.data.isOpen)].map(({ address, data: loan }) => (
            <tr key={address} className={loan.borrower === account?.address ? 'mine' : ''}>
              <td>
                <a className="mono" href={`#/pools/${poolAddress}/loans/${address}`}>{shortenAddress(loan.collateralMint)}</a>
              </td>
              <td className="mono">{shortenAddress(loan.borrower)}</td>
              <td className="num">{loan.isOpen ? formatAmount(owedOnLoan(loan, now), pool.quoteDecimals) : '—'}</td>
              <td className="num">{formatBps(loan.rateBps)}</td>
              <td>{loan.isOpen ? formatTime(loan.maturityTime) : '—'}</td>
              <td>{describeLoan(pool, loan)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** A pool: its value and rules, depositing and withdrawing, and its loans. */
export function PoolScreen({ poolAddress, pool, reload }: { poolAddress: Address; pool: Pool; reload: () => void }) {
  const now = useNow();
  const supply = useLoad(() => getMint(pool.shareMint).then((m) => m.supply), [pool]);
  const loans = useLoad(() => listLoans(poolAddress), [poolAddress, pool]);
  const at = { address: poolAddress, pool };
  return (
    <section>
      <PoolTabs poolAddress={poolAddress} active="overview" />
      <header className="page-header">
        <h1>Pool {shortenAddress(poolAddress)}</h1>
      </header>
      <EpochStatus pool={pool} now={now} />
      {supply.data !== undefined ? <PoolFacts address={poolAddress} pool={pool} shareSupply={supply.data} now={now} /> : <Loading what="the pool's shares" />}
      <h2>Your position</h2>
      <WithSigner prompt="Connect a wallet to deposit or withdraw.">
        {(signer) => supply.data !== undefined && <Position signer={signer} at={at} shareSupply={supply.data} now={now} onDone={reload} />}
      </WithSigner>
      <h2>Loans</h2>
      {loans.data ? <LoanList poolAddress={poolAddress} pool={pool} loans={loans.data} now={now} /> : <Loading what="the pool's loans" />}
      <ErrorNote error={loans.error ?? supply.error} />
    </section>
  );
}
