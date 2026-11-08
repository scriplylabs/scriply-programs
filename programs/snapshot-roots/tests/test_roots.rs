//! Runs the built program in LiteSVM: a signer posts roots, and a token's
//! snapshot is proven against the root read back from the chain.

#![allow(clippy::result_large_err)]

use {
    anchor_lang::{
        prelude::{AccountMeta, Clock, Pubkey},
        solana_program::{instruction::Instruction, system_program},
        AccountDeserialize, InstructionData, ToAccountMetas,
    },
    litesvm::{types::TransactionResult, LiteSVM},
    snapshot_roots::{
        token_snapshot::{
            merkle::{self, MERKLE_SCHEME_V1},
            Snapshot,
        },
        PostedRoot, SnapshotRoots, ROOTS_KEPT, ROOTS_SEED,
    },
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

const T0: i64 = 1_800_000_000;

struct Env {
    svm: LiteSVM,
    payer: Keypair,
}

impl Env {
    fn new() -> Self {
        let mut svm = LiteSVM::new();
        let bytes = include_bytes!(concat!(
            env!("CARGO_TARGET_TMPDIR"),
            "/../deploy/snapshot_roots.so"
        ));
        svm.add_program(snapshot_roots::id(), bytes).unwrap();
        let payer = Keypair::new();
        svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
        let mut env = Env { svm, payer };
        env.set_time(T0);
        env
    }

    fn set_time(&mut self, unix: i64) {
        let mut clock: Clock = self.svm.get_sysvar();
        clock.unix_timestamp = unix;
        clock.slot += 1;
        self.svm.set_sysvar(&clock);
        self.svm.expire_blockhash();
    }

    fn send(&mut self, ix: Instruction, signers: &[&Keypair]) -> TransactionResult {
        let msg = Message::new_with_blockhash(
            &[ix],
            Some(&self.payer.pubkey()),
            &self.svm.latest_blockhash(),
        );
        let mut all = vec![&self.payer];
        all.extend_from_slice(signers);
        let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &all).unwrap();
        let result = self.svm.send_transaction(tx);
        self.svm.expire_blockhash();
        result
    }

    fn send_expecting_error(&mut self, ix: Instruction, signers: &[&Keypair], name: &str) {
        match self.send(ix, signers) {
            Ok(_) => panic!("expected {name}, but the transaction succeeded"),
            Err(e) => {
                let logs = e.meta.logs.join("\n");
                assert!(logs.contains(name), "expected {name}, got:\n{logs}");
            }
        }
    }

    fn post_root_ix(
        &self,
        signer: &Pubkey,
        roots: Pubkey,
        root: [u8; 32],
        as_of: i64,
    ) -> Instruction {
        Instruction {
            program_id: snapshot_roots::id(),
            accounts: snapshot_roots::accounts::PostRoot {
                signer: *signer,
                payer: self.payer.pubkey(),
                roots,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: snapshot_roots::instruction::PostRoot {
                root: PostedRoot {
                    root,
                    scheme: MERKLE_SCHEME_V1,
                    as_of,
                    leaf_count: 3,
                },
            }
            .data(),
        }
    }

    fn roots(&self, signer: &Pubkey) -> SnapshotRoots {
        let account = self.svm.get_account(&roots_address(signer)).unwrap();
        SnapshotRoots::try_deserialize(&mut account.data.as_slice()).unwrap()
    }
}

fn roots_address(signer: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[ROOTS_SEED, signer.as_ref()], &snapshot_roots::id()).0
}

fn snapshots() -> Vec<Vec<u8>> {
    (1..=3u8)
        .map(|i| {
            Snapshot {
                version: 1,
                mint: [i; 32],
                as_of: T0,
                burn_debt_overdue: i == 2,
                ..Snapshot::default()
            }
            .to_bytes()
        })
        .collect()
}

#[test]
fn test_a_posted_root_proves_a_tokens_snapshot() {
    let mut env = Env::new();
    let signer = Keypair::new();
    let leaves = snapshots();
    let tree = merkle::Tree::build(&leaves).unwrap();
    let ix = env.post_root_ix(
        &signer.pubkey(),
        roots_address(&signer.pubkey()),
        tree.root(),
        T0,
    );
    env.send(ix, &[&signer]).unwrap();

    let roots = env.roots(&signer.pubkey());
    assert_eq!(roots.signer, signer.pubkey());
    assert_eq!(roots.version, snapshot_roots::ACCOUNT_VERSION);
    let latest = roots.latest().unwrap();
    assert_eq!(
        (latest.root, latest.scheme, latest.as_of, latest.leaf_count),
        (tree.root(), MERKLE_SCHEME_V1, T0, 3)
    );

    // What a program does with a keeper's snapshot and proof.
    assert!(merkle::verify(&latest.root, &leaves[1], &tree.prove(1)));
    let parsed = Snapshot::parse(&leaves[1]).unwrap();
    assert_eq!(parsed.mint, [2; 32]);
    assert!(parsed.burn_debt_overdue);

    // A keeper can't hide the overdue flag.
    let mut hidden = parsed.clone();
    hidden.burn_debt_overdue = false;
    assert!(!merkle::verify(
        &latest.root,
        &hidden.to_bytes(),
        &tree.prove(1)
    ));
}

#[test]
fn test_roots_must_be_newer() {
    let mut env = Env::new();
    let signer = Keypair::new();
    let account = roots_address(&signer.pubkey());
    env.send(
        env.post_root_ix(&signer.pubkey(), account, [1; 32], T0),
        &[&signer],
    )
    .unwrap();
    env.send_expecting_error(
        env.post_root_ix(&signer.pubkey(), account, [2; 32], T0),
        &[&signer],
        "RootNotNewer",
    );
    env.send_expecting_error(
        env.post_root_ix(&signer.pubkey(), account, [2; 32], T0 + 3_600),
        &[&signer],
        "RootFromFuture",
    );
    env.set_time(T0 + 86_400);
    env.send(
        env.post_root_ix(&signer.pubkey(), account, [2; 32], T0 + 86_400),
        &[&signer],
    )
    .unwrap();
    let roots = env.roots(&signer.pubkey());
    assert_eq!(roots.latest().unwrap().root, [2; 32]);
    assert!(roots.find(&[1; 32]).is_some());
}

#[test]
fn test_the_account_keeps_the_latest_roots() {
    let mut env = Env::new();
    let signer = Keypair::new();
    let account = roots_address(&signer.pubkey());
    for i in 0..=ROOTS_KEPT as i64 {
        let ix = env.post_root_ix(&signer.pubkey(), account, [i as u8 + 1; 32], T0 - 100 + i);
        env.send(ix, &[&signer]).unwrap();
    }
    let roots = env.roots(&signer.pubkey());
    assert_eq!(roots.posted, ROOTS_KEPT as u64 + 1);
    assert!(roots.find(&[1; 32]).is_none());
    assert!(roots.find(&[2; 32]).is_some());
}

#[test]
fn test_nobody_else_posts_to_a_signers_account() {
    let mut env = Env::new();
    let signer = Keypair::new();
    let intruder = Keypair::new();
    let account = roots_address(&signer.pubkey());
    env.send(
        env.post_root_ix(&signer.pubkey(), account, [1; 32], T0 - 10),
        &[&signer],
    )
    .unwrap();

    // Another key signing, against the signer's account.
    env.send_expecting_error(
        env.post_root_ix(&intruder.pubkey(), account, [9; 32], T0),
        &[&intruder],
        "ConstraintSeeds",
    );

    // The signer's key named but not signing.
    let mut ix = env.post_root_ix(&signer.pubkey(), account, [9; 32], T0);
    ix.accounts[0] = AccountMeta::new_readonly(signer.pubkey(), false);
    env.send_expecting_error(ix, &[], "AccountNotSigner");

    assert_eq!(env.roots(&signer.pubkey()).latest().unwrap().root, [1; 32]);
}
