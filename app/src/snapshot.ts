// A token's snapshot, layout version 1 (docs/snapshot.md), and where a
// signer serves it with its proof.

export const SNAPSHOT_V1_LEN = 168;

/** A snapshot's fields, as `token_snapshot::Snapshot` reads them. */
export type Snapshot = {
  version: number;
  network: string;
  mint: Uint8Array;
  asOf: bigint;
  reportHash: Uint8Array;
  networkSupply: bigint;
  outstandingUnits: bigint;
  burnDebt: bigint;
  burnDebtOverdue: boolean;
  agingCurrent: bigint;
  agingDays1To30: bigint;
  agingDays31To60: bigint;
  agingDays61To90: bigint;
  agingOver90: bigint;
  concentrationIndexBps: number;
  disputedShareBps: number;
  weightedDaysToDue: number;
  weightedDaysToPay: number;
  weightedDaysBeyondTerms: number;
  paidOnTimeShareBps: number;
  historyShareBps: number;
};

/** The snapshot in `bytes`, or an error when it is too short or version 0. */
export function parseSnapshot(bytes: Uint8Array): Snapshot {
  if (bytes.length < SNAPSHOT_V1_LEN) {
    throw new Error('snapshot too short');
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let at = 0;
  const u8 = () => view.getUint8(at++);
  const u16 = () => {
    const v = view.getUint16(at, true);
    at += 2;
    return v;
  };
  const u64 = () => {
    const v = view.getBigUint64(at, true);
    at += 8;
    return v;
  };
  const take = (n: number) => {
    const v = bytes.slice(at, at + n);
    at += n;
    return v;
  };
  const version = u8();
  if (version === 0) {
    throw new Error('unsupported snapshot version');
  }
  return {
    version,
    network: new TextDecoder().decode(take(16)).replace(/\0+$/, ''),
    mint: take(32),
    asOf: BigInt.asIntN(64, u64()),
    reportHash: take(32),
    networkSupply: u64(),
    outstandingUnits: u64(),
    burnDebt: u64(),
    burnDebtOverdue: u8() !== 0,
    agingCurrent: u64(),
    agingDays1To30: u64(),
    agingDays31To60: u64(),
    agingDays61To90: u64(),
    agingOver90: u64(),
    concentrationIndexBps: u16(),
    disputedShareBps: u16(),
    weightedDaysToDue: u16(),
    weightedDaysToPay: u16(),
    weightedDaysBeyondTerms: u16(),
    paidOnTimeShareBps: u16(),
    historyShareBps: u16(),
  };
}

/** A token's snapshot under one root, with the proof a pool checks it by. */
export type ProvenSnapshot = {
  root: Uint8Array;
  snapshotBytes: Uint8Array;
  proof: Uint8Array[];
  snapshot: Snapshot;
};
