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

/// How the layout writes an unsigned ratio with no value (its denominator
/// was zero). `BusinessFacts` reads it as `None`, so no rule compares it as
/// a number.
pub const RATIO_NONE: u32 = u32::MAX;

/// How the layout writes `operating_margin_bps` with no value (no revenue).
/// `BusinessFacts` reads it as `None`, so no rule compares it as a number.
pub const SIGNED_RATIO_NONE: i32 = i32::MAX;

/// The facts a version 3 snapshot appends when its business never
/// published: every byte 0. `BusinessFacts::parse_snapshot` reads them as
/// `None`.
pub const NEVER_PUBLISHED: [u8; BUSINESS_FACTS_LEN] = [0; BUSINESS_FACTS_LEN];

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
/// `None` when their denominator was zero; a procedure's result is `None`
/// when the signer didn't perform it. A pool decides what `None` means for
/// its own rule: it is never the best or the worst value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BusinessFacts {
    /// The end of the published statements' period, unix seconds: the
    /// balance sheet's date, and the midnight after the income statement's
    /// last day.
    pub period_end_exclusive: i64,
    /// HMAC-SHA256 of the statements, keyed by a key only the signer and
    /// the business hold; the business opens it to a lender it chooses.
    pub commitment: [u8; 32],
    pub current_ratio_bps: Option<u32>,
    pub quick_ratio_bps: Option<u32>,
    pub cash_to_current_assets_bps: Option<u32>,
    pub cash_to_current_liabilities_bps: Option<u32>,
    pub liabilities_to_assets_bps: Option<u32>,
    /// Operating income to revenue over the statements' year; negative for
    /// a loss.
    pub operating_margin_bps: Option<i32>,
    /// The year's revenue to receivables at `period_end_exclusive`.
    pub receivables_turnover_bps: Option<u32>,
    /// The bank's cash at `period_end_exclusive`, from the business's
    /// linked bank accounts, to the statements' cash
    /// (`PROCEDURE_CASH_AGREED_TO_BANK`).
    pub cash_agreed_to_bank_bps: Option<u32>,
    /// The invoices open at `period_end_exclusive` to the statements'
    /// receivables (`PROCEDURE_RECEIVABLES_TIED_TO_INVOICES`).
    pub receivables_tied_to_invoices_bps: Option<u32>,
    /// Of those invoices, the share by amount their customers confirmed
    /// (`PROCEDURE_RECEIVABLES_CONFIRMED`).
    pub receivables_confirmed_bps: Option<u32>,
}

fn read_ratio(v: u32) -> Option<u32> {
    (v != RATIO_NONE).then_some(v)
}

fn read_signed_ratio(v: i32) -> Option<i32> {
    (v != SIGNED_RATIO_NONE).then_some(v)
}

fn read_procedure(performed: u8, bit: u8, v: u32) -> Option<u32> {
    (performed & bit != 0).then_some(v)
}

impl BusinessFacts {
    fn read(r: &mut Reader) -> Self {
        let period_end_exclusive = r.u64() as i64;
        let commitment = r.array();
        let current_ratio_bps = read_ratio(r.u32());
        let quick_ratio_bps = read_ratio(r.u32());
        let cash_to_current_assets_bps = read_ratio(r.u32());
        let cash_to_current_liabilities_bps = read_ratio(r.u32());
        let liabilities_to_assets_bps = read_ratio(r.u32());
        let operating_margin_bps = read_signed_ratio(r.u32() as i32);
        let receivables_turnover_bps = read_ratio(r.u32());
        let performed = r.u8();
        Self {
            period_end_exclusive,
            commitment,
            current_ratio_bps,
            quick_ratio_bps,
            cash_to_current_assets_bps,
            cash_to_current_liabilities_bps,
            liabilities_to_assets_bps,
            operating_margin_bps,
            receivables_turnover_bps,
            cash_agreed_to_bank_bps: read_procedure(
                performed,
                PROCEDURE_CASH_AGREED_TO_BANK,
                r.u32(),
            ),
            receivables_tied_to_invoices_bps: read_procedure(
                performed,
                PROCEDURE_RECEIVABLES_TIED_TO_INVOICES,
                r.u32(),
            ),
            receivables_confirmed_bps: read_procedure(
                performed,
                PROCEDURE_RECEIVABLES_CONFIRMED,
                r.u32(),
            ),
        }
    }

    /// The facts' BUSINESS_FACTS_LEN bytes. A ratio is written as given:
    /// the signer clamps one past its field first (docs/business-leaf.md).
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
            b.extend_from_slice(&v.unwrap_or(RATIO_NONE).to_le_bytes());
        }
        b.extend_from_slice(
            &self
                .operating_margin_bps
                .unwrap_or(SIGNED_RATIO_NONE)
                .to_le_bytes(),
        );
        b.extend_from_slice(
            &self
                .receivables_turnover_bps
                .unwrap_or(RATIO_NONE)
                .to_le_bytes(),
        );
        let procedures = [
            (PROCEDURE_CASH_AGREED_TO_BANK, self.cash_agreed_to_bank_bps),
            (
                PROCEDURE_RECEIVABLES_TIED_TO_INVOICES,
                self.receivables_tied_to_invoices_bps,
            ),
            (
                PROCEDURE_RECEIVABLES_CONFIRMED,
                self.receivables_confirmed_bps,
            ),
        ];
        b.push(
            procedures
                .iter()
                .filter(|(_, v)| v.is_some())
                .fold(0, |bits, (bit, _)| bits | bit),
        );
        for (_, v) in procedures {
            b.extend_from_slice(&v.unwrap_or(0).to_le_bytes());
        }
        b
    }

    /// The business's facts a token's snapshot carries, appended from
    /// layout version 3: `None` for a version 2 snapshot, which has none,
    /// and for a business that never published (`NEVER_PUBLISHED`), so a
    /// pool can't read missing figures as figures. Refuses what
    /// `Snapshot::parse` refuses, and a version 3 or later snapshot shorter
    /// than SNAPSHOT_V3_LEN.
    ///
    /// ```ignore
    /// let snap = Snapshot::parse(&bytes)?;
    /// let Some(business) = BusinessFacts::parse_snapshot(&bytes)? else { return refuse() };
    /// ```
    pub fn parse_snapshot(bytes: &[u8]) -> Result<Option<Self>, SnapshotError> {
        let snapshot = crate::Snapshot::parse(bytes)?;
        if snapshot.version < SNAPSHOT_BUSINESS_VERSION {
            return Ok(None);
        }
        if bytes.len() < SNAPSHOT_V3_LEN {
            return Err(SnapshotError::TooShort);
        }
        let facts = Self::read(&mut Reader::new(bytes, SNAPSHOT_LEN));
        Ok((facts.period_end_exclusive != 0).then_some(facts))
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
            current_ratio_bps: Some(18_500),
            quick_ratio_bps: Some(12_000),
            cash_to_current_assets_bps: Some(3_100),
            cash_to_current_liabilities_bps: None,
            liabilities_to_assets_bps: Some(4_200),
            operating_margin_bps: Some(-750),
            receivables_turnover_bps: Some(81_000),
            cash_agreed_to_bank_bps: Some(9_870),
            receivables_tied_to_invoices_bps: None,
            receivables_confirmed_bps: Some(6_400),
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
        assert_eq!(&b[98..102], &18_500u32.to_le_bytes());
        assert_eq!(&b[102..106], &12_000u32.to_le_bytes());
        assert_eq!(&b[106..110], &3_100u32.to_le_bytes());
        assert_eq!(&b[110..114], &[0xff; 4], "no value is the type's maximum");
        assert_eq!(&b[114..118], &4_200u32.to_le_bytes());
        assert_eq!(&b[118..122], &(-750i32).to_le_bytes());
        assert_eq!(&b[122..126], &81_000u32.to_le_bytes());
        assert_eq!(
            b[126],
            PROCEDURE_CASH_AGREED_TO_BANK | PROCEDURE_RECEIVABLES_CONFIRMED
        );
        assert_eq!(&b[127..131], &9_870u32.to_le_bytes());
        assert_eq!(&b[131..135], &[0; 4], "a procedure not performed is 0");
        assert_eq!(&b[135..139], &6_400u32.to_le_bytes());
        assert_eq!(f.period_end_exclusive, 1_798_761_600);
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
            Some(sample_facts())
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
        assert_eq!(BusinessFacts::parse_snapshot(&bytes).unwrap(), None);
    }

    #[test]
    fn test_a_business_that_never_published_has_no_facts() {
        let mut bytes = Snapshot {
            version: 3,
            ..Snapshot::default()
        }
        .to_bytes();
        bytes.extend_from_slice(&NEVER_PUBLISHED);
        assert_eq!(BusinessFacts::parse_snapshot(&bytes).unwrap(), None);
    }

    /// A ratio with no value, or a procedure not performed, is never a
    /// number a rule compares: zero revenue doesn't read as the best margin.
    #[test]
    fn test_no_value_is_none_not_a_number() {
        let facts = BusinessFacts {
            operating_margin_bps: None,
            current_ratio_bps: None,
            cash_agreed_to_bank_bps: None,
            ..sample_facts()
        };
        let mut b = Snapshot {
            version: 3,
            ..Snapshot::default()
        }
        .to_bytes();
        b.extend_from_slice(&facts.to_bytes());
        let read = BusinessFacts::parse_snapshot(&b).unwrap().unwrap();
        assert_eq!(read.operating_margin_bps, None);
        assert_eq!(read.current_ratio_bps, None);
        assert_eq!(read.cash_agreed_to_bank_bps, None);
        assert_eq!(read, facts);
    }

    #[test]
    fn test_a_result_whose_bit_is_clear_is_none() {
        let mut b = sample().to_bytes();
        b[131..135].copy_from_slice(&7u32.to_le_bytes());
        let read = BusinessLeaf::parse(&b).unwrap();
        assert_eq!(read.facts.receivables_tied_to_invoices_bps, None);
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
