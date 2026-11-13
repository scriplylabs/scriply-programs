use anchor_lang::prelude::*;

#[event]
pub struct PoolCreated {
    pub pool: Pubkey,
    pub creator: Pubkey,
    pub mint: Pubkey,
}

#[event]
pub struct Deposited {
    pub pool: Pubkey,
    pub depositor: Pubkey,
    pub amount: u64,
    pub shares: u64,
}

#[event]
pub struct Withdrawn {
    pub pool: Pubkey,
    pub owner: Pubkey,
    pub shares: u64,
    pub amount: u64,
}

#[event]
pub struct BillListed {
    pub pool: Pubkey,
    pub anchor: Pubkey,
    pub listing: Pubkey,
    pub commitment: [u8; 32],
    pub amount: u64,
    pub due_date: i64,
}

#[event]
pub struct ListingCancelled {
    pub pool: Pubkey,
    pub listing: Pubkey,
}

#[event]
pub struct BillClaimed {
    pub pool: Pubkey,
    pub listing: Pubkey,
    pub supplier: Pubkey,
    pub paid: u64,
    pub fee: u64,
}

#[event]
pub struct ListingRepaid {
    pub pool: Pubkey,
    pub listing: Pubkey,
    pub amount: u64,
}
