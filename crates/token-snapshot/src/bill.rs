//! A bill leaf: the facts a signer vouches for about one bill an anchor
//! offered for early payment and an operator approved, in the same tree as
//! the tokens' snapshots. docs/bill-leaf.md is the spec.

use crate::leaf::LEAF_KIND_BILL;

/// The bill leaf's layout version this crate writes. It reads version 1
/// and every later one.
pub const BILL_LEAF_VERSION: u8 = 2;

/// The bytes of layout version 1, the shortest leaf this crate reads.
pub const BILL_LEAF_V1_LEN: usize = 135;

/// The bytes of layout version 2. A later version only appends fields.
pub const BILL_LEAF_LEN: usize = BILL_LEAF_V1_LEN + 32;

/// The first layout version that names the anchor's wallet.
pub const BILL_LEAF_ANCHOR_VERSION: u8 = 2;

/// Which documents an operator verified, as bits of `documents_verified`.
pub const DOCUMENT_VENDOR_INVOICE: u8 = 1;
pub const DOCUMENT_PURCHASE_ORDER: u8 = 2;
pub const DOCUMENT_PROOF_OF_DELIVERY: u8 = 4;

/// Why bytes aren't a bill leaf this crate can read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BillLeafError {
    /// Fewer bytes than the leaf's version needs: BILL_LEAF_V1_LEN for
    /// version 1, BILL_LEAF_LEN for version 2 or later.
    TooShort,
    /// The first byte isn't LEAF_KIND_BILL: another kind of leaf.
    NotABillLeaf,
    /// Version 0, which no layout has.
    UnsupportedVersion,
    /// A version before BILL_LEAF_ANCHOR_VERSION, which names no anchor, where
    /// one is needed: a listing proves the wallet that signs it.
    NoAnchor,
    /// `anchor` doesn't match `version`: a version 1 leaf with one, or a
    /// later one without.
    AnchorDisagreesWithVersion,
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
    /// The wallet the anchor offered the bill from, the only one that may
    /// list it; `None` in a version 1 leaf, which names none.
    pub anchor: Option<[u8; 32]>,
}

impl BillLeaf {
    /// Parses a bill leaf in layout version 1 or later, or says why it can't.
    ///
    /// ```ignore
    /// let bill = BillLeaf::parse(&bytes)?; // bill.anchor is None for version 1
    /// ```
    pub fn parse(bytes: &[u8]) -> Result<Self, BillLeafError> {
        if bytes.len() < 2 {
            return Err(BillLeafError::TooShort);
        }
        if bytes[0] != LEAF_KIND_BILL {
            return Err(BillLeafError::NotABillLeaf);
        }
        if bytes[1] == 0 {
            return Err(BillLeafError::UnsupportedVersion);
        }
        let has_anchor = bytes[1] >= BILL_LEAF_ANCHOR_VERSION;
        let len = if has_anchor {
            BILL_LEAF_LEN
        } else {
            BILL_LEAF_V1_LEN
        };
        if bytes.len() < len {
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
            anchor: has_anchor.then(|| r.array()),
        })
    }

    /// The leaf's bytes, as the tree holds them, for signers, keepers and
    /// tests: BILL_LEAF_LEN for version 2 and later, BILL_LEAF_V1_LEN for
    /// version 1, or AnchorDisagreesWithVersion when `anchor` doesn't match
    /// `version`.
    ///
    /// ```ignore
    /// let bytes = leaf.to_bytes()?; // bytes.len() == BILL_LEAF_LEN
    /// ```
    pub fn to_bytes(&self) -> Result<Vec<u8>, BillLeafError> {
        if self.anchor.is_some() != (self.version >= BILL_LEAF_ANCHOR_VERSION) {
            return Err(BillLeafError::AnchorDisagreesWithVersion);
        }
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
        if let Some(anchor) = &self.anchor {
            b.extend_from_slice(anchor);
        }
        Ok(b)
    }

    /// The bytes a listing sends: the leaf without its anchor, which the
    /// program takes from the wallet that signs (`join_anchor`), so a
    /// listing is 32 bytes shorter. NoAnchor for a version 1 leaf.
    ///
    /// ```ignore
    /// let listed = leaf.to_listed_bytes()?; // listed.len() == BILL_LEAF_V1_LEN
    /// ```
    pub fn to_listed_bytes(&self) -> Result<Vec<u8>, BillLeafError> {
        if self.version < BILL_LEAF_ANCHOR_VERSION {
            return Err(BillLeafError::NoAnchor);
        }
        let mut b = self.to_bytes()?;
        b.drain(BILL_LEAF_V1_LEN..BILL_LEAF_LEN);
        Ok(b)
    }

    /// Returns the leaf as the tree holds it from the bytes a listing sent
    /// (`to_listed_bytes`) and the wallet that signed it, or why they can't
    /// be one: too short, another kind of leaf, version 0, or a version
    /// before BILL_LEAF_ANCHOR_VERSION, which names no anchor (NoAnchor).
    /// A proof of the result fails unless the leaf names `anchor`.
    ///
    /// ```ignore
    /// let leaf = BillLeaf::join_anchor(&listed, &signer.to_bytes())?;
    /// ```
    pub fn join_anchor(listed: &[u8], anchor: &[u8; 32]) -> Result<Vec<u8>, BillLeafError> {
        if listed.len() < 2 {
            return Err(BillLeafError::TooShort);
        }
        if listed[0] != LEAF_KIND_BILL {
            return Err(BillLeafError::NotABillLeaf);
        }
        if listed[1] == 0 {
            return Err(BillLeafError::UnsupportedVersion);
        }
        if listed[1] < BILL_LEAF_ANCHOR_VERSION {
            return Err(BillLeafError::NoAnchor);
        }
        if listed.len() < BILL_LEAF_V1_LEN {
            return Err(BillLeafError::TooShort);
        }
        let mut b = Vec::with_capacity(listed.len() + 32);
        b.extend_from_slice(&listed[..BILL_LEAF_V1_LEN]);
        b.extend_from_slice(anchor);
        b.extend_from_slice(&listed[BILL_LEAF_V1_LEN..]);
        Ok(b)
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
            version: 2,
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
            anchor: Some([7; 32]),
        }
    }

    #[test]
    fn test_layout_round_trips() {
        let leaf = sample();
        let bytes = leaf.to_bytes().unwrap();
        assert_eq!(bytes.len(), BILL_LEAF_LEN);
        assert_eq!(BillLeaf::parse(&bytes).unwrap(), leaf);
    }

    /// Pins each field's offset as docs/bill-leaf.md gives it.
    #[test]
    fn test_layout_offsets_are_pinned() {
        let l = sample();
        let b = l.to_bytes().unwrap();
        assert_eq!(b[0], LEAF_KIND_BILL);
        assert_eq!(b[1], 2);
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
        assert_eq!(&b[135..167], &l.anchor.unwrap());
    }

    #[test]
    fn test_a_version_1_leaf_names_no_anchor() {
        let leaf = BillLeaf {
            version: 1,
            anchor: None,
            ..sample()
        };
        let bytes = leaf.to_bytes().unwrap();
        assert_eq!(bytes.len(), BILL_LEAF_V1_LEN);
        assert_eq!(BillLeaf::parse(&bytes).unwrap(), leaf);
    }

    #[test]
    fn test_a_version_2_leaf_without_its_anchor_is_too_short() {
        let bytes = sample().to_bytes().unwrap();
        assert_eq!(
            BillLeaf::parse(&bytes[..BILL_LEAF_V1_LEN]),
            Err(BillLeafError::TooShort)
        );
    }

    #[test]
    fn test_a_later_version_is_read_by_its_prefix() {
        let mut leaf = sample();
        leaf.version = 3;
        let mut bytes = leaf.to_bytes().unwrap();
        bytes.extend_from_slice(&[9, 9]);
        assert_eq!(BillLeaf::parse(&bytes).unwrap(), leaf);
    }

    #[test]
    fn test_a_leaf_whose_anchor_disagrees_with_its_version_isnt_written() {
        let named = BillLeaf {
            version: 1,
            ..sample()
        };
        assert_eq!(
            named.to_bytes(),
            Err(BillLeafError::AnchorDisagreesWithVersion)
        );
        let unnamed = BillLeaf {
            anchor: None,
            ..sample()
        };
        assert_eq!(
            unnamed.to_bytes(),
            Err(BillLeafError::AnchorDisagreesWithVersion)
        );
    }

    #[test]
    fn test_a_listing_sends_the_leaf_without_its_anchor() {
        let leaf = sample();
        let listed = leaf.to_listed_bytes().unwrap();
        assert_eq!(listed.len(), BILL_LEAF_V1_LEN);
        assert_eq!(
            BillLeaf::join_anchor(&listed, &[7; 32]).unwrap(),
            leaf.to_bytes().unwrap()
        );
        assert_ne!(
            BillLeaf::join_anchor(&listed, &[8; 32]).unwrap(),
            leaf.to_bytes().unwrap()
        );
    }

    #[test]
    fn test_a_later_versions_fields_follow_the_joined_anchor() {
        let mut leaf = sample();
        leaf.version = 3;
        let mut tree = leaf.to_bytes().unwrap();
        tree.extend_from_slice(&[9, 9]);
        let mut listed = leaf.to_listed_bytes().unwrap();
        listed.extend_from_slice(&[9, 9]);
        assert_eq!(BillLeaf::join_anchor(&listed, &[7; 32]).unwrap(), tree);
    }

    #[test]
    fn test_a_version_1_leaf_cant_be_joined_to_an_anchor() {
        let leaf = BillLeaf {
            version: 1,
            anchor: None,
            ..sample()
        };
        assert_eq!(leaf.to_listed_bytes(), Err(BillLeafError::NoAnchor));
        let bytes = leaf.to_bytes().unwrap();
        assert_eq!(
            BillLeaf::join_anchor(&bytes, &[7; 32]),
            Err(BillLeafError::NoAnchor)
        );
    }

    #[test]
    fn test_a_short_leaf_is_refused() {
        let bytes = sample().to_bytes().unwrap();
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
        let bytes = sample().to_bytes().unwrap();
        assert!(Snapshot::parse(&bytes).is_err());
    }
}
