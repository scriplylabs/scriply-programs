import { listPoolMaturities, listPoolSigners, listPools } from '../chain';
import { config } from '../config';
import { nav } from '../engine';
import { formatAmount, formatBps, formatDuration, shortenAddress } from '../format';
import { useLoad, useNow } from '../hooks';
import { ErrorNote, Loading } from '../ui';

/** Every pool the program holds on this network, as the chain lists them. */
export function PoolsScreen() {
  const pools = useLoad(listPools, []);
  const now = useNow(10_000);
  return (
    <section>
      <header className="page-header">
        <h1>Receivables pools</h1>
        <p className="muted">
          Every pool of the receivables-pool program on {config.network}, read from the chain. Anyone can create one, and nobody
          runs it: its parameters are fixed when it is created, and its rules decide whether, how much and at what rate to lend
          against a receivables token, from facts its signers post.
        </p>
      </header>
      {pools.isLoading && !pools.data && <Loading what="pools" />}
      <ErrorNote error={pools.error} />
      {pools.data?.length === 0 && <p className="muted">No pool lends on {config.network} yet.</p>}
      <div className="cards">
        {pools.data?.map(({ address, data: pool }) => (
          <a key={address} className="card" href={`#/pools/${address}`}>
            <div className="card-title">
              <span className="mono">{shortenAddress(address)}</span>
              {pool.requiresAttestation === 1 && <span className="tag">Attested depositors</span>}
            </div>
            <dl className="facts">
              <dt>Net asset value</dt>
              <dd>{formatAmount(nav(pool, now), pool.quoteDecimals)} USDC</dd>
              <dt>Available to lend</dt>
              <dd>{formatAmount(pool.idle, pool.quoteDecimals)} USDC</dd>
              <dt>Terms</dt>
              <dd>{listPoolMaturities(pool).map((m) => formatDuration(BigInt(m) * pool.epochSeconds)).join(', ')}</dd>
              <dt>Rate</dt>
              <dd>
                {formatBps(pool.risk.minRateBps)}–{formatBps(pool.risk.maxRateBps)} a year
              </dd>
              <dt>Advance</dt>
              <dd>up to {formatBps(Math.min(pool.risk.baseAdvanceBps, pool.risk.maxAdvanceBps))} of eligible face value</dd>
              <dt>Signers</dt>
              <dd>{listPoolSigners(pool).map((s) => shortenAddress(s)).join(', ')}</dd>
            </dl>
          </a>
        ))}
      </div>
    </section>
  );
}
