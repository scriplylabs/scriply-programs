# Bill leaf, layout version 1

A bill leaf is a set of facts about one bill an anchor offered for early payment, once an operator has reviewed it against its documents and approved it. The signer puts one in its daily tree for each approved offer, from the root after the approval until the offer is cancelled or repaid, beside the tokens' snapshots (`snapshot.md`), and posts the tree's root to `snapshot-roots` as it always does. A supplier pool checks a listing against it, so what the pool lends on is what the signer verified, not the anchor's word.

Nothing in it names the anchor, the vendor or the bill: the vendor's address is behind `commitment`, and the vendor's history with the anchor is in bands, which many vendors share.

## Leaf kinds

Every leaf in a tree says what it is in its first byte, which the leaf's hash covers, so no leaf can be read as another kind:

| First byte | Kind |
|---|---|
| `0x01`–`0x7f` | A token's snapshot; the byte is its layout version (`snapshot.md`) |
| `0x81` | A business leaf, about the business behind a wallet: reserved |
| `0x82` | A bill leaf (below) |

A leaf whose first byte is `0x80` or more has its own layout version in its second byte. `token_snapshot::Snapshot::parse` refuses every kind but a snapshot, and `BillLeaf::parse` every kind but a bill leaf.

## Layout

Fixed-width fields, little-endian, no padding. Dates are unix seconds (i64).

| Offset | Size | Field | Meaning |
|---:|---:|---|---|
| 0 | 1 | `kind` | `0x82` |
| 1 | 1 | `version` | `1`. A later version only appends fields |
| 2 | 16 | `network` | The network ID in ASCII, padded with zeros: `sol`, `sol-devnet` |
| 18 | 32 | `commitment` | `sha256(supplier's public key ‖ salt)`: the 32 bytes of the address the pool pays, then 32 random bytes kept off chain until the claim reveals them. The listing holds the same commitment |
| 50 | 8 | `asOf` | The root's `asOf` |
| 58 | 16 | `currency` | The bill's currency by its asset ID in ASCII, padded with zeros: `FIAT_USD` |
| 74 | 8 | `amount` | The offer's amount, in millionths of `currency` (u64). A pool converts it to its token's base units |
| 82 | 8 | `dueDate` | The bill's due date |
| 90 | 8 | `repayOn` | When the anchor repays the pool; never before `dueDate`. A listing's due date |
| 98 | 1 | `documentsVerified` | Which documents the approval verified, as bits: vendor's invoice `1`, purchase order `2`, proof of delivery `4` |
| 99 | 32 | `documentsHash` | SHA-256 over the verified files' SHA-256s, in the order of their kinds' bits, so what was shown can be proved later without publishing it |
| 131 | 1 | `vendorTenure` | How long the anchor has paid this vendor's bills: `0` never, `1` under 90 days, `2` 90–179, `3` 180–364, `4` a year or more |
| 132 | 1 | `vendorVolume` | What the anchor paid this vendor in the 365 days before `asOf`, as a multiple of this bill's amount: `0` nothing, `1` under 1×, `2` 1–3×, `3` 3–10×, `4` 10–50×, `5` 50× or more |
| 133 | 2 | `vendorHistoryVouched` | The share of that volume whose payments are matched to a transaction on a linked bank account or a verified wallet, bps (u16); `0` until the signer can match them |

Version 1 is 135 bytes. `bill::tests::test_layout_offsets_are_pinned` pins these offsets, and `bill-leaf-vectors.json` has bill leaves and snapshots in one tree, with its root and proofs, for a signer or keeper written in another language.

## Rules

- **The layout only grows at its end,** as the snapshot's does: a program with no admin can never change what it reads.
- **Naming the network stops replays:** a devnet bill leaf can't list a bill on mainnet.
- **One leaf per approved offer.** A bill offered again after a rejection or a cancel has a new salt, so a new commitment and a new leaf; a signer puts each commitment in a tree at most once.
- **Bands, not figures.** Exact tenure and volume would repeat on every bill to the same vendor, so anyone could group an anchor's listings by vendor.
- **A leaf may be proved to several pools.** The supplier is paid only on a claim that reveals the salt, which the signer gives only for the listing it records. The claim makes the salt public while the leaf is still in the roots, so a pool that reads bill leaves must refuse a second claim on a commitment whichever listing it comes through; the supplier pool keeps a claim record per commitment for that. Every other listing only holds its anchor's deposit until the anchor closes it.
