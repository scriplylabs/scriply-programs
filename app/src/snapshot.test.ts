import { describe, expect, it } from 'vitest';

import vectors from '../../docs/snapshot-vectors.json';
import { fromHex, toHex } from './format';
import { parseSnapshot, SNAPSHOT_V1_LEN } from './snapshot';

describe('parseSnapshot, against docs/snapshot-vectors.json', () => {
  const first = parseSnapshot(fromHex(vectors.leaves[0].snapshot));

  it('reads the network and the mint', () => {
    expect(first.version).toBe(1);
    expect(first.network).toBe('sol-devnet');
    expect(toHex(first.mint)).toBe('01'.repeat(32));
  });

  it('reads each field at its offset', () => {
    expect(first.networkSupply).toBe(1_000_000n);
    expect(first.outstandingUnits).toBe(2_000_000n);
    expect(first.burnDebt).toBe(1_000n);
    expect(first.burnDebtOverdue).toBe(false);
    expect(first.agingCurrent).toBe(100n);
    expect(first.agingOver90).toBe(500n);
    expect(first.concentrationIndexBps).toBe(1_001);
    expect(first.paidOnTimeShareBps).toBe(8_900);
    expect(first.historyShareBps).toBe(8_100);
  });

  it('reads every vector', () => {
    for (const leaf of vectors.leaves) {
      expect(parseSnapshot(fromHex(leaf.snapshot)).version).toBeGreaterThan(0);
    }
  });

  it('refuses a short snapshot', () => {
    expect(() => parseSnapshot(new Uint8Array(SNAPSHOT_V1_LEN - 1))).toThrow();
  });
});
