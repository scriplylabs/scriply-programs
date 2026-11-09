use anchor_lang::prelude::*;

use crate::{events::RootPosted, state::*};

#[derive(Accounts)]
pub struct PostRoot<'info> {
    /// The key whose roots these are. Programs trust it by this key, so
    /// only it can post to its account.
    pub signer: Signer<'info>,
    /// Pays for the account on the first post, and the fees. May be the
    /// signer.
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + SnapshotRoots::INIT_SPACE,
        seeds = [ROOTS_SEED, signer.key().as_ref()],
        bump
    )]
    pub roots: Account<'info, SnapshotRoots>,
    pub system_program: Program<'info, System>,
}

pub fn handle_post_root(ctx: Context<PostRoot>, posted: PostedRoot) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let signer = ctx.accounts.signer.key();
    let roots = &mut ctx.accounts.roots;
    if roots.posted == 0 {
        roots.version = ACCOUNT_VERSION;
        roots.signer = signer;
        roots.bump = ctx.bumps.roots;
    }
    roots.record(posted, now)?;
    emit!(RootPosted {
        signer,
        root: posted.root,
        scheme: posted.scheme,
        as_of: posted.as_of,
        leaf_count: posted.leaf_count,
    });
    Ok(())
}
