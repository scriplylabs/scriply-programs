# The receivables pool: how it decides

The receivables pool is one program that reads the facts Scriply posts about receivables tokens (the snapshot, `snapshot.md`) and lends against the tokens by its own rules. Scriply decides which facts exist and what the token requires of any program that holds it; everything below is this program's own design, and anyone may deploy it, fork it or write another that reads the same facts differently.

It is written in Rust and starts as a working example on devnet, served from scriply.tech, and is open source with its SDK and its dapp, so anyone can read how its rules use the token's facts and anyone can host a way to withdraw.

It is a vault, like Uniswap v1's pools: one pool, one share token, deposits and withdrawals at the pool's net asset value (NAV). Depositors don't pick a tranche or a tier inside a pool; one who wants different terms picks a different pool.

**Any holder of the token can borrow.** A unit is made whole at face value whoever holds it (Scriply's buy-back, or the collateral agent's redemption), so the pool lends against it whoever pledges it. A loan is keyed by its pool, token and borrower, and the cap per token counts every borrower's loans against the token.

**Anyone can create a pool, and Scriply is one signer among any.** The program is a public good, as Morpho Blue's markets are: anyone creates a pool with the parameters below, including whose roots it trusts, and nobody can change them afterwards. The snapshot's layout and the tree are published as an open standard, so another business that verifies orgs and their receivables as we do can post roots of its own tokens' snapshots, and create pools that trust its key, ours, or both. Depositors choose pools by the signers they trust. No pool is Scriply's to run, and the program works without Scriply.

## Its parameters are fixed when a pool is created

Whoever creates a pool sets these in a config file (`pools/*.toml` in the program's repository), and nothing can change them afterwards; depositors choose a pool by them. The upgrade authority is kept on devnet and renounced once the program has been audited, before it holds real money.

| Parameter | What it decides |
|---|---|
| Signers | Whose posted roots the pool reads, and so which tokens it takes (below): one to eight keys. Scriply's is one |
| Risk model | Which risk engine formula decides (V1 today). A later program adds a model beside it and never changes one, so no pool's rules change under it |
| Risk engine weights | What the risk engine refuses, how much it advances and at what rate (below), including the advance's ceiling, which may be up to 100% |
| Rate mode | Fixed: a loan's rate is set when it opens and never changes. Variable is declared and refused until it is built |
| Past-maturity spread | Added to a loan's rate once it is past maturity |
| Epoch | How long an epoch is (a week, say). The pool's own, never Solana's |
| Marking window | How long into an epoch keepers have to mark every loan before one nobody could read may be marked stale (a day, say) |
| Maturities | The terms a loan may take, each a whole number of epochs, so every loan matures at an epoch's start |
| Grace | Whole epochs after maturity before an unpaid loan is impaired |
| Impaired share | The share of what an impaired loan owed that NAV still counts, 0 unless set |
| Cap per token | The most of NAV the loans against one token may be, across its borrowers |
| Oldest root | How old the facts under a root may be when the pool reads them (two days, so a missed daily post doesn't stop it) |
| Depositor attestation | Optionally, the attestation a depositor's wallet must hold |

## Which tokens it takes

Any token with a snapshot under one of its signers' roots, with no list anyone keeps, including tokens created after the pool. Creating a mint is open to anyone, so the program never trusts the mint itself: it lends against units only of the mint a snapshot names, proven under one of its signers' roots. A signer puts only tokens it verifies in its tree (Scriply its own), so the root says both whose token it is and what its facts are, and no metadata field, feed account or mint authority has to be checked. An attacker's own mint has no leaf, and a leaf of another token names another mint.

The pool trusts its signers, which is no more than it trusts their facts. Each signer's token must have what the pool's rules count on, a collateral agent under a security agreement above all; the pool can't read a legal agreement, so a signer's root vouches for it, and depositors judge the signer.

## A loan is made from the token's facts

- **The facts are read from the signer's latest root.** The borrower's transaction carries the token's snapshot and its Merkle proof; the program checks that the proof shows the snapshot under the latest root one of its signers posted, that the root's facts are no older than the pool allows, and that the snapshot names this mint and the pool's network. Only the latest root is read, so nobody can pick an older, kinder snapshot still kept: a borrow sent just before a new post fails and is sent again with the new proof. The borrower pays the transaction's fees. The pool never pays a fee, holds no SOL and pays no one for data, so the number of tokens it takes costs it nothing.
- **What the chain shows, the pool checks against the chain.** The snapshot's totals count every network the token is on, and most of what it says (burn debt, aging, concentration, payment history) is worked out off-chain from the books, so the pool takes those from the snapshot. One field the chain does show: the snapshot carries `networkSupply`, the units the signer records on this network, and the program refuses when the mint's supply as the chain has it now is above it, since units minted after the snapshot aren't covered by its facts. A supply below it is taken: units burned since (a buy-back, a delivery) leave the snapshot describing more than exists, and refusing them would stop every borrow against a token for up to a day after any burn.
- **A risk engine decides from those facts** (`risk_engine.rs`, model V1, its weights the pool's parameters). It refuses while `burnDebtOverdue` is set, when the token's burn debt was overdue on more days of the last year than the pool allows (`daysBurnDebtOverdue`; history refuses new loans only, and a mark writes a loan down only on `burnDebtOverdue`), when more of the units are late (past 30, 60 or 90 days, as the pool sets) than the pool allows, and when the loan's term ends before the invoices are expected to be collected (their days to due plus the days their customers pay beyond terms). It advances on the eligible share (units current or up to 30 days past due, 31 to 60 days at a weight, less the disputed share) at a rate that falls as customer concentration rises, up to the pool's ceiling. It sets the yearly rate from a base plus spreads for the share past due, concentration, disputes and the customers' payment history, within the pool's bounds, fixed for the loan. The program then refuses a loan that would take the token's loans past its cap.
- **A loan is its own account**, holding what it owes, its rate, its maturity and the units it secures.

There is no collateral floor. Over-collateralizing protects nothing when the collateral fails with the borrower (the token's own credit); what protects the pool is the eligible share, the advance's ceiling, the rate and the cap. A pool may advance up to face value, and its depositors choose it knowing that: at a 90% advance, a 13-epoch loan at 15% owes about 93% of face at maturity and passes face during a grace at the past-maturity rate, so a buy-back at face no longer repays it.

## Every loan is marked each epoch

Each epoch, keepers mark every performing loan from its token's snapshot under the signer's latest root. A mark that finds `burnDebtOverdue` set writes the loan down at once: NAV counts it at the impaired share, without waiting for its maturity and grace. A loan whose token can't be read (it has left the tree) may be marked stale by anyone once the marking window closes, and NAV counts it at the impaired share for the rest of the epoch. Neither a fresh mark nor a payment lifts that before the next epoch's start, since by then the epoch's withdrawals have been settled at the discounted NAV. Deposits are priced at NAV before the stale discount, so nobody can buy shares at the discount and clear it in the same transaction, while a withdrawal never leaves at a NAV that hasn't counted it. The next epoch marks the loan afresh. An impaired loan can't be marked up or down, so marking leaves it out. Anyone can keep: a depositor who wants to withdraw, a borrower who wants to borrow, or a bot, and a mark carries a few proofs per transaction, so a pool lending against thousands of tokens needs many mark transactions each epoch (per-token feed accounts are the later fix).

## NAV is arithmetic on the pool's own state

NAV is the pool's idle USDC, plus each performing loan's principal and the interest it has accrued, and each impaired loan's impaired share, less the discount on loans marked stale. The program keeps running totals by maturity (principal, interest, what was repaid), so working out NAV takes the same few steps however many loans or tokens there are. Interest accrues into NAV every second, not when it is paid, so depositing just before a repayment captures none of it.

The first deposit can't set a share price that rounds later deposits to nothing: the share price counts a small fixed number of virtual shares and assets, the usual guard for a vault.

## An epoch's start settles the pool

The first instruction in a new epoch, whoever sends it, begins settling it, and a later one finishes once every performing loan is marked:

1. **Impairs the loans past maturity and grace.** Each maturity is a bucket, so every unpaid loan whose maturity plus grace has passed is impaired at once, by bucket: NAV counts it at its impaired share of what it owed at maturity, and it stops accruing into NAV. It goes on accruing what the borrower owes, and whatever it is paid later (a repayment, a buy-back, the agent's recovery) adds to NAV as it arrives.
2. **Waits for the epoch's marks.** Keepers mark every performing loan (above); deposits, loans and withdrawal requests wait too, so nobody acts on a NAV that hasn't counted what the marks find.
3. **Pays the withdrawals asked for during the epoch,** at the NAV per share when they're paid. If idle USDC can't pay them all, each is paid the same share of what it asked. Every settlement ends the generation it paid: anyone may claim a request's USDC for its owner, and the claim queues its unpaid shares again for the next epoch's start, where they still share the pool's interest and losses. A request is never paid from a later settlement's USDC at an earlier one's terms.
4. **Sets aside what withdrawals were paid** before any new loan is made.

Deposits are taken at the NAV per share before the stale discount once the epoch's marks are in. Withdrawals are asked for then and paid only at an epoch's start, after impairment and marking, so nobody who sees a loan failing can leave at a NAV that hasn't counted it: by the time any withdrawal is paid, every loan past its grace and every loan whose facts went bad has been. What isn't covered is a loan whose facts go bad between one epoch's marks and the next.


## Consequences

- A pool's depositors lend to every org that tokenizes, including orgs that tokenize after they deposit. What protects them is the rules, the advance and the cap per token, and the dapp says so.
- Losses are shared by every depositor pro rata. A withdrawal can wait while loans are out, up to the longest maturity plus grace, and each epoch's withdrawals wait for its marks, so a pool depends on someone keeping. Every keeper wants the same thing, so any one is enough.

## Open questions

- **The impaired share.** 0 is safe for those who stay, but someone who withdraws while a loan is impaired leaves at a NAV that counts none of its likely buy-back, and someone who deposits then buys a full share of whatever the buy-back later recovers, paid for by those already in. A share near what buy-backs recover would be fairer to all three, once there is a history to set it from.

## Alternatives considered

- **Ticks, as MetaStreet v2 has them.** Each deposit picks its own limit (an absolute amount or a share of the oracle's price), longest term and rate tier; a loan draws from the cheapest ticks first, which become its tranches, the highest taking a loss first. It lets depositors choose their risk within one pool and settles losses by seniority, but is more than a working example needs, and its code is Solidity on EVM, so only the design would carry over, without its audits. Tranches can be added later as a second share class if lenders ask.
- **Reading the snapshot once, at borrow.** NAV would need no facts after a loan opens, but it would count a loan whose token went bad in full until its maturity and grace passed, and a depositor who saw it coming could leave first. Marking each epoch from the daily root catches it within an epoch.
- **A collateral floor of 150%.** It sizes every loan the same whatever the token's facts say, and it protects nothing when the collateral fails with the borrower. The risk engine sizes each loan from the facts instead, and the ceiling is the pool's to set.
- **A pool per org.** It needs no trust chain, but splits depositors' money across as many pools as there are orgs, and a new org has no lenders until someone creates its pool.
- **Signers added by vote,** with a governance token or depositors' shares. It would let a pool trust a new verifier without a new pool, but a vote that adds a key is an admin by another name: whoever gathers enough votes adds a key of their own, signs a snapshot of a million dollars of open invoices, and borrows the pool's USDC. A delay longer than an epoch would let depositors leave first, but a new pool does the same with nothing to capture. Depositors' shares could vote later, if lenders ask.
- **A list of tokens the pool accepts.** Someone would have to keep it, which a pool with no admin can't have.
