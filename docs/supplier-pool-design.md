# The supplier pool: how it decides

The supplier pool is one program that reads the facts Scriply posts about businesses and their bills (the business leaf and the bill leaf, under the daily root in `snapshot.md`) and pays an anchor's suppliers early by its own rules, to be repaid by the anchor on a later date. Scriply decides which facts exist, how a bill is offered and claimed, and the interface its services drive; everything below is this program's own design, and anyone may deploy it, fork it or write another that reads the same facts differently.

## A pool is shared or bespoke, and its parameters are fixed when it is created

A pool is one token's money, lent to the suppliers of the anchors whose bills it lists. Like the receivables pool, anyone creates one from a config file in this repository, and nothing can change its parameters afterwards; depositors choose a pool by them. A pool may take any anchor whose facts pass its rules, shared by all of them, or be bespoke, taking only the wallets it names.

| Parameter | What it decides |
|---|---|
| Token | What it holds and pays in: a stablecoin of the bills' currency |
| Fee | What it keeps of each bill it pays early, in basis points: flat whatever the term (1.5%, say), or a curve over the days from the claim to the due date, so a longer wait costs the supplier more |
| Anchors | Any anchor, or only the wallets it names |
| Signers | Whose roots it reads the anchors' and bills' facts from |
| Rules | What it refuses, by its own thresholds on the ratios in the anchor's business leaf and the facts in the bill's leaf (which documents were verified, the vendor's history in bands), and how old a publication and a root it takes |
| Cap per anchor | Optionally, the most one anchor's listings may be, as a share of what the pool holds, so one default costs its depositors at most that share |

## Accounts and instructions

Amounts are in the token's base units; dates are Unix seconds, UTC.

| Account | Seeds | Holds |
|---|---|---|
| Pool | `["pool", creator, mint]` | Its parameters (above); its vault is the pool's associated token account |
| Anchor | `["anchor", pool, wallet]` | What the anchor's wallet owes the pool across its open listings |
| Listing | `["listing", pool, commitment]` | The anchor's wallet, the commitment, the amount, the due date, the fee deposit's lamports, and whether it is claimed |

| Instruction | Signed by | Does |
|---|---|---|
| `create_pool(params)` | Its creator | Creates the pool and its vault, with parameters nobody can change afterwards |
| `list_bill(commitment, amount, due_date, fee_deposit, business_leaf, business_proof, bill_leaf, bill_proof)` | The anchor | Checks that the business leaf is the signing wallet's and the bill leaf is this commitment's, each under the latest root one of the pool's signers posted, no older than the pool allows, on the pool's network, and that the amount and due date are the bill leaf's; refuses an anchor the pool doesn't take, facts that fail a rule, or listings that would pass its cap, naming which; creates the listing and the anchor's account if it has none, paying their rent and the deposit, and adds the amount to what the anchor owes |
| `cancel_listing()` | The anchor | Closes an unclaimed listing to the anchor |
| `claim(salt)` | Anyone (the fee payer) | With the transaction's ed25519 check of the supplier's signature over the claim message: checks the commitment, pays the amount less the fee (the pool's fee in basis points for the days from now to the due date, of the amount in base units, rounded down) to the supplier's token account (creating it), refunds the fee payer from the deposit, marks the listing claimed |
| `repay()` | The anchor | Pays the amount into the vault, takes it off what the anchor owes, and closes the claimed listing to the anchor |

Deposits, withdrawals (on epochs, as the receivables pool's are) and what counts as a late repayment are still to be designed, reusing the receivables pool's where they can. Once a listing is made the pool waits for its due date and the anchor's repayment; whether it marks open listings from the anchor's newer leaves, so its NAV counts an anchor whose facts worsen, is decided with the withdrawals.

## Consequences

- A shared pool's credit is every listing anchor's, so its depositors judge each by the rules the pool was created with, without its name: from what Scriply verified of it and what it publishes about itself, read by its wallet. One anchor's default is shared by every depositor, up to the cap per anchor if the pool has one. A bespoke pool's credit is its anchors' alone.
- A cap per anchor is a share of what the pool holds, so a young shared pool with one anchor lends that anchor only its share.

## Alternatives considered

- **The listings in one pool account.** Every listing and claim would write one account, which caps at 10 MiB and serializes them. Rejected for an account per listing.
- **A pool per anchor,** the first design. Depositors judge one business, but each anchor needs its own pool, agent and depositors before its first bill is paid, and money in one anchor's pool sits idle while another's suppliers wait. Replaced by pools anyone creates, shared by every anchor or bespoke to some (2026-11-12).
