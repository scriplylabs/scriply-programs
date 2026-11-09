# scriply-programs

Solana programs for lending against receivables tokens, built with [Anchor](https://www.anchor-lang.com/). Open source under Apache-2.0.

## receivables-pool

A lending pool with no admin. Anyone creates a pool with fixed parameters, including whose posted roots of tokens' facts it trusts and its risk engine's weights, and nothing changes them afterwards. Depositors put in USDC for shares at the pool's net asset value (NAV). A holder of a receivables token pledges units with the token's snapshot proven against a root, and the risk engine (`src/risk_engine.rs`) decides how much to advance and at what rate. Every epoch, each loan is marked from its token's latest facts before withdrawals are paid.

`pools/devnet.toml` lists every parameter with what it decides, and `create-pool` (`cli/`) creates a pool from it.

### Instructions

| Instruction | Who | What |
|---|---|---|
| `create_pool` | Anyone | Creates a pool from its parameters (`pools/*.toml`): its signers, network, epochs and marking window, maturities, grace, oldest root, cap per token, impaired share, past-maturity spread, rate mode, risk model and the risk engine's weights, and optionally the SAS attestation depositors must hold |
| `settle_epoch` | Anyone | Moves the epoch's settlement on: at a new epoch, impairs loans past maturity and grace and begins marking; once every performing loan is marked, pays queued withdrawals at the NAV per share then. Otherwise does nothing, so clients send it first |
| `mark_loans` | Anyone (keepers) | Marks loans from their tokens' snapshots, proven against one of the pool's signers' roots. The risk engine keeps a loan performing or writes it down |
| `mark_stale` | Anyone | Once the epoch's marking window has closed, marks a loan nobody could read; NAV counts it at the impaired share for the rest of the epoch |
| `deposit` | Depositors | USDC for shares at the current NAV per share |
| `request_withdrawal` | Depositors | Moves shares to escrow, to be paid at the next epoch's start. If idle USDC can't pay every request, each is paid the same share and the rest waits |
| `claim_withdrawal` | Depositors | Takes the USDC set aside for a request |
| `borrow` | Holders of a token | Pledges units with the token's snapshot and proof, and borrows up to the advance the risk engine allows, at the rate it sets, fixed for the loan |
| `repay` | Anyone | Pays towards a loan, interest first; once nothing is owed it closes |
| `withdraw_units` | The borrower | Moves a closed loan's remaining units from its vault to the borrower, once the borrower's account for the token is thawed |
| `sync_loan` | Anyone | Sets USDC paid for a loan's units (a buy-back, the collateral agent's redemption) against the loan and records the units left in its vault |

### How it decides

- **Repayments go only to the pool's own vaults.** `repay` and `sync_loan` take the pool's USDC vault and the loan's own vault and proceeds account only at their associated token addresses. The quote token must be a classic SPL token, so no transfer fee or delegate leaves the vault short of what the pool counts.
- **Closing a loan never moves its units.** They wait in the loan's vault for `withdraw_units`, so an issuer freezing the borrower's account can't block a repayment. A borrower's next loan against the token waits for them to be withdrawn.
- **Facts come only from roots.** A signer posts the Merkle root of every token's latest snapshot to `snapshot-roots`; a borrower or keeper hands the pool a token's snapshot and its proof. The root must be the latest one of the pool's signers posted, and no older than the pool allows, so nobody can pick an older, kinder root still kept; the snapshot must name this network and this mint. `docs/snapshot.md` is the layout and the tree.
- **The risk engine decides the advance and the rate** (`src/risk_engine.rs`, model V1). It refuses while burn debt is overdue, when more of the token's units are past the pool's late threshold (30, 60 or 90 days) than it allows, and when the term ends before the invoices are expected to be collected (their weighted days to due plus the days their customers pay beyond terms). The eligible share counts units current or 1–30 days past due, 31–60 days at a weight, and nothing older, less the disputed share; the advance rate falls as customer concentration passes a threshold, up to a ceiling the pool sets (100% is allowed). The rate adds spreads for the share past due, concentration and disputes to a base; on units whose customers have a payment history, spreads for days paid beyond terms and the share not paid on time; and on the rest, a no-history spread; all within the pool's bounds, and is fixed when the loan opens. The cap per token counts every borrower's loans against the token.
- **Any holder can borrow.** The units are worth face value whoever holds them, so a loan is keyed by pool, token and borrower: several holders of one token can borrow, each from their own vault.
- **Every epoch, every performing loan is read again.** `settle_epoch` begins the epoch; keepers mark each performing loan from its token's latest root, and a loan nobody can read is marked stale after the window and counted at the impaired share for the rest of the epoch: neither a fresh mark nor a payment lifts that before the next epoch's start, so nobody can deposit at the discount and then clear it. Withdrawals are paid only once every performing loan is marked, at the NAV when they are paid, and deposits, borrows and withdrawal requests wait for that too. An impaired loan can't be marked up or down, so marking leaves it out. With several signers, a loan's token must be read from a root at least as new as the one it was last read from.
- **A loan goes bad when its facts say so, or when it's late.** A mark that finds burn debt overdue writes the loan down at once; a loan past maturity and grace is impaired at an epoch's start. Either way NAV counts it at the impaired share, and anything paid later adds to NAV as it arrives. Between marks, NAV is idle USDC plus each performing loan's principal and interest accrued every second, summed by maturity so it takes the same 64 steps however many loans there are.
- **USDC paid for a loan's units repays the loan.** Whoever buys units back pays USDC into the loan's proceeds account (the associated token account of the loan's address); `sync_loan` applies it to interest, then principal, and sends the rest to the borrower.

### Limits of this version

- **One open loan per borrower per token per pool.** Each borrower's units sit in their own vault, which the token's issuer thaws once, so a borrower's second loan against a token waits for the first to close. A loan whose units are all burned and never paid for stays open and impaired, and needs no more marks.
- **Fixed rates only.** `RateMode::Variable` is declared and refused until it is built.
- **Proofs are large.** A transaction holds about two tokens' proofs at thousands of tokens; see `docs/snapshot.md`.
- **One withdrawal request per wallet** at a time; it closes once paid in full.
- **A borrower's vault must be thawed before their first loan against a token in a pool.** Receivables tokens' accounts start frozen, and only the issuer (or, later, a gate program) thaws one. `borrow` creates the vault if it is missing, but a new vault is frozen, so a first `borrow` that also creates it fails. The client first creates the vault (the associated token account of the loan's address, which the borrower pays rent for), then asks the issuer to thaw it, then borrows. Scriply thaws by one fixed rule, read on chain, and reviews no pool: the token is one it issues, the pool is an account of this program, and the borrower's own account for the token is thawed. Anyone asks at `POST /v1/public/pools/{network}/{poolAddress}/loan-vault-thaws` on Scriply's API with `{tokenAddress, borrower}`. The vault stays thawed for every later loan of that borrower against that token in that pool. The rule is safe only because the program moves units out of a vault to nobody but its borrower, so every new way out must keep to that.
- **Not audited.** The upgrade authority stays with Scriply on devnet and is renounced once the program has been audited, before it holds real money.

## snapshot-roots

Where each signer posts the Merkle root of every receivables token's latest snapshot. A signer posts to its own account, at an address derived from its key, so nobody else can post there and nothing administers it. A program that trusts a signer reads its account and verifies a token's snapshot, with the proof a keeper hands it, against a root there. `docs/snapshot.md` has the tree's rules.

| Instruction | Who | What |
|---|---|---|
| `post_root` | A signer | Posts a root, its tree scheme, its leaf count and when its snapshots were true. Only a root newer than the signer's latest is taken; the account keeps the latest 8 |

## Versions

Every account the programs own starts with a `version` byte, and the pool keeps 256 spare bytes for fields a later layout adds. A pool stores the risk model and rate mode it was created with: a later program adds a model beside V1 and never changes it, so no pool's rules change under it. Snapshots carry their layout version, posted roots their tree scheme, and config files their own `version`.

## token-snapshot

The crate both programs and any Rust signer or keeper share: the snapshot's layout, and the Merkle tree (`merkle::verify` on chain, `merkle::Tree` to build a tree and its proofs off chain). `docs/snapshot-vectors.json` pins its output for other languages; after a deliberate change, regenerate it with `UPDATE_SNAPSHOT_VECTORS=1 cargo test -p token-snapshot --test vectors`.

## create-pool

```bash
cargo run -p create-pool -- --config pools/devnet.toml --url <rpc url> \
    --quote-mint <usdc mint> --keypair <payer keypair> --dry-run
```

Reads the config (`crates/pool-config`), prints the parameters and the pool's addresses, and without `--dry-run` sends `create_pool`. The URL has no default, so a pool is never created on a cluster nobody named. `pools/devnet.toml` trusts a key nobody holds until Scriply's devnet signer replaces it.

## Development

Needs Rust, the Solana CLI (Agave) and Anchor 1.2.2 (through `avm`).

```bash
anchor build        # the program and its IDL, in target/deploy and target/idl
cargo test          # unit tests, and integration tests running the built program in LiteSVM
cargo fmt --all
cargo clippy --all-targets
```

The integration tests load the programs from `target/deploy/`, so run `anchor build` after changing the program.

Each program's address is set by its keypair in `target/deploy/` (`receivables_pool-keypair.json`, `snapshot_roots-keypair.json`), which `.gitignore` keeps out of the repo. Keep copies: deploying to the same addresses needs them.

## License

Apache-2.0. See `LICENSE` and `NOTICE`.
