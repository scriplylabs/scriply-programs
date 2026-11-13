//! Finding the supplier's signature over the claim message in the
//! transaction: the ed25519 program checked it in an earlier instruction,
//! and this program reads that instruction to know what was checked.

use anchor_lang::{
    compat::solana_instructions_sysvar::{load_current_index_checked, load_instruction_at_checked},
    prelude::*,
};

use crate::{constants::*, error::SupplierPoolError};

/// The ed25519 instruction's header for one signature: the count and a
/// padding byte, then seven u16 offsets.
const HEADER_LEN: usize = 2 + 14;

/// The claim message for the listing at `listing`.
pub fn claim_message(listing: &Pubkey) -> Vec<u8> {
    let mut msg = CLAIM_MESSAGE_PREFIX.to_vec();
    msg.extend_from_slice(listing.to_string().as_bytes());
    msg
}

/// Returns Ok when an instruction before this one in the transaction is the
/// ed25519 program's check of `supplier`'s signature over the claim message
/// for `listing`, carried in that instruction's own data, and
/// ClaimNotSigned otherwise.
///
/// ```ignore
/// require_claim_signature(&instructions, &supplier, &listing)?;
/// ```
pub fn require_claim_signature(
    instructions: &AccountInfo,
    supplier: &Pubkey,
    listing: &Pubkey,
) -> Result<()> {
    let message = claim_message(listing);
    let current = load_current_index_checked(instructions)? as usize;
    for index in 0..current {
        let ix = load_instruction_at_checked(index, instructions)?;
        if ix.program_id != ED25519_PROGRAM_ID {
            continue;
        }
        if is_check_of(&ix.data, supplier, &message) {
            return Ok(());
        }
    }
    err!(SupplierPoolError::ClaimNotSigned)
}

/// Whether `data`, an ed25519 instruction's, checks exactly one signature by
/// `signer` over `message`, all three read from `data` itself.
fn is_check_of(data: &[u8], signer: &Pubkey, message: &[u8]) -> bool {
    if data.len() < HEADER_LEN || data[0] != 1 {
        return false;
    }
    let at = |i: usize| u16::from_le_bytes([data[2 + 2 * i], data[3 + 2 * i]]);
    let (sig_offset, sig_ix) = (at(0) as usize, at(1));
    let (key_offset, key_ix) = (at(2) as usize, at(3));
    let (msg_offset, msg_len, msg_ix) = (at(4) as usize, at(5) as usize, at(6));
    // u16::MAX names the ed25519 instruction itself: an offset into another
    // instruction would let the check read bytes this program never sees.
    if sig_ix != u16::MAX || key_ix != u16::MAX || msg_ix != u16::MAX {
        return false;
    }
    let slice = |offset: usize, len: usize| data.get(offset..offset.checked_add(len)?);
    if slice(sig_offset, 64).is_none() {
        return false;
    }
    slice(key_offset, 32) == Some(signer.as_ref()) && slice(msg_offset, msg_len) == Some(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ed25519 instruction's data as the runtime's helper lays it out:
    /// header, public key, signature, message.
    fn ed25519_data(key: &Pubkey, message: &[u8], ix_index: u16) -> Vec<u8> {
        let key_offset = HEADER_LEN as u16;
        let sig_offset = key_offset + 32;
        let msg_offset = sig_offset + 64;
        let mut d = vec![1u8, 0];
        for v in [
            sig_offset,
            ix_index,
            key_offset,
            ix_index,
            msg_offset,
            message.len() as u16,
            ix_index,
        ] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        d.extend_from_slice(key.as_ref());
        d.extend_from_slice(&[7u8; 64]);
        d.extend_from_slice(message);
        d
    }

    #[test]
    fn test_a_check_of_the_supplier_over_the_message_passes() {
        let key = Pubkey::new_unique();
        let msg = claim_message(&Pubkey::new_unique());
        assert!(is_check_of(&ed25519_data(&key, &msg, u16::MAX), &key, &msg));
    }

    #[test]
    fn test_another_signer_or_message_fails() {
        let key = Pubkey::new_unique();
        let msg = claim_message(&Pubkey::new_unique());
        let data = ed25519_data(&key, &msg, u16::MAX);
        assert!(!is_check_of(&data, &Pubkey::new_unique(), &msg));
        assert!(!is_check_of(
            &data,
            &key,
            &claim_message(&Pubkey::new_unique())
        ));
    }

    #[test]
    fn test_offsets_into_another_instruction_fail() {
        let key = Pubkey::new_unique();
        let msg = claim_message(&Pubkey::new_unique());
        assert!(!is_check_of(&ed25519_data(&key, &msg, 0), &key, &msg));
    }

    #[test]
    fn test_two_signatures_fail() {
        let key = Pubkey::new_unique();
        let msg = claim_message(&Pubkey::new_unique());
        let mut data = ed25519_data(&key, &msg, u16::MAX);
        data[0] = 2;
        assert!(!is_check_of(&data, &key, &msg));
    }
}
