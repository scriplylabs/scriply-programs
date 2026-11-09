//! Token moves, skipped when the amount is zero.

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Burn, Mint, MintTo, TokenInterface, TransferChecked};

pub fn transfer<'info>(
    program: &Interface<'info, TokenInterface>,
    from: AccountInfo<'info>,
    mint: &InterfaceAccount<'info, Mint>,
    to: AccountInfo<'info>,
    authority: AccountInfo<'info>,
    signer_seeds: &[&[&[u8]]],
    amount: u64,
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    let accounts = TransferChecked {
        from,
        mint: mint.to_account_info(),
        to,
        authority,
    };
    token_interface::transfer_checked(
        CpiContext::new_with_signer(program.key(), accounts, signer_seeds),
        amount,
        mint.decimals,
    )
}

pub fn mint_to<'info>(
    program: &Interface<'info, TokenInterface>,
    mint: &InterfaceAccount<'info, Mint>,
    to: AccountInfo<'info>,
    authority: AccountInfo<'info>,
    signer_seeds: &[&[&[u8]]],
    amount: u64,
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    let accounts = MintTo {
        mint: mint.to_account_info(),
        to,
        authority,
    };
    token_interface::mint_to(
        CpiContext::new_with_signer(program.key(), accounts, signer_seeds),
        amount,
    )
}

pub fn burn<'info>(
    program: &Interface<'info, TokenInterface>,
    mint: &InterfaceAccount<'info, Mint>,
    from: AccountInfo<'info>,
    authority: AccountInfo<'info>,
    signer_seeds: &[&[&[u8]]],
    amount: u64,
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    let accounts = Burn {
        mint: mint.to_account_info(),
        from,
        authority,
    };
    token_interface::burn(
        CpiContext::new_with_signer(program.key(), accounts, signer_seeds),
        amount,
    )
}
