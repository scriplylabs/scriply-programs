//! A receivables token's snapshot, a bill leaf, and the Merkle tree a signer
//! commits them to. docs/snapshot.md and docs/bill-leaf.md are the specs.
//!
//! A signer (a business that verifies orgs and their receivables, such as
//! Scriply) builds a tree whose leaves are every token's latest snapshot and
//! posts its root on chain. Anyone then proves one token's snapshot to a
//! program with the snapshot and its proof:
//!
//! ```
//! use token_snapshot::{merkle::{self, Tree}, Snapshot};
//!
//! let leaves: Vec<Vec<u8>> = (1..=3u8)
//!     .map(|i| Snapshot { mint: [i; 32], ..Snapshot::default() }.to_bytes())
//!     .collect();
//! let tree = Tree::build(&leaves).unwrap();
//! assert!(merkle::verify(&tree.root(), &leaves[1], &tree.prove(1)));
//! ```

mod bill;
pub mod leaf;
pub mod merkle;
mod snapshot;

pub use bill::*;
pub use snapshot::*;
