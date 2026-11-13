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
| Rules | What it refuses, by its own thresholds on the facts in the bill's leaf (which documents were verified, the vendor's history in bands, how far off the repayment date is), and how old a root it takes. Rules on the ratios in the anchor's business leaf come with that leaf: this version reads no business leaf |
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

A listing's address includes the anchor's wallet because bill leaves are public: keyed by the commitment alone, anyone could list a leaf first and take its address in the pool, and the anchor could never list it there. Before a claim, a stranger's listing of the same leaf holds only the stranger's deposit, since the salt is secret until a claim reveals it and only the anchor's recorded listing is claimed. A claim reveals it while the leaf is still under the signer's latest roots (until a root posted after the payment drops it, and the pool accepts roots some days old), so after one anyone could list the leaf again (in any pool, from any wallet, or at the same address once repaid) and replay the supplier's signature and salt. The claim record stops that: a claim creates it, so a commitment pays its supplier once across every pool of this program.

| Instruction | Signed by | Does |
|---|---|---|
| `create_pool(params)` | Its creator | Creates the pool and its vault, with parameters nobody can change afterwards |
| `deposit(amount)` | Anyone | The pool's token for shares at its NAV per share: what it holds idle plus what anchors owe on bills it paid |
| `withdraw(shares)` | A depositor | Burns shares for what they are worth at the NAV, paid from idle; refused when idle can't pay it all |
| `list_bill(commitment, {root, leaf, proof})` | The anchor | Checks that the bill leaf is this commitment's, under the latest root one of the pool's signers posted, no older than the pool allows, on the pool's network and in its currency, and lists it at the leaf's amount (converted to the token's base units) and with the leaf's `repayOn` as its due date; refuses an anchor the pool doesn't take, facts that fail a rule, or listings that would pass its cap, naming which; creates the listing and the anchor's account if it has none, paying their rent and the deposit (the claim's network fee, its claim record's rent and a token account's rent), and adds the amount to what the anchor owes |
| `cancel_listing()` | The anchor | Closes an unclaimed listing to the anchor |
| `claim(salt)` | Anyone (the fee payer) | With the transaction's ed25519 check of the supplier's signature over `scriply-supplier-finance-claim:{listing address}`, in an earlier instruction and carrying its key, signature and message in its own data: checks that `sha256(supplier ‖ salt)` is the commitment and the due date hasn't passed, pays the amount less the fee (the pool's fee in basis points for the days from now to the due date, rounded up, of the amount, rounded down) to the supplier's associated token account (creating it), creates the commitment's claim record (refused if one exists), refunds the fee payer the network fee, the record's rent and that account's rent from the deposit, marks the listing claimed. Refused while idle can't pay it, so it waits for deposits or repayments |
| `repay()` | The anchor | Pays the amount into the vault, takes it off what the anchor owes, and closes the claimed listing to the anchor; a listing written off counts in NAV again |
| `write_off()` | Anyone | Once a claimed listing is past its due date by the pool's write-off days, unrepaid: takes it out of NAV. The anchor still owes it, and it still counts toward the most the anchor may owe |

## Limits of this version

- **Withdrawals are paid at once, from idle only.** A depositor can't leave while every token is out to suppliers; it waits for repayments. Epochs, as the receivables pool's, come when a pool holds money that isn't the team's own.
- **The fee counts in NAV when the claim is paid,** not as the days pass, so a deposit sent just before a claim and withdrawn just after takes a share of its fee, from the depositors already in. Accruing the fee to the due date closes it.
- **A late listing counts at face until it is written off, then at nothing.** From its due date until the pool's write-off days have passed, NAV counts a claimed listing as if it will be repaid, so a depositor leaving in that window is paid at a price the default may not bear, and the depositors who stay carry it. Anyone may write it off after that (`write_off`), so the price falls on a known day rather than never; marking a late listing down day by day, or pricing withdrawals in epochs, closes the window.
- **Listings are not funded when made.** A pool can list more than it holds idle (up to the most each anchor may owe); a claim it can't pay waits.
- **No business leaf.** The pool judges the bill, not the anchor: a bespoke pool names the anchors it takes, and a shared pool takes any wallet whose bill leaf passes. Rules over the anchor's ratios come with the business leaf.
- **Not audited.** The upgrade authority stays with Scriply on devnet and is renounced, or handed to someone else, before the program holds real money.

## Consequences

- A shared pool's credit is every listing anchor's, so its depositors judge each by the rules the pool was created with, without its name: from what Scriply verified of it and what it publishes about itself, read by its wallet. One anchor's default is shared by every depositor, up to the most that anchor's wallet may owe. A bespoke pool's credit is its anchors' alone.
- The most owed per anchor is fixed when the pool is created, so it doesn't grow or shrink with the pool: a pool that grows lends each anchor no more, and one that shrinks may have one anchor owing much of what it holds.

## Alternatives considered

- **The listings in one pool account.** Every listing and claim would write one account, which caps at 10 MiB and serializes them. Rejected for an account per listing.
- **A pool per anchor,** the first design. Depositors judge one business, but each anchor needs its own pool, agent and depositors before its first bill is paid, and money in one anchor's pool sits idle while another's suppliers wait. Replaced by pools anyone creates, shared by every anchor or bespoke to some (2026-11-12).
