//! A test chain with the pool and snapshot-roots programs, a USDC-like
//! quote token, and a receivables token set up as the services set one up:
//! Token-2022, six decimals, accounts frozen by default, and a permanent
//! delegate. The pool is created from `pools/devnet.toml`, trusting the
//! test's own signer.

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
        token_2022::spl_token_2022::{
            self,
            extension::{default_account_state, ExtensionType},
            state::{AccountState, Mint},
        },
    },
    litesvm::{types::TransactionResult, LiteSVM},
    receivables_pool::{
        snapshot::{ProvenSnapshot, Snapshot},
        state::*,
        AUTHORITY_SEED, EXPOSURE_SEED, GENERATION_SEED, LOAN_SEED, POOL_SEED, SHARES_SEED,
        WITHDRAWAL_SEED,
    },
    snapshot_roots::{
        token_snapshot::merkle::{Tree, MERKLE_SCHEME_V1},
        PostedRoot, ROOTS_SEED,
    },
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

pub const DAY: i64 = 86_400;
pub const EPOCH: i64 = 7 * DAY;
pub const T0: i64 = 1_800_000_000;
pub const USDC: u64 = 1_000_000;
pub const UNIT: u64 = 1_000_000;

pub struct Env {
    pub svm: LiteSVM,
    /// Mints both tokens, and is the receivables token's freeze authority
    /// and permanent delegate, as Scriply's multisig is.
    pub issuer: Keypair,
    /// Posts roots of tokens' snapshots, as Scriply's signer does.
    pub oracle: Keypair,
    pub creator: Keypair,
    pub usdc: Pubkey,
    pub token: Pubkey,
    pub pool: Pubkey,
    pub authority: Pubkey,
    pub share_mint: Pubkey,
    pub quote_vault: Pubkey,
    pub share_escrow: Pubkey,
    pub now: i64,
    last_root_as_of: i64,
}

/// The parameters in pools/devnet.toml, trusting `signer`.
pub fn devnet_params(signer: Pubkey) -> PoolParams {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../pools/devnet.toml");
    let mut params = pool_config::load(path).unwrap();
    params.signers = vec![signer];
    params
}

/// One token's snapshot proven against a posted tree.
pub struct Posted {
    pub tree: Tree,
    pub leaves: Vec<Vec<u8>>,
}

impl Posted {
    pub fn proven(&self, index: usize) -> ProvenSnapshot {
        ProvenSnapshot {
            root: self.tree.root(),
            snapshot: self.leaves[index].clone(),
            proof: self.tree.prove(index),
        }
    }
}

impl Env {
    pub fn new() -> Self {
        Self::with_params(|_| {})
    }

    pub fn with_params(customize: impl FnOnce(&mut PoolParams)) -> Self {
        let mut env = Self::without_pool();
        let mut params = devnet_params(env.oracle.pubkey());
        customize(&mut params);
        let ix = env.create_pool_ix(params);
        let creator = env.creator.insecure_clone();
        env.send(&[ix], &[&creator]).unwrap();
        env
    }

    /// The chain and tokens, with the pool's addresses worked out but the
    /// pool not yet created.
    pub fn without_pool() -> Self {
        let mut svm = LiteSVM::new();
        let pool_program = include_bytes!(concat!(
            env!("CARGO_TARGET_TMPDIR"),
            "/../deploy/receivables_pool.so"
        ));
        svm.add_program(receivables_pool::id(), pool_program)
            .unwrap();
        let roots_program = include_bytes!(concat!(
            env!("CARGO_TARGET_TMPDIR"),
            "/../deploy/snapshot_roots.so"
        ));
        svm.add_program(snapshot_roots::id(), roots_program)
            .unwrap();
        let issuer = Keypair::new();
        let oracle = Keypair::new();
        let creator = Keypair::new();
        for k in [&issuer, &oracle, &creator] {
            svm.airdrop(&k.pubkey(), 100_000_000_000).unwrap();
        }
        let mut env = Env {
            svm,
            issuer,
            oracle,
            creator,
            usdc: Pubkey::default(),
            token: Pubkey::default(),
            pool: Pubkey::default(),
            authority: Pubkey::default(),
            share_mint: Pubkey::default(),
            quote_vault: Pubkey::default(),
            share_escrow: Pubkey::default(),
            now: T0,
            last_root_as_of: 0,
        };
        env.set_time(T0);
        env.usdc = env.create_usdc();
        env.token = env.create_receivables_token();

        let seed = 1u64;
        env.pool = pda(&[
            POOL_SEED,
            env.creator.pubkey().as_ref(),
            &seed.to_le_bytes(),
        ]);
        env.authority = pda(&[AUTHORITY_SEED, env.pool.as_ref()]);
        env.share_mint = pda(&[SHARES_SEED, env.pool.as_ref()]);
        env.quote_vault = ata(&env.authority, &env.usdc, &spl_token::ID);
        env.share_escrow = ata(&env.authority, &env.share_mint, &spl_token::ID);
        env
    }

    pub fn create_pool_ix(&self, params: PoolParams) -> Instruction {
        Instruction::new_with_bytes(
            receivables_pool::id(),
            &receivables_pool::instruction::CreatePool { seed: 1, params }.data(),
            receivables_pool::accounts::CreatePool {
                creator: self.creator.pubkey(),
                pool: self.pool,
                pool_authority: self.authority,
                quote_mint: self.usdc,
                share_mint: self.share_mint,
                quote_vault: self.quote_vault,
                share_escrow: self.share_escrow,
                quote_token_program: spl_token::ID,
                share_token_program: spl_token::ID,
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

    fn create_receivables_token(&mut self) -> Pubkey {
        let mint = Keypair::new();
        let issuer = self.issuer.insecure_clone();
        let len = ExtensionType::try_calculate_account_len::<Mint>(&[
            ExtensionType::DefaultAccountState,
            ExtensionType::PermanentDelegate,
        ])
        .unwrap();
        let ixs = [
            system_instruction::create_account(
                &issuer.pubkey(),
                &mint.pubkey(),
                self.svm.minimum_balance_for_rent_exemption(len),
                len as u64,
                &spl_token_2022::ID,
            ),
            spl_token_2022::instruction::initialize_permanent_delegate(
                &spl_token_2022::ID,
                &mint.pubkey(),
                &issuer.pubkey(),
            )
            .unwrap(),
            default_account_state::instruction::initialize_default_account_state(
                &spl_token_2022::ID,
                &mint.pubkey(),
                &AccountState::Frozen,
            )
            .unwrap(),
            spl_token_2022::instruction::initialize_mint2(
                &spl_token_2022::ID,
                &mint.pubkey(),
                &issuer.pubkey(),
                Some(&issuer.pubkey()),
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

    /// Creates a USDC account owned by `owner` that is not its associated
    /// token account, as anyone can.
    pub fn side_usdc_account(&mut self, owner: &Pubkey) -> Pubkey {
        let account = Keypair::new();
        let payer = self.issuer.insecure_clone();
        // An SPL token account with no extensions.
        let len = 165;
        let rent = self.svm.minimum_balance_for_rent_exemption(len);
        let ixs = [
            system_instruction::create_account(
                &payer.pubkey(),
                &account.pubkey(),
                rent,
                len as u64,
                &spl_token::ID,
            ),
            spl_token::instruction::initialize_account3(
                &spl_token::ID,
                &account.pubkey(),
                &self.usdc,
                owner,
            )
            .unwrap(),
        ];
        self.send(&ixs, &[&payer, &account]).unwrap();
        account.pubkey()
    }

    /// Creates `owner`'s account for the receivables token, which starts
    /// frozen, and thaws it, as Scriply does for an attested wallet.
    pub fn thawed_token_account(&mut self, owner: &Pubkey) -> Pubkey {
        let account = ata(owner, &self.token, &spl_token_2022::ID);
        let issuer = self.issuer.insecure_clone();
        let ixs = [
            spl_associated_token_account::instruction::create_associated_token_account_idempotent(
                &issuer.pubkey(),
                owner,
                &self.token,
                &spl_token_2022::ID,
            ),
            spl_token_2022::instruction::thaw_account(
                &spl_token_2022::ID,
                &account,
                &self.token,
                &issuer.pubkey(),
                &[],
            )
            .unwrap(),
        ];
        self.send(&ixs, &[&issuer]).unwrap();
        account
    }

    /// Tokenizes `units` to `owner`'s wallet.
    pub fn mint_units(&mut self, owner: &Pubkey, units: u64) -> Pubkey {
        let account = self.thawed_token_account(owner);
        let issuer = self.issuer.insecure_clone();
        let ix = spl_token_2022::instruction::mint_to_checked(
            &spl_token_2022::ID,
            &self.token,
            &account,
            &issuer.pubkey(),
            &[],
            units,
            6,
        )
        .unwrap();
        self.send(&[ix], &[&issuer]).unwrap();
        account
    }

    /// A buy-back: burns `units` from `borrower`'s loan vault with
    /// the permanent delegate and pays `usdc` for them to the loan's
    /// proceeds.
    pub fn buy_back(&mut self, borrower: &Pubkey, units: u64, usdc: u64) {
        let issuer = self.issuer.insecure_clone();
        let funding = self.fund_usdc(&issuer.pubkey(), usdc);
        let loan = self.loan(borrower);
        let ixs = [
            spl_token_2022::instruction::burn_checked(
                &spl_token_2022::ID,
                &self.collateral_vault(borrower),
                &self.token,
                &issuer.pubkey(),
                &[],
                units,
                6,
            )
            .unwrap(),
            spl_token::instruction::transfer_checked(
                &spl_token::ID,
                &funding,
                &self.usdc,
                &ata(&loan, &self.usdc, &spl_token::ID),
                &issuer.pubkey(),
                &[],
                usdc,
                6,
            )
            .unwrap(),
        ];
        self.send(&ixs, &[&issuer]).unwrap();
    }

    pub fn loan(&self, borrower: &Pubkey) -> Pubkey {
        pda(&[
            LOAN_SEED,
            self.pool.as_ref(),
            self.token.as_ref(),
            borrower.as_ref(),
        ])
    }

    pub fn exposure(&self) -> Pubkey {
        pda(&[EXPOSURE_SEED, self.pool.as_ref(), self.token.as_ref()])
    }

    pub fn collateral_vault(&self, borrower: &Pubkey) -> Pubkey {
        ata(&self.loan(borrower), &self.token, &spl_token_2022::ID)
    }

    /// Creates `borrower`'s loan vault and thaws it: the token's accounts
    /// start frozen, so the issuer thaws it before the borrower's first loan.
    pub fn thaw_vault(&mut self, borrower: &Pubkey) {
        let loan = self.loan(borrower);
        self.thawed_token_account(&loan);
    }

    /// The token's snapshot now: every unit backed by invoices not yet due.
    pub fn snapshot(&self) -> Snapshot {
        let mut network = [0u8; 16];
        network[..10].copy_from_slice(b"sol-devnet");
        Snapshot {
            version: 1,
            network,
            mint: self.token.to_bytes(),
            as_of: self.now - 60,
            report_hash: [9; 32],
            network_supply: self.token_supply(),
            outstanding_units: self.token_supply(),
            burn_debt: 0,
            burn_debt_overdue: false,
            aging_current: self.token_supply(),
            aging_days_1_to_30: 0,
            aging_days_31_to_60: 0,
            aging_days_61_to_90: 0,
            aging_over_90: 0,
            concentration_index_bps: 2_000,
            disputed_share_bps: 0,
            // Due in 20 days, from customers with a full history of paying
            // on time, so the rate is the base and a 4-epoch term passes.
            weighted_days_to_due: 20,
            weighted_days_to_pay: 30,
            weighted_days_beyond_terms: 0,
            paid_on_time_share_bps: 10_000,
            history_share_bps: 10_000,
        }
    }

    pub fn roots(&self) -> Pubkey {
        roots_of(&self.oracle.pubkey())
    }

    /// Builds a tree of `snapshots` and posts its root as the oracle,
    /// dated a minute ago (or just after the last root).
    pub fn post(&mut self, snapshots: &[Snapshot]) -> Posted {
        let oracle = self.oracle.insecure_clone();
        self.post_as(&oracle, snapshots)
    }

    pub fn post_as(&mut self, signer: &Keypair, snapshots: &[Snapshot]) -> Posted {
        let leaves: Vec<Vec<u8>> = snapshots.iter().map(Snapshot::to_bytes).collect();
        let tree = Tree::build(&leaves).unwrap();
        let as_of = (self.now - 60).max(self.last_root_as_of + 1);
        self.last_root_as_of = as_of;
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
        Posted { tree, leaves }
    }

    /// Posts a tree holding the token's snapshot now, among two other
    /// tokens', and returns the token's proven snapshot.
    pub fn post_token(&mut self) -> ProvenSnapshot {
        let s = self.snapshot();
        self.post_with(s)
    }

    /// The same, with the token's snapshot as given.
    pub fn post_with(&mut self, s: Snapshot) -> ProvenSnapshot {
        let mut others = [s.clone(), s.clone()];
        others[0].mint = [1; 32];
        others[1].mint = [2; 32];
        let posted = self.post(&[others[0].clone(), s, others[1].clone()]);
        posted.proven(1)
    }

    pub fn settle_ix(&self, payer: &Pubkey) -> Instruction {
        let generation = self.pool_state().withdrawal_generation;
        Instruction::new_with_bytes(
            receivables_pool::id(),
            &receivables_pool::instruction::SettleEpoch {}.data(),
            receivables_pool::accounts::SettleEpoch {
                payer: *payer,
                pool: self.pool,
                pool_authority: self.authority,
                share_mint: self.share_mint,
                share_escrow: self.share_escrow,
                generation_record: self.generation_record(generation),
                share_token_program: spl_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        )
    }

    pub fn settle(&mut self) {
        let ix = self.settle_ix(&self.creator.pubkey());
        let creator = self.creator.insecure_clone();
        self.send(&[ix], &[&creator]).unwrap();
    }

    /// What a keeper does at an epoch's start: settles, marks every loan of
    /// `borrowers` from a freshly posted root, and settles again to pay
    /// withdrawals.
    pub fn settle_and_mark(&mut self, borrowers: &[&Pubkey]) {
        self.settle();
        if !borrowers.is_empty() {
            let proven = self.post_token();
            let ix = self.mark_loans_ix(vec![proven], borrowers);
            let creator = self.creator.insecure_clone();
            self.send(&[ix], &[&creator]).unwrap();
        }
        self.settle();
    }

    pub fn mark_loans_ix(
        &self,
        snapshots: Vec<ProvenSnapshot>,
        borrowers: &[&Pubkey],
    ) -> Instruction {
        let mut accounts = receivables_pool::accounts::MarkLoans {
            pool: self.pool,
            roots: self.roots(),
        }
        .to_account_metas(None);
        accounts.extend(
            borrowers
                .iter()
                .map(|b| anchor_lang::prelude::AccountMeta::new(self.loan(b), false)),
        );
        Instruction::new_with_bytes(
            receivables_pool::id(),
            &receivables_pool::instruction::MarkLoans { snapshots }.data(),
            accounts,
        )
    }

    pub fn mark_stale_ix(&self, borrowers: &[&Pubkey]) -> Instruction {
        let mut accounts =
            receivables_pool::accounts::MarkStale { pool: self.pool }.to_account_metas(None);
        accounts.extend(
            borrowers
                .iter()
                .map(|b| anchor_lang::prelude::AccountMeta::new(self.loan(b), false)),
        );
        Instruction::new_with_bytes(
            receivables_pool::id(),
            &receivables_pool::instruction::MarkStale {}.data(),
            accounts,
        )
    }

    pub fn generation_record(&self, generation: u64) -> Pubkey {
        pda(&[
            GENERATION_SEED,
            self.pool.as_ref(),
            &generation.to_le_bytes(),
        ])
    }

    pub fn deposit_ix(
        &self,
        depositor: &Pubkey,
        amount: u64,
        attestation: Option<Pubkey>,
    ) -> Instruction {
        Instruction::new_with_bytes(
            receivables_pool::id(),
            &receivables_pool::instruction::Deposit { amount }.data(),
            receivables_pool::accounts::Deposit {
                depositor: *depositor,
                pool: self.pool,
                pool_authority: self.authority,
                quote_mint: self.usdc,
                share_mint: self.share_mint,
                quote_vault: self.quote_vault,
                depositor_quote: ata(depositor, &self.usdc, &spl_token::ID),
                depositor_shares: ata(depositor, &self.share_mint, &spl_token::ID),
                attestation,
                quote_token_program: spl_token::ID,
                share_token_program: spl_token::ID,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        )
    }

    /// A funded depositor who has deposited `amount`.
    pub fn depositor(&mut self, amount: u64) -> Keypair {
        let d = self.wallet();
        self.fund_usdc(&d.pubkey(), amount);
        let ix = self.deposit_ix(&d.pubkey(), amount, None);
        self.send(&[ix], &[&d]).unwrap();
        d
    }

    pub fn borrow_ix(
        &self,
        borrower: &Pubkey,
        units: u64,
        principal: u64,
        maturity_epochs: u16,
        proven: ProvenSnapshot,
    ) -> Instruction {
        self.borrow_ix_with_roots(
            borrower,
            units,
            principal,
            maturity_epochs,
            proven,
            self.roots(),
        )
    }

    pub fn borrow_ix_with_roots(
        &self,
        borrower: &Pubkey,
        units: u64,
        principal: u64,
        maturity_epochs: u16,
        proven: ProvenSnapshot,
        roots: Pubkey,
    ) -> Instruction {
        let loan = self.loan(borrower);
        Instruction::new_with_bytes(
            receivables_pool::id(),
            &receivables_pool::instruction::Borrow {
                units,
                principal,
                maturity_epochs,
                proven,
            }
            .data(),
            receivables_pool::accounts::Borrow {
                borrower: *borrower,
                pool: self.pool,
                pool_authority: self.authority,
                quote_mint: self.usdc,
                quote_vault: self.quote_vault,
                borrower_quote: ata(borrower, &self.usdc, &spl_token::ID),
                collateral_mint: self.token,
                borrower_collateral: ata(borrower, &self.token, &spl_token_2022::ID),
                loan,
                exposure: self.exposure(),
                collateral_vault: self.collateral_vault(borrower),
                loan_proceeds: ata(&loan, &self.usdc, &spl_token::ID),
                roots,
                quote_token_program: spl_token::ID,
                collateral_token_program: spl_token_2022::ID,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        )
    }

    fn loan_tokens(&self, borrower: &Pubkey) -> receivables_pool::accounts::LoanTokens {
        let loan = self.loan(borrower);
        receivables_pool::accounts::LoanTokens {
            pool_authority: self.authority,
            quote_vault: self.quote_vault,
            collateral_vault: self.collateral_vault(borrower),
            loan_proceeds: ata(&loan, &self.usdc, &spl_token::ID),
            borrower_quote: ata(borrower, &self.usdc, &spl_token::ID),
            borrower_collateral: ata(borrower, &self.token, &spl_token_2022::ID),
            quote_token_program: spl_token::ID,
            collateral_token_program: spl_token_2022::ID,
        }
    }

    pub fn sync_loan_ix(&self, borrower: &Pubkey) -> Instruction {
        self.sync_loan_ix_into(borrower, self.quote_vault)
    }

    /// `sync_loan` paying into `quote_vault` instead of the pool's vault.
    pub fn sync_loan_ix_into(&self, borrower: &Pubkey, quote_vault: Pubkey) -> Instruction {
        let mut loan_tokens = self.loan_tokens(borrower);
        loan_tokens.quote_vault = quote_vault;
        Instruction::new_with_bytes(
            receivables_pool::id(),
            &receivables_pool::instruction::SyncLoan {}.data(),
            receivables_pool::accounts::SyncLoan {
                pool: self.pool,
                loan: self.loan(borrower),
                exposure: self.exposure(),
                loan_tokens,
                quote_mint: self.usdc,
                collateral_mint: self.token,
            }
            .to_account_metas(None),
        )
    }

    pub fn repay_ix(&self, payer: &Pubkey, borrower: &Pubkey, amount: u64) -> Instruction {
        Instruction::new_with_bytes(
            receivables_pool::id(),
            &receivables_pool::instruction::Repay { amount }.data(),
            receivables_pool::accounts::Repay {
                payer: *payer,
                payer_quote: ata(payer, &self.usdc, &spl_token::ID),
                pool: self.pool,
                loan: self.loan(borrower),
                exposure: self.exposure(),
                loan_tokens: self.loan_tokens(borrower),
                quote_mint: self.usdc,
                collateral_mint: self.token,
            }
            .to_account_metas(None),
        )
    }

    pub fn request_withdrawal_ix(&self, owner: &Pubkey, shares: u64) -> Instruction {
        Instruction::new_with_bytes(
            receivables_pool::id(),
            &receivables_pool::instruction::RequestWithdrawal { shares }.data(),
            receivables_pool::accounts::RequestWithdrawal {
                owner: *owner,
                pool: self.pool,
                pool_authority: self.authority,
                share_mint: self.share_mint,
                owner_shares: ata(owner, &self.share_mint, &spl_token::ID),
                share_escrow: self.share_escrow,
                request: pda(&[WITHDRAWAL_SEED, self.pool.as_ref(), owner.as_ref()]),
                share_token_program: spl_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        )
    }

    pub fn claim_withdrawal_ix(&self, owner: &Pubkey, generation: Option<u64>) -> Instruction {
        Instruction::new_with_bytes(
            receivables_pool::id(),
            &receivables_pool::instruction::ClaimWithdrawal {}.data(),
            receivables_pool::accounts::ClaimWithdrawal {
                owner: *owner,
                pool: self.pool,
                pool_authority: self.authority,
                quote_mint: self.usdc,
                quote_vault: self.quote_vault,
                owner_quote: ata(owner, &self.usdc, &spl_token::ID),
                request: pda(&[WITHDRAWAL_SEED, self.pool.as_ref(), owner.as_ref()]),
                generation_record: generation.map(|g| self.generation_record(g)),
                quote_token_program: spl_token::ID,
            }
            .to_account_metas(None),
        )
    }

    pub fn pool_state(&self) -> Pool {
        let account = self.svm.get_account(&self.pool).unwrap();
        bytemuck::pod_read_unaligned(&account.data[8..8 + std::mem::size_of::<Pool>()])
    }

    pub fn loan_state(&self, borrower: &Pubkey) -> Loan {
        let account = self.svm.get_account(&self.loan(borrower)).unwrap();
        Loan::try_deserialize(&mut account.data.as_slice()).unwrap()
    }

    pub fn exposure_state(&self) -> TokenExposure {
        let account = self.svm.get_account(&self.exposure()).unwrap();
        TokenExposure::try_deserialize(&mut account.data.as_slice()).unwrap()
    }

    /// A token account's balance; both token programs keep it at bytes 64..72.
    pub fn balance(&self, account: &Pubkey) -> u64 {
        self.svm
            .get_account(account)
            .map(|a| u64::from_le_bytes(a.data[64..72].try_into().unwrap()))
            .unwrap_or(0)
    }

    pub fn usdc_balance(&self, owner: &Pubkey) -> u64 {
        self.balance(&ata(owner, &self.usdc, &spl_token::ID))
    }

    pub fn units_of(&self, owner: &Pubkey) -> u64 {
        self.balance(&ata(owner, &self.token, &spl_token_2022::ID))
    }

    pub fn shares_of(&self, owner: &Pubkey) -> u64 {
        self.balance(&ata(owner, &self.share_mint, &spl_token::ID))
    }

    pub fn token_supply(&self) -> u64 {
        let account = self.svm.get_account(&self.token).unwrap();
        u64::from_le_bytes(account.data[36..44].try_into().unwrap())
    }
}

pub fn pda(seeds: &[&[u8]]) -> Pubkey {
    Pubkey::find_program_address(seeds, &receivables_pool::id()).0
}

pub fn roots_of(signer: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[ROOTS_SEED, signer.as_ref()], &snapshot_roots::id()).0
}
