//! A bill leaf: the facts a signer vouches for about one bill an anchor
//! offered for early payment and an operator approved, in the same tree as
//! the tokens' snapshots. docs/bill-leaf.md is the spec.

use crate::leaf::LEAF_KIND_BILL;

/// The bill leaf's layout version this crate writes and reads, and every
/// later one.
pub const BILL_LEAF_VERSION: u8 = 1;

/// The bytes of layout version 1. A later version only appends fields.
pub const BILL_LEAF_LEN: usize = 135;

/// Which documents an operator verified, as bits of `documents_verified`.
pub const DOCUMENT_VENDOR_INVOICE: u8 = 1;
pub const DOCUMENT_PURCHASE_ORDER: u8 = 2;
pub const DOCUMENT_PROOF_OF_DELIVERY: u8 = 4;

/// Why bytes aren't a bill leaf this crate can read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BillLeafError {
    /// Fewer than BILL_LEAF_LEN bytes.
    TooShort,
    /// The first byte isn't LEAF_KIND_BILL: another kind of leaf.
    NotABillLeaf,
    /// A version before BILL_LEAF_VERSION.
    UnsupportedVersion,
}

/// One approved offer's facts. Amounts are in millionths of `currency`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BillLeaf {
    pub version: u8,
    /// The network ID in ASCII, padded with zeros: `sol`, `sol-devnet`.
    pub network: [u8; 16],
    /// `sha256(supplier's public key || salt)`, the listing's commitment.
    pub commitment: [u8; 32],
    /// The root's `as_of`, unix seconds.
    pub as_of: i64,
    /// The bill's currency by asset ID in ASCII, padded with zeros: `FIAT_USD`.
    pub currency: [u8; 16],
    /// The offer's amount, in millionths of `currency`.
    pub amount: u64,
    /// The bill's due date, unix seconds.
    pub due_date: i64,
    /// When the anchor repays the pool, unix seconds; never before `due_date`.
    pub repay_on: i64,
    /// Which documents the approval verified: `DOCUMENT_*` bits.
    pub documents_verified: u8,
    /// SHA-256 over the verified files' SHA-256s, in the order of their kinds.
    pub documents_hash: [u8; 32],
    /// How long the anchor has paid this vendor's bills, as a band: `0`
    /// never, `1` under 90 days, `2` 90–179, `3` 180–364, `4` a year or more.
    pub vendor_tenure: u8,
    /// What the anchor paid this vendor in the 365 days before `as_of`, as a
    /// multiple of this bill, as a band: `0` nothing, `1` under 1×, `2`
    /// 1–3×, `3` 3–10×, `4` 10–50×, `5` 50× or more.
    pub vendor_volume: u8,
    /// The share of that volume whose payments are matched to a bank or
    /// wallet transaction, bps; `0` until the signer can match them.
    pub vendor_history_vouched_bps: u16,
}

impl BillLeaf {
    /// Parses a bill leaf in layout version 1 or later, or says why it can't.
    ///
    /// ```ignore
    /// let bill = BillLeaf::parse(&bytes)?; // reads the first 135 bytes
    /// ```
    pub fn parse(bytes: &[u8]) -> Result<Self, BillLeafError> {
        if bytes.len() < 2 {
            return Err(BillLeafError::TooShort);
        }
        if bytes[0] != LEAF_KIND_BILL {
            return Err(BillLeafError::NotABillLeaf);
        }
        if bytes[1] < BILL_LEAF_VERSION {
            return Err(BillLeafError::UnsupportedVersion);
        }
        if bytes.len() < BILL_LEAF_LEN {
            return Err(BillLeafError::TooShort);
        }
        let mut r = crate::leaf::Reader::new(bytes, 2);
        Ok(Self {
            version: bytes[1],
            network: r.array(),
            commitment: r.array(),
            as_of: r.u64() as i64,
            currency: r.array(),
            amount: r.u64(),
            due_date: r.u64() as i64,
            repay_on: r.u64() as i64,
            documents_verified: r.u8(),
            documents_hash: r.array(),
            vendor_tenure: r.u8(),
            vendor_volume: r.u8(),
            vendor_history_vouched_bps: r.u16(),
        })
    }

    /// The leaf in layout version 1, for signers, keepers and tests.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(BILL_LEAF_LEN);
        b.push(LEAF_KIND_BILL);
        b.push(self.version);
        b.extend_from_slice(&self.network);
        b.extend_from_slice(&self.commitment);
        b.extend_from_slice(&self.as_of.to_le_bytes());
        b.extend_from_slice(&self.currency);
        b.extend_from_slice(&self.amount.to_le_bytes());
        b.extend_from_slice(&self.due_date.to_le_bytes());
        b.extend_from_slice(&self.repay_on.to_le_bytes());
        b.push(self.documents_verified);
        b.extend_from_slice(&self.documents_hash);
        b.push(self.vendor_tenure);
        b.push(self.vendor_volume);
        b.extend_from_slice(&self.vendor_history_vouched_bps.to_le_bytes());
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Snapshot;

    fn sample() -> BillLeaf {
        let mut network = [0u8; 16];
        network[..10].copy_from_slice(b"sol-devnet");
        let mut currency = [0u8; 16];
        currency[..8].copy_from_slice(b"FIAT_USD");
        BillLeaf {
            version: 1,
            network,
            commitment: [5; 32],
            as_of: 1_800_000_000,
            currency,
            amount: 12_500_000_000,
            due_date: 1_802_000_000,
            repay_on: 1_804_000_000,
            documents_verified: DOCUMENT_VENDOR_INVOICE | DOCUMENT_PROOF_OF_DELIVERY,
            documents_hash: [6; 32],
            vendor_tenure: 3,
            vendor_volume: 2,
            vendor_history_vouched_bps: 0,
        }
    }

    #[test]
    fn test_layout_round_trips() {
        let leaf = sample();
        let bytes = leaf.to_bytes();
        assert_eq!(bytes.len(), BILL_LEAF_LEN);
        assert_eq!(BillLeaf::parse(&bytes).unwrap(), leaf);
    }

    /// Pins each field's offset as docs/bill-leaf.md gives it.
    #[test]
    fn test_layout_offsets_are_pinned() {
        let l = sample();
        let b = l.to_bytes();
        assert_eq!(b[0], LEAF_KIND_BILL);
        assert_eq!(b[1], 1);
        assert_eq!(&b[2..18], &l.network);
        assert_eq!(&b[18..50], &l.commitment);
        assert_eq!(&b[50..58], &l.as_of.to_le_bytes());
        assert_eq!(&b[58..74], &l.currency);
        assert_eq!(&b[74..82], &l.amount.to_le_bytes());
        assert_eq!(&b[82..90], &l.due_date.to_le_bytes());
        assert_eq!(&b[90..98], &l.repay_on.to_le_bytes());
        assert_eq!(b[98], l.documents_verified);
        assert_eq!(&b[99..131], &l.documents_hash);
        assert_eq!(b[131], l.vendor_tenure);
        assert_eq!(b[132], l.vendor_volume);
        assert_eq!(&b[133..135], &l.vendor_history_vouched_bps.to_le_bytes());
    }

    #[test]
    fn test_a_later_version_is_read_by_its_prefix() {
        let mut leaf = sample();
        leaf.version = 2;
        let mut bytes = leaf.to_bytes();
        bytes.extend_from_slice(&[9, 9]);
        assert_eq!(BillLeaf::parse(&bytes).unwrap(), leaf);
    }

    #[test]
    fn test_a_short_leaf_is_refused() {
        let bytes = sample().to_bytes();
        assert_eq!(
            BillLeaf::parse(&bytes[..BILL_LEAF_LEN - 1]),
            Err(BillLeafError::TooShort)
        );
    }

    #[test]
    fn test_a_snapshot_is_not_a_bill_leaf() {
        let snapshot = Snapshot {
            version: 2,
            ..Snapshot::default()
        }
        .to_bytes();
        assert_eq!(BillLeaf::parse(&snapshot), Err(BillLeafError::NotABillLeaf));
    }

    #[test]
    fn test_a_bill_leaf_is_not_a_snapshot() {
        let bytes = sample().to_bytes();
        assert!(Snapshot::parse(&bytes).is_err());
    }
}
