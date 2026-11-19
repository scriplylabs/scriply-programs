# Business leaf, layout version 2

A business leaf is a set of facts about the business behind one wallet: the financial statements it published from its books, what the signer found when it checked them, and what the signer reads about the business on the root's day: its bank's cash now, and what it owes supplier pools and how it has repaid them. The signer puts one in its daily tree for each wallet of a business that published, while the wallet holds a standing KYB attestation from the signer on the network, beside the tokens' snapshots and the bill leaves (`snapshot.md`, `bill-leaf.md`), and posts the tree's root to `snapshot-roots` as it always does. A pool that judges a business by its wallet, such as a supplier pool judging the anchor that lists a bill, proves this leaf.

A token's snapshot carries the same facts, about the business that issued it, from layout version 3, and the facts at `asOf` from version 4 (`snapshot.md`), so a lending pool reads them from the leaf it already reads.

Nothing in it names the business: it is keyed by its wallet, and its figures stay private, behind `commitment`.

## Published figures, checked figures

The business publishes its balance sheet and income statement for a period it picks, from its books, and an officer certifies them. The ratios are computed from those statements, so they are the business's own figures. The signer then performs procedures on them against sources the business doesn't write, and reports what it found, not whether it passed: a pool sets its own thresholds, and can rely on a ratio only as far as the procedures behind it. The signer issues no opinion on the statements.

| Procedure | Bit | Result |
|---|---|---|
| Cash agreed to the bank | `1` | The bank's cash at `periodEndExclusive`, read from the business's linked bank accounts, to the statements' cash. Book cash in an account with no linked bank lowers it, as does cash the books claim and the bank doesn't hold. Pending or late-posted transactions near the date make it approximate |
| Receivables tied to invoices | `2` | The invoices open at `periodEndExclusive`, as the signer reads them from the business's accounting system or ledger, to the statements' receivables. Receivables raised by journal entries, without invoices, lower it |
| Receivables confirmed | `4` | Of those open invoices, the share by amount whose customers confirmed them to the business through the signer |

A procedure the signer couldn't perform (no linked bank account, no receivables) has its bit clear and its result 0, which means nothing; `BusinessFacts` reads it as `None`.

## Facts at `asOf`

The published figures are as of the period's end, which the business picks. From version 2 the signer adds what it reads on the root's day itself, so a pool can tell whether cash has left since, and how the business has treated supplier pools (`AsOfFacts`):

| Field | Meaning | No value when |
|---|---|---|
| `bankCashNowToCurrentLiabilities` | The bank's latest cash, across the business's linked bank accounts in its books, to the statements' current liabilities | No bank account is linked and in the books, one's latest balance was reported more than 5 days before `asOf` or isn't in the statements' currency, or current liabilities are 0 |
| `supplierFinanceToCurrentLiabilities` | What the business owes supplier pools at `asOf` (its listings listed and not yet repaid or withdrawn, across every pool and network the signer attests bills for), to the statements' current liabilities | A listing isn't in the statements' currency, or current liabilities are 0 |
| `establishedVendorShare` | Of what it owes supplier pools, the share for vendors it paid a bill itself (not through a pool) at least 180 days before `asOf` | It owes supplier pools nothing, or a listing isn't in the statements' currency |
| `overdueUnpaid` | Listings a pool paid out that the business hasn't repaid, a whole day or more past their `repayOn`, written off or not | |
| `mostDaysOverdue` | The most whole days one of those is past its `repayOn` | |
| `writtenOffUnrepaid` | Listings a pool wrote off that the business hasn't repaid | |
| `writtenOffInYear` | Listings written off in the 365 days before `asOf` | |
| `repaidInYear` | Listings repaid in the 365 days before `asOf` | |
| `repaidLateInYear` | Of those, the ones repaid a whole day or more after their `repayOn` | |

Days are whole days of 86,400 seconds after `repayOn`, an instant like every date here: a listing is overdue, or was repaid late, once at least one whole day had passed since `repayOn` (`floor((t - repayOn) / 86400) >= 1`), and that floor is its days overdue. A signer that sets `repayOn` to a UTC midnight, the start of the day the business picked, makes that "on time any time that day, overdue from the next". The signer reads the listings from each pool's history on chain and its own record of the offers; a pool's own account sees only its listings. The counts are in bands, so the record can't be matched to the pools' public listings one by one to find which are this business's (every wallet's leaf already shares the business's commitment and ratios, so bands don't hide that its wallets belong together):

| Band | Count (`COUNT_*`) | Days overdue (`DAYS_OVERDUE_*`) |
|---:|---|---|
| 0 | None | Not overdue |
| 1 | 1 | 1 to 7 |
| 2 | 2 to 4 | 8 to 30 |
| 3 | 5 to 9 | 31 to 60 |
| 4 | 10 or more | 61 to 90 |
| 5 | | Over 90 |

## Layout

Fixed-width fields, little-endian, no padding. Dates are unix seconds (i64). Ratios are basis points (u32, and `operatingMargin` i32), truncated toward zero. A ratio whose denominator is zero has no value and is the type's maximum (`0xffffffff`, or `0x7fffffff` for `operatingMargin`); `BusinessFacts` reads it as `None`, so a pool's rule decides what no value means and never compares it as a number (zero revenue isn't the best margin). A signer clamps a value outside its field to the field, below the maximum, before writing it: a u32 ratio to `0` through `0xfffffffe` (a negative one, from a negative balance, is `0`), `operatingMargin` to `-0x80000000` through `0x7ffffffe`, so no value written ever reads as none or wraps.

| Offset | Size | Field | Meaning |
|---:|---:|---|---|
| 0 | 1 | `kind` | `0x81` |
| 1 | 1 | `version` | `2`. A later version only appends fields |
| 2 | 16 | `network` | The network ID in ASCII, padded with zeros: `sol`, `sol-devnet` |
| 18 | 32 | `wallet` | The business's wallet |
| 50 | 8 | `asOf` | The root's `asOf` |
| 58 | 8 | `periodEndExclusive` | The end of the published period: the balance sheet's date, and the midnight after the income statement's last day. The income statement covers the 12 months before it |
| 66 | 32 | `commitment` | HMAC-SHA256, keyed by a key only the signer and the business hold, of the statements' encoding. The signer gives the business both; the business gives a lender it chooses the encoded statements and the key, and the lender checks that the HMAC of those bytes under that key is this. The encoding is deterministic CBOR (RFC 8949 §4.2.1) of the statements: their period, currency, and balance sheet and income statement lines by account, so the bytes checked are the bytes read. A change to what is hashed is a new layout version |
| 98 | 4 | `currentRatio` | Current assets to current liabilities |
| 102 | 4 | `quickRatio` | Cash and receivables to current liabilities |
| 106 | 4 | `cashToCurrentAssets` | The statements' cash to current assets |
| 110 | 4 | `cashToCurrentLiabilities` | The statements' cash to current liabilities |
| 114 | 4 | `liabilitiesToAssets` | Total liabilities to total assets |
| 118 | 4 | `operatingMargin` | Operating income to revenue over the year (i32); negative for a loss |
| 122 | 4 | `receivablesTurnover` | The year's revenue to receivables at `periodEndExclusive` |
| 126 | 1 | `proceduresPerformed` | Which procedures the signer performed, as bits (above) |
| 127 | 4 | `cashAgreedToBank` | The procedure's result |
| 131 | 4 | `receivablesTiedToInvoices` | The procedure's result |
| 135 | 4 | `receivablesConfirmed` | The procedure's result |
| 139 | 4 | `bankCashNowToCurrentLiabilities` | Facts at `asOf` (above), from version 2 |
| 143 | 4 | `supplierFinanceToCurrentLiabilities` | |
| 147 | 4 | `establishedVendorShare` | |
| 151 | 1 | `overdueUnpaid` | A count band |
| 152 | 1 | `mostDaysOverdue` | A days band |
| 153 | 1 | `writtenOffUnrepaid` | A count band |
| 154 | 1 | `writtenOffInYear` | A count band |
| 155 | 1 | `repaidInYear` | A count band |
| 156 | 1 | `repaidLateInYear` | A count band |

Version 2 is 157 bytes, and version 1 was its first 139, without the facts at `asOf`. The 99 bytes from offset 58 are the business's facts, which a version 4 snapshot appends (a version 3 snapshot, their first 81). `BusinessLeaf::parse` reads both versions; a version 1 leaf's `as_of_facts` is `None`. `business::tests::test_layout_offsets_are_pinned` pins these offsets, and `business-leaf-vectors.json` has business leaves, version 4 snapshots and a bill leaf in one tree, with its root and proofs, for a signer or keeper written in another language.

## Rules

- **The layout only grows at its end,** as the snapshot's does: a program with no admin can never change what it reads. Facts the signer reports later (when the books were closed, an accountant's review of the period) are appended.
- **Freshness is the pool's to judge, by `periodEndExclusive`.** A leaf's `asOf` is the root's, so a leaf posted today can carry figures a year old. A pool sets how old a period it accepts.
- **Version 1 doesn't say whether cash has left since the period:** a business can publish the morning after a large payment and its figures still show the cash. Version 2's `bankCashNowToCurrentLiabilities` does, from a bank balance at most 5 days old. A pool reading version 1 leaves either accepts that or requires version 2.
- **No source dates.** A leaf says when nothing was last read: a fact the signer can't read freshly enough has no value (bank cash now), and a token whose books its signer couldn't read for more than 5 days leaves the tree (`snapshot.md`). The dates would tell anyone watching a wallet when its business last signed in to its accounting system.
- **A business's record follows it to every wallet, once it has published.** Every wallet of a business that published carries the same facts at `asOf`, so such a business whose listing was written off can't list again from a new wallet with a clean record. A business that never published has no business leaf, and its token's snapshot has no business facts (`BusinessFacts::parse_snapshot` is `None`), so its supplier pool record is nowhere: a wallet with no leaf, or a snapshot with no facts, has no record at all, not a clean one, and a pool that needs a record refuses it as it chooses.
- **Naming the network and the wallet stops replays:** a devnet leaf, or another wallet's, can't be used for this wallet on this network.
- **One leaf per wallet, and none for a wallet two businesses hold.** A signer leaves out a wallet whose attestation two businesses hold, since whose record it would carry can't be told, and a pool that reads bill leaves judges a listing by the wallet's leaf as the record of the business that offered the bill. It puts each other wallet in a tree at most once, from the business's latest publication: a later `periodEndExclusive` replaces an earlier one, and the same period published again corrects it.
- **Figures stay private.** Only ratios, procedure results and the commitment are public. A ratio can't be reversed into a figure, and the commitment can't be tested against a guess without its key.
