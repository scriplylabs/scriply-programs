use crate::leaf::{Reader, LEAF_KIND_MIN};

/// The layout version this crate reads, and every later one.
pub const SNAPSHOT_VERSION: u8 = 2;

/// The bytes of layout version 2. A later version only appends fields, so a
/// program reads the first SNAPSHOT_LEN bytes of any version and ignores
/// the rest; the signature, or the Merkle leaf, covers all of them.
pub const SNAPSHOT_LEN: usize = 172;

/// Why bytes aren't a snapshot this crate can read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotError {
    /// Fewer than SNAPSHOT_LEN bytes.
    TooShort,
    /// A version before SNAPSHOT_VERSION, which lacks fields this crate reads.
    UnsupportedVersion,
    /// Another kind of leaf: its first byte is a kind, not a version.
    NotASnapshot,
}

/// The facts a signer vouches for about one token on one network at one
/// time. Amounts are in the token's base units, shares in basis points.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub version: u8,
    /// The network ID in ASCII, padded with zeros: `sol`, `sol-devnet`.
    pub network: [u8; 16],
    /// The token's mint address.
    pub mint: [u8; 32],
    /// When the facts were true, unix seconds.
    pub as_of: i64,
    pub report_hash: [u8; 32],
    pub network_supply: u64,
    pub outstanding_units: u64,
    pub burn_debt: u64,
    pub burn_debt_overdue: bool,
    pub aging_current: u64,
    pub aging_days_1_to_30: u64,
    pub aging_days_31_to_60: u64,
    pub aging_days_61_to_90: u64,
    pub aging_over_90: u64,
    pub concentration_index_bps: u16,
    pub disputed_share_bps: u16,
    /// Days until each open invoice is due (0 once past due), averaged by
    /// the units it backs.
    pub weighted_days_to_due: u16,
    /// Days the invoices' customers have taken to pay, from invoice date to
    /// the payment reaching the org's bank, averaged by the units each
    /// customer owes now.
    pub weighted_days_to_pay: u16,
    /// Days those customers paid after the due date (0 when on time), on
    /// the same weighting.
    pub weighted_days_beyond_terms: u16,
    /// Share of those customers' paid invoice volume that reached the bank
    /// by its due date, bps.
    pub paid_on_time_share_bps: u16,
    /// Share of units owed by customers with a payment history the three
    /// fields above are drawn from, bps.
    pub history_share_bps: u16,
    /// Over the 365 days before `as_of`, the unit-days of burn debt owed
    /// (debt still owed counts up to `as_of`) over the units owed in that
    /// window (owed at its start or recorded during it), to the nearest day:
    /// how long a unit owed stayed owed, not how many were. Version 2.
    pub weighted_days_in_burn_debt: u16,
    /// Days in the 365 before `as_of` the token's burn debt was overdue:
    /// owed past its grace. Version 2.
    pub days_burn_debt_overdue: u16,
}

impl Snapshot {
    /// Parses a snapshot in layout version 2 or later, or says why it can't.
    ///
    /// ```ignore
    /// let s = Snapshot::parse(&bytes)?; // reads the first 172 bytes
    /// ```
    pub fn parse(bytes: &[u8]) -> Result<Self, SnapshotError> {
        let Some(&version) = bytes.first() else {
            return Err(SnapshotError::TooShort);
        };
        if version >= LEAF_KIND_MIN {
            return Err(SnapshotError::NotASnapshot);
        }
        if version < SNAPSHOT_VERSION {
            return Err(SnapshotError::UnsupportedVersion);
        }
        if bytes.len() < SNAPSHOT_LEN {
            return Err(SnapshotError::TooShort);
        }
        let mut r = Reader::new(bytes, 1);
        Ok(Self {
            version,
            network: r.array(),
            mint: r.array(),
            as_of: r.u64() as i64,
            report_hash: r.array(),
            network_supply: r.u64(),
            outstanding_units: r.u64(),
            burn_debt: r.u64(),
            burn_debt_overdue: r.u8() != 0,
            aging_current: r.u64(),
            aging_days_1_to_30: r.u64(),
            aging_days_31_to_60: r.u64(),
            aging_days_61_to_90: r.u64(),
            aging_over_90: r.u64(),
            concentration_index_bps: r.u16(),
            disputed_share_bps: r.u16(),
            weighted_days_to_due: r.u16(),
            weighted_days_to_pay: r.u16(),
            weighted_days_beyond_terms: r.u16(),
            paid_on_time_share_bps: r.u16(),
            history_share_bps: r.u16(),
            weighted_days_in_burn_debt: r.u16(),
            days_burn_debt_overdue: r.u16(),
        })
    }

    /// The snapshot in layout version 2, for signers, keepers and tests.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(SNAPSHOT_LEN);
        b.push(self.version);
        b.extend_from_slice(&self.network);
        b.extend_from_slice(&self.mint);
        b.extend_from_slice(&self.as_of.to_le_bytes());
        b.extend_from_slice(&self.report_hash);
        for v in [self.network_supply, self.outstanding_units, self.burn_debt] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.push(self.burn_debt_overdue as u8);
        for v in [
            self.aging_current,
            self.aging_days_1_to_30,
            self.aging_days_31_to_60,
            self.aging_days_61_to_90,
            self.aging_over_90,
        ] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in [
            self.concentration_index_bps,
            self.disputed_share_bps,
            self.weighted_days_to_due,
            self.weighted_days_to_pay,
            self.weighted_days_beyond_terms,
            self.paid_on_time_share_bps,
            self.history_share_bps,
            self.weighted_days_in_burn_debt,
            self.days_burn_debt_overdue,
        ] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Snapshot {
        let mut network = [0u8; 16];
        network[..10].copy_from_slice(b"sol-devnet");
        Snapshot {
            version: 2,
            network,
            mint: [9; 32],
            as_of: 1_800_000_000,
            report_hash: [7; 32],
            network_supply: 1_000_000_000,
            outstanding_units: 1_000_000_000,
            burn_debt: 0,
            burn_debt_overdue: false,
            aging_current: 900_000_000,
            aging_days_1_to_30: 100_000_000,
            aging_days_31_to_60: 0,
            aging_days_61_to_90: 0,
            aging_over_90: 0,
            concentration_index_bps: 2_500,
            disputed_share_bps: 3,
            weighted_days_to_due: 21,
            weighted_days_to_pay: 38,
            weighted_days_beyond_terms: 8,
            paid_on_time_share_bps: 7_200,
            history_share_bps: 9_100,
            weighted_days_in_burn_debt: 4,
            days_burn_debt_overdue: 11,
        }
    }

    #[test]
    fn test_layout_round_trips() {
        let s = sample();
        let bytes = s.to_bytes();
        assert_eq!(bytes.len(), SNAPSHOT_LEN);
        assert_eq!(Snapshot::parse(&bytes).unwrap(), s);
    }

    /// The layout is frozen once a mainnet pool pins a signer: this pins
    /// each field's offset as docs/snapshot.md gives it.
    #[test]
    fn test_layout_offsets_are_pinned() {
        let s = sample();
        let b = s.to_bytes();
        assert_eq!(b[0], 2);
        assert_eq!(&b[1..17], &s.network);
        assert_eq!(&b[17..49], &s.mint);
        assert_eq!(&b[49..57], &s.as_of.to_le_bytes());
        assert_eq!(&b[57..89], &s.report_hash);
        assert_eq!(&b[89..97], &s.network_supply.to_le_bytes());
        assert_eq!(&b[97..105], &s.outstanding_units.to_le_bytes());
        assert_eq!(&b[105..113], &s.burn_debt.to_le_bytes());
        assert_eq!(b[113], 0);
        assert_eq!(&b[114..122], &s.aging_current.to_le_bytes());
        assert_eq!(&b[122..130], &s.aging_days_1_to_30.to_le_bytes());
        assert_eq!(&b[146..154], &s.aging_over_90.to_le_bytes());
        assert_eq!(&b[154..156], &s.concentration_index_bps.to_le_bytes());
        assert_eq!(&b[156..158], &s.disputed_share_bps.to_le_bytes());
        assert_eq!(&b[158..160], &s.weighted_days_to_due.to_le_bytes());
        assert_eq!(&b[160..162], &s.weighted_days_to_pay.to_le_bytes());
        assert_eq!(&b[162..164], &s.weighted_days_beyond_terms.to_le_bytes());
        assert_eq!(&b[164..166], &s.paid_on_time_share_bps.to_le_bytes());
        assert_eq!(&b[166..168], &s.history_share_bps.to_le_bytes());
        assert_eq!(&b[168..170], &s.weighted_days_in_burn_debt.to_le_bytes());
        assert_eq!(&b[170..172], &s.days_burn_debt_overdue.to_le_bytes());
    }

    #[test]
    fn test_a_later_version_is_read_by_its_prefix() {
        let mut s = sample();
        s.version = 3;
        let mut bytes = s.to_bytes();
        bytes.extend_from_slice(&[1, 2, 3, 4]);
        assert_eq!(Snapshot::parse(&bytes).unwrap(), s);
    }

    #[test]
    fn test_a_short_snapshot_is_refused() {
        let bytes = sample().to_bytes();
        assert_eq!(
            Snapshot::parse(&bytes[..SNAPSHOT_LEN - 1]),
            Err(SnapshotError::TooShort)
        );
    }

    #[test]
    fn test_another_kind_of_leaf_is_refused() {
        let mut bytes = sample().to_bytes();
        bytes[0] = LEAF_KIND_MIN;
        assert_eq!(Snapshot::parse(&bytes), Err(SnapshotError::NotASnapshot));
    }

    #[test]
    fn test_a_version_before_2_is_refused() {
        for version in [0, 1] {
            let mut bytes = sample().to_bytes();
            bytes[0] = version;
            assert_eq!(
                Snapshot::parse(&bytes),
                Err(SnapshotError::UnsupportedVersion)
            );
        }
    }
}
