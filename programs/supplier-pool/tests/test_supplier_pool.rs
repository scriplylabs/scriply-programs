//! The supplier pool end to end in LiteSVM: deposits, a bill listed on its
//! proven leaf, the supplier's claim, the anchor's repayment, and every
//! rule that refuses. Build the programs first (`anchor build`).

mod common;

use common::*;
use snapshot_roots::token_snapshot::DOCUMENT_PURCHASE_ORDER;
use solana_signer::Signer;
use supplier_pool::{
    claim_signature::claim_message,
    state::{AnchorAccount, FeeSchedule, Listing},
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
    let proven = env.post_bill(&bill);
    let stranger = env.wallet();
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
    let proven = env.post_bill(&bill);
    let stranger = env.wallet();
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
