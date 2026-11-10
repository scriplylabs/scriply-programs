use anchor_lang::prelude::*;

pub const POOL_SEED: &[u8] = b"pool";
pub const AUTHORITY_SEED: &[u8] = b"authority";
pub const SHARES_SEED: &[u8] = b"shares";
pub const LOAN_SEED: &[u8] = b"loan";
pub const WITHDRAWAL_SEED: &[u8] = b"withdrawal";
pub const GENERATION_SEED: &[u8] = b"generation";
pub const EXPOSURE_SEED: &[u8] = b"exposure";

/// The layout version this program writes in every account it owns. A
/// later program reading an account checks it before trusting the layout.
pub const ACCOUNT_VERSION: u8 = 1;

/// Basis points in one whole.
pub const BPS: u64 = 10_000;

/// Interest is simple and counts a 365-day year: 500 USDC at 12% for 90 days
/// owes 14.79 USDC of interest.
pub const SECONDS_PER_YEAR: u64 = 31_536_000;

/// A snapshot's day counts are whole days of this many seconds.
pub const SECONDS_PER_DAY: u64 = 86_400;

pub const MAX_SIGNERS: usize = 8;
pub const MAX_MATURITIES: usize = 8;

/// Loans are grouped by the epoch they mature in, one bucket per maturity,
/// reused round-robin. A pool's longest maturity plus its grace must fit in
/// fewer epochs than this, so two live maturities never share a bucket.
pub const BUCKETS: usize = 64;

/// Shares have this many more decimals than the quote token, and the share
/// price counts this many virtual shares and one virtual base unit of the
/// quote token: the usual guard that stops a first deposit setting a share
/// price that rounds later deposits to nothing.
pub const SHARE_DECIMALS_OFFSET: u8 = 3;
pub const VIRTUAL_SHARES: u64 = 1_000;
pub const VIRTUAL_ASSETS: u64 = 1;

/// The Solana Attestation Service, the same on mainnet and devnet.
pub const SAS_PROGRAM_ID: Pubkey = pubkey!("22zoJMtdu4tQc2PzL74ZUT7FrwgB1Udec8DdW4yw4BdG");

/// A SAS attestation account's discriminator.
pub const SAS_ATTESTATION_DISCRIMINATOR: u8 = 2;
