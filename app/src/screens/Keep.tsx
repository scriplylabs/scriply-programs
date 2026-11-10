import type { Address, TransactionSendingSigner } from '@solana/kit';

import { buildMarkLoanBatches, buildMarkStale, buildSettleEpoch, type PoolAt } from '../actions';
import { listLoans, proveToken, sendInstructions, type Found } from '../chain';
import { currentEpoch, epochStart, isLoanImpaired } from '../engine';
import type { Loan, Pool } from '../generated/receivables-pool';
import { formatTime, shortenAddress } from '../format';
import { useLoad, useNow } from '../hooks';
import { ActionButton, ErrorNote, Loading } from '../ui';
import { WithSigner } from '../wallet';
import { EpochStatus, PoolTabs } from './Pool';

/** Loans a mark stale sends at once; each is one writable account. */
const STALE_PER_TRANSACTION = 20;

/** The open, performing loans not yet marked in the settled epoch, by token. */
function groupUnmarked(pool: Pool, loans: Found<Loan>[]): Map<Address, Address[]> {
  const byMint = new Map<Address, Address[]>();
  for (const { address, data } of loans) {
    if (!data.isOpen || isLoanImpaired(pool, data) || data.markedEpoch === pool.settledEpoch) {
      continue;
    }
    byMint.set(data.collateralMint, [...(byMint.get(data.collateralMint) ?? []), address]);
  }
  return byMint;
}

function Keeper({ signer, at, now, onDone }: { signer: TransactionSendingSigner; at: PoolAt; now: bigint; onDone: () => void }) {
  const { pool } = at;
  const loans = useLoad(() => listLoans(at.address), [at.address, pool]);
  const isEpochOpen = pool.settledEpoch === currentEpoch(pool, now);
  const windowCloses = epochStart(pool, pool.settledEpoch) + pool.markWindowSeconds;
  const unmarked = loans.data && isEpochOpen ? groupUnmarked(pool, loans.data) : new Map<Address, Address[]>();
  const unmarkedLoans = [...unmarked.values()].flat();
  return (
    <>
      <div className="panel">
        <h3>Settle the epoch</h3>
        <p className="muted">
          Starts a new epoch (impairing loans past maturity and grace) and, once every loan is marked, pays the withdrawals asked for. It
          does nothing when there is nothing to do, so it is safe to send any time. You pay the rent of a generation record when a settlement
          writes one.
        </p>
        <ActionButton label="Settle" run={async () => sendInstructions(signer, [await buildSettleEpoch(signer, at)])} onDone={onDone} />
      </div>
      <div className="panel">
        <h3>Mark loans</h3>
        <p className="muted">
          Reads each open loan's token from its signer's latest root, so NAV counts it at what its facts say. Each transaction carries one
          token's proof, so a token with many loans takes several.
        </p>
        {!loans.data && (loans.error ? <ErrorNote error={loans.error} /> : <Loading what="the pool's loans" />)}
        {loans.data && !isEpochOpen && <p className="muted">Settle the new epoch first.</p>}
        {loans.data && isEpochOpen && unmarked.size === 0 && <p className="ok">Every performing loan is marked this epoch.</p>}
        {[...unmarked.entries()].map(([mint, loanAddresses]) => (
          <div key={mint} className="row">
            <span>
              <span className="mono">{shortenAddress(mint)}</span> · {loanAddresses.length} loan{loanAddresses.length === 1 ? '' : 's'}
            </span>
            <ActionButton
              label="Mark"
              run={async () => {
                const proof = await proveToken(pool, mint);
                if ('missing' in proof) {
                  throw new Error(proof.missing);
                }
                let last;
                for (const batch of await buildMarkLoanBatches(signer, at, proof, loanAddresses)) {
                  last = await sendInstructions(signer, batch);
                }
                return last;
              }}
              onDone={() => {
                loans.reload();
                onDone();
              }}
            />
          </div>
        ))}
      </div>
      <div className="panel">
        <h3>Mark stale</h3>
        <p className="muted">
          Once the marking window closes ({formatTime(windowCloses)}), a loan nobody could mark is counted at the pool's impaired share for
          the rest of the epoch, so withdrawals never wait on a token whose facts nobody can prove.
        </p>
        <ActionButton
          label={`Mark ${unmarkedLoans.length} stale`}
          disabled={!isEpochOpen || now < windowCloses || unmarkedLoans.length === 0}
          run={async () => {
            let last;
            for (let i = 0; i < unmarkedLoans.length; i += STALE_PER_TRANSACTION) {
              last = await sendInstructions(signer, await buildMarkStale(signer, at, unmarkedLoans.slice(i, i + STALE_PER_TRANSACTION)));
            }
            return last;
          }}
          onDone={() => {
            loans.reload();
            onDone();
          }}
        />
      </div>
    </>
  );
}

/** Keeping a pool: what anyone may send to move its epoch on. */
export function KeepScreen({ poolAddress, pool, reload }: { poolAddress: Address; pool: Pool; reload: () => void }) {
  const now = useNow(5_000);
  return (
    <section>
      <PoolTabs poolAddress={poolAddress} active="keep" />
      <header className="page-header">
        <h1>Keep {shortenAddress(poolAddress)}</h1>
        <p className="muted">
          The pool has no operator. Each epoch, someone must start it, mark every loan from its token's latest facts, and pay the withdrawals:
          a depositor who wants to withdraw, a borrower who wants to borrow, or a bot. Nothing here needs permission, and a keeper can't change a
          fact: a snapshot that isn't under the signer's root fails its proof.
        </p>
      </header>
      <EpochStatus pool={pool} now={now} />
      <WithSigner prompt="Connect a wallet to pay the fees.">{(signer) => <Keeper signer={signer} at={{ address: poolAddress, pool }} now={now} onDone={reload} />}</WithSigner>
    </section>
  );
}
