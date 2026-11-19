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
| Rules | What it refuses, by its own thresholds on the facts in the bill's leaf (which documents were verified, the vendor's history in bands, how far off the repayment date is), and how old a root it takes |
| Business rules | What it refuses from the anchor's record with supplier pools in its business leaf: the highest band it takes of listings overdue and unpaid, of the days the worst of them is overdue, of listings written off and unrepaid, of those written off in the past year, and of those repaid late in it (`docs/business-leaf.md`). Zero takes none |
| Most owed per anchor | The most one anchor's wallet may owe across its open listings, in the token's base units, so one default costs its depositors at most that. A fixed amount rather than a share of what the pool holds: a share could be raised by depositing before a listing and withdrawing after it. It counts per wallet, so in a shared pool a business listing from several wallets owes up to that much on each; a pool that must bound a business names its anchors |
| Write-off days | How many days past a listing's due date its anchor may go unrepaid before anyone may take it out of NAV |

`pools/supplier-devnet.toml` lists every parameter with what it decides, and `create-supplier-pool` (`cli/`) creates a pool from it.

## Accounts and instructions

Amounts are in the token's base units; dates are Unix seconds, UTC.

| Account | Seeds | Holds |
|---|---|---|
| Pool | `["pool", creator, mint]` | Its parameters (above); its vault is the pool's associated token account |
| Anchor | `["anchor", pool, wallet]` | What the anchor's wallet owes the pool across its open listings |
| Listing | `["listing", pool, anchor wallet, commitment]` | The anchor's wallet, the commitment, the amount, the due date, the fee deposit's lamports, and whether it is claimed, by whom and for how much |
| Claim record | `["claimed", commitment]` | That the commitment was claimed, the listing that paid and when: one per program, created by the claim |

A listing's address includes the anchor's wallet, and the bill leaf names that wallet (version 2), so only it can list the bill (the listing sends the leaf without it, and the program completes it with the signing wallet before proving it): bill leaves are public, and a leaf another wallet could list would be judged by that wallet's business leaf while the bill's business carried the loss. A claim reveals the salt while the leaf is still under the signer's latest roots (until a root posted after the payment drops it, and the pool accepts roots some days old), so after one the anchor's wallet could list the leaf again (in another pool, or at the same address once repaid) and replay the supplier's signature and salt. The claim record stops that: a claim creates it, so a commitment pays its supplier once across every pool of this program.

| Instruction | Signed by | Does |
|---|---|---|
| `create_pool(params)` | Its creator | Creates the pool and its vault, with parameters nobody can change afterwards |
| `deposit(amount)` | Anyone | The pool's token for shares at its NAV per share: what it holds idle plus what anchors owe on bills it paid |
| `withdraw(shares)` | A depositor | Burns shares for what they are worth at the NAV, paid from idle; refused when idle can't pay it all |
| `list_bill(commitment, {root, leaf, proof}, {leaf, proof})` | The anchor | Checks that the bill leaf is this commitment's, under the latest root one of the pool's signers posted, no older than the pool allows, on the pool's network and in its currency, that the bill leaf, which the listing sends without its anchor, is proven once completed with the signing wallet (so another wallet's proof fails; a version 1 leaf, which names none, is `NoAnchor`), and that the business leaf, under the same root, is the anchor wallet's on the pool's network with a record (version 2 or later), and lists it at the leaf's amount (converted to the token's base units) and with the leaf's `repayOn` as its due date; refuses an anchor the pool doesn't take, facts that fail a rule, or listings that would pass its cap, naming which; creates the listing and the anchor's account if it has none, paying their rent and the deposit (the claim's network fee, its claim record's rent and a token account's rent), and adds the amount to what the anchor owes |
| `cancel_listing()` | The anchor | Closes an unclaimed listing to the anchor |
| `claim(salt)` | Anyone (the fee payer) | With the transaction's ed25519 check of the supplier's signature over `scriply-supplier-finance-claim:{listing address}`, in an earlier instruction and carrying its key, signature and message in its own data: checks that `sha256(supplier ‖ salt)` is the commitment and the due date hasn't passed, pays the amount less the fee (the pool's fee in basis points for the days from now to the due date, rounded up, of the amount, rounded down) to the supplier's associated token account (creating it), creates the commitment's claim record (refused if one exists), refunds the fee payer the network fee, the record's rent and that account's rent from the deposit, marks the listing claimed. Refused while idle can't pay it, so it waits for deposits or repayments |
| `repay()` | The anchor | Pays the amount into the vault, takes it off what the anchor owes, and closes the claimed listing to the anchor; a listing written off counts in NAV again |
| `write_off()` | Anyone | Once a claimed listing is past its due date by the pool's write-off days, unrepaid: takes it out of NAV. The anchor still owes it, and it still counts toward the most the anchor may owe |

## Limits of this version

- **Withdrawals are paid at once, from idle only.** A depositor can't leave while every token is out to suppliers; it waits for repayments. Epochs, as the receivables pool's, come when a pool holds money that isn't the team's own.
- **The fee counts in NAV when the claim is paid,** not as the days pass, so a deposit sent just before a claim and withdrawn just after takes a share of its fee, from the depositors already in. Accruing the fee to the due date closes it.
- **A late listing counts at face until it is written off, then at nothing.** From its due date until the pool's write-off days have passed, NAV counts a claimed listing as if it will be repaid, so a depositor leaving in that window is paid at a price the default may not bear, and the depositors who stay carry it. Anyone may write it off after that (`write_off`), so the price falls on a known day rather than never; marking a late listing down day by day, or pricing withdrawals in epochs, closes the window.
- **Listings are not funded when made.** A pool can list more than it holds idle (up to the most each anchor may owe); a claim it can't pay waits.
- **The anchor is judged by its record with supplier pools, not its ratios.** A shared pool reads the anchor wallet's business leaf and refuses by the record's bands; rules over the published ratios, and over how old the published period is, come later. A wallet whose business never published has no business leaf, so it can't list in any pool of this version: it has no record, not a clean one.
- **The record is as old as the latest root, up to the pool's oldest root age (two days on devnet).** A listing overdue since that root's `asOf` isn't in it yet, so until the next root a business just gone overdue can still list from each of its wallets in each pool. A listing is overdue from its first whole day past its repayment date, and written off only after the pool's write-off days, so a business late in one pool shows as overdue in every pool's next root, long before a write-off.
- **The program judges the wallet the bill leaf names, which the signer ties to the bill's business.** Only that wallet can list the bill, and its business leaf is the record the pool reads. The program can't check that, so the signer keeps two rules: a bill leaf is in the tree only while the business holds its wallet with the signer's attestation, and a wallet whose attestation two businesses hold has no business leaf, so it can't list (`bill-leaf.md`, `business-leaf.md`).
- **A pool created before the business rules reads them as zeros,** where its spare bytes were: it takes only an anchor with nothing overdue, written off or repaid late.
- **Not audited.** The upgrade authority stays with Scriply on devnet and is renounced, or handed to someone else, before the program holds real money.

## Consequences

- A shared pool's credit is every listing anchor's, so its depositors judge each by the rules the pool was created with, without its name: from what Scriply verified of it and what it publishes about itself, read by its wallet. One anchor's default is shared by every depositor, up to the most that anchor's wallet may owe. A bespoke pool's credit is its anchors' alone.
- The most owed per anchor is fixed when the pool is created, so it doesn't grow or shrink with the pool: a pool that grows lends each anchor no more, and one that shrinks may have one anchor owing much of what it holds.

## Alternatives considered

- **The listings in one pool account.** Every listing and claim would write one account, which caps at 10 MiB and serializes them. Rejected for an account per listing.
- **A pool per anchor,** the first design. Depositors judge one business, but each anchor needs its own pool, agent and depositors before its first bill is paid, and money in one anchor's pool sits idle while another's suppliers wait. Replaced by pools anyone creates, shared by every anchor or bespoke to some (2026-11-12).
- **A count of each anchor wallet's unrepaid write-offs on its anchor account,** refusing a wallet with one, as a backstop until business leaves existed. It holds only within one pool and one wallet, where the business leaf's record follows the business to every wallet and pool. Not built, since the business leaf came first (2026-11-19).
- **The business leaf under a root of its own.** Proving it under the bill leaf's root keeps the listing one root and one roots account, and 32 bytes smaller: a listing is a legacy transaction (1232 bytes) while wallets can't sign v1, and both proofs must fit in it. The listing sends the bill leaf without its anchor, which the program takes from the signing wallet, so a legacy listing fits while the tree holds up to 256 leaves, with 4 bytes to spare.
- **A bill leaf that names no anchor, checked by the signer alone.** Smaller by 32 bytes, but any wallet could list the bill and be judged by its own record: the signer withholding the salt kept the supplier from being paid, but a pool reading only the chain couldn't tell. Replaced by version 2's `anchor`.
- **A listing that carries the bill leaf's anchor.** The program would compare it with the signing wallet. It costs a listing 32 bytes, so a legacy listing would fit only while the tree held at most 128 leaves of any kind. Replaced by taking the anchor from the signing wallet.
