use anchor_lang::{
    prelude::*,
    system_program::{self, Allocate, Assign, CreateAccount, Transfer},
};
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::{constants::*, error::PoolError, events::EpochSettled, state::*, tokens};

#[derive(Accounts)]
pub struct SettleEpoch<'info> {
    /// Anyone may settle; they pay the rent of a generation record when one
    /// is written.
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(mut, has_one = share_mint)]
    pub pool: AccountLoader<'info, Pool>,
    /// CHECK: the PDA that owns the share escrow and mints shares.
    #[account(seeds = [AUTHORITY_SEED, pool.key().as_ref()], bump)]
    pub pool_authority: UncheckedAccount<'info>,
    #[account(mut)]
    pub share_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        associated_token::mint = share_mint,
        associated_token::authority = pool_authority,
        associated_token::token_program = share_token_program,
    )]
    pub share_escrow: Box<InterfaceAccount<'info, TokenAccount>>,
    /// CHECK: the record of the pool's current withdrawal generation, at
    /// [GENERATION_SEED, pool, generation]; written only if this settlement
    /// ends the generation, and checked in the handler.
    #[account(mut)]
    pub generation_record: UncheckedAccount<'info>,
    #[account(address = anchor_spl::token::ID)]
    pub share_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

/// Moves the current epoch's settlement on as far as it can go, and
/// otherwise does nothing, so clients can put it before any instruction:
///
/// 1. When a new epoch has started, impairs the loans past maturity and
///    grace and begins marking: every performing loan must then be marked from
///    its token's facts (`mark_loans`, or `mark_stale` once the window
///    closes).
/// 2. Once every performing loan is marked, pays the withdrawals asked for
///    at the NAV per share then, setting their USDC aside. NAV is worked out
///    when they're paid, not at the epoch's start, because repayments and
///    syncs made during marking are already in the pool's state.
///
/// A pool with no open loans does both in one call.
pub fn handle_settle_epoch(ctx: Context<SettleEpoch>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let pool_key = ctx.accounts.pool.key();
    let mut pool = ctx.accounts.pool.load_mut()?;
    let epoch = pool.current_epoch(now);
    if pool.settled_epoch != epoch {
        pool.open_epoch(epoch)?;
    }
    if pool.withdrawals_epoch == epoch || !pool.is_marked() {
        return Ok(());
    }
    pool.withdrawals_epoch = epoch;
    let settlement = pool.settle_withdrawals(ctx.accounts.share_mint.supply, now)?;
    let authority_bump = pool.authority_bump;
    drop(pool);

    let a = &ctx.accounts;
    tokens::burn(
        &a.share_token_program,
        &a.share_mint,
        a.share_escrow.to_account_info(),
        a.pool_authority.to_account_info(),
        &[&[AUTHORITY_SEED, pool_key.as_ref(), &[authority_bump]]],
        settlement.shares_burned,
    )?;
    if let Some((generation, final_paid_per_norm)) = settlement.ended_generation {
        write_generation_record(
            &a.payer,
            &a.generation_record,
            &a.system_program,
            WithdrawalGeneration {
                version: ACCOUNT_VERSION,
                pool: pool_key,
                generation,
                final_paid_per_norm,
            },
        )?;
    }

    emit!(EpochSettled {
        pool: pool_key,
        epoch,
        withdrawals_paid: settlement.paid,
        shares_burned: settlement.shares_burned,
    });
    Ok(())
}

/// Creates the generation's record at its PDA. Anyone can send lamports to
/// an address before it exists, which would make a plain create fail and
/// stop the pool settling, so an address already funded is topped up,
/// allocated and assigned instead.
fn write_generation_record<'info>(
    payer: &Signer<'info>,
    record: &UncheckedAccount<'info>,
    system: &Program<'info, System>,
    data: WithdrawalGeneration,
) -> Result<()> {
    let generation = data.generation.to_le_bytes();
    let (expected, bump) = Pubkey::find_program_address(
        &[GENERATION_SEED, data.pool.as_ref(), &generation],
        &crate::ID,
    );
    require_keys_eq!(record.key(), expected, PoolError::WrongGenerationRecord);
    let seeds: &[&[u8]] = &[GENERATION_SEED, data.pool.as_ref(), &generation, &[bump]];
    let space = 8 + WithdrawalGeneration::INIT_SPACE;
    let rent = Rent::get()?.minimum_balance(space);

    if record.lamports() == 0 {
        system_program::create_account(
            CpiContext::new_with_signer(
                system.key(),
                CreateAccount {
                    from: payer.to_account_info(),
                    to: record.to_account_info(),
                },
                &[seeds],
            ),
            rent,
            space as u64,
            &crate::ID,
        )?;
    } else {
        let top_up = rent.saturating_sub(record.lamports());
        if top_up > 0 {
            system_program::transfer(
                CpiContext::new(
                    system.key(),
                    Transfer {
                        from: payer.to_account_info(),
                        to: record.to_account_info(),
                    },
                ),
                top_up,
            )?;
        }
        system_program::allocate(
            CpiContext::new_with_signer(
                system.key(),
                Allocate {
                    account_to_allocate: record.to_account_info(),
                },
                &[seeds],
            ),
            space as u64,
        )?;
        system_program::assign(
            CpiContext::new_with_signer(
                system.key(),
                Assign {
                    account_to_assign: record.to_account_info(),
                },
                &[seeds],
            ),
            &crate::ID,
        )?;
    }

    let mut buf = record.try_borrow_mut_data()?;
    data.try_serialize(&mut &mut buf[..])
}
