//! A test chain with the supplier-pool and snapshot-roots programs and a
//! USDC-like token: a pool trusting the test's own signer, an anchor, and a
//! supplier whose address an offer commits to.

#![allow(dead_code)]
// LiteSVM's own result type; its error is large, and tests don't care.
#![allow(clippy::result_large_err)]

use {
    anchor_lang::{
        prelude::{Clock, Pubkey},
        solana_program::{instruction::Instruction, system_instruction, system_program},
        AccountDeserialize, InstructionData, ToAccountMetas,
    },
    anchor_spl::{
        associated_token::{
            get_associated_token_address_with_program_id as ata, spl_associated_token_account,
        },
        token::spl_token,
    },
    litesvm::{types::TransactionResult, LiteSVM},
    snapshot_roots::{
        token_snapshot::{
            merkle::{Tree, MERKLE_SCHEME_V1},
            BillLeaf, Snapshot, DOCUMENT_PROOF_OF_DELIVERY, DOCUMENT_VENDOR_INVOICE,
        },
        PostedRoot, ROOTS_SEED,
    },
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_sha256_hasher::hashv,
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
    supplier_pool::{
        claim_signature::claim_message, facts::ProvenLeaf, state::*, ANCHOR_SEED, CLAIMED_SEED,
        ED25519_PROGRAM_ID, LISTING_SEED, POOL_SEED, SHARES_SEED,
    },
};

pub const DAY: i64 = 86_400;
pub const T0: i64 = 1_800_000_000;
pub const USDC: u64 = 1_000_000;

pub fn padded(s: &str) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[..s.len()].copy_from_slice(s.as_bytes());
    out
}

/// A shared pool on devnet paying in USDC for USD bills: 1.5% flat, the
/// vendor's invoice required, terms up to 120 days, a day-old root at most,
/// and no anchor taking more than half the pool.
pub fn demo_params(signer: Pubkey) -> PoolParams {
    PoolParams {
        network: padded("sol-devnet"),
        currency: padded("FIAT_USD"),
        signers: vec![signer],
        anchors: vec![],
        fee: FeeSchedule::Flat { bps: 150 },
        rules: BillRules {
            required_documents: DOCUMENT_VENDOR_INVOICE,
            min_vendor_tenure: 0,
            min_vendor_volume: 0,
            max_term_days: 120,
        },
        max_root_age: 2 * DAY as u32,
        max_owed_per_anchor: 15_000 * USDC,
    }
}

pub struct Env {
    pub svm: LiteSVM,
    pub issuer: Keypair,
    /// Posts roots, as Scriply's signer does.
    pub oracle: Keypair,
    pub creator: Keypair,
    pub anchor: Keypair,
    pub supplier: Keypair,
    /// Submits claims, as custody's fee payer does.
    pub fee_payer: Keypair,
    pub usdc: Pubkey,
    pub pool: Pubkey,
    pub share_mint: Pubkey,
    pub vault: Pubkey,
    pub now: i64,
    last_root_as_of: i64,
}

/// A bill offered and approved: its salt, its commitment and its leaf.
#[derive(Clone)]
pub struct Bill {
    pub salt: [u8; 32],
    pub commitment: [u8; 32],
    pub leaf: BillLeaf,
}

impl Env {
    pub fn new() -> Self {
        Self::with_params(|_| {})
    }

    pub fn with_params(customize: impl FnOnce(&mut PoolParams)) -> Self {
        let mut env = Self::without_pool();
        let mut params = demo_params(env.oracle.pubkey());
        customize(&mut params);
        let ix = env.create_pool_ix(params);
        let creator = env.creator.insecure_clone();
        env.send(&[ix], &[&creator]).unwrap();
        env
    }

    pub fn without_pool() -> Self {
        let mut svm = LiteSVM::new();
        let program = include_bytes!(concat!(
            env!("CARGO_TARGET_TMPDIR"),
            "/../deploy/supplier_pool.so"
        ));
        svm.add_program(supplier_pool::id(), program).unwrap();
        let roots_program = include_bytes!(concat!(
            env!("CARGO_TARGET_TMPDIR"),
            "/../deploy/snapshot_roots.so"
        ));
        svm.add_program(snapshot_roots::id(), roots_program)
            .unwrap();
        let keys: Vec<Keypair> = (0..6).map(|_| Keypair::new()).collect();
        for k in &keys {
            svm.airdrop(&k.pubkey(), 100_000_000_000).unwrap();
        }
        let mut keys = keys.into_iter();
        let mut env = Env {
            svm,
            issuer: keys.next().unwrap(),
            oracle: keys.next().unwrap(),
            creator: keys.next().unwrap(),
            anchor: keys.next().unwrap(),
            supplier: Keypair::new(),
            fee_payer: keys.next().unwrap(),
            usdc: Pubkey::default(),
            pool: Pubkey::default(),
            share_mint: Pubkey::default(),
            vault: Pubkey::default(),
            now: T0,
            last_root_as_of: 0,
        };
        env.set_time(T0);
        env.usdc = env.create_usdc();
        env.pool = pda(&[POOL_SEED, env.creator.pubkey().as_ref(), env.usdc.as_ref()]);
        env.share_mint = pda(&[SHARES_SEED, env.pool.as_ref()]);
        env.vault = ata(&env.pool, &env.usdc, &spl_token::ID);
        env
    }

    pub fn create_pool_ix(&self, params: PoolParams) -> Instruction {
        Instruction::new_with_bytes(
            supplier_pool::id(),
            &supplier_pool::instruction::CreatePool { params }.data(),
            supplier_pool::accounts::CreatePool {
                creator: self.creator.pubkey(),
                pool: self.pool,
                mint: self.usdc,
                share_mint: self.share_mint,
                vault: self.vault,
                token_program: spl_token::ID,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        )
    }

    pub fn set_time(&mut self, unix: i64) {
        self.now = unix;
        let mut clock: Clock = self.svm.get_sysvar();
        clock.unix_timestamp = unix;
        clock.slot += 1;
        self.svm.set_sysvar(&clock);
        self.svm.expire_blockhash();
    }

    pub fn send(&mut self, ixs: &[Instruction], signers: &[&Keypair]) -> TransactionResult {
        let payer = signers[0].pubkey();
        let msg = Message::new_with_blockhash(ixs, Some(&payer), &self.svm.latest_blockhash());
        let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), signers).unwrap();
        let result = self.svm.send_transaction(tx);
        self.svm.expire_blockhash();
        result
    }

    /// Sends and expects failure with the program error `name`.
    pub fn send_expecting_error(&mut self, ixs: &[Instruction], signers: &[&Keypair], name: &str) {
        match self.send(ixs, signers) {
            Ok(_) => panic!("expected {name}, but the transaction succeeded"),
            Err(e) => {
                let logs = e.meta.logs.join("\n");
                assert!(logs.contains(name), "expected {name}, got:\n{logs}");
            }
        }
    }

    pub fn wallet(&mut self) -> Keypair {
        let k = Keypair::new();
        self.svm.airdrop(&k.pubkey(), 10_000_000_000).unwrap();
        k
    }

    fn create_usdc(&mut self) -> Pubkey {
        let mint = Keypair::new();
        let issuer = self.issuer.insecure_clone();
        let len = <spl_token::state::Mint as anchor_lang::solana_program::program_pack::Pack>::LEN;
        let ixs = [
            system_instruction::create_account(
                &issuer.pubkey(),
                &mint.pubkey(),
                self.svm.minimum_balance_for_rent_exemption(len),
                len as u64,
                &spl_token::ID,
            ),
            spl_token::instruction::initialize_mint2(
                &spl_token::ID,
                &mint.pubkey(),
                &issuer.pubkey(),
                None,
                6,
            )
            .unwrap(),
        ];
        self.send(&ixs, &[&issuer, &mint]).unwrap();
        mint.pubkey()
    }

    /// Creates `owner`'s USDC account if needed and mints `amount` to it.
    pub fn fund_usdc(&mut self, owner: &Pubkey, amount: u64) -> Pubkey {
        let account = ata(owner, &self.usdc, &spl_token::ID);
        let issuer = self.issuer.insecure_clone();
        let ixs = [
            spl_associated_token_account::instruction::create_associated_token_account_idempotent(
                &issuer.pubkey(),
                owner,
                &self.usdc,
                &spl_token::ID,
            ),
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &self.usdc,
                &account,
                &issuer.pubkey(),
                &[],
                amount,
            )
            .unwrap(),
        ];
        self.send(&ixs, &[&issuer]).unwrap();
        account
    }

    pub fn usdc_balance(&self, owner: &Pubkey) -> u64 {
        self.token_balance(&ata(owner, &self.usdc, &spl_token::ID))
    }

    pub fn token_balance(&self, account: &Pubkey) -> u64 {
        match self.svm.get_account(account) {
            Some(a) if !a.data.is_empty() => {
                <spl_token::state::Account as anchor_lang::solana_program::program_pack::Pack>::unpack(&a.data)
                    .unwrap()
                    .amount
            }
            _ => 0,
        }
    }

    pub fn lamports(&self, key: &Pubkey) -> u64 {
        self.svm.get_account(key).map(|a| a.lamports).unwrap_or(0)
    }

    pub fn pool_state(&self) -> Pool {
        self.read(&self.pool)
    }

    pub fn read<T: AccountDeserialize>(&self, key: &Pubkey) -> T {
        let account = self.svm.get_account(key).unwrap();
        T::try_deserialize(&mut account.data.as_slice()).unwrap()
    }

    pub fn deposit_ix(&self, depositor: &Pubkey, amount: u64) -> Instruction {
        Instruction::new_with_bytes(
            supplier_pool::id(),
            &supplier_pool::instruction::Deposit { amount }.data(),
            supplier_pool::accounts::Deposit {
                depositor: *depositor,
                pool: self.pool,
                mint: self.usdc,
                share_mint: self.share_mint,
                vault: self.vault,
                depositor_tokens: ata(depositor, &self.usdc, &spl_token::ID),
                depositor_shares: ata(depositor, &self.share_mint, &spl_token::ID),
                token_program: spl_token::ID,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        )
    }

    /// Funds a new depositor with `amount` and deposits it all.
    pub fn deposit(&mut self, amount: u64) -> Keypair {
        let depositor = self.wallet();
        self.fund_usdc(&depositor.pubkey(), amount);
        let ix = self.deposit_ix(&depositor.pubkey(), amount);
        self.send(&[ix], &[&depositor]).unwrap();
        depositor
    }

    pub fn withdraw_ix(&self, owner: &Pubkey, shares: u64) -> Instruction {
        Instruction::new_with_bytes(
            supplier_pool::id(),
            &supplier_pool::instruction::Withdraw { shares }.data(),
            supplier_pool::accounts::Withdraw {
                owner: *owner,
                pool: self.pool,
                mint: self.usdc,
                share_mint: self.share_mint,
                vault: self.vault,
                owner_shares: ata(owner, &self.share_mint, &spl_token::ID),
                owner_tokens: ata(owner, &self.usdc, &spl_token::ID),
                token_program: spl_token::ID,
            }
            .to_account_metas(None),
        )
    }

    pub fn shares_of(&self, owner: &Pubkey) -> u64 {
        self.token_balance(&ata(owner, &self.share_mint, &spl_token::ID))
    }

    /// A bill to `self.supplier` for `dollars`, due in 30 days and repaid
    /// in 60, with the vendor's invoice and proof of delivery verified.
    pub fn bill(&self, dollars: u64) -> Bill {
        let salt = [42u8; 32];
        self.bill_to(&self.supplier.pubkey(), salt, dollars)
    }

    pub fn bill_to(&self, supplier: &Pubkey, salt: [u8; 32], dollars: u64) -> Bill {
        let commitment = hashv(&[supplier.as_ref(), &salt]).to_bytes();
        Bill {
            salt,
            commitment,
            leaf: BillLeaf {
                version: 1,
                network: padded("sol-devnet"),
                commitment,
                as_of: 0,
                currency: padded("FIAT_USD"),
                amount: dollars * USDC,
                due_date: self.now + 30 * DAY,
                repay_on: self.now + 60 * DAY,
                documents_verified: DOCUMENT_VENDOR_INVOICE | DOCUMENT_PROOF_OF_DELIVERY,
                documents_hash: [3; 32],
                vendor_tenure: 3,
                vendor_volume: 2,
                vendor_history_vouched_bps: 0,
            },
        }
    }

    /// Posts a tree with the bill's leaf among a token's snapshot and
    /// another bill's leaf, as the signer's daily tree holds them, and
    /// returns the bill's proven leaf.
    pub fn post_bill(&mut self, bill: &Bill) -> ProvenLeaf {
        let oracle = self.oracle.insecure_clone();
        self.post_bill_as(&oracle, bill)
    }

    pub fn post_bill_as(&mut self, signer: &Keypair, bill: &Bill) -> ProvenLeaf {
        let as_of = (self.now - 60).max(self.last_root_as_of + 1);
        self.last_root_as_of = as_of;
        let mut leaf = bill.leaf.clone();
        leaf.as_of = as_of;
        let mut other = leaf.clone();
        other.commitment = [0xee; 32];
        let leaves = vec![
            Snapshot {
                version: 2,
                network: padded("sol-devnet"),
                ..Snapshot::default()
            }
            .to_bytes(),
            leaf.to_bytes(),
            other.to_bytes(),
        ];
        let tree = Tree::build(&leaves).unwrap();
        let ix = Instruction::new_with_bytes(
            snapshot_roots::id(),
            &snapshot_roots::instruction::PostRoot {
                root: PostedRoot {
                    root: tree.root(),
                    scheme: MERKLE_SCHEME_V1,
                    as_of,
                    leaf_count: leaves.len() as u32,
                },
            }
            .data(),
            snapshot_roots::accounts::PostRoot {
                signer: signer.pubkey(),
                payer: signer.pubkey(),
                roots: roots_of(&signer.pubkey()),
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        );
        self.send(&[ix], &[signer]).unwrap();
        ProvenLeaf {
            root: tree.root(),
            leaf: leaves[1].clone(),
            proof: tree.prove(1),
        }
    }

    pub fn listing_of(&self, anchor: &Pubkey, commitment: &[u8; 32]) -> Pubkey {
        pda(&[
            LISTING_SEED,
            self.pool.as_ref(),
            anchor.as_ref(),
            commitment,
        ])
    }

    pub fn anchor_account_of(&self, anchor: &Pubkey) -> Pubkey {
        pda(&[ANCHOR_SEED, self.pool.as_ref(), anchor.as_ref()])
    }

    pub fn list_ix(
        &self,
        anchor: &Pubkey,
        commitment: [u8; 32],
        proven: ProvenLeaf,
    ) -> Instruction {
        self.list_ix_with_roots(anchor, commitment, proven, &self.oracle.pubkey())
    }

    pub fn list_ix_with_roots(
        &self,
        anchor: &Pubkey,
        commitment: [u8; 32],
        proven: ProvenLeaf,
        signer: &Pubkey,
    ) -> Instruction {
        Instruction::new_with_bytes(
            supplier_pool::id(),
            &supplier_pool::instruction::ListBill { commitment, proven }.data(),
            supplier_pool::accounts::ListBill {
                anchor: *anchor,
                pool: self.pool,
                anchor_account: self.anchor_account_of(anchor),
                listing: self.listing_of(anchor, &commitment),
                roots: roots_of(signer),
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        )
    }

    /// Posts the bill and lists it from the anchor's wallet.
    pub fn list(&mut self, bill: &Bill) -> Pubkey {
        let proven = self.post_bill(bill);
        let anchor = self.anchor.insecure_clone();
        let ix = self.list_ix(&anchor.pubkey(), bill.commitment, proven);
        self.send(&[ix], &[&anchor]).unwrap();
        self.listing_of(&anchor.pubkey(), &bill.commitment)
    }

    /// The ed25519 program's check of `signer`'s signature over `message`,
    /// laid out as the runtime's helper does: header, key, signature,
    /// message, each in this instruction's own data.
    pub fn ed25519_ix(signer: &Keypair, message: &[u8]) -> Instruction {
        let signature = signer.sign_message(message);
        let (key_at, sig_at, msg_at) = (16u16, 48u16, 112u16);
        let mut data = vec![1u8, 0];
        for v in [
            sig_at,
            u16::MAX,
            key_at,
            u16::MAX,
            msg_at,
            message.len() as u16,
            u16::MAX,
        ] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        data.extend_from_slice(signer.pubkey().as_ref());
        data.extend_from_slice(signature.as_ref());
        data.extend_from_slice(message);
        Instruction::new_with_bytes(ED25519_PROGRAM_ID, &data, vec![])
    }

    /// The claim record of the listing's commitment, or of the supplier's
    /// and salt's when the listing is gone.
    pub fn claim_record_of(&self, listing: &Pubkey, supplier: &Pubkey, salt: [u8; 32]) -> Pubkey {
        let commitment = match self.svm.get_account(listing) {
            Some(a) if !a.data.is_empty() => {
                Listing::try_deserialize(&mut a.data.as_slice())
                    .unwrap()
                    .commitment
            }
            _ => hashv(&[supplier.as_ref(), &salt]).to_bytes(),
        };
        pda(&[CLAIMED_SEED, &commitment])
    }

    pub fn claim_ix(&self, listing: &Pubkey, supplier: &Pubkey, salt: [u8; 32]) -> Instruction {
        Instruction::new_with_bytes(
            supplier_pool::id(),
            &supplier_pool::instruction::Claim { salt }.data(),
            supplier_pool::accounts::Claim {
                fee_payer: self.fee_payer.pubkey(),
                pool: self.pool,
                listing: *listing,
                claim_record: self.claim_record_of(listing, supplier, salt),
                supplier: *supplier,
                supplier_tokens: ata(supplier, &self.usdc, &spl_token::ID),
                mint: self.usdc,
                vault: self.vault,
                instructions: anchor_lang::compat::solana_instructions_sysvar::ID,
                token_program: spl_token::ID,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        )
    }

    /// The supplier signs the claim and the fee payer submits it.
    pub fn claim(&mut self, listing: &Pubkey, bill: &Bill) -> TransactionResult {
        let supplier = self.supplier.insecure_clone();
        let fee_payer = self.fee_payer.insecure_clone();
        let ixs = [
            Self::ed25519_ix(&supplier, &claim_message(listing)),
            self.claim_ix(listing, &supplier.pubkey(), bill.salt),
        ];
        self.send(&ixs, &[&fee_payer])
    }

    pub fn cancel_ix(&self, anchor: &Pubkey, listing: &Pubkey) -> Instruction {
        Instruction::new_with_bytes(
            supplier_pool::id(),
            &supplier_pool::instruction::CancelListing {}.data(),
            supplier_pool::accounts::CancelListing {
                anchor: *anchor,
                pool: self.pool,
                anchor_account: self.anchor_account_of(anchor),
                listing: *listing,
            }
            .to_account_metas(None),
        )
    }

    pub fn repay_ix(&self, anchor: &Pubkey, listing: &Pubkey) -> Instruction {
        Instruction::new_with_bytes(
            supplier_pool::id(),
            &supplier_pool::instruction::Repay {}.data(),
            supplier_pool::accounts::Repay {
                anchor: *anchor,
                pool: self.pool,
                anchor_account: self.anchor_account_of(anchor),
                listing: *listing,
                mint: self.usdc,
                vault: self.vault,
                anchor_tokens: ata(anchor, &self.usdc, &spl_token::ID),
                token_program: spl_token::ID,
            }
            .to_account_metas(None),
        )
    }
}

pub fn pda(seeds: &[&[u8]]) -> Pubkey {
    Pubkey::find_program_address(seeds, &supplier_pool::id()).0
}

pub fn roots_of(signer: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[ROOTS_SEED, signer.as_ref()], &snapshot_roots::id()).0
}
