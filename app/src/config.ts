// What a build of the app talks to, set when it is built (app/README.md).
// Nothing defaults: a build with a setting missing fails on load, rather
// than quietly pointing at a cluster or an API nobody chose.

function readSetting(name: string, value: string | undefined): string {
  if (!value) {
    throw new Error(`${name} is not set: see app/README.md`);
  }
  return value;
}

/** The networks the pool program's snapshots name, with their wallet chain. */
const chains = {
  'sol-devnet': 'solana:devnet',
  sol: 'solana:mainnet',
} as const;

export type Network = keyof typeof chains;

function readNetwork(value: string): Network {
  if (!(value in chains)) {
    throw new Error(`VITE_NETWORK must be one of ${Object.keys(chains).join(', ')}`);
  }
  return value as Network;
}

const network = readNetwork(readSetting('VITE_NETWORK', import.meta.env.VITE_NETWORK));

export const config = {
  /** The network ID snapshots and pools name ("sol-devnet"). */
  network,
  /** The wallet-standard chain wallets sign for. */
  chain: chains[network],
  rpcUrl: readSetting('VITE_RPC_URL', import.meta.env.VITE_RPC_URL),
  /** Where snapshots and their proofs are served, and loan vault thaws asked for. */
  snapshotApiUrl: readSetting('VITE_SNAPSHOT_API_URL', import.meta.env.VITE_SNAPSHOT_API_URL).replace(/\/$/, ''),
};
