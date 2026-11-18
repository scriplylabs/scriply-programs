# Business leaf, layout version 1

A business leaf is a set of facts about the business behind one wallet: the financial statements it published from its books, and what the signer found when it checked them. The signer puts one in its daily tree for each wallet of a business that published, while the wallet holds a standing KYB attestation from the signer on the network, beside the tokens' snapshots and the bill leaves (`snapshot.md`, `bill-leaf.md`), and posts the tree's root to `snapshot-roots` as it always does. A pool that judges a business by its wallet, such as a supplier pool judging the anchor that lists a bill, proves this leaf.

A token's snapshot carries the same facts, about the business that issued it, from layout version 3 (`snapshot.md`), so a lending pool reads them from the leaf it already reads.

Nothing in it names the business: it is keyed by its wallet, and its figures stay private, behind `commitment`.

## Published figures, checked figures

The business publishes its balance sheet and income statement for a period it picks, from its books, and an officer certifies them. The ratios are computed from those statements, so they are the business's own figures. The signer then performs procedures on them against sources the business doesn't write, and reports what it found, not whether it passed: a pool sets its own thresholds, and can rely on a ratio only as far as the procedures behind it. The signer issues no opinion on the statements.

| Procedure | Bit | Result |
|---|---|---|
| Cash agreed to the bank | `1` | The bank's cash at `periodEndExclusive`, read from the business's linked bank accounts, to the statements' cash. Book cash in an account with no linked bank lowers it, as does cash the books claim and the bank doesn't hold. Pending or late-posted transactions near the date make it approximate |
| Receivables tied to invoices | `2` | The invoices open at `periodEndExclusive`, as the signer reads them from the business's accounting system or ledger, to the statements' receivables. Receivables raised by journal entries, without invoices, lower it |
| Receivables confirmed | `4` | Of those open invoices, the share by amount whose customers confirmed them to the business through the signer |

A procedure the signer couldn't perform (no linked bank account, no receivables) has its bit clear and its result 0, which means nothing; `BusinessFacts` reads it as `None`.

## Layout

Fixed-width fields, little-endian, no padding. Dates are unix seconds (i64). Ratios are basis points (u32, and `operatingMargin` i32), truncated toward zero. A ratio whose denominator is zero has no value and is the type's maximum (`0xffffffff`, or `0x7fffffff` for `operatingMargin`); `BusinessFacts` reads it as `None`, so a pool's rule decides what no value means and never compares it as a number (zero revenue isn't the best margin). A signer clamps a value outside its field to the field, below the maximum, before writing it: a u32 ratio to `0` through `0xfffffffe` (a negative one, from a negative balance, is `0`), `operatingMargin` to `-0x80000000` through `0x7ffffffe`, so no value written ever reads as none or wraps.

| Offset | Size | Field | Meaning |
|---:|---:|---|---|
| 0 | 1 | `kind` | `0x81` |
| 1 | 1 | `version` | `1`. A later version only appends fields |
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

Version 1 is 139 bytes; the 81 bytes from offset 58 are the business's facts, which a version 3 snapshot appends. `business::tests::test_layout_offsets_are_pinned` pins these offsets, and `business-leaf-vectors.json` has business leaves, version 3 snapshots and a bill leaf in one tree, with its root and proofs, for a signer or keeper written in another language.

## Rules

- **The layout only grows at its end,** as the snapshot's does: a program with no admin can never change what it reads. Facts the signer reports later (when the books were closed and last read, the bank's cash now, what the business owes supplier pools, an accountant's review of the period) are appended.
- **Naming the network and the wallet stops replays:** a devnet leaf, or another wallet's, can't be used for this wallet on this network.
- **One leaf per wallet.** A signer puts each wallet in a tree at most once, from the business's latest publication: a later `periodEndExclusive` replaces an earlier one, and the same period published again corrects it.
- **Figures stay private.** Only ratios, procedure results and the commitment are public. A ratio can't be reversed into a figure, and the commitment can't be tested against a guess without its key.
