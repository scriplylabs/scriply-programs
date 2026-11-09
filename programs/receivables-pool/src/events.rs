use anchor_lang::prelude::*;

#[event]
pub struct PoolCreated {
    pub pool: Pubkey,
    pub creator: Pubkey,
}

#[event]
pub struct Deposited {
    pub pool: Pubkey,
    pub depositor: Pubkey,
    pub amount: u64,
    pub shares: u64,
}

#[event]
pub struct WithdrawalRequested {
    pub pool: Pubkey,
    pub owner: Pubkey,
    pub shares: u64,
}

#[event]
pub struct WithdrawalClaimed {
    pub pool: Pubkey,
    pub owner: Pubkey,
    pub amount: u64,
}

#[event]
pub struct EpochSettled {
    pub pool: Pubkey,
    pub epoch: u64,
    pub withdrawals_paid: u64,
    pub shares_burned: u64,
}

#[event]
pub struct LoanOpened {
    pub pool: Pubkey,
    pub collateral_mint: Pubkey,
    pub borrower: Pubkey,
    pub units: u64,
    pub principal: u64,
    pub rate_bps: u16,
    pub maturity_epoch: u64,
}

#[event]
pub struct LoanMarked {
    pub pool: Pubkey,
    pub loan: Pubkey,
    /// True when the token's facts wrote the loan down.
    pub is_written_down: bool,
    /// True when the token couldn't be read and the loan was marked stale.
    pub is_stale: bool,
}

#[event]
pub struct LoanPaid {
    pub pool: Pubkey,
    pub collateral_mint: Pubkey,
    pub applied: u64,
    pub excess: u64,
}

#[event]
pub struct LoanClosed {
    pub pool: Pubkey,
    pub collateral_mint: Pubkey,
}

#[event]
pub struct UnitsWithdrawn {
    pub pool: Pubkey,
    pub collateral_mint: Pubkey,
    pub borrower: Pubkey,
    pub units: u64,
}
