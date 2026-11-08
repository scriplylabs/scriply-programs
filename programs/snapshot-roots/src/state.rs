use anchor_lang::prelude::*;

use crate::error::RootsError;
use token_snapshot::merkle::MERKLE_SCHEME_V1;

/// The layout version this program writes in a roots account.
pub const ACCOUNT_VERSION: u8 = 1;

pub const ROOTS_SEED: &[u8] = b"roots";

/// How many of a signer's latest roots its account keeps. A keeper proving
/// a batch of snapshots against one root isn't broken by a newer post, as
/// long as fewer than this many arrive before it finishes.
pub const ROOTS_KEPT: usize = 8;

/// How far into the future a root's `as_of` may be, for clock drift.
pub const MAX_CLOCK_SKEW: i64 = 60;

/// One posted root.
#[derive(
    AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, Default, PartialEq, Eq, InitSpace,
)]
pub struct PostedRoot {
    pub root: [u8; 32],
    /// How the tree was built: `token_snapshot::merkle::MERKLE_SCHEME_V1`.
    pub scheme: u8,
    /// When every snapshot under it was true, unix seconds.
    pub as_of: i64,
    pub leaf_count: u32,
}

/// A signer's latest roots, at `[ROOTS_SEED, signer]`.
#[account]
#[derive(InitSpace)]
pub struct SnapshotRoots {
    pub version: u8,
    pub signer: Pubkey,
    pub bump: u8,
    /// Roots ever posted; the latest is at `(posted - 1) % ROOTS_KEPT`.
    pub posted: u64,
    pub roots: [PostedRoot; ROOTS_KEPT],
}

impl SnapshotRoots {
    /// The latest root, or None before the first post.
    pub fn latest(&self) -> Option<&PostedRoot> {
        match self.posted {
            0 => None,
            n => Some(&self.roots[((n - 1) % ROOTS_KEPT as u64) as usize]),
        }
    }

    /// The newest kept root equal to `root`, or None if it was never posted
    /// or has rolled out. A program verifying a proof against a root the
    /// caller names looks it up here, and decides from its `as_of` whether
    /// it is fresh enough. The same root is posted again when no token's
    /// facts changed, and the newest post is when they were last true.
    pub fn find(&self, root: &[u8; 32]) -> Option<&PostedRoot> {
        let kept = self.posted.min(ROOTS_KEPT as u64);
        (0..kept)
            .map(|back| &self.roots[((self.posted - 1 - back) % ROOTS_KEPT as u64) as usize])
            .find(|r| r.root == *root)
    }

    /// Records `posted` as the latest root, if it is newer than the latest
    /// and no later than `now` allows.
    pub fn record(&mut self, posted: PostedRoot, now: i64) -> Result<()> {
        require!(posted.leaf_count > 0, RootsError::EmptyTree);
        require!(
            posted.scheme == MERKLE_SCHEME_V1,
            RootsError::UnsupportedScheme
        );
        require!(
            posted.as_of <= now.saturating_add(MAX_CLOCK_SKEW),
            RootsError::RootFromFuture
        );
        if let Some(latest) = self.latest() {
            require!(posted.as_of > latest.as_of, RootsError::RootNotNewer);
        }
        self.roots[(self.posted % ROOTS_KEPT as u64) as usize] = posted;
        self.posted += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> SnapshotRoots {
        SnapshotRoots {
            version: ACCOUNT_VERSION,
            signer: Pubkey::default(),
            bump: 0,
            posted: 0,
            roots: [PostedRoot::default(); ROOTS_KEPT],
        }
    }

    fn root(i: u8) -> PostedRoot {
        PostedRoot {
            root: [i; 32],
            scheme: MERKLE_SCHEME_V1,
            as_of: 1_000 + i as i64,
            leaf_count: 1,
        }
    }

    #[test]
    fn test_latest_follows_posts() {
        let mut r = empty();
        assert!(r.latest().is_none());
        r.record(root(1), 2_000).unwrap();
        r.record(root(2), 2_000).unwrap();
        assert_eq!(r.latest(), Some(&root(2)));
    }

    #[test]
    fn test_old_roots_roll_out() {
        let mut r = empty();
        for i in 1..=(ROOTS_KEPT as u8 + 1) {
            r.record(root(i), 2_000).unwrap();
        }
        assert!(r.find(&[1; 32]).is_none());
        assert_eq!(r.find(&[2; 32]), Some(&root(2)));
        assert_eq!(r.latest(), Some(&root(ROOTS_KEPT as u8 + 1)));
    }

    #[test]
    fn test_a_root_posted_again_is_found_at_its_newest() {
        let mut r = empty();
        r.record(root(1), 2_000).unwrap();
        let mut again = root(1);
        again.as_of = 1_500;
        r.record(again, 2_000).unwrap();
        assert_eq!(r.find(&[1; 32]).unwrap().as_of, 1_500);
    }

    #[test]
    fn test_an_unposted_zero_root_is_not_found() {
        let r = empty();
        assert!(r.find(&[0; 32]).is_none());
    }

    #[test]
    fn test_a_root_not_newer_is_refused() {
        let mut r = empty();
        r.record(root(2), 2_000).unwrap();
        assert!(r.record(root(2), 2_000).is_err());
        assert!(r.record(root(1), 2_000).is_err());
    }

    #[test]
    fn test_a_root_from_the_future_is_refused() {
        let mut r = empty();
        assert!(r.record(root(1), 1_001 - MAX_CLOCK_SKEW - 1).is_err());
        assert!(r.record(root(1), 1_001 - MAX_CLOCK_SKEW).is_ok());
    }

    #[test]
    fn test_an_unknown_scheme_is_refused() {
        let mut r = empty();
        let mut p = root(1);
        p.scheme = 2;
        assert!(r.record(p, 2_000).is_err());
    }

    #[test]
    fn test_an_empty_tree_is_refused() {
        let mut r = empty();
        let mut p = root(1);
        p.leaf_count = 0;
        assert!(r.record(p, 2_000).is_err());
    }
}
