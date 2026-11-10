// Everything the app reads from the chain, and how it sends a transaction.
// Pools and loans are read from the program itself, so which pools show
// depends on nobody's list.

import {
  address,
  appendTransactionMessageInstructions,
  compileTransaction,
  createSolanaRpc,
  createTransactionMessage,
  fetchEncodedAccount,
  getBase58Decoder,
  getBase64EncodedWireTransaction,
  getBase64Encoder,
  getProgramDerivedAddress,
  isTransactionMessageWithinSizeLimit,
  getAddressEncoder,
  getU64Encoder,
  pipe,
  setTransactionMessageFeePayerSigner,
  setTransactionMessageLifetimeUsingBlockhash,
  signAndSendTransactionMessageWithSigners,
  type Address,
  type Base58EncodedBytes,
  type Blockhash,
  type ReadonlyUint8Array,
  type Instruction,
  type Signature,
  type TransactionSendingSigner,
  type TransactionSigner,
} from '@solana/kit';
import { AccountState, findAssociatedTokenPda, getMintDecoder, getTokenDecoder } from '@solana-program/token';

import { config } from './config';
import { getProvenSnapshot } from './api';
import { toHex } from './format';
import {
  getLoanDecoder,
  getPoolDecoder,
  getReceivablesPoolErrorMessage,
  LOAN_DISCRIMINATOR,
  POOL_DISCRIMINATOR,
  RECEIVABLES_POOL_PROGRAM_ADDRESS,
  type Loan,
  type Pool,
  type ReceivablesPoolError,
} from './generated/receivables-pool';
import { fetchMaybeSnapshotRoots, findRootsPda } from './generated/snapshot-roots';
import type { ProvenSnapshot } from './snapshot';

export const rpc = createSolanaRpc(config.rpcUrl);

export const TOKEN_2022_PROGRAM_ADDRESS = address('TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb');

/** A program account with its address. */
export type Found<T> = { address: Address; data: T };

/** The network a pool names, decoded from its padded ASCII. */
export function readPoolNetwork(pool: Pool): string {
  return new TextDecoder().decode(pool.network).replace(/\0+$/, '');
}

/** The keys whose roots the pool reads. */
export function listPoolSigners(pool: Pool): Address[] {
  return pool.signers.slice(0, pool.signerCount);
}

/** The loan terms the pool offers, in epochs. */
export function listPoolMaturities(pool: Pool): number[] {
  return pool.maturities.slice(0, pool.maturityCount);
}

function toBase58(bytes: ReadonlyUint8Array): Base58EncodedBytes {
  return getBase58Decoder().decode(bytes) as Base58EncodedBytes;
}

async function getProgramAccounts(filters: { offset: bigint; bytes: Base58EncodedBytes }[]) {
  const accounts = await rpc
    .getProgramAccounts(RECEIVABLES_POOL_PROGRAM_ADDRESS, {
      encoding: 'base64',
      filters: filters.map((f) => ({ memcmp: { offset: f.offset, bytes: f.bytes, encoding: 'base58' as const } })),
    })
    .send();
  return accounts.map((a) => ({ address: a.pubkey, bytes: getBase64Encoder().encode(a.account.data[0]) }));
}

/** Every pool of the program that lends on this app's network. */
export async function listPools(): Promise<Found<Pool>[]> {
  const accounts = await getProgramAccounts([{ offset: 0n, bytes: toBase58(POOL_DISCRIMINATOR) }]);
  return accounts
    .map((a) => ({ address: a.address, data: getPoolDecoder().decode(a.bytes) }))
    .filter((p) => readPoolNetwork(p.data) === config.network);
}

/** The pool at `poolAddress`, or undefined when it isn't one. */
export async function getPool(poolAddress: Address): Promise<Pool | undefined> {
  const account = await fetchEncodedAccount(rpc, poolAddress);
  if (!account.exists || account.programAddress !== RECEIVABLES_POOL_PROGRAM_ADDRESS) {
    return undefined;
  }
  const bytes = account.data;
  if (POOL_DISCRIMINATOR.some((b, i) => bytes[i] !== b)) {
    return undefined;
  }
  return getPoolDecoder().decode(bytes);
}

/** Every loan a pool has made, open or closed: one per borrower per token. */
export async function listLoans(poolAddress: Address): Promise<Found<Loan>[]> {
  // A loan's pool follows its 8-byte discriminator and 1-byte version.
  const accounts = await getProgramAccounts([
    { offset: 0n, bytes: toBase58(LOAN_DISCRIMINATOR) },
    { offset: 9n, bytes: poolAddress as string as Base58EncodedBytes },
  ]);
  return accounts.map((a) => ({ address: a.address, data: getLoanDecoder().decode(a.bytes) }));
}

/** A mint's supply and decimals; a Token-2022 mint's base fields read the same. */
export type MintFacts = { supply: bigint; decimals: number; programAddress: Address };

export async function getMint(mint: Address): Promise<MintFacts> {
  const account = await fetchEncodedAccount(rpc, mint);
  if (!account.exists) {
    throw new Error(`no mint at ${mint}`);
  }
  const decoded = getMintDecoder().decode(account.data.slice(0, 82));
  return { supply: decoded.supply, decimals: decoded.decimals, programAddress: account.programAddress };
}

/** A token account's balance and whether it is frozen, or undefined when it doesn't exist. */
export type TokenAccountFacts = { mint: Address; owner: Address; amount: bigint; isFrozen: boolean };

export async function getTokenAccount(tokenAccount: Address): Promise<TokenAccountFacts | undefined> {
  const account = await fetchEncodedAccount(rpc, tokenAccount);
  if (!account.exists) {
    return undefined;
  }
  const t = getTokenDecoder().decode(account.data.slice(0, 165));
  return { mint: t.mint, owner: t.owner, amount: t.amount, isFrozen: t.state === AccountState.Frozen };
}

/** The associated token account of `owner` for `mint`. */
export async function findTokenAccount(owner: Address, mint: Address, tokenProgram: Address): Promise<Address> {
  const [ata] = await findAssociatedTokenPda({ owner, mint, tokenProgram });
  return ata;
}

/** `owner`'s Token-2022 balances above zero: the receivables tokens it may pledge among them. */
export async function listToken2022Holdings(owner: Address): Promise<Found<TokenAccountFacts>[]> {
  const { value } = await rpc
    .getTokenAccountsByOwner(owner, { programId: TOKEN_2022_PROGRAM_ADDRESS }, { encoding: 'base64' })
    .send();
  return value
    .map((a) => {
      const t = getTokenDecoder().decode(getBase64Encoder().encode(a.account.data[0]).slice(0, 165));
      return {
        address: a.pubkey,
        data: { mint: t.mint, owner: t.owner, amount: t.amount, isFrozen: t.state === AccountState.Frozen },
      };
    })
    .filter((h) => h.data.amount > 0n);
}

/** The record of a pool's withdrawal generation, which `settle_epoch` may write. */
export async function findGenerationRecord(poolAddress: Address, generation: bigint): Promise<Address> {
  const [record] = await getProgramDerivedAddress({
    programAddress: RECEIVABLES_POOL_PROGRAM_ADDRESS,
    seeds: ['generation', getAddressEncoder().encode(poolAddress), getU64Encoder().encode(generation)],
  });
  return record;
}

/** A signer's latest root, from its account in the snapshot-roots program. */
export type LatestRoot = { signer: Address; rootsAccount: Address; root: Uint8Array; asOf: bigint };

/**
 * The newest of the latest roots the pool's signers posted, which is the
 * only root the pool reads a token's facts from, or undefined before any
 * signer has posted.
 */
export async function getLatestRoot(pool: Pool): Promise<LatestRoot | undefined> {
  let newest: LatestRoot | undefined;
  for (const signer of listPoolSigners(pool)) {
    const [rootsAccount] = await findRootsPda({ signer });
    const roots = await fetchMaybeSnapshotRoots(rpc, rootsAccount);
    if (!roots.exists || roots.data.posted === 0n) {
      continue;
    }
    const latest = roots.data.roots[Number((roots.data.posted - 1n) % BigInt(roots.data.roots.length))];
    if (!newest || latest.asOf > newest.asOf) {
      newest = { signer, rootsAccount, root: Uint8Array.from(latest.root), asOf: latest.asOf };
    }
  }
  return newest;
}

/** A token's facts under the pool's latest root, ready to hand the pool. */
export type TokenProof = { latest: LatestRoot; proven: ProvenSnapshot };

/**
 * The token's snapshot and proof under the newest root the pool's signers
 * posted, or why there is none: no signer has posted, or the tree under
 * that root doesn't hold the token.
 */
export async function proveToken(pool: Pool, mint: Address): Promise<TokenProof | { missing: string }> {
  const latest = await getLatestRoot(pool);
  if (!latest) {
    return { missing: "None of this pool's signers has posted a root yet." };
  }
  const proven = await getProvenSnapshot(mint, toHex(latest.root));
  if (!proven) {
    return { missing: "This token isn't under the signer's latest root: it isn't a receivables token, or it was tokenized after the root was built." };
  }
  return { latest, proven };
}

/** A failed simulation or send, with the program's own reason when it gave one. */
export class TransactionError extends Error {
  constructor(
    message: string,
    readonly logs: readonly string[] = [],
  ) {
    super(message);
  }
}

/** The pool program's message for a custom error code, or undefined. */
function describeProgramError(code: number): string | undefined {
  return getReceivablesPoolErrorMessage(code as ReceivablesPoolError) || undefined;
}

function describeFailure(err: unknown, logs: readonly string[]): string {
  const custom = JSON.stringify(err, (_, v) => (typeof v === 'bigint' ? Number(v) : v)).match(/"Custom":(\d+)/);
  if (custom) {
    const reason = describeProgramError(Number(custom[1]));
    if (reason) {
      return reason;
    }
  }
  const logged = logs.find((l) => l.includes('Error Message:'));
  return logged ? logged.split('Error Message:')[1].trim() : `The transaction failed: ${JSON.stringify(err, (_, v) => (typeof v === 'bigint' ? Number(v) : v))}`;
}

/** A blockhash that only sizes a transaction: every blockhash is 32 bytes. */
const SIZING_LIFETIME = { blockhash: '11111111111111111111111111111111' as Blockhash, lastValidBlockHeight: 0n };

/** Whether `instructions`, paid by `feePayer`, fit in one transaction. */
export function isWithinOneTransaction(feePayer: TransactionSigner, instructions: Instruction[]): boolean {
  return isTransactionMessageWithinSizeLimit(
    pipe(
      createTransactionMessage({ version: 0 }),
      (m) => setTransactionMessageFeePayerSigner(feePayer, m),
      (m) => setTransactionMessageLifetimeUsingBlockhash(SIZING_LIFETIME, m),
      (m) => appendTransactionMessageInstructions(instructions, m),
    ),
  );
}

/**
 * Simulates `instructions` as one transaction paid by `signer`, then asks
 * the wallet to sign and send it, and waits until it is confirmed. A
 * simulation that fails is a TransactionError with the program's reason,
 * and nothing is signed.
 */
export async function sendInstructions(signer: TransactionSendingSigner, instructions: Instruction[]): Promise<Signature> {
  const { value: blockhash } = await rpc.getLatestBlockhash().send();
  const message = pipe(
    createTransactionMessage({ version: 0 }),
    (m) => setTransactionMessageFeePayerSigner(signer, m),
    (m) => setTransactionMessageLifetimeUsingBlockhash(blockhash, m),
    (m) => appendTransactionMessageInstructions(instructions, m),
  );

  const unsigned = getBase64EncodedWireTransaction(compileTransaction(message));
  const { value: simulation } = await rpc
    .simulateTransaction(unsigned, { encoding: 'base64', sigVerify: false, replaceRecentBlockhash: true })
    .send();
  if (simulation.err) {
    const logs = simulation.logs ?? [];
    throw new TransactionError(describeFailure(simulation.err, logs), logs);
  }

  const signature = getBase58Decoder().decode(await signAndSendTransactionMessageWithSigners(message)) as Signature;
  for (let i = 0; i < 60; i++) {
    const { value } = await rpc.getSignatureStatuses([signature]).send();
    const status = value[0];
    if (status?.err) {
      throw new TransactionError(describeFailure(status.err, []));
    }
    if (status?.confirmationStatus === 'confirmed' || status?.confirmationStatus === 'finalized') {
      return signature;
    }
    await new Promise((r) => setTimeout(r, 1_000));
  }
  throw new TransactionError(`Sent, but not confirmed within a minute: look up ${signature} in an explorer.`);
}

/** An explorer link for a transaction or an address on this app's network. */
export function explorerUrl(kind: 'tx' | 'address', value: string): string {
  const cluster = config.network === 'sol-devnet' ? '?cluster=devnet' : '';
  return `https://explorer.solana.com/${kind}/${value}${cluster}`;
}
