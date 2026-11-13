use anchor_lang::prelude::*;

pub const POOL_SEED: &[u8] = b"pool";
pub const SHARES_SEED: &[u8] = b"shares";
pub const ANCHOR_SEED: &[u8] = b"anchor";
pub const LISTING_SEED: &[u8] = b"listing";

/// The layout version this program writes in every account it owns. A
/// later program reading an account checks it before trusting the layout.
pub const ACCOUNT_VERSION: u8 = 1;

/// Basis points in one whole.
pub const BPS: u64 = 10_000;

pub const SECONDS_PER_DAY: i64 = 86_400;

pub const MAX_SIGNERS: usize = 4;
pub const MAX_ANCHORS: usize = 8;

/// A bill leaf's amount is in millionths of its currency.
pub const BILL_AMOUNT_DECIMALS: u8 = 6;

/// The message a supplier's wallet signs to claim, followed by the
/// listing's address in base58. Scriply's services build the same text.
pub const CLAIM_MESSAGE_PREFIX: &[u8] = b"scriply-supplier-finance-claim:";

/// What a claim refunds its fee payer for the transaction's network fee:
/// two signatures' worth, the fee payer's and the ed25519 check's.
pub const CLAIM_NETWORK_FEE_LAMPORTS: u64 = 10_000;

/// A classic SPL token account's size, whose rent a claim refunds when it
/// creates the supplier's account.
pub const TOKEN_ACCOUNT_LEN: usize = 165;

/// Shares have this many more decimals than the token, and the share price
/// counts this many virtual shares and one virtual base unit of the token:
/// the guard that stops a first deposit setting a share price that rounds
/// later deposits to nothing.
pub const SHARE_DECIMALS_OFFSET: u8 = 3;
pub const VIRTUAL_SHARES: u64 = 1_000;
pub const VIRTUAL_ASSETS: u64 = 1;

/// The native program that checks ed25519 signatures in a transaction.
pub const ED25519_PROGRAM_ID: Pubkey = pubkey!("Ed25519SigVerify111111111111111111111111111");
