//! Reading a listing's facts: the bill's bill leaf and the anchor's business
//! leaf, both proven against the latest root one of the pool's signers
//! posted to the snapshot-roots program. The layouts are `token_snapshot`'s;
//! docs/bill-leaf.md and docs/business-leaf.md are their specs.

use anchor_lang::prelude::*;
use snapshot_roots::{
    token_snapshot::{
        merkle::{self, MERKLE_SCHEME_V1},
        AsOfFacts, BillLeaf, BusinessLeaf,
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

/// A leaf and its proof under the root of the `ProvenLeaf` it comes with,
/// as the anchor hands in its business leaf: one root for both keeps the
/// listing's transaction small.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, Default)]
pub struct LeafProof {
    /// The leaf's bytes.
    pub leaf: Vec<u8>,
    /// The leaf's siblings from the bottom up.
    pub proof: Vec<[u8; 32]>,
}

/// What a listing is decided on: the bill's facts and the anchor's record
/// with supplier pools.
pub struct ListingFacts {
    pub bill: BillLeaf,
    pub record: AsOfFacts,
}

/// Returns the bill leaf `proven` shows for `commitment`, and the record
/// `business` shows for `anchor` under the same root, read from `roots` at
/// `now`, or why the pool can't use them: the roots aren't a pool signer's,
/// the root isn't the signer's latest or is older than the pool allows, a
/// proof fails, the bill leaf isn't a bill leaf of this network, currency
/// and commitment, or the business leaf isn't `anchor`'s on this network
/// with a record (version 2 or later).
///
/// Only the latest root is read, so nobody can pick an older one still kept
/// whose facts were kinder. A wallet with no business leaf has no record at
/// all, not a clean one, so it can't list.
///
/// ```ignore
/// let facts = read_listing_facts(&pool, &roots, &proven, &business, &commitment, &anchor, now)?;
/// ```
pub fn read_listing_facts(
    pool: &Pool,
    roots: &SnapshotRoots,
    proven: &ProvenLeaf,
    business: &LeafProof,
    commitment: &[u8; 32],
    anchor: &Pubkey,
    now: i64,
) -> Result<ListingFacts> {
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

    require!(
        merkle::verify(&proven.root, &business.leaf, &business.proof),
        SupplierPoolError::InvalidProof
    );
    let leaf = BusinessLeaf::parse(&business.leaf)
        .map_err(|_| error!(SupplierPoolError::NotABusinessLeaf))?;
    require!(
        leaf.network == pool.network && leaf.wallet == anchor.to_bytes(),
        SupplierPoolError::WrongBusiness
    );
    let record = leaf
        .facts
        .as_of_facts
        .ok_or_else(|| error!(SupplierPoolError::NoSupplierPoolRecord))?;
    Ok(ListingFacts { bill, record })
}
