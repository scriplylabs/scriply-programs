//! Reading a token's facts: its snapshot, proven against a root one of the
//! pool's signers posted to the snapshot-roots program. The layout is
//! `token_snapshot`'s; docs/snapshot.md is its spec.

use anchor_lang::prelude::*;
use snapshot_roots::{
    token_snapshot::merkle::{self, MERKLE_SCHEME_V1},
    SnapshotRoots,
};

use crate::{error::PoolError, state::Pool};

pub use snapshot_roots::token_snapshot::{Snapshot, SnapshotError, SNAPSHOT_V1_LEN};

/// A token's snapshot and its proof, as a keeper or borrower hands them in.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct ProvenSnapshot {
    /// The root the proof is against; it must be among the signer's kept
    /// roots.
    pub root: [u8; 32],
    /// The snapshot's bytes, in layout version 1 or later.
    pub snapshot: Vec<u8>,
    /// The leaf's siblings from the bottom up.
    pub proof: Vec<[u8; 32]>,
}

/// A token's facts, and the `as_of` of the root they were read from.
pub struct TokenFacts {
    pub snapshot: Snapshot,
    pub root_as_of: i64,
}

/// Returns the facts `proven` shows for `mint`, read from `roots` at `now`,
/// or why the pool can't use them: the roots aren't a pool signer's, the
/// root isn't kept or is older than the pool allows, the proof fails, or
/// the snapshot is of another network or token.
///
/// ```ignore
/// let facts = read_token_facts(&pool, &roots, &proven, mint, now)?;
/// ```
pub fn read_token_facts(
    pool: &Pool,
    roots: &SnapshotRoots,
    proven: &ProvenSnapshot,
    mint: Pubkey,
    now: i64,
) -> Result<TokenFacts> {
    require!(pool.is_signer(&roots.signer), PoolError::UnknownSigner);
    let posted = roots
        .find(&proven.root)
        .filter(|p| p.scheme == MERKLE_SCHEME_V1)
        .ok_or_else(|| error!(PoolError::RootNotFound))?;
    require!(
        now.saturating_sub(posted.as_of) <= pool.max_root_age as i64,
        PoolError::RootTooOld
    );
    require!(
        merkle::verify(&proven.root, &proven.snapshot, &proven.proof),
        PoolError::InvalidProof
    );
    let snapshot = Snapshot::parse(&proven.snapshot).map_err(|e| match e {
        SnapshotError::TooShort => error!(PoolError::SnapshotTooShort),
        SnapshotError::UnsupportedVersion => error!(PoolError::UnsupportedSnapshotVersion),
    })?;
    require!(snapshot.network == pool.network, PoolError::WrongNetwork);
    require_keys_eq!(
        Pubkey::new_from_array(snapshot.mint),
        mint,
        PoolError::WrongMint
    );
    Ok(TokenFacts {
        snapshot,
        root_as_of: posted.as_of,
    })
}
