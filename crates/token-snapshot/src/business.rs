//! A business leaf: the facts a signer vouches for about the business behind
//! a wallet, from the financial statements it published and the procedures
//! the signer performed on them. A token's snapshot from layout version 3
//! carries the same facts after its own. docs/business-leaf.md is the spec.

use crate::leaf::{Reader, LEAF_KIND_BUSINESS};
use crate::snapshot::{SnapshotError, SNAPSHOT_LEN};

/// The business leaf's layout version this crate writes. It reads version 1
/// and every later one.
pub const BUSINESS_LEAF_VERSION: u8 = 2;

/// The bytes of layout version 1, the shortest leaf this crate reads.
pub const BUSINESS_LEAF_V1_LEN: usize = 58 + BUSINESS_FACTS_V1_LEN;

/// The bytes of layout version 2. A later version only appends fields.
pub const BUSINESS_LEAF_LEN: usize = 58 + BUSINESS_FACTS_LEN;

/// The bytes of the business's facts in a version 1 business leaf and a
/// version 3 snapshot: what it published and the procedures on it.
pub const BUSINESS_FACTS_V1_LEN: usize = 81;

/// The bytes of the facts the signer reads at the root's `as_of`
/// (`AsOfFacts`), appended to the business's facts from business leaf
/// version 2 and snapshot version 4.
pub const AS_OF_FACTS_LEN: usize = 18;

/// The bytes of the business's facts in a version 2 business leaf and a
/// version 4 snapshot.
pub const BUSINESS_FACTS_LEN: usize = BUSINESS_FACTS_V1_LEN + AS_OF_FACTS_LEN;

/// The snapshot layout version that appends the business's facts.
pub const SNAPSHOT_BUSINESS_VERSION: u8 = 3;

/// The snapshot layout version that appends the facts at `as_of` after
/// them.
pub const SNAPSHOT_AS_OF_FACTS_VERSION: u8 = 4;

/// The bytes of a snapshot in layout version 3.
pub const SNAPSHOT_V3_LEN: usize = SNAPSHOT_LEN + BUSINESS_FACTS_V1_LEN;

/// The bytes of a snapshot in layout version 4.
pub const SNAPSHOT_V4_LEN: usize = SNAPSHOT_LEN + BUSINESS_FACTS_LEN;

/// How the layout writes an unsigned ratio with no value (its denominator
/// was zero). `BusinessFacts` reads it as `None`, so no rule compares it as
/// a number.
pub const RATIO_NONE: u32 = u32::MAX;

/// How the layout writes `operating_margin_bps` with no value (no revenue).
/// `BusinessFacts` reads it as `None`, so no rule compares it as a number.
pub const SIGNED_RATIO_NONE: i32 = i32::MAX;

/// The facts a version 4 snapshot appends when its business never
/// published: every byte 0 (a version 3 snapshot's are its first
/// BUSINESS_FACTS_V1_LEN). `BusinessFacts::parse_snapshot` reads them as
/// `None`.
pub const NEVER_PUBLISHED: [u8; BUSINESS_FACTS_LEN] = [0; BUSINESS_FACTS_LEN];

/// Which procedures the signer performed on the published statements, as
/// bits of `procedures_performed`.
pub const PROCEDURE_CASH_AGREED_TO_BANK: u8 = 1;
pub const PROCEDURE_RECEIVABLES_TIED_TO_INVOICES: u8 = 2;
pub const PROCEDURE_RECEIVABLES_CONFIRMED: u8 = 4;

/// A count of the business's supplier pool listings, in bands
/// (`AsOfFacts`), so its record can't be matched to the pools' public
/// listings one by one.
pub const COUNT_NONE: u8 = 0;
pub const COUNT_ONE: u8 = 1;
pub const COUNT_TWO_TO_FOUR: u8 = 2;
pub const COUNT_FIVE_TO_NINE: u8 = 3;
pub const COUNT_TEN_OR_MORE: u8 = 4;

/// The most days a listing is overdue, in bands (`AsOfFacts`).
pub const DAYS_OVERDUE_NONE: u8 = 0;
pub const DAYS_OVERDUE_1_TO_7: u8 = 1;
pub const DAYS_OVERDUE_8_TO_30: u8 = 2;
pub const DAYS_OVERDUE_31_TO_60: u8 = 3;
pub const DAYS_OVERDUE_61_TO_90: u8 = 4;
pub const DAYS_OVERDUE_OVER_90: u8 = 5;

/// Why bytes aren't a business leaf this crate can read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusinessLeafError {
    /// Fewer bytes than the leaf's version needs: BUSINESS_LEAF_V1_LEN
    /// for version 1, BUSINESS_LEAF_LEN for version 2 or later.
    TooShort,
    /// The first byte isn't LEAF_KIND_BUSINESS: another kind of leaf.
    NotABusinessLeaf,
    /// Version 0, which no layout has.
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
    /// What the signer read at the root's `as_of`; `None` in a version 1
    /// business leaf or a version 3 snapshot, which carry none.
    pub as_of_facts: Option<AsOfFacts>,
}

/// What the signer reads about the business at the root's `as_of`, rather
/// than from its publication: the bank's cash now, and what it owes supplier
/// pools and how it has repaid them, across every pool and network. Ratios
/// are in basis points, `None` with no value; the record of listings is in
/// bands (`COUNT_*`, `DAYS_OVERDUE_*`) over the business's listings in every
/// supplier pool the signer attests bills for. docs/business-leaf.md
/// defines each.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AsOfFacts {
    /// The bank's latest cash, across the business's linked bank accounts
    /// in its books, to the statements' current liabilities; `None` when
    /// none is linked, one account's balance is more than 5 days old or in
    /// another currency than the statements', or current liabilities are 0.
    pub bank_cash_now_to_current_liabilities_bps: Option<u32>,
    /// What the business owes supplier pools now to the statements'
    /// current liabilities; `None` when a listing is in another currency
    /// than the statements', or current liabilities are 0.
    pub supplier_finance_to_current_liabilities_bps: Option<u32>,
    /// Of what it owes supplier pools now, the share for vendors it has
    /// paid itself for at least 180 days; `None` when it owes nothing or a
    /// listing is in another currency than the statements'.
    pub established_vendor_share_bps: Option<u32>,
    /// Listings a pool paid out that the business hasn't repaid, a whole
    /// day (86,400 seconds) or more past their `repayOn`, written off or
    /// not.
    pub overdue_unpaid: u8,
    /// The most whole days one of those is past its `repayOn`.
    pub most_days_overdue: u8,
    /// Listings written off that the business hasn't repaid.
    pub written_off_unrepaid: u8,
    /// Listings written off in the 365 days before `as_of`.
    pub written_off_in_year: u8,
    /// Listings repaid in the 365 days before `as_of`.
    pub repaid_in_year: u8,
    /// Of those, the ones repaid a whole day or more after their
    /// `repayOn`.
    pub repaid_late_in_year: u8,
}

impl AsOfFacts {
    fn read(r: &mut Reader) -> Self {
        Self {
            bank_cash_now_to_current_liabilities_bps: read_ratio(r.u32()),
            supplier_finance_to_current_liabilities_bps: read_ratio(r.u32()),
            established_vendor_share_bps: read_ratio(r.u32()),
            overdue_unpaid: r.u8(),
            most_days_overdue: r.u8(),
            written_off_unrepaid: r.u8(),
            written_off_in_year: r.u8(),
            repaid_in_year: r.u8(),
            repaid_late_in_year: r.u8(),
        }
    }

    fn append_to(&self, b: &mut Vec<u8>) {
        for v in [
            self.bank_cash_now_to_current_liabilities_bps,
            self.supplier_finance_to_current_liabilities_bps,
            self.established_vendor_share_bps,
        ] {
            b.extend_from_slice(&v.unwrap_or(RATIO_NONE).to_le_bytes());
        }
        b.extend_from_slice(&[
            self.overdue_unpaid,
            self.most_days_overdue,
            self.written_off_unrepaid,
            self.written_off_in_year,
            self.repaid_in_year,
            self.repaid_late_in_year,
        ]);
    }
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
    /// Reads the facts at r, and the facts at `as_of` after them when
    /// `has_as_of_facts`; the caller checks the length first.
    fn read(r: &mut Reader, has_as_of_facts: bool) -> Self {
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
            as_of_facts: has_as_of_facts.then(|| AsOfFacts::read(r)),
        }
    }

    /// The facts' bytes: BUSINESS_FACTS_LEN with `as_of_facts`, as a
    /// version 2 business leaf and a version 4 snapshot carry them, and
    /// BUSINESS_FACTS_V1_LEN without. A ratio is written as given: the
    /// signer clamps one past its field first (docs/business-leaf.md).
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
        if let Some(f) = &self.as_of_facts {
            f.append_to(&mut b);
        }
        b
    }

    /// The business's facts a token's snapshot carries, appended from
    /// layout version 3, with the facts at `as_of` from version 4: `None`
    /// for a version 2 snapshot, which has none, and for a business that
    /// never published (`NEVER_PUBLISHED`), so a pool can't read missing
    /// figures as figures. Refuses what `Snapshot::parse` refuses, a
    /// version 3 snapshot shorter than SNAPSHOT_V3_LEN, and a version 4 or
    /// later one shorter than SNAPSHOT_V4_LEN.
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
        let has_as_of_facts = snapshot.version >= SNAPSHOT_AS_OF_FACTS_VERSION;
        let len = if has_as_of_facts {
            SNAPSHOT_V4_LEN
        } else {
            SNAPSHOT_V3_LEN
        };
        if bytes.len() < len {
            return Err(SnapshotError::TooShort);
        }
        let facts = Self::read(&mut Reader::new(bytes, SNAPSHOT_LEN), has_as_of_facts);
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
    /// can't. A version 1 leaf has no facts at `as_of`
    /// (`facts.as_of_facts` is `None`).
    ///
    /// ```ignore
    /// let business = BusinessLeaf::parse(&bytes)?; // reads the first 157 bytes of version 2
    /// let Some(now) = business.facts.as_of_facts else { return refuse() };
    /// ```
    pub fn parse(bytes: &[u8]) -> Result<Self, BusinessLeafError> {
        if bytes.len() < 2 {
            return Err(BusinessLeafError::TooShort);
        }
        if bytes[0] != LEAF_KIND_BUSINESS {
            return Err(BusinessLeafError::NotABusinessLeaf);
        }
        if bytes[1] == 0 {
            return Err(BusinessLeafError::UnsupportedVersion);
        }
        let has_as_of_facts = bytes[1] >= 2;
        let len = if has_as_of_facts {
            BUSINESS_LEAF_LEN
        } else {
            BUSINESS_LEAF_V1_LEN
        };
        if bytes.len() < len {
            return Err(BusinessLeafError::TooShort);
        }
        let mut r = Reader::new(bytes, 2);
        Ok(Self {
            version: bytes[1],
            network: r.array(),
            wallet: r.array(),
            as_of: r.u64() as i64,
            facts: BusinessFacts::read(&mut r, has_as_of_facts),
        })
    }

    /// The leaf's bytes, for signers, keepers and tests: version 2 when its
    /// facts hold `as_of_facts`, as `version` should then say, and version
    /// 1 without.
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
            as_of_facts: Some(sample_as_of_facts()),
        }
    }

    fn sample_as_of_facts() -> AsOfFacts {
        AsOfFacts {
            bank_cash_now_to_current_liabilities_bps: Some(7_250),
            supplier_finance_to_current_liabilities_bps: Some(1_900),
            established_vendor_share_bps: None,
            overdue_unpaid: COUNT_ONE,
            most_days_overdue: DAYS_OVERDUE_8_TO_30,
            written_off_unrepaid: COUNT_NONE,
            written_off_in_year: COUNT_ONE,
            repaid_in_year: COUNT_FIVE_TO_NINE,
            repaid_late_in_year: COUNT_TWO_TO_FOUR,
        }
    }

    fn sample() -> BusinessLeaf {
        let mut network = [0u8; 16];
        network[..10].copy_from_slice(b"sol-devnet");
        BusinessLeaf {
            version: 2,
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
        assert_eq!(BUSINESS_LEAF_LEN, 157);
        assert_eq!(BusinessLeaf::parse(&bytes).unwrap(), leaf);
    }

    /// A version 1 leaf, which a signer wrote before the facts at `as_of`,
    /// is still read, with none of them.
    #[test]
    fn test_a_version_1_leaf_has_no_facts_at_as_of() {
        let mut leaf = sample();
        leaf.version = 1;
        leaf.facts.as_of_facts = None;
        let bytes = leaf.to_bytes();
        assert_eq!(bytes.len(), BUSINESS_LEAF_V1_LEN);
        assert_eq!(BUSINESS_LEAF_V1_LEN, 139);
        assert_eq!(BusinessLeaf::parse(&bytes).unwrap(), leaf);
    }

    /// Pins each field's offset as docs/business-leaf.md gives it.
    #[test]
    fn test_layout_offsets_are_pinned() {
        let l = sample();
        let f = &l.facts;
        let b = l.to_bytes();
        assert_eq!(b[0], LEAF_KIND_BUSINESS);
        assert_eq!(b[1], 2);
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
        assert_eq!(&b[139..143], &7_250u32.to_le_bytes());
        assert_eq!(&b[143..147], &1_900u32.to_le_bytes());
        assert_eq!(&b[147..151], &[0xff; 4], "no value is the type's maximum");
        assert_eq!(b[151], COUNT_ONE);
        assert_eq!(b[152], DAYS_OVERDUE_8_TO_30);
        assert_eq!(b[153], COUNT_NONE);
        assert_eq!(b[154], COUNT_ONE);
        assert_eq!(b[155], COUNT_FIVE_TO_NINE);
        assert_eq!(b[156], COUNT_TWO_TO_FOUR);
        assert_eq!(f.period_end_exclusive, 1_798_761_600);
    }

    #[test]
    fn test_a_later_version_is_read_by_its_prefix() {
        let mut leaf = sample();
        leaf.version = 3;
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
        // A version 2 leaf with only version 1's bytes is refused, never
        // read as having no facts at `as_of`.
        assert_eq!(
            BusinessLeaf::parse(&bytes[..BUSINESS_LEAF_V1_LEN]),
            Err(BusinessLeafError::TooShort)
        );
    }

    #[test]
    fn test_version_0_is_refused() {
        let mut bytes = sample().to_bytes();
        bytes[1] = 0;
        assert_eq!(
            BusinessLeaf::parse(&bytes),
            Err(BusinessLeafError::UnsupportedVersion)
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
        .to_bytes()
        .unwrap();
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

    fn sample_v1_facts() -> BusinessFacts {
        BusinessFacts {
            as_of_facts: None,
            ..sample_facts()
        }
    }

    fn snapshot_with(version: u8, facts: &BusinessFacts) -> Vec<u8> {
        let mut b = Snapshot {
            version,
            ..Snapshot::default()
        }
        .to_bytes();
        b.extend_from_slice(&facts.to_bytes());
        b
    }

    #[test]
    fn test_a_version_3_snapshot_carries_the_facts_after_version_2s() {
        let bytes = snapshot_with(3, &sample_v1_facts());
        assert_eq!(bytes.len(), SNAPSHOT_V3_LEN);
        assert_eq!(SNAPSHOT_V3_LEN, 253);
        assert_eq!(
            BusinessFacts::parse_snapshot(&bytes).unwrap(),
            Some(sample_v1_facts())
        );
        // A program built for version 2 still reads its prefix.
        assert_eq!(Snapshot::parse(&bytes).unwrap().version, 3);
    }

    #[test]
    fn test_a_version_4_snapshot_carries_the_facts_at_as_of_after_version_3s() {
        let bytes = snapshot_with(4, &sample_facts());
        assert_eq!(bytes.len(), SNAPSHOT_V4_LEN);
        assert_eq!(SNAPSHOT_V4_LEN, 271);
        assert_eq!(
            BusinessFacts::parse_snapshot(&bytes).unwrap(),
            Some(sample_facts())
        );
        // A program built for version 3 still reads its facts.
        assert_eq!(
            &bytes[..SNAPSHOT_V3_LEN][SNAPSHOT_LEN..],
            &sample_v1_facts().to_bytes()[..]
        );
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
            version: 4,
            ..Snapshot::default()
        }
        .to_bytes();
        bytes.extend_from_slice(&NEVER_PUBLISHED);
        assert_eq!(BusinessFacts::parse_snapshot(&bytes).unwrap(), None);
        let mut v3 = Snapshot {
            version: 3,
            ..Snapshot::default()
        }
        .to_bytes();
        v3.extend_from_slice(&NEVER_PUBLISHED[..BUSINESS_FACTS_V1_LEN]);
        assert_eq!(BusinessFacts::parse_snapshot(&v3).unwrap(), None);
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
        let b = snapshot_with(4, &facts);
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
        let bytes = snapshot_with(3, &sample_v1_facts());
        assert_eq!(
            BusinessFacts::parse_snapshot(&bytes[..SNAPSHOT_V3_LEN - 1]),
            Err(SnapshotError::TooShort)
        );
    }

    /// A version 4 snapshot with only version 3's bytes is refused, never
    /// read as having no facts at `as_of`.
    #[test]
    fn test_a_short_version_4_snapshot_is_refused() {
        let bytes = snapshot_with(4, &sample_facts());
        assert_eq!(
            BusinessFacts::parse_snapshot(&bytes[..SNAPSHOT_V4_LEN - 1]),
            Err(SnapshotError::TooShort)
        );
    }
}
