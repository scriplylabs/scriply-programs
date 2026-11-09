//! The check that a wallet holds a Solana Attestation Service attestation
//! of a pool's credential and schema. An attestation's nonce is the wallet
//! it attests, so its address is derived from the wallet.

use anchor_lang::prelude::*;

use crate::{constants::*, error::PoolError};

/// Fails unless `attestation` is the SAS attestation of `credential` and
/// `schema` for `wallet`, and has not expired at `now`. An expiry of 0
/// never expires.
pub fn require_attestation(
    attestation: &AccountInfo,
    credential: &Pubkey,
    schema: &Pubkey,
    wallet: &Pubkey,
    now: i64,
) -> Result<()> {
    require_keys_eq!(
        *attestation.owner,
        SAS_PROGRAM_ID,
        PoolError::InvalidAttestation
    );
    let (expected, _) = Pubkey::find_program_address(
        &[
            b"attestation",
            credential.as_ref(),
            schema.as_ref(),
            wallet.as_ref(),
        ],
        &SAS_PROGRAM_ID,
    );
    require_keys_eq!(attestation.key(), expected, PoolError::InvalidAttestation);

    // Layout: discriminator u8, nonce, credential, schema, data (u32 length
    // and bytes), signer, expiry i64, ...
    let data = attestation.try_borrow_data()?;
    let invalid = || error!(PoolError::InvalidAttestation);
    let key_at = |at: usize| data.get(at..at + 32).ok_or_else(invalid);
    require!(
        data.first() == Some(&SAS_ATTESTATION_DISCRIMINATOR),
        PoolError::InvalidAttestation
    );
    require!(key_at(1)? == wallet.as_ref(), PoolError::InvalidAttestation);
    require!(
        key_at(33)? == credential.as_ref(),
        PoolError::InvalidAttestation
    );
    require!(
        key_at(65)? == schema.as_ref(),
        PoolError::InvalidAttestation
    );
    let len_bytes = data.get(97..101).ok_or_else(invalid)?;
    let data_len = u32::from_le_bytes(len_bytes.try_into().map_err(|_| invalid())?) as usize;
    let expiry_at = 101usize
        .checked_add(data_len)
        .and_then(|at| at.checked_add(32))
        .ok_or_else(invalid)?;
    let expiry_bytes = data.get(expiry_at..expiry_at + 8).ok_or_else(invalid)?;
    let expiry = i64::from_le_bytes(expiry_bytes.try_into().map_err(|_| invalid())?);
    require!(expiry == 0 || expiry > now, PoolError::InvalidAttestation);
    Ok(())
}
