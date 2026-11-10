import type { Address, TransactionSendingSigner } from '@solana/kit';
import { TOKEN_PROGRAM_ADDRESS } from '@solana-program/token';
import { useState } from 'react';

import { buildRepay, buildSyncLoan, buildWithdrawUnits, type PoolAt } from '../actions';
import { findTokenAccount, getMint, getTokenAccount, rpc, sendInstructions, TOKEN_2022_PROGRAM_ADDRESS } from '../chain';
import { owedOnLoan } from '../engine';
import { fetchMaybeLoan, type Loan, type Pool } from '../generated/receivables-pool';
import { formatAmount, formatBps, formatTime, parseAmount, shortenAddress } from '../format';
import { useLoad, useNow } from '../hooks';
import { ActionButton, AddressLink, AmountField, ErrorNote, Loading, Stat } from '../ui';
import { WithSigner } from '../wallet';
import { describeLoan, PoolTabs } from './Pool';

/**
 * How far ahead "Repay all" counts interest. `repay` takes at most what is
 * owed when it lands, so a day's margin closes the loan however long the
 * wallet stays open, and costs the payer nothing extra.
 */
const REPAY_ALL_MARGIN_SECONDS = 86_400n;

function Actions({ signer, at, loanAddress, loan, repayAll, onDone }: { signer: TransactionSendingSigner; at: PoolAt; loanAddress: Address; loan: Loan; repayAll: bigint; onDone: () => void }) {
  const [amount, setAmount] = useState('');
  const d = at.pool.quoteDecimals;
  const repay = parseAmount(amount, d);
  const isBorrower = signer.address === loan.borrower;
  return (
    <div className="panel-grid">
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

/** One loan: what it owes, its units, and repaying, syncing and withdrawing. */
export function LoanScreen({ poolAddress, pool, loanAddress }: { poolAddress: Address; pool: Pool; loanAddress: Address }) {
  const now = useNow();
  const state = useLoad(async () => {
    const loan = await fetchMaybeLoan(rpc, loanAddress);
    if (!loan.exists || loan.data.pool !== poolAddress) {
      throw new Error('No loan of this pool at that address.');
    }
    const [vault, proceeds, mint] = await Promise.all([
      findTokenAccount(loanAddress, loan.data.collateralMint, TOKEN_2022_PROGRAM_ADDRESS).then(getTokenAccount),
      findTokenAccount(loanAddress, pool.quoteMint, TOKEN_PROGRAM_ADDRESS).then(getTokenAccount),
      getMint(loan.data.collateralMint),
    ]);
    return { loan: loan.data, vaultUnits: vault?.amount ?? 0n, proceeds: proceeds?.amount ?? 0n, mintDecimals: mint.decimals };
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
          <div className="stats">
            <Stat label="Status" value={describeLoan(pool, state.data.loan)} />
            <Stat label="Owes now" value={`${formatAmount(state.data.loan.isOpen ? owedOnLoan(state.data.loan, now) : 0n, d)} USDC`} />
            <Stat label="Principal" value={`${formatAmount(state.data.loan.principal, d)} USDC`} />
            <Stat label="Rate" value={formatBps(state.data.loan.rateBps)} hint={`${formatBps(state.data.loan.lateRateBps)} past maturity`} />
            <Stat label="Matures" value={formatTime(state.data.loan.maturityTime)} />
            <Stat label="Units in vault" value={formatAmount(state.data.vaultUnits, state.data.mintDecimals)} />
            <Stat label="Paid for its units" value={`${formatAmount(state.data.proceeds, d)} USDC`} hint="waiting to be synced" />
          </div>
          <dl className="facts">
            <dt>Token</dt>
            <dd><AddressLink address={state.data.loan.collateralMint} /></dd>
            <dt>Borrower</dt>
            <dd><AddressLink address={state.data.loan.borrower} /></dd>
          </dl>
          <WithSigner prompt="Connect a wallet to repay or sync this loan.">
            {(signer) => (
              <Actions
                signer={signer}
                at={{ address: poolAddress, pool }}
                loanAddress={loanAddress}
                loan={state.data!.loan}
                repayAll={state.data!.loan.isOpen ? owedOnLoan(state.data!.loan, now + REPAY_ALL_MARGIN_SECONDS) : 0n}
                onDone={state.reload}
              />
            )}
          </WithSigner>
        </>
      )}
    </section>
  );
}
