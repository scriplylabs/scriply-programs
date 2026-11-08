//! The Merkle tree a signer commits every token's snapshot to.
//!
//! - A leaf is `sha256(0x00 || snapshot)`, over all the snapshot's bytes.
//! - A node is `sha256(0x01 || lo || hi)`, where `lo` is the smaller of its
//!   two children compared byte by byte, so a proof needs no left-or-right.
//! - At a level with an odd number of nodes, the last one moves up as it is.
//!
//! The prefixes keep a node from passing as a leaf. docs/snapshot.md is the
//! spec, and docs/snapshot-vectors.json has vectors for other languages.

use solana_sha256_hasher::hashv;

pub type Hash = [u8; 32];

/// The tree as this module builds it: the prefixes and sorted pairs above.
/// A posted root names its scheme, so a later scheme can't be misread as
/// this one.
pub const MERKLE_SCHEME_V1: u8 = 1;

/// The longest proof `verify` takes: a tree of 2^32 leaves.
pub const MAX_PROOF_LEN: usize = 32;

const LEAF_PREFIX: u8 = 0x00;
const NODE_PREFIX: u8 = 0x01;

/// The leaf for a snapshot's bytes.
pub fn hash_leaf(snapshot: &[u8]) -> Hash {
    hashv(&[&[LEAF_PREFIX], snapshot]).to_bytes()
}

/// The node above two children, in either order.
pub fn hash_pair(a: &Hash, b: &Hash) -> Hash {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    hashv(&[&[NODE_PREFIX], lo, hi]).to_bytes()
}

/// Whether `proof` shows that `snapshot` is a leaf of the tree with `root`.
///
/// ```ignore
/// if !merkle::verify(&posted.root, &snapshot, &proof) { /* refuse */ }
/// ```
pub fn verify(root: &Hash, snapshot: &[u8], proof: &[Hash]) -> bool {
    if proof.len() > MAX_PROOF_LEN {
        return false;
    }
    let mut at = hash_leaf(snapshot);
    for sibling in proof {
        at = hash_pair(&at, sibling);
    }
    at == *root
}

/// A whole tree, built off chain by a signer, and by keepers to prove the
/// snapshots a program needs.
pub struct Tree {
    /// Every level from the leaves up; the last holds only the root.
    levels: Vec<Vec<Hash>>,
}

impl Tree {
    /// The tree over `snapshots` in the order given, or None if there are
    /// none. A signer puts each token in at most once.
    ///
    /// ```ignore
    /// let tree = Tree::build(&snapshots).expect("at least one token");
    /// ```
    pub fn build(snapshots: &[Vec<u8>]) -> Option<Self> {
        if snapshots.is_empty() {
            return None;
        }
        let mut levels = vec![snapshots.iter().map(|s| hash_leaf(s)).collect::<Vec<_>>()];
        while levels[levels.len() - 1].len() > 1 {
            let below = &levels[levels.len() - 1];
            let above = below
                .chunks(2)
                .map(|pair| match pair {
                    [a, b] => hash_pair(a, b),
                    [last] => *last,
                    _ => unreachable!("chunks(2) yields one or two"),
                })
                .collect();
            levels.push(above);
        }
        Some(Self { levels })
    }

    pub fn root(&self) -> Hash {
        self.levels[self.levels.len() - 1][0]
    }

    pub fn leaf_count(&self) -> usize {
        self.levels[0].len()
    }

    /// The proof for the leaf at `index`: its sibling at each level from the
    /// bottom, skipping a level where it moved up alone.
    ///
    /// Panics if `index` isn't a leaf's.
    pub fn prove(&self, index: usize) -> Vec<Hash> {
        assert!(index < self.leaf_count(), "no leaf at {index}");
        let mut proof = Vec::new();
        let mut at = index;
        for level in &self.levels[..self.levels.len() - 1] {
            if let Some(sibling) = level.get(at ^ 1) {
                proof.push(*sibling);
            }
            at /= 2;
        }
        proof
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Snapshot;

    fn snapshots(n: u8) -> Vec<Vec<u8>> {
        (1..=n)
            .map(|i| {
                Snapshot {
                    version: 1,
                    mint: [i; 32],
                    ..Snapshot::default()
                }
                .to_bytes()
            })
            .collect()
    }

    #[test]
    fn test_every_leaf_proves_for_every_size() {
        for n in 1..=17 {
            let leaves = snapshots(n);
            let tree = Tree::build(&leaves).unwrap();
            for (i, leaf) in leaves.iter().enumerate() {
                assert!(verify(&tree.root(), leaf, &tree.prove(i)), "n={n} i={i}");
            }
        }
    }

    #[test]
    fn test_one_leaf_is_its_own_root() {
        let leaves = snapshots(1);
        let tree = Tree::build(&leaves).unwrap();
        assert_eq!(tree.root(), hash_leaf(&leaves[0]));
        assert!(tree.prove(0).is_empty());
    }

    #[test]
    fn test_no_snapshots_build_no_tree() {
        assert!(Tree::build(&[]).is_none());
    }

    #[test]
    fn test_a_changed_snapshot_fails() {
        let leaves = snapshots(5);
        let tree = Tree::build(&leaves).unwrap();
        let mut forged = Snapshot::parse(&leaves[2]).unwrap();
        forged.burn_debt_overdue = true;
        assert!(!verify(&tree.root(), &forged.to_bytes(), &tree.prove(2)));
    }

    #[test]
    fn test_another_leafs_proof_fails() {
        let leaves = snapshots(5);
        let tree = Tree::build(&leaves).unwrap();
        assert!(!verify(&tree.root(), &leaves[2], &tree.prove(3)));
    }

    /// A node's two children, concatenated, are 64 bytes, not a snapshot,
    /// and the leaf prefix keeps them from hashing to the node anyway.
    #[test]
    fn test_a_node_cannot_pass_as_a_leaf() {
        let leaves = snapshots(4);
        let tree = Tree::build(&leaves).unwrap();
        let (a, b) = (hash_leaf(&leaves[0]), hash_leaf(&leaves[1]));
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let node_preimage = [lo, hi].concat();
        // Above leaf 0's node, its proof is the node's sibling.
        assert!(!verify(&tree.root(), &node_preimage, &tree.prove(0)[1..]));
    }

    #[test]
    fn test_a_proof_too_long_fails() {
        let leaves = snapshots(1);
        let root = hash_leaf(&leaves[0]);
        assert!(!verify(&root, &leaves[0], &[[0; 32]; MAX_PROOF_LEN + 1]));
    }
}
