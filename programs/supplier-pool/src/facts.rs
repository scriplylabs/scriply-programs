//! Reading a bill's facts: its bill leaf, proven against a root one of the
//! pool's signers posted to the snapshot-roots program. The layout is
//! `token_snapshot`'s; docs/bill-leaf.md is its spec.

use anchor_lang::prelude::*;
use snapshot_roots::{
    token_snapshot::{
        merkle::{self, MERKLE_SCHEME_V1},
        BillLeaf,
    },
    SnapshotRoots,
};

use crate::{error::SupplierPoolError, state::Pool};

/// A bill leaf and its proof, as the anchor hands them in.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct ProvenLeaf {
    /// The root the proof is against; it must be the signer's latest.
    pub root: [u8; 32],
    /// The leaf's bytes.
    pub leaf: Vec<u8>,
    /// The leaf's siblings from the bottom up.
    pub proof: Vec<[u8; 32]>,
}

/// Returns the bill leaf `proven` shows for `commitment`, read from `roots`
/// at `now`, or why the pool can't use it: the roots aren't a pool signer's,
/// the root isn't the signer's latest or is older than the pool allows, the
/// proof fails, or the leaf isn't a bill leaf of this network, currency and
/// commitment.
///
/// Only the latest root is read, so nobody can pick an older one still kept
/// whose facts were kinder.
///
/// ```ignore
/// let bill = read_bill_leaf(&pool, &roots, &proven, &commitment, now)?;
/// ```
pub fn read_bill_leaf(
    pool: &Pool,
    roots: &SnapshotRoots,
    proven: &ProvenLeaf,
    commitment: &[u8; 32],
    now: i64,
) -> Result<BillLeaf> {
    require!(
        pool.is_signer(&roots.signer),
        SupplierPoolError::UnknownSigner
    );
    let posted = roots
        .latest()
        .filter(|p| p.root == proven.root && p.scheme == MERKLE_SCHEME_V1)
        .ok_or_else(|| error!(SupplierPoolError::RootNotLatest))?;
    require!(
        now.saturating_sub(posted.as_of) <= pool.max_root_age as i64,
        SupplierPoolError::RootTooOld
    );
    require!(
        merkle::verify(&proven.root, &proven.leaf, &proven.proof),
        SupplierPoolError::InvalidProof
    );
    let bill =
        BillLeaf::parse(&proven.leaf).map_err(|_| error!(SupplierPoolError::NotABillLeaf))?;
    require!(
        bill.network == pool.network,
        SupplierPoolError::WrongNetwork
    );
    require!(
        bill.currency == pool.currency,
        SupplierPoolError::WrongCurrency
    );
    require!(
        bill.commitment == *commitment,
        SupplierPoolError::WrongCommitment
    );
    Ok(bill)
}
