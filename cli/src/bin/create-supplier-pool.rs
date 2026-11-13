//! Creates a supplier pool from its config file.
//!
//! ```text
//! cargo run -p create-pool --bin create-supplier-pool -- \
//!     --config pools/supplier-devnet.toml \
//!     --url https://api.devnet.solana.com \
//!     --mint 4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU \
//!     --keypair ~/.config/solana/id.json \
//!     --dry-run
//! ```
//!
//! The keypair pays and is recorded as the pool's creator, which gives it
//! no authority over the pool; one keypair creates one pool per token.
//! `--dry-run` prints the parameters and the pool's addresses without
//! sending anything.

use std::{path::PathBuf, process::ExitCode, str::FromStr};

use anchor_lang::{
    prelude::Pubkey,
    solana_program::{instruction::Instruction, system_program},
    InstructionData, ToAccountMetas,
};
use anchor_spl::{associated_token::get_associated_token_address_with_program_id as ata, token};
use clap::Parser;
use serde::Deserialize;
use solana_commitment_config::CommitmentConfig;
use solana_keypair::read_keypair_file;
use solana_rpc_client::rpc_client::RpcClient;
use solana_signer::Signer;
use solana_transaction::Transaction;
use supplier_pool::{
    state::{BillRules, FeeSchedule, PoolParams},
    POOL_SEED, SHARES_SEED,
};

#[derive(Parser)]
#[command(about = "Creates a supplier pool from its config file")]
struct Args {
    /// The pool's config file.
    #[arg(long)]
    config: PathBuf,
    /// The cluster's RPC URL. There is no default, so a pool is never
    /// created on a cluster nobody named.
    #[arg(long)]
    url: String,
    /// The token the pool pays in (USDC on the cluster).
    #[arg(long)]
    mint: String,
    /// Pays for the pool's accounts.
    #[arg(long)]
    keypair: PathBuf,
    /// Print what would be created, and send nothing.
    #[arg(long)]
    dry_run: bool,
}

/// pools/supplier-*.toml, as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    version: u32,
    network: String,
    currency: String,
    signers: Vec<String>,
    anchors: Vec<String>,
    max_root_age_seconds: u32,
    cap_per_anchor_bps: u16,
    fee: FeeConfig,
    rules: RulesConfig,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum FeeConfig {
    Flat {
        bps: u16,
    },
    PerDay {
        base_bps: u16,
        bps_per_30_days: u16,
        max_bps: u16,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RulesConfig {
    required_documents: u8,
    min_vendor_tenure: u8,
    min_vendor_volume: u8,
    max_term_days: u16,
}

fn pad16(field: &str, s: &str) -> Result<[u8; 16], String> {
    if s.is_empty() || s.len() > 16 || !s.is_ascii() {
        return Err(format!("{field} {s:?} must be 1 to 16 ASCII characters"));
    }
    let mut out = [0u8; 16];
    out[..s.len()].copy_from_slice(s.as_bytes());
    Ok(out)
}

fn parse_keys(field: &str, keys: &[String]) -> Result<Vec<Pubkey>, String> {
    keys.iter()
        .map(|k| Pubkey::from_str(k).map_err(|_| format!("{field}: {k:?} is not a public key")))
        .collect()
}

fn load_params(path: &PathBuf) -> Result<PoolParams, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let c: Config = toml::from_str(&text).map_err(|e| e.to_string())?;
    if c.version != 1 {
        return Err(format!("config version {} is not 1", c.version));
    }
    Ok(PoolParams {
        network: pad16("network", &c.network)?,
        currency: pad16("currency", &c.currency)?,
        signers: parse_keys("signers", &c.signers)?,
        anchors: parse_keys("anchors", &c.anchors)?,
        fee: match c.fee {
            FeeConfig::Flat { bps } => FeeSchedule::Flat { bps },
            FeeConfig::PerDay {
                base_bps,
                bps_per_30_days,
                max_bps,
            } => FeeSchedule::PerDay {
                base_bps,
                bps_per_30_days,
                max_bps,
            },
        },
        rules: BillRules {
            required_documents: c.rules.required_documents,
            min_vendor_tenure: c.rules.min_vendor_tenure,
            min_vendor_volume: c.rules.min_vendor_volume,
            max_term_days: c.rules.max_term_days,
        },
        max_root_age: c.max_root_age_seconds,
        cap_per_anchor_bps: c.cap_per_anchor_bps,
    })
}

fn run(args: Args) -> Result<(), String> {
    let params = load_params(&args.config)?;
    let mint =
        Pubkey::from_str(&args.mint).map_err(|_| format!("{:?} is not a public key", args.mint))?;
    let payer = read_keypair_file(&args.keypair)
        .map_err(|e| format!("reading {}: {e}", args.keypair.display()))?;
    let creator = payer.pubkey();
    let program = supplier_pool::id();
    let pda = |seeds: &[&[u8]]| Pubkey::find_program_address(seeds, &program).0;
    let pool = pda(&[POOL_SEED, creator.as_ref(), mint.as_ref()]);
    let share_mint = pda(&[SHARES_SEED, pool.as_ref()]);
    let vault = ata(&pool, &mint, &token::ID);

    println!("{params:#?}");
    println!("pool:        {pool}");
    println!("share mint:  {share_mint}");
    println!("vault:       {vault}");
    println!("creator:     {creator}");
    if args.dry_run {
        println!("dry run: nothing sent");
        return Ok(());
    }

    let ix = Instruction::new_with_bytes(
        program,
        &supplier_pool::instruction::CreatePool { params }.data(),
        supplier_pool::accounts::CreatePool {
            creator,
            pool,
            mint,
            share_mint,
            vault,
            token_program: token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_the_devnet_config_loads() {
        let path = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../pools/supplier-devnet.toml"
        ));
        let p = load_params(&path).unwrap();
        assert_eq!(&p.network[..10], b"sol-devnet");
        assert_eq!(&p.currency[..8], b"FIAT_USD");
        assert_eq!(p.fee, FeeSchedule::Flat { bps: 150 });
        assert_eq!(p.signers.len(), 1);
    }
}
