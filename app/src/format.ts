// Amounts on chain are whole base units (bigint); people read and type them
// in whole tokens.

/** `amount` base units as a decimal string with at most `shown` decimals. */
export function formatAmount(amount: bigint, decimals: number, shown = 2): string {
  const negative = amount < 0n;
  const abs = negative ? -amount : amount;
  const scale = 10n ** BigInt(decimals);
  const whole = abs / scale;
  const fraction = (abs % scale).toString().padStart(decimals, '0').slice(0, shown).replace(/0+$/, '');
  const grouped = whole.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ',');
  return `${negative ? '-' : ''}${grouped}${fraction ? `.${fraction}` : ''}`;
}

/**
 * The base units `text` names in whole tokens, or undefined when it isn't a
 * positive amount with at most `decimals` decimals.
 */
export function parseAmount(text: string, decimals: number): bigint | undefined {
  const match = /^(\d*)(?:\.(\d*))?$/.exec(text.trim().replace(/,/g, ''));
  if (!match || (match[1] === '' && !match[2])) {
    return undefined;
  }
  const fraction = match[2] ?? '';
  if (fraction.length > decimals) {
    return undefined;
  }
  const units = BigInt(match[1] || '0') * 10n ** BigInt(decimals) + BigInt(fraction.padEnd(decimals, '0') || '0');
  return units > 0n ? units : undefined;
}

/** Basis points as a percentage: 1250 is "12.5%". */
export function formatBps(bps: number | bigint): string {
  return `${(Number(bps) / 100).toLocaleString(undefined, { maximumFractionDigits: 2 })}%`;
}

/** Unix seconds as a date and time in the reader's locale. */
export function formatTime(unixSeconds: bigint | number): string {
  return new Date(Number(unixSeconds) * 1000).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
}

/** Seconds as the largest whole unit that fits: "7 days", "1 hour". */
export function formatDuration(seconds: bigint | number): string {
  const s = Number(seconds);
  const units: [number, string][] = [
    [86_400, 'day'],
    [3_600, 'hour'],
    [60, 'minute'],
  ];
  for (const [size, name] of units) {
    if (s >= size && s % size === 0) {
      const n = s / size;
      return `${n} ${name}${n === 1 ? '' : 's'}`;
    }
  }
  return `${s} seconds`;
}

/** An address shortened for a list: "A5yY…QpQFD". */
export function shortenAddress(address: string): string {
  return `${address.slice(0, 4)}…${address.slice(-5)}`;
}

export function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

export function fromHex(hex: string): Uint8Array {
  if (hex.length % 2 !== 0 || /[^0-9a-f]/i.test(hex)) {
    throw new Error('not hex');
  }
  return Uint8Array.from(hex.match(/../g) ?? [], (b) => parseInt(b, 16));
}
