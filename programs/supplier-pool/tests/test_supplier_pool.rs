//! The supplier pool end to end in LiteSVM: deposits, a bill listed on its
//! proven leaf, the supplier's claim, the anchor's repayment, and every
//! rule that refuses. Build the programs first (`anchor build`).

mod common;

use common::*;
use snapshot_roots::token_snapshot::{AsOfFacts, DOCUMENT_PURCHASE_ORDER};
use solana_signer::Signer;
use supplier_pool::{
    claim_signature::claim_message,
    events::{BillClaimed, ListingCancelled, ListingRepaid, ListingWrittenOff},
    state::{AnchorAccount, BusinessRules, FeeSchedule, Listing},
    CLAIM_NETWORK_FEE_LAMPORTS,
};

#[test]
fn test_a_bill_is_listed_claimed_and_repaid() {
    let mut env = Env::new();
    let depositor = env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    let listing = env.list(&bill);

    let l: Listing = env.read(&listing);
    assert_eq!(l.amount, 10_000 * USDC);
    assert_eq!(l.due_date, bill.leaf.repay_on);
    assert!(!l.claimed);
    let a: AnchorAccount = env.read(&env.anchor_account_of(&env.anchor.pubkey()));
    assert_eq!(a.owed, 10_000 * USDC);
    assert_eq!(env.pool_state().listed, 10_000 * USDC);

    let fee_payer_before = env.lamports(&env.fee_payer.pubkey());
    env.claim(&listing, &bill).unwrap();
    // 1.5% of 10,000 USDC.
    assert_eq!(env.usdc_balance(&env.supplier.pubkey()), 9_850 * USDC);
    let pool = env.pool_state();
    assert_eq!(pool.idle, 90_150 * USDC);
    assert_eq!(pool.outstanding, 10_000 * USDC);
    assert_eq!(pool.listed, 0);
    // The fee payer paid the network fee (its own signature and the
    // ed25519 check's) and the supplier's account's rent, and was refunded
    // both from the listing's deposit.
    assert_eq!(env.lamports(&env.fee_payer.pubkey()), fee_payer_before);
    let l: Listing = env.read(&listing);
    let rent = env.svm.minimum_balance_for_rent_exemption(165);
    assert_eq!(
        l.fee_deposit, 0,
        "deposit {CLAIM_NETWORK_FEE_LAMPORTS} + {rent} + the claim record's rent all refunded"
    );

    let anchor = env.anchor.insecure_clone();
    env.fund_usdc(&anchor.pubkey(), 10_000 * USDC);
    let anchor_lamports = env.lamports(&anchor.pubkey());
    let ix = env.repay_ix(&anchor.pubkey(), &listing);
    env.send(&[ix], &[&anchor]).unwrap();
    assert!(env
        .svm
        .get_account(&listing)
        .is_none_or(|a| a.lamports == 0));
    assert!(env.lamports(&anchor.pubkey()) > anchor_lamports);
    let pool = env.pool_state();
    assert_eq!(pool.idle, 100_150 * USDC);
    assert_eq!(pool.outstanding, 0);
    let a: AnchorAccount = env.read(&env.anchor_account_of(&anchor.pubkey()));
    assert_eq!(a.owed, 0);

    // The depositor takes out what it put in plus the fee, less rounding.
    let shares = env.shares_of(&depositor.pubkey());
    let ix = env.withdraw_ix(&depositor.pubkey(), shares);
    env.send(&[ix], &[&depositor]).unwrap();
    let out = env.usdc_balance(&depositor.pubkey());
    assert!(out > 100_149 * USDC && out <= 100_150 * USDC, "{out}");
}

#[test]
fn test_a_per_day_fee_counts_the_days_to_the_due_date() {
    let mut env = Env::with_params(|p| {
        p.fee = FeeSchedule::PerDay {
            base_bps: 50,
            bps_per_30_days: 60,
            max_bps: 500,
        }
    });
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    let listing = env.list(&bill);
    env.claim(&listing, &bill).unwrap();
    // 60 days to repayment: 50 + 120 bps = 1.7%.
    assert_eq!(env.usdc_balance(&env.supplier.pubkey()), 9_830 * USDC);
}

#[test]
fn test_an_unclaimed_listing_is_cancelled_with_its_deposit() {
    let mut env = Env::new();
    env.deposit(100_000 * USDC);
    let anchor = env.anchor.insecure_clone();
    let before = env.lamports(&anchor.pubkey());
    let bill = env.bill(10_000);
    let listing = env.list(&bill);
    let ix = env.cancel_ix(&anchor.pubkey(), &listing);
    env.send(&[ix], &[&anchor]).unwrap();
    assert!(env
        .svm
        .get_account(&listing)
        .is_none_or(|a| a.lamports == 0));
    // Only the two transactions' fees and the anchor account's rent are gone.
    let anchor_rent = env
        .svm
        .get_account(&env.anchor_account_of(&anchor.pubkey()))
        .unwrap()
        .lamports;
    assert_eq!(
        env.lamports(&anchor.pubkey()),
        before - anchor_rent - 10_000
    );
    assert_eq!(env.pool_state().listed, 0);
    let a: AnchorAccount = env.read(&env.anchor_account_of(&anchor.pubkey()));
    assert_eq!(a.owed, 0);
}

#[test]
fn test_a_claimed_listing_is_not_cancelled() {
    let mut env = Env::new();
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    let listing = env.list(&bill);
    env.claim(&listing, &bill).unwrap();
    let anchor = env.anchor.insecure_clone();
    let ix = env.cancel_ix(&anchor.pubkey(), &listing);
    env.send_expecting_error(&[ix], &[&anchor], "AlreadyClaimed");
}

#[test]
fn test_an_unclaimed_listing_is_not_repaid() {
    let mut env = Env::new();
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    let listing = env.list(&bill);
    let anchor = env.anchor.insecure_clone();
    env.fund_usdc(&anchor.pubkey(), 10_000 * USDC);
    let ix = env.repay_ix(&anchor.pubkey(), &listing);
    env.send_expecting_error(&[ix], &[&anchor], "NotClaimed");
}

#[test]
fn test_only_the_anchor_repays_or_cancels() {
    let mut env = Env::new();
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    let listing = env.list(&bill);
    let stranger = env.wallet();
    let ix = env.cancel_ix(&stranger.pubkey(), &listing);
    assert!(env.send(&[ix], &[&stranger]).is_err());
}

#[test]
fn test_a_claim_twice_is_refused() {
    let mut env = Env::new();
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    let listing = env.list(&bill);
    env.claim(&listing, &bill).unwrap();
    // Refused by the commitment's claim record, before the listing's own
    // flag is read.
    assert!(
        env.claim(&listing, &bill).is_err(),
        "a second claim succeeded"
    );
    assert_eq!(env.usdc_balance(&env.supplier.pubkey()), 9_850 * USDC);
}

#[test]
fn test_a_claimed_leaf_listed_by_another_wallet_pays_nothing_again() {
    let mut env = Env::new();
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    let listing = env.list(&bill);
    env.claim(&listing, &bill).unwrap();
    // The claim revealed the salt; the leaf is still in the latest root.
    let stranger = env.wallet();
    let proven = env.post_bill(&bill);
    let ix = env.list_ix(&stranger.pubkey(), bill.commitment, proven);
    env.send(&[ix], &[&stranger]).unwrap();
    let theirs = env.listing_of(&stranger.pubkey(), &bill.commitment);
    assert!(env.claim(&theirs, &bill).is_err(), "a replayed claim paid");
    assert_eq!(env.usdc_balance(&env.supplier.pubkey()), 9_850 * USDC);
}

#[test]
fn test_a_repaid_leaf_listed_again_pays_nothing_again() {
    let mut env = Env::new();
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    let listing = env.list(&bill);
    env.claim(&listing, &bill).unwrap();
    let anchor = env.anchor.insecure_clone();
    env.fund_usdc(&anchor.pubkey(), 10_000 * USDC);
    let ix = env.repay_ix(&anchor.pubkey(), &listing);
    env.send(&[ix], &[&anchor]).unwrap();
    // Listed again at the same address, the old signature and salt replayed.
    let again = env.list(&bill);
    assert_eq!(again, listing);
    assert!(env.claim(&again, &bill).is_err(), "a replayed claim paid");
    assert_eq!(env.usdc_balance(&env.supplier.pubkey()), 9_850 * USDC);
}

#[test]
fn test_a_claim_without_the_salt_is_refused() {
    let mut env = Env::new();
    env.deposit(100_000 * USDC);
    let mut bill = env.bill(10_000);
    let listing = env.list(&bill);
    bill.salt = [1; 32];
    match env.claim(&listing, &bill) {
        Ok(_) => panic!("a claim with the wrong salt succeeded"),
        Err(e) => assert!(e.meta.logs.join("\n").contains("WrongSupplier")),
    }
}

#[test]
fn test_a_claim_without_the_suppliers_signature_is_refused() {
    let mut env = Env::new();
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    let listing = env.list(&bill);
    let fee_payer = env.fee_payer.insecure_clone();
    let supplier = env.supplier.pubkey();

    // No signature at all.
    let ix = env.claim_ix(&listing, &supplier, bill.salt);
    env.send_expecting_error(&[ix], &[&fee_payer], "ClaimNotSigned");

    // Someone else's signature over the claim message.
    let other = solana_keypair::Keypair::new();
    let ixs = [
        Env::ed25519_ix(&other, &claim_message(&listing)),
        env.claim_ix(&listing, &supplier, bill.salt),
    ];
    env.send_expecting_error(&ixs, &[&fee_payer], "ClaimNotSigned");

    // The supplier's signature over another listing's message.
    let ixs = [
        Env::ed25519_ix(&env.supplier, &claim_message(&fee_payer.pubkey())),
        env.claim_ix(&listing, &supplier, bill.salt),
    ];
    env.send_expecting_error(&ixs, &[&fee_payer], "ClaimNotSigned");
}

#[test]
fn test_a_claim_after_the_repayment_date_is_refused() {
    let mut env = Env::new();
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    let listing = env.list(&bill);
    env.set_time(bill.leaf.repay_on);
    match env.claim(&listing, &bill) {
        Ok(_) => panic!("a late claim succeeded"),
        Err(e) => assert!(e.meta.logs.join("\n").contains("RepaymentDatePassed")),
    }
}

#[test]
fn test_a_claim_the_pool_cannot_pay_waits() {
    let mut env = Env::with_params(|p| p.max_owed_per_anchor = u64::MAX);
    let depositor = env.deposit(10_000 * USDC);
    let bill = env.bill(10_000);
    let listing = env.list(&bill);
    // The depositor takes half out before the supplier claims.
    let shares = env.shares_of(&depositor.pubkey()) / 2;
    let ix = env.withdraw_ix(&depositor.pubkey(), shares);
    env.send(&[ix], &[&depositor]).unwrap();
    match env.claim(&listing, &bill) {
        Ok(_) => panic!("a claim the pool can't pay succeeded"),
        Err(e) => assert!(e.meta.logs.join("\n").contains("PoolShort")),
    }
    env.deposit(10_000 * USDC);
    env.claim(&listing, &bill).unwrap();
}

#[test]
fn test_a_listing_unrepaid_past_the_pools_days_is_written_off_and_repaid_back() {
    let mut env = Env::new();
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    let listing = env.list(&bill);
    // Anyone: a stranger's wallet pays for it.
    let keeper = env.wallet();
    let ix = env.write_off_ix(&listing);
    env.send_expecting_error(&[ix], &[&keeper], "NotClaimed");
    env.claim(&listing, &bill).unwrap();
    let nav_before = {
        let p = env.pool_state();
        p.idle + p.outstanding
    };

    // 29 days late: not yet.
    env.set_time(bill.leaf.repay_on + 29 * DAY);
    let ix = env.write_off_ix(&listing);
    env.send_expecting_error(&[ix], &[&keeper], "NotYetOverdue");

    // 30 days late: anyone writes it off, and NAV stops counting it.
    env.set_time(bill.leaf.repay_on + 30 * DAY);
    let ix = env.write_off_ix(&listing);
    env.send(&[ix], &[&keeper]).unwrap();
    let pool = env.pool_state();
    assert_eq!(pool.outstanding, 0);
    assert_eq!(pool.written_off, 10_000 * USDC);
    assert_eq!(pool.idle + pool.outstanding, nav_before - 10_000 * USDC);
    let ix = env.write_off_ix(&listing);
    env.send_expecting_error(&[ix], &[&keeper], "AlreadyWrittenOff");

    // The anchor still owes it; its repayment counts again.
    let anchor = env.anchor.insecure_clone();
    env.fund_usdc(&anchor.pubkey(), 10_000 * USDC);
    let ix = env.repay_ix(&anchor.pubkey(), &listing);
    env.send(&[ix], &[&anchor]).unwrap();
    let pool = env.pool_state();
    assert_eq!(pool.written_off, 0);
    assert_eq!(pool.idle + pool.outstanding, nav_before);
}

#[test]
fn test_a_withdrawal_is_paid_only_from_idle() {
    let mut env = Env::with_params(|p| p.max_owed_per_anchor = u64::MAX);
    let depositor = env.deposit(10_000 * USDC);
    let bill = env.bill(10_000);
    let listing = env.list(&bill);
    env.claim(&listing, &bill).unwrap();
    let shares = env.shares_of(&depositor.pubkey());
    let ix = env.withdraw_ix(&depositor.pubkey(), shares);
    env.send_expecting_error(&[ix], &[&depositor], "NotEnoughIdle");
}

#[test]
fn test_a_bill_missing_a_required_document_is_refused() {
    let mut env = Env::with_params(|p| {
        p.rules.required_documents = DOCUMENT_PURCHASE_ORDER;
    });
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    expect_listing_refused(&mut env, &bill, "DocumentsNotVerified");
}

#[test]
fn test_a_vendor_too_new_or_too_small_is_refused() {
    let mut env = Env::with_params(|p| p.rules.min_vendor_tenure = 4);
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    expect_listing_refused(&mut env, &bill, "VendorTenureTooShort");

    let mut env = Env::with_params(|p| p.rules.min_vendor_volume = 3);
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    expect_listing_refused(&mut env, &bill, "VendorVolumeTooLow");
}

#[test]
fn test_a_term_past_the_pools_longest_is_refused() {
    let mut env = Env::with_params(|p| p.rules.max_term_days = 45);
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    expect_listing_refused(&mut env, &bill, "TermTooLong");
}

#[test]
fn test_an_anchor_a_bespoke_pool_doesnt_name_is_refused() {
    let mut env = Env::with_params(|p| p.anchors = vec![solana_keypair::Keypair::new().pubkey()]);
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    expect_listing_refused(&mut env, &bill, "AnchorNotTaken");
}

#[test]
fn test_a_bespoke_pool_takes_the_anchors_it_names() {
    let mut env = Env::without_pool();
    let mut params = demo_params(env.oracle.pubkey());
    params.anchors = vec![env.anchor.pubkey()];
    let ix = env.create_pool_ix(params);
    let creator = env.creator.insecure_clone();
    env.send(&[ix], &[&creator]).unwrap();
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    env.list(&bill);
}

#[test]
fn test_listings_past_the_cap_per_anchor_are_refused() {
    let mut env = Env::new();
    // However much the pool holds, an anchor owes it at most 15,000: the
    // first fits, the second doesn't.
    env.deposit(1_000_000 * USDC);
    let first = env.bill(10_000);
    env.list(&first);
    let second = env.bill_to(&env.supplier.pubkey(), [7; 32], 10_000);
    expect_listing_refused(&mut env, &second, "OverAnchorCap");
}

#[test]
fn test_a_leaf_for_another_commitment_or_network_is_refused() {
    let mut env = Env::new();
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    let proven = env.post_bill(&bill);
    let anchor = env.anchor.insecure_clone();
    let ix = env.list_ix(&anchor.pubkey(), [9; 32], proven);
    env.send_expecting_error(&[ix], &[&anchor], "WrongCommitment");

    let mut mainnet = env.bill(10_000);
    mainnet.leaf.network = padded("sol");
    expect_listing_refused(&mut env, &mainnet, "WrongNetwork");

    let mut euro = env.bill(10_000);
    euro.leaf.currency = padded("FIAT_EUR");
    expect_listing_refused(&mut env, &euro, "WrongCurrency");
}

#[test]
fn test_a_changed_leaf_fails_its_proof() {
    let mut env = Env::new();
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    let mut proven = env.post_bill(&bill);
    // The anchor claims the bill is twice as large.
    proven.leaf[74..82].copy_from_slice(&(20_000 * USDC).to_le_bytes());
    let anchor = env.anchor.insecure_clone();
    let ix = env.list_ix(&anchor.pubkey(), bill.commitment, proven);
    env.send_expecting_error(&[ix], &[&anchor], "InvalidProof");
}

#[test]
fn test_only_the_signers_latest_fresh_root_is_read() {
    let mut env = Env::new();
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    let old = env.post_bill(&bill);
    env.set_time(env.now + 60);
    let other = env.bill_to(&env.supplier.pubkey(), [8; 32], 1_000);
    env.post_bill(&other);
    let anchor = env.anchor.insecure_clone();
    let ix = env.list_ix(&anchor.pubkey(), bill.commitment, old);
    env.send_expecting_error(&[ix], &[&anchor], "RootNotLatest");

    let fresh = env.post_bill(&bill);
    env.set_time(env.now + 3 * DAY);
    let ix = env.list_ix(&anchor.pubkey(), bill.commitment, fresh);
    env.send_expecting_error(&[ix], &[&anchor], "RootTooOld");
}

#[test]
fn test_a_root_from_a_signer_the_pool_doesnt_trust_is_refused() {
    let mut env = Env::new();
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    let impostor = env.wallet();
    let proven = env.post_bill_as(&impostor, &bill);
    let anchor = env.anchor.insecure_clone();
    let ix = env.list_ix_with_roots(
        &anchor.pubkey(),
        bill.commitment,
        proven,
        &impostor.pubkey(),
    );
    env.send_expecting_error(&[ix], &[&anchor], "UnknownSigner");
}

#[test]
fn test_a_token_snapshot_is_not_a_bill() {
    let mut env = Env::new();
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    let mut proven = env.post_bill(&bill);
    // The snapshot is the tree's first leaf; prove it in the bill's place.
    let tree_leaf_0 = {
        use snapshot_roots::token_snapshot::Snapshot;
        Snapshot {
            version: 2,
            network: padded("sol-devnet"),
            ..Snapshot::default()
        }
        .to_bytes()
    };
    let bill_hash = snapshot_roots::token_snapshot::merkle::hash_leaf(&proven.leaf);
    proven.proof[0] = bill_hash;
    proven.leaf = tree_leaf_0;
    let anchor = env.anchor.insecure_clone();
    let ix = env.list_ix(&anchor.pubkey(), bill.commitment, proven);
    env.send_expecting_error(&[ix], &[&anchor], "NotABillLeaf");
}

#[test]
fn test_someone_else_listing_the_leaf_leaves_the_anchors_listing_free() {
    let mut env = Env::new();
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    let stranger = env.wallet();
    let proven = env.post_bill(&bill);
    let ix = env.list_ix(&stranger.pubkey(), bill.commitment, proven.clone());
    env.send(&[ix], &[&stranger]).unwrap();
    let anchor = env.anchor.insecure_clone();
    let ix = env.list_ix(&anchor.pubkey(), bill.commitment, proven);
    env.send(&[ix], &[&anchor]).unwrap();
    assert_ne!(
        env.listing_of(&stranger.pubkey(), &bill.commitment),
        env.listing_of(&anchor.pubkey(), &bill.commitment)
    );
}

#[test]
fn test_invalid_parameters_are_refused() {
    for (customize, name) in [
        (
            Box::new(|p: &mut supplier_pool::PoolParams| p.signers.clear())
                as Box<dyn Fn(&mut supplier_pool::PoolParams)>,
            "InvalidSigners",
        ),
        (
            Box::new(|p: &mut supplier_pool::PoolParams| p.fee = FeeSchedule::Flat { bps: 10_001 }),
            "InvalidFee",
        ),
        (
            Box::new(|p: &mut supplier_pool::PoolParams| p.max_owed_per_anchor = 0),
            "InvalidCap",
        ),
        (
            Box::new(|p: &mut supplier_pool::PoolParams| p.rules.min_vendor_tenure = 5),
            "InvalidBand",
        ),
        (
            Box::new(|p: &mut supplier_pool::PoolParams| {
                p.business_rules.max_written_off_in_year = 5
            }),
            "InvalidBand",
        ),
        (
            Box::new(|p: &mut supplier_pool::PoolParams| p.business_rules.max_days_overdue = 6),
            "InvalidBand",
        ),
    ] {
        let mut env = Env::without_pool();
        let mut params = demo_params(env.oracle.pubkey());
        customize(&mut params);
        let ix = env.create_pool_ix(params);
        let creator = env.creator.insecure_clone();
        env.send_expecting_error(&[ix], &[&creator], name);
    }
}

fn expect_listing_refused(env: &mut Env, bill: &Bill, name: &str) {
    let proven = env.post_bill(bill);
    let anchor = env.anchor.insecure_clone();
    let ix = env.list_ix(&anchor.pubkey(), bill.commitment, proven);
    env.send_expecting_error(&[ix], &[&anchor], name);
}

#[test]
fn test_every_listing_event_names_its_anchor_and_commitment() {
    // An indexer that reads only a pool's history finds each listing's bill
    // from its events alone, without reading the listing's account.
    let mut env = Env::new();
    env.deposit(100_000 * USDC);
    let anchor = env.anchor.insecure_clone();

    let other = env.bill_to(&env.supplier.pubkey(), [7u8; 32], 1_000);
    let other_listing = env.list(&other);
    let ix = env.cancel_ix(&anchor.pubkey(), &other_listing);
    let cancelled = env.send(&[ix], &[&anchor]).unwrap();
    let e: Vec<ListingCancelled> = events_of(&cancelled.logs);
    assert_eq!(
        (e.len(), e[0].listing, e[0].anchor, e[0].commitment),
        (1, other_listing, anchor.pubkey(), other.commitment)
    );

    let bill = env.bill(10_000);
    let listing = env.list(&bill);
    let claimed = env.claim(&listing, &bill).unwrap();
    let e: Vec<BillClaimed> = events_of(&claimed.logs);
    assert_eq!(
        (e.len(), e[0].listing, e[0].anchor, e[0].commitment),
        (1, listing, anchor.pubkey(), bill.commitment)
    );

    env.set_time(bill.leaf.repay_on + 30 * DAY);
    let keeper = env.wallet();
    let ix = env.write_off_ix(&listing);
    let written_off = env.send(&[ix], &[&keeper]).unwrap();
    let e: Vec<ListingWrittenOff> = events_of(&written_off.logs);
    assert_eq!(
        (e.len(), e[0].listing, e[0].anchor, e[0].commitment),
        (1, listing, anchor.pubkey(), bill.commitment)
    );

    env.fund_usdc(&anchor.pubkey(), 10_000 * USDC);
    let ix = env.repay_ix(&anchor.pubkey(), &listing);
    let repaid = env.send(&[ix], &[&anchor]).unwrap();
    let e: Vec<ListingRepaid> = events_of(&repaid.logs);
    assert_eq!(
        (e.len(), e[0].listing, e[0].anchor, e[0].commitment),
        (1, listing, anchor.pubkey(), bill.commitment)
    );
}

fn list_with_record(record: AsOfFacts, name: Option<&str>) {
    list_with_record_in(Env::new(), record, name);
}

fn list_with_record_in(mut env: Env, record: AsOfFacts, name: Option<&str>) {
    env.deposit(100_000 * USDC);
    let anchor = env.anchor.insecure_clone();
    env.set_record(&anchor.pubkey(), record);
    let bill = env.bill(10_000);
    match name {
        Some(name) => expect_listing_refused(&mut env, &bill, name),
        None => {
            env.list(&bill);
        }
    }
}

#[test]
fn test_each_part_of_the_anchors_record_is_held_to_its_rule() {
    use snapshot_roots::token_snapshot::{
        COUNT_ONE, COUNT_TEN_OR_MORE, COUNT_TWO_TO_FOUR, DAYS_OVERDUE_1_TO_7, DAYS_OVERDUE_8_TO_30,
    };
    // At the pool's band, each lists; one band past it, each is refused
    // naming its rule (demo_params: 1, 1, 0, 0, any).
    let at = AsOfFacts {
        overdue_unpaid: COUNT_ONE,
        most_days_overdue: DAYS_OVERDUE_1_TO_7,
        repaid_late_in_year: COUNT_TEN_OR_MORE,
        ..AsOfFacts::default()
    };
    list_with_record(at.clone(), None);
    type Worsen = fn(&mut AsOfFacts);
    let cases: [(Worsen, &str); 4] = [
        (
            |r| r.overdue_unpaid = COUNT_TWO_TO_FOUR,
            "TooManyOverdueUnpaid",
        ),
        (
            |r| r.most_days_overdue = DAYS_OVERDUE_8_TO_30,
            "TooManyDaysOverdue",
        ),
        (
            |r| r.written_off_unrepaid = COUNT_ONE,
            "TooManyWrittenOffUnrepaid",
        ),
        (
            |r| r.written_off_in_year = COUNT_ONE,
            "TooManyWrittenOffInYear",
        ),
    ];
    for (worsen, name) in cases {
        let mut record = at.clone();
        worsen(&mut record);
        list_with_record(record, Some(name));
    }
    let strict = Env::with_params(|p| p.business_rules.max_repaid_late_in_year = COUNT_ONE);
    let late = AsOfFacts {
        repaid_late_in_year: COUNT_TWO_TO_FOUR,
        ..AsOfFacts::default()
    };
    list_with_record_in(strict, late, Some("TooManyRepaidLateInYear"));
}

#[test]
fn test_a_pool_whose_business_rules_are_all_zero_takes_only_a_clean_record() {
    use snapshot_roots::token_snapshot::{COUNT_ONE, DAYS_OVERDUE_1_TO_7};
    // A pool created before the rules reads zeros where they now are.
    let zero = BusinessRules {
        max_overdue_unpaid: 0,
        max_days_overdue: 0,
        max_written_off_unrepaid: 0,
        max_written_off_in_year: 0,
        max_repaid_late_in_year: 0,
    };
    list_with_record_in(
        Env::with_params(|p| p.business_rules = zero),
        AsOfFacts::default(),
        None,
    );
    let one_late_day = AsOfFacts {
        overdue_unpaid: COUNT_ONE,
        most_days_overdue: DAYS_OVERDUE_1_TO_7,
        ..AsOfFacts::default()
    };
    list_with_record_in(
        Env::with_params(|p| p.business_rules = zero),
        one_late_day,
        Some("TooManyOverdueUnpaid"),
    );
}

#[test]
fn test_a_wallet_whose_business_never_published_cant_list() {
    let mut env = Env::new();
    env.deposit(100_000 * USDC);
    let anchor = env.anchor.insecure_clone();
    env.forget_business(&anchor.pubkey());
    let bill = env.bill(10_000);
    expect_listing_refused(&mut env, &bill, "InvalidProof");
}

#[test]
fn test_another_wallets_business_leaf_is_refused() {
    let mut env = Env::new();
    env.deposit(100_000 * USDC);
    let clean = env.wallet();
    let anchor = env.anchor.insecure_clone();
    env.set_record(
        &anchor.pubkey(),
        AsOfFacts {
            written_off_unrepaid: snapshot_roots::token_snapshot::COUNT_ONE,
            ..AsOfFacts::default()
        },
    );
    let bill = env.bill(10_000);
    let proven = env.post_bill(&bill);
    // The defaulted anchor hands in another business's clean leaf.
    let theirs = env.business_proof(&clean.pubkey());
    let ix = env.list_ix_with(
        &anchor.pubkey(),
        bill.commitment,
        proven,
        theirs,
        &env.oracle.pubkey(),
    );
    env.send_expecting_error(&[ix], &[&anchor], "WrongBusiness");
}

#[test]
fn test_a_business_leaf_without_a_record_is_refused() {
    let mut env = Env::new();
    env.deposit(100_000 * USDC);
    let anchor = env.anchor.insecure_clone();
    env.set_version_1_leaf(&anchor.pubkey());
    let bill = env.bill(10_000);
    expect_listing_refused(&mut env, &bill, "NoSupplierPoolRecord");
}

#[test]
fn test_the_bill_leaf_cant_stand_in_for_the_business_leaf() {
    let mut env = Env::new();
    env.deposit(100_000 * USDC);
    let bill = env.bill(10_000);
    let proven = env.post_bill(&bill);
    let anchor = env.anchor.insecure_clone();
    let business = supplier_pool::facts::LeafProof {
        leaf: proven.leaf.clone(),
        proof: proven.proof.clone(),
    };
    let ix = env.list_ix_with(
        &anchor.pubkey(),
        bill.commitment,
        proven,
        business,
        &env.oracle.pubkey(),
    );
    env.send_expecting_error(&[ix], &[&anchor], "NotABusinessLeaf");
}
