use anchor_lang::prelude::*;

#[event]
pub struct RootPosted {
    pub signer: Pubkey,
    pub root: [u8; 32],
    pub scheme: u8,
    pub as_of: i64,
    pub leaf_count: u32,
}
