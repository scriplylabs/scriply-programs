import type { Signature } from '@solana/kit';
import { useState, type ReactNode } from 'react';

import { explorerUrl } from './chain';
import { shortenAddress } from './format';

export function Stat({ label, value, hint }: { label: string; value: ReactNode; hint?: string }) {
  return (
    <div className="stat">
      <span className="stat-label">{label}</span>
      <span className="stat-value">{value}</span>
      {hint && <span className="stat-hint">{hint}</span>}
    </div>
  );
}

export function AddressLink({ address, kind = 'address' }: { address: string; kind?: 'address' | 'tx' }) {
  return (
    <a className="mono" href={explorerUrl(kind, address)} target="_blank" rel="noreferrer" title={address}>
      {shortenAddress(address)}
    </a>
  );
}

export function Loading({ what }: { what: string }) {
  return <p className="muted">Reading {what} from the chain…</p>;
}

export function ErrorNote({ error }: { error?: string }) {
  return error ? <p className="error">{error}</p> : null;
}

/**
 * A button that runs a transaction (or several steps), shows that it is
 * under way, and then its signature or why it failed.
 */
export function ActionButton({
  label,
  run,
  onDone,
  disabled,
  className,
}: {
  label: string;
  run: () => Promise<Signature | void>;
  onDone?: () => void;
  disabled?: boolean;
  className?: string;
}) {
  const [state, setState] = useState<{ isRunning: boolean; error?: string; signature?: Signature }>({ isRunning: false });
  return (
    <div className="action">
      <button
        className={className}
        disabled={disabled || state.isRunning}
        onClick={async () => {
          setState({ isRunning: true });
          try {
            const signature = await run();
            setState({ isRunning: false, signature: signature ?? undefined });
            onDone?.();
          } catch (e) {
            setState({ isRunning: false, error: e instanceof Error ? e.message : String(e) });
          }
        }}
      >
        {state.isRunning ? 'Working…' : label}
      </button>
      {state.signature && (
        <p className="ok">
          Confirmed: <AddressLink address={state.signature} kind="tx" />
        </p>
      )}
      <ErrorNote error={state.error} />
    </div>
  );
}

export function AmountField({
  label,
  value,
  onChange,
  unit,
  hint,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  unit: string;
  hint?: ReactNode;
}) {
  return (
    <label className="field">
      <span>{label}</span>
      <span className="field-input">
        <input inputMode="decimal" placeholder="0.00" value={value} onChange={(e) => onChange(e.target.value)} />
        <span className="unit">{unit}</span>
      </span>
      {hint && <span className="field-hint">{hint}</span>}
    </label>
  );
}
