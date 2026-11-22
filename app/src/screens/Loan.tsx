import type { Address, TransactionSendingSigner } from '@solana/kit';
import { TOKEN_PROGRAM_ADDRESS } from '@solana-program/token';
import { useState } from 'react';

import { buildCollectFromBorrower, buildRenewCollection, getOwedThroughGraceByBorrower, buildRepay, buildSyncLoan, buildWithdrawUnits, type PoolAt } from '../actions';
import { findTokenAccount, getMint, getTokenAccount, rpc, sendInstructions, TOKEN_2022_PROGRAM_ADDRESS } from '../chain';
import { graceSeconds, isLoanImpaired, owedOnLoan } from '../engine';
import { fetchMaybeLoan, findPoolAuthorityPda, type Loan, type Pool } from '../generated/receivables-pool';
import { formatAmount, formatBps, formatTime, parseAmount, shortenAddress } from '../format';
import { useLoad, useNow } from '../hooks';
import { ActionButton, AddressLink, AmountField, ErrorNote, Loading, Stat } from '../ui';
import { WithSigner } from '../wallet';
import { PoolTabs } from './Pool';

/**
 * How far ahead "Repay all" counts interest. `repay` takes at most what is
 * owed when it lands, so a day's margin closes the loan however long the
 * wallet stays open, and costs the payer nothing extra.
 */
const REPAY_ALL_MARGIN_SECONDS = 86_400n;

/** Where a loan stands, and what can move it on. */
function describeLoanState(pool: Pool, loan: Loan, now: bigint, vaultUnits: bigint): { status: string; detail: string } {
  if (!loan.isOpen) {
    if (loan.writtenOff > 0n) {
      return { status: 'Written off', detail: 'Its units are gone and what was paid for them didn’t repay it. USDC paid for it later still goes to the pool.' };
    }
    return {
      status: 'Repaid',
      detail: vaultUnits > 0n ? 'Nothing is owed. Its units wait in its vault for the borrower to withdraw.' : 'Nothing is owed, and its units are back with the borrower.',
    };
  }
  if (loan.isWrittenDown) {
    return {
      status: 'Written down',
      detail: 'The token’s burn debt is overdue, so the pool counts this loan at its impaired share. It waits on repayment or the buy-back, whose USDC repays it once synced.',
    };
  }
  if (isLoanImpaired(pool, loan)) {
    return {
      status: 'Impaired',
      detail: 'Past maturity and grace, so the pool counts it at its impaired share. It waits on repayment, collection from the borrower, or the buy-back once its units become burn debt.',
    };
  }
  if (now >= loan.maturityTime) {
    return {
      status: 'Matured',
      detail: `In grace until ${formatTime(loan.maturityTime + graceSeconds(pool))}, at the late rate. Anyone may collect it from the borrower’s USDC account, or repay it.`,
    };
  }
  if (loan.staleDiscount > 0n) {
    return { status: 'Marked stale', detail: `Nobody could read its token this epoch. It matures ${formatTime(loan.maturityTime)}.` };
  }
  return { status: 'Performing', detail: `It matures ${formatTime(loan.maturityTime)}. Repay any time before then and nothing is collected.` };
}

/** What the pool may take from the borrower's USDC account now. */
/** `owedThroughGrace` is what all the borrower's open loans in the pool owe through grace, which one approval covers. */
type Collection = { approved: bigint; balance: bigint; owedThroughGrace: bigint };

function Actions({
  signer,
  at,
  loanAddress,
  loan,
  repayAll,
  collection,
  now,
  onDone,
}: {
  signer: TransactionSendingSigner;
  at: PoolAt;
  loanAddress: Address;
  loan: Loan;
  repayAll: bigint;
  collection: Collection;
  now: bigint;
  onDone: () => void;
}) {
  const [amount, setAmount] = useState('');
  const d = at.pool.quoteDecimals;
  const repay = parseAmount(amount, d);
  const isBorrower = signer.address === loan.borrower;
  const isMatured = now >= loan.maturityTime;
  const owed = loan.isOpen ? owedOnLoan(loan, now) : 0n;
  const collectable = [owed, collection.approved, collection.balance].reduce((a, b) => (a < b ? a : b));
  return (
    <div className="panel-grid">
      {loan.isOpen && (
        <div className="panel">
          <h3>Collect from the borrower</h3>
          <p className="muted">
            Once the loan matures, anyone may take what it owes from the borrower’s USDC account, up to what they approved when they borrowed and
            what the account holds. Interest goes first; once nothing is owed the loan closes and its units wait for the borrower.
          </p>
          <p className="muted small">
            {isMatured
              ? collectable > 0n
                ? `${formatAmount(collectable, d)} USDC can be collected now.`
                : collection.approved === 0n
                  ? 'The borrower’s USDC account doesn’t approve this pool: the approval was revoked, or another app or pool was approved instead. Nothing can be collected.'
                  : 'The borrower’s USDC account is empty, so nothing can be collected.'
              : `It can be collected from ${formatTime(loan.maturityTime)}.`}
          </p>
          <ActionButton
            label="Collect"
            disabled={!isMatured || collectable === 0n}
            run={async () => sendInstructions(signer, await buildCollectFromBorrower(signer, at, loanAddress, loan))}
            onDone={onDone}
          />
        </div>
      )}
      {loan.isOpen && isBorrower && collection.approved < collection.owedThroughGrace && (
        <div className="panel">
          <h3>Approve collection again</h3>
          <p className="muted">
            Your USDC account approves the pool for {formatAmount(collection.approved, d)} USDC, less than the {formatAmount(collection.owedThroughGrace, d)} USDC
            your open loans in this pool will owe through grace, this one among them; approving again covers them all. An account approves one program at a time, so approving another app or pool replaces this one.
          </p>
          <ActionButton label="Approve" run={async () => sendInstructions(signer, await buildRenewCollection(signer, at, BigInt(Math.floor(Date.now() / 1000))))} onDone={onDone} />
        </div>
      )}
      {loan.isOpen && (
        <div className="panel">
          <h3>Repay</h3>
          <p className="muted">
            Anyone may repay a loan, interest first. Once nothing is owed it closes, and its units wait in its vault for the borrower. The pool
            takes no more than is owed when the payment lands, so an amount above it is safe: the rest stays in your account.
          </p>
          <AmountField label="Amount" unit="USDC" value={amount} onChange={setAmount} hint={<button className="link" onClick={() => setAmount(formatAmount(repayAll, d, d).replace(/,/g, ''))}>Repay all</button>} />
          <ActionButton
            label="Repay"
            disabled={!repay}
            run={async () => sendInstructions(signer, await buildRepay(signer, at, loanAddress, loan, repay!))}
            onDone={onDone}
          />
        </div>
      )}
      <div className="panel">
        <h3>Sync</h3>
        <p className="muted">
          Sets USDC paid for the loan's units (a buy-back, the collateral agent's redemption) against it, and records the units left in its vault.
          Anyone may.
        </p>
        <ActionButton label="Sync the loan" run={async () => sendInstructions(signer, await buildSyncLoan(signer, at, loanAddress, loan))} onDone={onDone} />
      </div>
      {!loan.isOpen && isBorrower && (
        <div className="panel">
          <h3>Withdraw units</h3>
          <p className="muted">Moves the closed loan's units back to your account for the token.</p>
          <ActionButton label="Withdraw units" run={async () => sendInstructions(signer, await buildWithdrawUnits(signer, at, loan))} onDone={onDone} />
        </div>
      )}
    </div>
  );
}

/** One loan: where it stands, what it owes, its units, and collecting, repaying, syncing and withdrawing. */
export function LoanScreen({ poolAddress, pool, loanAddress }: { poolAddress: Address; pool: Pool; loanAddress: Address }) {
  const now = useNow();
  const state = useLoad(async () => {
    const loan = await fetchMaybeLoan(rpc, loanAddress);
    if (!loan.exists || loan.data.pool !== poolAddress) {
      throw new Error('No loan of this pool at that address.');
    }
    const [vault, proceeds, mint, borrowerQuote, [poolAuthority], owedThroughGrace] = await Promise.all([
      findTokenAccount(loanAddress, loan.data.collateralMint, TOKEN_2022_PROGRAM_ADDRESS).then(getTokenAccount),
      findTokenAccount(loanAddress, pool.quoteMint, TOKEN_PROGRAM_ADDRESS).then(getTokenAccount),
      getMint(loan.data.collateralMint),
      findTokenAccount(loan.data.borrower, pool.quoteMint, TOKEN_PROGRAM_ADDRESS).then(getTokenAccount),
      findPoolAuthorityPda({ pool: poolAddress }),
      getOwedThroughGraceByBorrower({ address: poolAddress, pool }, loan.data.borrower, BigInt(Math.floor(Date.now() / 1000))),
    ]);
    const collection: Collection = {
      approved: borrowerQuote?.delegate === poolAuthority ? borrowerQuote.delegatedAmount : 0n,
      balance: borrowerQuote?.amount ?? 0n,
      owedThroughGrace,
    };
    return { loan: loan.data, vaultUnits: vault?.amount ?? 0n, proceeds: proceeds?.amount ?? 0n, mintDecimals: mint.decimals, collection };
  }, [loanAddress, poolAddress, pool]);
  const d = pool.quoteDecimals;
  return (
    <section>
      <PoolTabs poolAddress={poolAddress} active="overview" />
      <header className="page-header">
        <h1>Loan {shortenAddress(loanAddress)}</h1>
      </header>
      {!state.data && (state.error ? <ErrorNote error={state.error} /> : <Loading what="the loan" />)}
      {state.data && (
        <>
          <p className="notice">
            <strong>{describeLoanState(pool, state.data.loan, now, state.data.vaultUnits).status}.</strong>{' '}
            {describeLoanState(pool, state.data.loan, now, state.data.vaultUnits).detail}
          </p>
          <div className="stats">
            <Stat label="Owes now" value={`${formatAmount(state.data.loan.isOpen ? owedOnLoan(state.data.loan, now) : 0n, d)} USDC`} />
            <Stat label="Principal" value={`${formatAmount(state.data.loan.principal, d)} USDC`} />
            <Stat label="Rate" value={formatBps(state.data.loan.rateBps)} hint={`${formatBps(state.data.loan.lateRateBps)} past maturity`} />
            <Stat label="Matures" value={formatTime(state.data.loan.maturityTime)} />
            <Stat label="Units in vault" value={formatAmount(state.data.vaultUnits, state.data.mintDecimals)} />
            <Stat label="Paid for its units" value={`${formatAmount(state.data.proceeds, d)} USDC`} hint="waiting to be synced" />
            <Stat label="Approved to collect" value={`${formatAmount(state.data.collection.approved, d)} USDC`} hint="from the borrower's USDC account" />
            <Stat label="Borrower holds" value={`${formatAmount(state.data.collection.balance, d)} USDC`} />
          </div>
          <dl className="facts">
            <dt>Token</dt>
            <dd><AddressLink address={state.data.loan.collateralMint} /></dd>
            <dt>Borrower</dt>
            <dd><AddressLink address={state.data.loan.borrower} /></dd>
          </dl>
          <WithSigner prompt="Connect a wallet to collect, repay or sync this loan.">
            {(signer) => (
              <Actions
                signer={signer}
                at={{ address: poolAddress, pool }}
                loanAddress={loanAddress}
                loan={state.data!.loan}
                repayAll={state.data!.loan.isOpen ? owedOnLoan(state.data!.loan, now + REPAY_ALL_MARGIN_SECONDS) : 0n}
                collection={state.data!.collection}
                now={now}
                onDone={state.reload}
              />
            )}
          </WithSigner>
        </>
      )}
    </section>
  );
}
