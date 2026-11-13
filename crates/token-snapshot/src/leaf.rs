//! What kind of leaf a tree holds, by its first byte.
//!
//! A token's snapshot starts with its layout version, below
//! `LEAF_KIND_MIN`; every other kind of leaf starts with its kind, at or
//! above it, then its own layout version. The hash covers the first byte, so
//! one kind is never read as another.

/// The first byte of a leaf that isn't a token's snapshot is at least this.
pub const LEAF_KIND_MIN: u8 = 0x80;

/// A business leaf, about the business behind a wallet: reserved, not yet
/// laid out.
pub const LEAF_KIND_BUSINESS: u8 = 0x81;

/// A bill leaf: an approved offer of a bill for early payment.
pub const LEAF_KIND_BILL: u8 = 0x82;

/// Reads fixed-width little-endian fields; each parser checks the length
/// first.
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8], at: usize) -> Self {
        Self { bytes, at }
    }

    pub(crate) fn array<const N: usize>(&mut self) -> [u8; N] {
        let mut out = [0u8; N];
        out.copy_from_slice(&self.bytes[self.at..self.at + N]);
        self.at += N;
        out
    }

    pub(crate) fn u8(&mut self) -> u8 {
        self.array::<1>()[0]
    }

    pub(crate) fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.array())
    }

    pub(crate) fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.array())
    }
}
