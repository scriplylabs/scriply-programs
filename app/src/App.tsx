import { isAddress, type Address } from '@solana/kit';

import { getPool } from './chain';
import { config } from './config';
import { useLoad, useRoute } from './hooks';
import { BorrowScreen } from './screens/Borrow';
import { KeepScreen } from './screens/Keep';
import { LoanScreen } from './screens/Loan';
import { PoolScreen } from './screens/Pool';
import { PoolsScreen } from './screens/Pools';
import { ErrorNote, Loading } from './ui';
import { WalletButton } from './wallet';

/** A pool's screens, once the pool is read. */
function PoolRoutes({ poolAddress, rest }: { poolAddress: Address; rest: string[] }) {
  const pool = useLoad(() => getPool(poolAddress), [poolAddress]);
  if (pool.isLoading && !pool.data) {
    return <Loading what="the pool" />;
  }
  if (!pool.data) {
    return pool.error ? <ErrorNote error={pool.error} /> : <p className="error">No pool of the program at {poolAddress}.</p>;
  }
  const [screen, id] = rest;
  switch (screen) {
    case 'borrow':
      return <BorrowScreen poolAddress={poolAddress} pool={pool.data} />;
    case 'keep':
      return <KeepScreen poolAddress={poolAddress} pool={pool.data} reload={pool.reload} />;
    case 'loans':
      return id && isAddress(id) ? <LoanScreen poolAddress={poolAddress} pool={pool.data} loanAddress={id} /> : <p className="error">Not a loan's address.</p>;
    default:
      return <PoolScreen poolAddress={poolAddress} pool={pool.data} reload={pool.reload} />;
  }
}

export function App() {
  const [first, poolAddress, ...rest] = useRoute();
  return (
    <>
      <header className="topbar">
        <a href="#/" className="brand">Receivables pools</a>
        <span className="network">{config.network}</span>
        <WalletButton />
      </header>
      <main>
        {first === 'pools' && poolAddress && isAddress(poolAddress) ? <PoolRoutes key={poolAddress} poolAddress={poolAddress} rest={rest} /> : <PoolsScreen />}
      </main>
      <footer className="footer muted">
        Open source: the program, its rules and this app are in{' '}
        <a href="https://github.com/scriplylabs/scriply-programs" target="_blank" rel="noreferrer">scriply-programs</a>. Unaudited; devnet only.
      </footer>
    </>
  );
}
