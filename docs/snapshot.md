# Token snapshot, layout version 2

A snapshot is a set of facts about one receivables token on one network at one time, which a signer (a business that verifies orgs and their receivables, such as Scriply) vouches for. A signer commits every token's latest snapshot at once in a Merkle tree and posts its root on chain (below), and a program accepts a token's snapshot proven against a root posted by one of the keys it pinned when it was created.

Every token's snapshot has the same layout, so a program integrates once and reads any token's snapshot from any signer that uses this format.

## Layout

Fixed-width fields, little-endian, no padding. Amounts are in the token's base units, shares in basis points, and flags `0` or `1`.

| Offset | Size | Field | Meaning |
|---:|---:|---|---|
| 0 | 1 | `version` | `2`. A later version only appends fields |
| 1 | 16 | `network` | The network ID in ASCII, padded with zeros: `sol`, `sol-devnet` |
| 17 | 32 | `mint` | The token's mint address |
| 49 | 8 | `asOf` | When the facts were true, unix seconds (i64) |
| 57 | 32 | `reportHash` | SHA-256 of the latest published collateral report |
| 89 | 8 | `networkSupply` | Units the signer records on this network; a program compares it with the mint's supply |
| 97 | 8 | `outstandingUnits` | Units outstanding on every network |
| 105 | 8 | `burnDebt` | Units owed for burning that no wallet of the org held |
| 113 | 1 | `burnDebtOverdue` | `1` once burn debt is past its deadline |
| 114 | 8 | `agingCurrent` | Units backed by invoices not yet due |
| 122 | 8 | `agingDays1To30` | Units backed by invoices 1–30 days past due |
| 130 | 8 | `agingDays31To60` | 31–60 days past due |
| 138 | 8 | `agingDays61To90` | 61–90 days past due |
| 146 | 8 | `agingOver90` | Over 90 days past due |
| 154 | 2 | `concentrationIndex` | Herfindahl–Hirschman index of customer concentration, 0–10000 |
| 156 | 2 | `disputedShare` | Share of units backed by disputed invoices, bps |
| 158 | 2 | `weightedDaysToDue` | Days until each open invoice is due, 0 once past due, averaged by the units it backs |
| 160 | 2 | `weightedDaysToPay` | Days the invoices' customers have taken to pay, from invoice date to the payment reaching the org's bank, averaged by the units each customer owes now |
| 162 | 2 | `weightedDaysBeyondTerms` | Days those customers paid after the due date, 0 when on time, on the same weighting |
| 164 | 2 | `paidOnTimeShare` | Share of those customers' paid invoice volume that reached the bank by its due date, bps |
| 166 | 2 | `historyShare` | Share of units owed by customers with a payment history, which the three fields above are drawn from, bps |
| 168 | 2 | `weightedDaysInBurnDebt` | Over the 365 days before `asOf`, the unit-days of burn debt owed (debt still owed counts up to `asOf`) over the units owed in that window (owed at its start or recorded during it), to the nearest day. It says how long a unit owed stayed owed, not how many were: one unit owed all year gives 365, as a thousand do. Version 2 |
| 170 | 2 | `daysBurnDebtOverdue` | Days in the 365 before `asOf` that the token's burn debt was overdue (owed past its deadline), rounded up, so any time overdue counts. Version 2 |

Version 2 is 172 bytes; version 1 was its first 168, without the burn-debt history. `snapshot::tests::test_layout_offsets_are_pinned` pins these offsets.

## Rules

- **The layout only grows at its end.** A program with no admin can never change what it reads, so each field's offset, unit and meaning are permanent. A field whose meaning would change is a new field, appended. A program built for version 2 reads the first 172 bytes of any later version and ignores the rest; the Merkle leaf still covers all of them.
- **Naming the network and the mint stops replays:** a devnet snapshot, or another token's, can't be used for this token on this network.
- **A snapshot is checked against the chain where the chain can say:** a borrow is refused when the mint's supply is above `networkSupply`, since units minted after the snapshot are outside its facts. A supply below it is accepted: roots are posted daily, burns happen as invoices are paid, and a burn only shrinks what the facts describe.
- **The layout evolves freely on devnet and freezes when the first mainnet pool pins a signer's key.**
- **Nothing in a snapshot identifies a customer.** Every field is a total or an average over the token's open tokenized invoices, weighted by units. Customers' credit events, such as bankruptcy, are the signer's to act on before it vouches for the facts; a pool reads only the composition.
- **Burn-debt history outlasts the debt.** `burnDebt` and `burnDebtOverdue` say what is owed now, and clear once the org delivers the units; the two history fields keep, for a year, how long its debts lasted and how long they ran past the deadline, so a token whose org once ran late still shows it after paying. Both are 0 for a token with no burn debt in the year, including one whose mint is younger than a year.
- **Payment history is vouched to the bank.** A paid invoice counts towards `weightedDaysToPay`, `weightedDaysBeyondTerms` and `paidOnTimeShare` only once its payment is matched to the cash reaching the org's bank, so the dates are the bank's, not the books'. Until a signer can do that, it posts the three as 0 and `historyShare` as 0.

## Every token at once: the Merkle root

A signer commits every token's latest snapshot at once by posting one Merkle root to the `snapshot-roots` program, at its own account `["roots", signer]`. A keeper (any program run off chain: the signer's, a depositor's, a dapp's) reads which tokens a pool holds, takes those tokens' snapshots and proofs from the signer's published tree, and hands them to the pool, which verifies each against the root. The keeper can't change a fact, since an altered snapshot no longer hashes to the root; all it can do is not show up.

- **Leaf:** `sha256(0x00 || snapshot)`, over every byte of the snapshot, whatever its version.
- **Node:** `sha256(0x01 || lo || hi)`, where `lo` is the smaller of the two children compared byte by byte. Sorting the pair means a proof is only the siblings, bottom up, with no left-or-right.
- **Odd levels:** the last node of a level with an odd count moves up unchanged, and its proof skips that level.
- **One leaf per token.** A signer puts each mint in a tree at most once; a program can't check this, so it is part of what the signer vouches for.
- **Proofs are at most 32 hashes** (a tree of 2^32 leaves).

The prefixes keep a node's two children from passing as a snapshot. `crates/token-snapshot` implements this (`merkle::verify` on chain, `merkle::Tree` off it), and `docs/snapshot-vectors.json` has snapshots, leaves, a root and proofs for a signer or keeper written in another language to check itself against.

A signer's account keeps its latest 8 roots, each with when its snapshots were true (`as_of`) and its leaf count, and takes only a root newer than its latest. A program that accepts any kept root lets a keeper or borrower choose the older of two snapshots for a token, so the receivables pool accepts only the signer's latest root (`SnapshotRoots::latest`), no older than its oldest allowed. A program that must accept older roots finds one with `SnapshotRoots::find`, and should accept only a root at least as new as the one it last read the token from.

Each posted root names its `scheme`, the tree's construction above (`MERKLE_SCHEME_V1`); the program takes only schemes it knows, and a later construction is a new scheme, so no root is read by the wrong rules. When no token's facts changed, the same root is posted again, and `find` returns its newest post.

**Proof size limits a transaction.** A proof for a tree of 5,000 tokens is 13 hashes (416 bytes) beside the 172-byte snapshot. A v0 transaction (at most 1232 bytes) holds a borrow with a proof of at most 5 hashes (32 tokens); a v1 transaction (at most 4096 bytes) holds one with a proof of 20 hashes (about a million tokens) in 1701 bytes, and marks several tokens' loans. A feed account per token, which anyone writes once from a proof and pools then read with no proof at all, would make borrows and marks smaller still, and let other programs read the facts.
