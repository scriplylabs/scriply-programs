import { useCallback, useEffect, useState } from 'react';

/** A value loaded once its inputs are known, and reloaded on demand. */
export type Loaded<T> = { data?: T; error?: string; isLoading: boolean; reload: () => void };

export function useLoad<T>(load: () => Promise<T>, deps: unknown[]): Loaded<T> {
  const [state, setState] = useState<{ data?: T; error?: string; isLoading: boolean }>({ isLoading: true });
  const [version, setVersion] = useState(0);
  const run = useCallback(load, deps);
  useEffect(() => {
    let isCurrent = true;
    setState((s) => ({ ...s, isLoading: true }));
    run().then(
      (data) => isCurrent && setState({ data, isLoading: false }),
      (e: unknown) => isCurrent && setState({ error: e instanceof Error ? e.message : String(e), isLoading: false }),
    );
    return () => {
      isCurrent = false;
    };
  }, [run, version]);
  return { ...state, reload: () => setVersion((v) => v + 1) };
}

/** The current unix time in seconds, moving on every `everyMs`. */
export function useNow(everyMs = 1_000): bigint {
  const [now, setNow] = useState(() => BigInt(Math.floor(Date.now() / 1000)));
  useEffect(() => {
    const id = setInterval(() => setNow(BigInt(Math.floor(Date.now() / 1000))), everyMs);
    return () => clearInterval(id);
  }, [everyMs]);
  return now;
}

/** The app's location: the path after `#`, split into its parts. */
export function useRoute(): string[] {
  const read = () => window.location.hash.replace(/^#\/?/, '').split('/').filter(Boolean);
  const [route, setRoute] = useState(read);
  useEffect(() => {
    const onChange = () => setRoute(read());
    window.addEventListener('hashchange', onChange);
    return () => window.removeEventListener('hashchange', onChange);
  }, []);
  return route;
}
