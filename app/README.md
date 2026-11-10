# Receivables pool app

A static web app for the receivables pool: anyone can host it, and it needs no server of its own. It lists every pool of the program on a network straight from the chain, so which pools show depends on nobody's list.

| Screen | What it does |
|---|---|
| Pools | Every pool on the network, with its NAV, idle USDC, terms, rates and signers |
| Lend | A pool's value and fixed rules; deposit, request a withdrawal, claim it; the pool's loans |
| Borrow | Pledge a receivables token the wallet holds. It shows the token's facts under the pool's latest root and what the risk engine allows, then creates the loan's vault, asks for it to be thawed, waits until it reads thawed on chain, and borrows |
| Loan | What a loan owes and the units in its vault; repay (anyone), sync (anyone), withdraw units (the borrower) |
| Keep | Settle the epoch, mark loans from their tokens' latest roots, mark stale the ones nobody could read |

Every transaction is simulated before the wallet is asked to sign it, and a refusal shows the program's own reason. Wallets connect through the Wallet Standard (Phantom, Solflare, Backpack, …).

`src/engine.ts` ports the program's NAV, interest and risk-engine arithmetic so the app can preview what the program will decide; the program still decides. `src/engine.test.ts` runs the Rust tests' cases against it, so a change to `risk_engine.rs` or `math.rs` needs the same change there.

## Settings

Set when the app is built (`.env.example`); nothing defaults, so a build with one missing fails on load.

| Variable | What |
|---|---|
| `VITE_NETWORK` | The network pools and snapshots name: `sol-devnet` or `sol` |
| `VITE_RPC_URL` | A Solana RPC for that network |
| `VITE_SNAPSHOT_API_URL` | Where the pools' signer serves a token's snapshot with its proof (`GET /v1/public/receivables-tokens/{network}/{token}/snapshots?root=`) and takes loan vault thaws (`POST /v1/public/pools/{network}/{pool}/loan-vault-thaws`) |

The app reads a token's proof under the newest root any of the pool's signers posted, and asks this one URL for it. A pool with two signers works only while the newest root is the one this URL serves: when the other signer's root is newer, the app says the token isn't under the signer's latest root.

## Limits

A proof adds a 32-byte hash each time the tokens in the signer's tree double. The app sends a v0 transaction (at most 1232 bytes) when one holds it, and a v1 transaction (at most 4096 bytes) when it doesn't, if the wallet says it signs v1. A borrow fits in v0 while the tree holds at most 32 tokens (5 hashes), and in v1 far past any real tree (20 hashes, about a million tokens, is 1701 bytes). Marking splits a token's loans over as many v0 transactions as they need, so it needs v1 only when one loan with its proof is too big for v0. A v1 transaction carries its own compute and account-data budgets, which the app sets from the simulation it already runs.

## Development

Needs Node 22 and npm 11 (`packageManager` in `package.json`).

```bash
cp .env.example .env.local
npm install
npm run dev         # http://localhost:5173
npm test            # the engine and snapshot tests
npm run typecheck
npm run build       # dist/, a static site
```

After changing a program, run `anchor build` in the repo root, then `npm run generate` to regenerate `src/generated/` from the IDLs, and commit both.

## Deploying

`npm run build` with the settings in the environment, then upload `dist/` to any static host. Routes are in the URL's `#`, so the host needs no rewrites. There is no deploy from CI: a build is made and uploaded by hand.
