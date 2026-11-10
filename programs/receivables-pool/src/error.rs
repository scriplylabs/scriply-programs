use anchor_lang::prelude::*;

#[error_code]
pub enum PoolError {
    // Creating a pool
    #[msg("A pool needs between 1 and 8 signers")]
    InvalidSigners,
    #[msg("A pool needs between 1 and 8 maturities, each at least one epoch")]
    InvalidMaturities,
    #[msg("The longest maturity plus grace must be fewer than 64 epochs")]
    MaturityTooLong,
    #[msg("The advance rates must satisfy base <= max <= 10000 bps")]
    InvalidAdvance,
    #[msg("The rates must satisfy min <= base <= max")]
    InvalidRate,
    #[msg("A share in bps must be at most 10000, and the cap per token above 0")]
    InvalidShare,
    #[msg("An epoch must be at least 60 seconds")]
    InvalidEpoch,
    #[msg("The marking window must be shorter than an epoch")]
    InvalidMarkWindow,
    #[msg("The oldest root must be at least one second")]
    InvalidRootAge,
    #[msg("A token's decimals must be at most 18")]
    InvalidDecimals,
    #[msg("This program doesn't build that rate mode yet")]
    RateModeNotSupported,
    #[msg(
        "Late units are counted past 30, 60 or 90 days, and the beyond-terms cap is at least a day"
    )]
    InvalidLateRule,

    // Every instruction that changes the pool
    #[msg("A new epoch has started: settle it first")]
    EpochNotSettled,

    // A token's facts
    #[msg("The roots account is not one of the pool's signers'")]
    UnknownSigner,
    #[msg("That root is not the signer's latest")]
    RootNotLatest,
    #[msg("The root is older than the pool allows")]
    RootTooOld,
    #[msg("The root is older than the one this loan was last read from")]
    RootOlderThanLastRead,
    #[msg("The proof doesn't show the snapshot under the root")]
    InvalidProof,
    #[msg("The snapshot is shorter than its layout")]
    SnapshotTooShort,
    #[msg("The snapshot's version is not one this program reads")]
    UnsupportedSnapshotVersion,
    #[msg("The snapshot is of another network")]
    WrongNetwork,
    #[msg("The snapshot is of another token")]
    WrongMint,
    #[msg("Units were minted after the snapshot, so its facts don't cover them")]
    MintedSinceSnapshot,
    #[msg("The token has overdue burn debt")]
    BurnDebtOverdue,
    #[msg("The token's burn debt was overdue for more days in the past year than the pool allows")]
    BurnDebtHistoryOverdue,
    #[msg("None of the token's units are eligible")]
    NoEligibleUnits,
    #[msg("More of the token's units are late than the pool allows")]
    TooMuchLate,
    #[msg("The term ends before the token's invoices are expected to be collected")]
    TermTooShort,

    // Loans
    #[msg("This borrower already has an open loan against this token in this pool")]
    LoanOpen,
    #[msg("The loan is closed")]
    LoanClosed,
    #[msg("Units from this borrower's last loan are still in its vault: withdraw them first")]
    UnitsNotWithdrawn,
    #[msg("That maturity is not one of the pool's")]
    MaturityNotOffered,
    #[msg("The loan exceeds the advance the risk engine allows for the units pledged")]
    ExceedsAdvance,
    #[msg("The loan would take this token's loans past the pool's cap")]
    ExceedsTokenCap,
    #[msg("The pool has too little idle USDC for the loan")]
    InsufficientLiquidity,
    #[msg("Two live maturities would share a bucket")]
    BucketInUse,
    #[msg("The account is not an open loan of this pool")]
    NotALoan,
    #[msg("The epoch's marking window is still open")]
    MarkWindowOpen,
    #[msg("The loan was already marked this epoch")]
    AlreadyMarked,

    // Deposits and withdrawals
    #[msg("This pool requires the depositor's wallet to hold its attestation")]
    AttestationRequired,
    #[msg("The attestation is not the pool's for this wallet, or has expired")]
    InvalidAttestation,
    #[msg("The amount must be above zero")]
    ZeroAmount,
    #[msg("The deposit is too small to buy a share")]
    DepositTooSmall,
    #[msg("The generation record passed is not the one this account needs")]
    WrongGenerationRecord,

    #[msg("Arithmetic overflow")]
    MathOverflow,
}
