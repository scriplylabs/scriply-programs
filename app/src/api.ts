// The signer's public API: a token's snapshot with its proof, and asking
// for a loan vault's thaw. Anyone may host this app against any signer that
// serves the same routes.

import { config } from './config';
import { fromHex } from './format';
import { parseSnapshot, type ProvenSnapshot } from './snapshot';

/**
 * The token's snapshot under `root` (hex), from the signer's API, or
 * undefined when the tree under that root doesn't hold the token: it isn't
 * a receivables token, or it was tokenized after the root was built.
 */
export async function getProvenSnapshot(token: string, root: string): Promise<ProvenSnapshot | undefined> {
  const url = `${config.snapshotApiUrl}/v1/public/receivables-tokens/${config.network}/${token}/snapshots?root=${root}`;
  const res = await fetch(url);
  if (res.status === 404) {
    return undefined;
  }
  if (!res.ok) {
    throw new Error(`the snapshot API answered ${res.status}`);
  }
  const body = (await res.json()) as { root: string; snapshot: string; proof: string[] };
  const snapshotBytes = fromHex(body.snapshot);
  return {
    root: fromHex(body.root),
    snapshotBytes,
    proof: body.proof.map(fromHex),
    snapshot: parseSnapshot(snapshotBytes),
  };
}

/**
 * Asks the signer to thaw the vault of the borrower's loan against `token`
 * in `pool`. It answers once the request is queued: the vault is thawed
 * when it reads thawed on chain. Refused when the thaw rule fails
 * (README: "A borrower's vault must be thawed").
 */
export async function requestLoanVaultThaw(pool: string, token: string, borrower: string): Promise<void> {
  const res = await fetch(`${config.snapshotApiUrl}/v1/public/pools/${config.network}/${pool}/loan-vault-thaws`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ tokenAddress: token, borrower }),
  });
  if (!res.ok) {
    const problem = (await res.json().catch(() => ({}))) as { detail?: string };
    throw new Error(problem.detail ?? `the thaw request was refused (${res.status})`);
  }
}
