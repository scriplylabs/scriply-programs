//! A business leaf: the facts a signer vouches for about the business behind
//! a wallet, from the financial statements it published and the procedures
//! the signer performed on them. A token's snapshot from layout version 3
//! carries the same facts after its own. docs/business-leaf.md is the spec.

use crate::leaf::{Reader, LEAF_KIND_BUSINESS};
use crate::snapshot::{SnapshotError, SNAPSHOT_LEN};

/// The business leaf's layout version this crate writes and reads, and every
/// later one.
pub const BUSINESS_LEAF_VERSION: u8 = 1;

/// The bytes of layout version 1. A later version only appends fields.
pub const BUSINESS_LEAF_LEN: usize = 58 + BUSINESS_FACTS_LEN;

/// The bytes of the business's facts, in a business leaf and appended to a
/// token's snapshot from layout version 3.
pub const BUSINESS_FACTS_LEN: usize = 81;

/// The snapshot layout version that appends the business's facts.
pub const SNAPSHOT_BUSINESS_VERSION: u8 = 3;

/// The bytes of a snapshot in layout version 3.
pub const SNAPSHOT_V3_LEN: usize = SNAPSHOT_LEN + BUSINESS_FACTS_LEN;

/// An unsigned ratio with no value: its denominator was zero.
pub const RATIO_NONE: u32 = u32::MAX;

/// A signed ratio (`operating_margin_bps`) with no value: no revenue.
pub const SIGNED_RATIO_NONE: i32 = i32::MAX;

/// Which procedures the signer performed on the published statements, as
/// bits of `procedures_performed`.
pub const PROCEDURE_CASH_AGREED_TO_BANK: u8 = 1;
pub const PROCEDURE_RECEIVABLES_TIED_TO_INVOICES: u8 = 2;
pub const PROCEDURE_RECEIVABLES_CONFIRMED: u8 = 4;

/// Why bytes aren't a business leaf this crate can read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusinessLeafError {
    /// Fewer than BUSINESS_LEAF_LEN bytes.
    TooShort,
    /// The first byte isn't LEAF_KIND_BUSINESS: another kind of leaf.
    NotABusinessLeaf,
    /// A version before BUSINESS_LEAF_VERSION.
    UnsupportedVersion,
}

/// What a business published about itself and what the signer found when
/// it checked it. Ratios are in basis points, truncated toward zero, and
/// `RATIO_NONE` (or `SIGNED_RATIO_NONE`) when their denominator was zero. A
/// business that never published has every field 0, `period_end_exclusive`
/// included.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BusinessFacts {
    /// The end of the published statements' period, unix seconds: the
    /// balance sheet's date, and the midnight after the income statement's
    /// last day. 0 when the business never published.
    pub period_end_exclusive: i64,
    /// HMAC-SHA256 of the statements, keyed by a key only the signer and
    /// the business hold; the business opens it to a lender it chooses.
    pub commitment: [u8; 32],
    pub current_ratio_bps: u32,
    pub quick_ratio_bps: u32,
    pub cash_to_current_assets_bps: u32,
    pub cash_to_current_liabilities_bps: u32,
    pub liabilities_to_assets_bps: u32,
    /// Operating income to revenue over the statements' year; negative for
    /// a loss.
    pub operating_margin_bps: i32,
    /// The year's revenue to receivables at `period_end_exclusive`.
    pub receivables_turnover_bps: u32,
    /// Which procedures the signer performed: `PROCEDURE_*` bits. A result
    /// whose bit is clear is 0 and means nothing.
    pub procedures_performed: u8,
    /// The bank's cash at `period_end_exclusive`, from the business's
    /// linked bank accounts, to the statements' cash.
    pub cash_agreed_to_bank_bps: u32,
    /// The invoices open at `period_end_exclusive` to the statements'
    /// receivables.
    pub receivables_tied_to_invoices_bps: u32,
    /// Of those invoices, the share by amount their customers confirmed.
    pub receivables_confirmed_bps: u32,
}

impl BusinessFacts {
    fn read(r: &mut Reader) -> Self {
        Self {
            period_end_exclusive: r.u64() as i64,
            commitment: r.array(),
            current_ratio_bps: r.u32(),
            quick_ratio_bps: r.u32(),
            cash_to_current_assets_bps: r.u32(),
            cash_to_current_liabilities_bps: r.u32(),
            liabilities_to_assets_bps: r.u32(),
            operating_margin_bps: r.u32() as i32,
            receivables_turnover_bps: r.u32(),
            procedures_performed: r.u8(),
            cash_agreed_to_bank_bps: r.u32(),
            receivables_tied_to_invoices_bps: r.u32(),
            receivables_confirmed_bps: r.u32(),
        }
    }

    /// The facts' BUSINESS_FACTS_LEN bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(BUSINESS_FACTS_LEN);
        b.extend_from_slice(&self.period_end_exclusive.to_le_bytes());
        b.extend_from_slice(&self.commitment);
        for v in [
            self.current_ratio_bps,
            self.quick_ratio_bps,
            self.cash_to_current_assets_bps,
            self.cash_to_current_liabilities_bps,
            self.liabilities_to_assets_bps,
        ] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&self.operating_margin_bps.to_le_bytes());
        b.extend_from_slice(&self.receivables_turnover_bps.to_le_bytes());
        b.push(self.procedures_performed);
        for v in [
            self.cash_agreed_to_bank_bps,
            self.receivables_tied_to_invoices_bps,
            self.receivables_confirmed_bps,
        ] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b
    }

    /// The business's facts a token's snapshot carries: those appended from
    /// layout version 3, or all 0 for a version 2 snapshot, which has none.
    /// Refuses what `Snapshot::parse` refuses, and a version 3 or later
    /// snapshot shorter than SNAPSHOT_V3_LEN.
    ///
    /// ```ignore
    /// let snap = Snapshot::parse(&bytes)?;
    /// let business = BusinessFacts::parse_snapshot(&bytes)?;
    /// ```
    pub fn parse_snapshot(bytes: &[u8]) -> Result<Self, SnapshotError> {
        let snapshot = crate::Snapshot::parse(bytes)?;
        if snapshot.version < SNAPSHOT_BUSINESS_VERSION {
            return Ok(Self::default());
        }
        if bytes.len() < SNAPSHOT_V3_LEN {
            return Err(SnapshotError::TooShort);
        }
        Ok(Self::read(&mut Reader::new(bytes, SNAPSHOT_LEN)))
    }
}

/// One wallet's business facts at the root's `as_of`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BusinessLeaf {
    pub version: u8,
    /// The network ID in ASCII, padded with zeros: `sol`, `sol-devnet`.
    pub network: [u8; 16],
    /// The business's wallet, which holds a standing KYB attestation.
    pub wallet: [u8; 32],
    /// The root's `as_of`, unix seconds.
    pub as_of: i64,
    pub facts: BusinessFacts,
}

impl BusinessLeaf {
    /// Parses a business leaf in layout version 1 or later, or says why it
    /// can't.
    ///
    /// ```ignore
    /// let business = BusinessLeaf::parse(&bytes)?; // reads the first 139 bytes
    /// ```
    pub fn parse(bytes: &[u8]) -> Result<Self, BusinessLeafError> {
        if bytes.len() < 2 {
            return Err(BusinessLeafError::TooShort);
        }
        if bytes[0] != LEAF_KIND_BUSINESS {
            return Err(BusinessLeafError::NotABusinessLeaf);
        }
        if bytes[1] < BUSINESS_LEAF_VERSION {
            return Err(BusinessLeafError::UnsupportedVersion);
        }
        if bytes.len() < BUSINESS_LEAF_LEN {
            return Err(BusinessLeafError::TooShort);
        }
        let mut r = Reader::new(bytes, 2);
        Ok(Self {
            version: bytes[1],
            network: r.array(),
            wallet: r.array(),
            as_of: r.u64() as i64,
            facts: BusinessFacts::read(&mut r),
        })
    }

    /// The leaf in layout version 1, for signers, keepers and tests.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(BUSINESS_LEAF_LEN);
        b.push(LEAF_KIND_BUSINESS);
        b.push(self.version);
        b.extend_from_slice(&self.network);
        b.extend_from_slice(&self.wallet);
        b.extend_from_slice(&self.as_of.to_le_bytes());
        b.extend_from_slice(&self.facts.to_bytes());
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BillLeaf, Snapshot};

    fn sample_facts() -> BusinessFacts {
        BusinessFacts {
            period_end_exclusive: 1_798_761_600,
            commitment: [3; 32],
            current_ratio_bps: 18_500,
            quick_ratio_bps: 12_000,
            cash_to_current_assets_bps: 3_100,
            cash_to_current_liabilities_bps: RATIO_NONE,
            liabilities_to_assets_bps: 4_200,
            operating_margin_bps: -750,
            receivables_turnover_bps: 81_000,
            procedures_performed: PROCEDURE_CASH_AGREED_TO_BANK | PROCEDURE_RECEIVABLES_CONFIRMED,
            cash_agreed_to_bank_bps: 9_870,
            receivables_tied_to_invoices_bps: 0,
            receivables_confirmed_bps: 6_400,
        }
    }

    fn sample() -> BusinessLeaf {
        let mut network = [0u8; 16];
        network[..10].copy_from_slice(b"sol-devnet");
        BusinessLeaf {
            version: 1,
            network,
            wallet: [4; 32],
            as_of: 1_800_000_000,
            facts: sample_facts(),
        }
    }

    #[test]
    fn test_layout_round_trips() {
        let leaf = sample();
        let bytes = leaf.to_bytes();
        assert_eq!(bytes.len(), BUSINESS_LEAF_LEN);
        assert_eq!(BUSINESS_LEAF_LEN, 139);
        assert_eq!(BusinessLeaf::parse(&bytes).unwrap(), leaf);
    }

    /// Pins each field's offset as docs/business-leaf.md gives it.
    #[test]
    fn test_layout_offsets_are_pinned() {
        let l = sample();
        let f = &l.facts;
        let b = l.to_bytes();
        assert_eq!(b[0], LEAF_KIND_BUSINESS);
        assert_eq!(b[1], 1);
        assert_eq!(&b[2..18], &l.network);
        assert_eq!(&b[18..50], &l.wallet);
        assert_eq!(&b[50..58], &l.as_of.to_le_bytes());
        assert_eq!(&b[58..66], &f.period_end_exclusive.to_le_bytes());
        assert_eq!(&b[66..98], &f.commitment);
        assert_eq!(&b[98..102], &f.current_ratio_bps.to_le_bytes());
        assert_eq!(&b[102..106], &f.quick_ratio_bps.to_le_bytes());
        assert_eq!(&b[106..110], &f.cash_to_current_assets_bps.to_le_bytes());
        assert_eq!(
            &b[110..114],
            &f.cash_to_current_liabilities_bps.to_le_bytes()
        );
        assert_eq!(&b[114..118], &f.liabilities_to_assets_bps.to_le_bytes());
        assert_eq!(&b[118..122], &f.operating_margin_bps.to_le_bytes());
        assert_eq!(&b[122..126], &f.receivables_turnover_bps.to_le_bytes());
        assert_eq!(b[126], f.procedures_performed);
        assert_eq!(&b[127..131], &f.cash_agreed_to_bank_bps.to_le_bytes());
        assert_eq!(
            &b[131..135],
            &f.receivables_tied_to_invoices_bps.to_le_bytes()
        );
        assert_eq!(&b[135..139], &f.receivables_confirmed_bps.to_le_bytes());
    }

    #[test]
    fn test_a_later_version_is_read_by_its_prefix() {
        let mut leaf = sample();
        leaf.version = 2;
        let mut bytes = leaf.to_bytes();
        bytes.extend_from_slice(&[9, 9]);
        assert_eq!(BusinessLeaf::parse(&bytes).unwrap(), leaf);
    }

    #[test]
    fn test_a_short_leaf_is_refused() {
        let bytes = sample().to_bytes();
        assert_eq!(
            BusinessLeaf::parse(&bytes[..BUSINESS_LEAF_LEN - 1]),
            Err(BusinessLeafError::TooShort)
        );
    }

    #[test]
    fn test_other_kinds_are_not_business_leaves() {
        let snapshot = Snapshot {
            version: 2,
            ..Snapshot::default()
        }
        .to_bytes();
        assert_eq!(
            BusinessLeaf::parse(&snapshot),
            Err(BusinessLeafError::NotABusinessLeaf)
        );
        let bill = BillLeaf {
            version: 1,
            ..BillLeaf::default()
        }
        .to_bytes();
        assert_eq!(
            BusinessLeaf::parse(&bill),
            Err(BusinessLeafError::NotABusinessLeaf)
        );
    }

    #[test]
    fn test_a_business_leaf_is_no_other_kind() {
        let bytes = sample().to_bytes();
        assert!(Snapshot::parse(&bytes).is_err());
        assert!(BillLeaf::parse(&bytes).is_err());
    }

    fn snapshot_v3() -> Vec<u8> {
        let mut b = Snapshot {
            version: 3,
            ..Snapshot::default()
        }
        .to_bytes();
        b.extend_from_slice(&sample_facts().to_bytes());
        b
    }

    #[test]
    fn test_a_version_3_snapshot_carries_the_facts_after_version_2s() {
        let bytes = snapshot_v3();
        assert_eq!(bytes.len(), SNAPSHOT_V3_LEN);
        assert_eq!(SNAPSHOT_V3_LEN, 253);
        assert_eq!(
            BusinessFacts::parse_snapshot(&bytes).unwrap(),
            sample_facts()
        );
        // A program built for version 2 still reads its prefix.
        assert_eq!(Snapshot::parse(&bytes).unwrap().version, 3);
    }

    #[test]
    fn test_a_version_2_snapshot_has_no_facts() {
        let bytes = Snapshot {
            version: 2,
            ..Snapshot::default()
        }
        .to_bytes();
        assert_eq!(
            BusinessFacts::parse_snapshot(&bytes).unwrap(),
            BusinessFacts::default()
        );
    }

    #[test]
    fn test_a_short_version_3_snapshot_is_refused() {
        let bytes = snapshot_v3();
        assert_eq!(
            BusinessFacts::parse_snapshot(&bytes[..SNAPSHOT_V3_LEN - 1]),
            Err(SnapshotError::TooShort)
        );
    }
}
