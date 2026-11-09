use anchor_lang::prelude::*;

#[error_code]
pub enum RootsError {
    #[msg("A tree has at least one leaf")]
    EmptyTree,
    #[msg("This program doesn't know that tree scheme")]
    UnsupportedScheme,
    #[msg("A root must be newer than the signer's latest")]
    RootNotNewer,
    #[msg("The root is dated in the future")]
    RootFromFuture,
}
