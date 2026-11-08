//! Where each signer posts the Merkle root of its receivables tokens'
//! snapshots.
//!
//! A signer (a business that verifies orgs and their receivables, such as
//! Scriply) builds a tree whose leaves are every token's latest snapshot and
//! posts the root to its own account, at an address derived from its key.
//! Nobody else can post there and nothing administers it. A program that
//! trusts a signer reads that account and verifies a token's snapshot, with
//! its proof, against a root in it: `token_snapshot::merkle::verify`.
//! docs/snapshot.md is the spec.

pub mod error;
pub mod events;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;

pub use instructions::*;
pub use state::*;
/// The snapshot layout and Merkle proofs, for programs reading roots.
pub use token_snapshot;

declare_id!("BZ3g7Fh9QSvwZbq6rDeKq11Tq99SQ1SmyC6qzZYT82ea");

#[program]
pub mod snapshot_roots {
    use super::*;

    /// Posts the root of a tree of `leaf_count` snapshots, built by
    /// `scheme` and all true as of `as_of`, to the signer's account,
    /// creating it on the first post.
    pub fn post_root(ctx: Context<PostRoot>, root: PostedRoot) -> Result<()> {
        instructions::post_root::handle_post_root(ctx, root)
    }
}
