//! Creates a receivables pool from its config file.
//!
//! ```text
//! cargo run -p create-pool -- \
//!     --config pools/devnet.toml \
//!     --url https://api.devnet.solana.com \
//!     --quote-mint 4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU \
//!     --keypair ~/.config/solana/id.json \
//!     --dry-run
//! ```
//!
//! The keypair pays and is recorded as the pool's creator, which gives it
//! no authority over the pool. `--dry-run` prints the parameters and the
//! pool's addresses without sending anything.

use std::{path::PathBuf, process::ExitCode, str::FromStr};

use anchor_lang::{
    prelude::Pubkey,
    solana_program::{instruction::Instruction, system_program},
    InstructionData, ToAccountMetas,
};
use anchor_spl::{associated_token::get_associated_token_address_with_program_id as ata, token};
use clap::Parser;
use receivables_pool::{state::PoolParams, AUTHORITY_SEED, POOL_SEED, SHARES_SEED};
use solana_commitment_config::CommitmentConfig;
use solana_keypair::read_keypair_file;
use solana_rpc_client::rpc_client::RpcClient;
use solana_signer::Signer;
use solana_transaction::Transaction;

#[derive(Parser)]
#[command(about = "Creates a receivables pool from its config file")]
struct Args {
    /// The pool's config file.
    #[arg(long)]
    config: PathBuf,
    /// The cluster's RPC URL. There is no default, so a pool is never
    /// created on a cluster nobody named.
    #[arg(long)]
    url: String,
    /// The token the pool lends and takes deposits in (USDC on the cluster).
    #[arg(long)]
    quote_mint: String,
    /// Pays for the pool's accounts.
    #[arg(long)]
    keypair: PathBuf,
    /// Tells apart pools one keypair creates.
    #[arg(long, default_value_t = 0)]
    seed: u64,
    /// Print what would be created, and send nothing.
    #[arg(long)]
    dry_run: bool,
}

/// The accounts a pool is created with.
struct PoolAddresses {
    pool: Pubkey,
    authority: Pubkey,
    share_mint: Pubkey,
    quote_vault: Pubkey,
    share_escrow: Pubkey,
}

fn derive_pool_addresses(creator: &Pubkey, seed: u64, quote_mint: &Pubkey) -> PoolAddresses {
    let program = receivables_pool::id();
    let pda = |seeds: &[&[u8]]| Pubkey::find_program_address(seeds, &program).0;
    let pool = pda(&[POOL_SEED, creator.as_ref(), &seed.to_le_bytes()]);
    let authority = pda(&[AUTHORITY_SEED, pool.as_ref()]);
    let share_mint = pda(&[SHARES_SEED, pool.as_ref()]);
    PoolAddresses {
        pool,
        authority,
        share_mint,
        quote_vault: ata(&authority, quote_mint, &token::ID),
        share_escrow: ata(&authority, &share_mint, &token::ID),
    }
}

fn build_create_pool_ix(
    creator: &Pubkey,
    seed: u64,
    quote_mint: &Pubkey,
    a: &PoolAddresses,
    params: PoolParams,
) -> Instruction {
    Instruction::new_with_bytes(
        receivables_pool::id(),
        &receivables_pool::instruction::CreatePool { seed, params }.data(),
        receivables_pool::accounts::CreatePool {
            creator: *creator,
            pool: a.pool,
            pool_authority: a.authority,
            quote_mint: *quote_mint,
            share_mint: a.share_mint,
            quote_vault: a.quote_vault,
            share_escrow: a.share_escrow,
            quote_token_program: token::ID,
            share_token_program: token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn run(args: Args) -> Result<(), String> {
    let params = pool_config::load(&args.config).map_err(|e| e.to_string())?;
    let quote_mint = Pubkey::from_str(&args.quote_mint)
        .map_err(|_| format!("{:?} is not a public key", args.quote_mint))?;
    let payer = read_keypair_file(&args.keypair)
        .map_err(|e| format!("reading {}: {e}", args.keypair.display()))?;
    let creator = payer.pubkey();
    let a = derive_pool_addresses(&creator, args.seed, &quote_mint);

    println!("{params:#?}");
    println!("pool:         {}", a.pool);
    println!("share mint:   {}", a.share_mint);
    println!("quote vault:  {}", a.quote_vault);
    println!("creator:      {creator}");
    if args.dry_run {
        println!("dry run: nothing sent");
        return Ok(());
    }

    let ix = build_create_pool_ix(&creator, args.seed, &quote_mint, &a, params);
    let rpc = RpcClient::new_with_commitment(args.url, CommitmentConfig::confirmed());
    let blockhash = rpc.get_latest_blockhash().map_err(|e| e.to_string())?;
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&creator), &[&payer], blockhash);
    let signature = rpc
        .send_and_confirm_transaction(&tx)
        .map_err(|e| e.to_string())?;
    println!("created: {signature}");
    Ok(())
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
