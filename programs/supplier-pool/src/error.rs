use anchor_lang::prelude::*;

#[error_code]
pub enum SupplierPoolError {
    // Creating a pool
    #[msg("A pool needs between 1 and 4 signers")]
    InvalidSigners,
    #[msg("A pool names at most 8 anchors")]
    TooManyAnchors,
    #[msg("A fee must be at most 10000 bps, and a per-day fee's base at most its maximum")]
    InvalidFee,
    #[msg("The most an anchor may owe must be above zero")]
    InvalidCap,
    #[msg("The oldest root must be at least one second, and the longest term at least a day")]
    InvalidAge,
    #[msg("A token's decimals must be at most 18")]
    InvalidDecimals,
    #[msg("A band rule names a band its leaf doesn't have")]
    InvalidBand,

    // A bill's facts
    #[msg("The roots account is not one of the pool's signers'")]
    UnknownSigner,
    #[msg("That root is not the signer's latest")]
    RootNotLatest,
    #[msg("The signer's latest root is older than the pool allows")]
    RootTooOld,
    #[msg("The proof doesn't show the leaf under the root")]
    InvalidProof,
    #[msg("The leaf is not a bill leaf this program reads")]
    NotABillLeaf,
    #[msg("The bill leaf is of another network")]
    WrongNetwork,
    #[msg("The bill leaf is in another currency than the pool's")]
    WrongCurrency,
    #[msg("The bill leaf is for another commitment")]
    WrongCommitment,
    #[msg("The bill's amount isn't a whole number of the token's base units")]
    AmountNotRepresentable,

    // The pool's rules
    #[msg("The pool doesn't take this anchor")]
    AnchorNotTaken,
    #[msg("The bill's documents verified are not the ones the pool requires")]
    DocumentsNotVerified,
    #[msg("The anchor has paid this vendor for less time than the pool requires")]
    VendorTenureTooShort,
    #[msg("The anchor has paid this vendor less, against this bill, than the pool requires")]
    VendorVolumeTooLow,
    #[msg("The bill is repaid later than the pool's longest term")]
    TermTooLong,
    #[msg("The bill's repayment date has passed")]
    RepaymentDatePassed,
    #[msg("The anchor's open listings would pass the most the pool lets one anchor owe")]
    OverAnchorCap,

    // A claim
    #[msg("The listing is already claimed")]
    AlreadyClaimed,
    #[msg("The listing hasn't been claimed")]
    NotClaimed,
    #[msg(
        "No ed25519 check of the supplier's signature over the claim message is in the transaction"
    )]
    ClaimNotSigned,
    #[msg("The supplier's address and the salt aren't the listing's commitment")]
    WrongSupplier,
    #[msg("The supplier's token account is not its associated token account")]
    WrongSupplierAccount,
    #[msg("The pool holds too little idle to pay this claim")]
    PoolShort,

    // A write-off
    #[msg("The listing isn't past its due date by the pool's days yet")]
    NotYetOverdue,
    #[msg("The listing is already written off")]
    AlreadyWrittenOff,

    // Deposits and withdrawals
    #[msg("The amount must be above zero")]
    ZeroAmount,
    #[msg("The deposit is too small for one share")]
    DepositTooSmall,
    #[msg("The pool holds too little idle to pay this withdrawal")]
    NotEnoughIdle,

    #[msg("Arithmetic overflow")]
    MathOverflow,

    // The anchor's business leaf. After every other error, since the
    // services map refusals by their codes, which follow this order.
    #[msg("The leaf is not a business leaf this program reads")]
    NotABusinessLeaf,
    #[msg("The business leaf is of another wallet or network than the listing's")]
    WrongBusiness,
    #[msg("The business leaf carries no record with supplier pools: its version is older than 2")]
    NoSupplierPoolRecord,
    #[msg("The anchor has more listings overdue and unpaid than the pool takes")]
    TooManyOverdueUnpaid,
    #[msg("A listing of the anchor's is more days overdue than the pool takes")]
    TooManyDaysOverdue,
    #[msg("The anchor has more listings written off and unrepaid than the pool takes")]
    TooManyWrittenOffUnrepaid,
    #[msg("The anchor had more listings written off in the past year than the pool takes")]
    TooManyWrittenOffInYear,
    #[msg("The anchor repaid more listings late in the past year than the pool takes")]
    TooManyRepaidLateInYear,
}
