mod common;

use {
    anchor_lang::prelude::Pubkey,
    anchor_spl::{
        associated_token::get_associated_token_address_with_program_id as ata, token::spl_token,
    },
    common::*,
    receivables_pool::{
        snapshot::ProvenSnapshot,
        state::{DepositorAttestation, RateMode},
        SAS_PROGRAM_ID,
    },
    solana_keypair::Keypair,
    solana_signer::Signer,
};

/// A holder with `units` of the token and a USDC account, and their loan
/// vault thawed.
fn borrower(env: &mut Env, units: u64) -> Keypair {
    let holder = env.wallet();
    env.mint_units(&holder.pubkey(), units);
    env.fund_usdc(&holder.pubkey(), 0);
    env.thaw_vault(&holder.pubkey());
    holder
}

fn borrow(env: &mut Env, holder: &Keypair, units: u64, principal: u64, maturity_epochs: u16) {
    let proven = env.post_token();
    let ixs = [
        env.settle_ix(&holder.pubkey()),
        env.borrow_ix(&holder.pubkey(), units, principal, maturity_epochs, proven),
    ];
    env.send(&ixs, &[holder]).unwrap();
}

#[test]
fn test_adr_example_end_to_end() {
    // An org pledges 1000 units, borrows 500 USDC at 12% for 13
    // weeks, its customers pay, the units are bought back at face, and the
    // pool keeps 514.79 and returns 485.21.
    let mut env = Env::new();
    let depositor = env.depositor(2_000 * USDC);
    let org = borrower(&mut env, 1_000 * UNIT);

    borrow(&mut env, &org, 1_000 * UNIT, 500 * USDC, 13);
    assert_eq!(env.usdc_balance(&org.pubkey()), 500 * USDC);
    assert_eq!(
        env.balance(&env.collateral_vault(&org.pubkey())),
        1_000 * UNIT
    );
    let loan = env.loan_state(&org.pubkey());
    assert!(loan.is_open);
    assert_eq!(
        (
            loan.principal,
            loan.units,
            loan.maturity_epoch,
            loan.rate_bps
        ),
        (500 * USDC, 1_000 * UNIT, 13, 1_200)
    );
    assert_eq!(env.exposure_state().principal, 500 * USDC);

    env.set_time(T0 + 90 * DAY);
    env.buy_back(&org.pubkey(), 1_000 * UNIT, 1_000 * USDC);
    let ixs = [
        env.settle_ix(&env.creator.pubkey()),
        env.sync_loan_ix(&org.pubkey()),
    ];
    let creator = env.creator.insecure_clone();
    env.send(&ixs, &[&creator]).unwrap();

    assert_eq!(env.usdc_balance(&org.pubkey()), 500 * USDC + 485_205_480);
    assert!(!env.loan_state(&org.pubkey()).is_open);
    assert_eq!(env.exposure_state().principal, 0);
    let pool = env.pool_state();
    assert_eq!(pool.idle, 2_014_794_520);
    assert_eq!(pool.nav(env.now).unwrap(), 2_014_794_520);

    // No loan is left to mark, so the next settle finishes the epoch, and
    // the depositor leaves with the interest at the next epoch's start.
    env.settle();
    let shares = env.shares_of(&depositor.pubkey());
    let ix = env.request_withdrawal_ix(&depositor.pubkey(), shares);
    env.send(&[ix], &[&depositor]).unwrap();
    env.set_time(T0 + 13 * EPOCH);
    env.settle();
    let ix = env.claim_withdrawal_ix(&depositor.pubkey(), 0);
    env.send(&[ix], &[&depositor]).unwrap();
    let paid = env.usdc_balance(&depositor.pubkey());
    assert!(
        (2_014_794_519..=2_014_794_520).contains(&paid),
        "paid {paid}"
    );
}

#[test]
fn test_repaying_returns_the_units_and_frees_the_token() {
    let mut env = Env::new();
    env.depositor(2_000 * USDC);
    let org = borrower(&mut env, 1_000 * UNIT);
    borrow(&mut env, &org, 1_000 * UNIT, 500 * USDC, 4);

    // Repaying needs the epoch opened, not its marking finished. The issuer
    // has frozen the borrower's account, which doesn't stop the repayment.
    env.set_time(T0 + 10 * DAY);
    env.fund_usdc(&org.pubkey(), 10 * USDC);
    env.freeze_token_account(&org.pubkey());
    let ixs = [
        env.settle_ix(&org.pubkey()),
        env.repay_ix(&org.pubkey(), &org.pubkey(), u64::MAX),
    ];
    env.send(&ixs, &[&org]).unwrap();

    // 500 at 12% for 10 days is 1.643835 of interest.
    assert_eq!(env.usdc_balance(&org.pubkey()), 510 * USDC - 501_643_835);
    assert!(!env.loan_state(&org.pubkey()).is_open);
    let pool = env.pool_state();
    assert_eq!((pool.performing_loans(), pool.marked_loans), (0, 0));

    // The units wait in the loan's vault until the borrower withdraws them,
    // which waits for the account to be thawed.
    let vault = env.collateral_vault(&org.pubkey());
    assert_eq!(env.balance(&vault), 1_000 * UNIT);
    let ix = env.withdraw_units_ix(&org.pubkey());
    assert!(env.send(&[ix], &[&org]).is_err());
    env.thaw_token_account(&org.pubkey());
    let ix = env.withdraw_units_ix(&org.pubkey());
    env.send(&[ix], &[&org]).unwrap();
    assert_eq!(env.balance(&vault), 0);
    assert_eq!(env.units_of(&org.pubkey()), 1_000 * UNIT);

    // The same loan account takes the borrower's next loan.
    borrow(&mut env, &org, 1_000 * UNIT, 100 * USDC, 4);
    assert!(env.loan_state(&org.pubkey()).is_open);
}

#[test]
fn test_units_sent_to_a_vault_before_its_first_loan_dont_block_it() {
    // The vault is thawed before its loan exists, so anyone holding the
    // token can send it units; no loan exists to withdraw them through.
    let mut env = Env::new();
    env.depositor(2_000 * USDC);
    let org = borrower(&mut env, 1_000 * UNIT);
    let holder = env.wallet();
    env.mint_units(&holder.pubkey(), 1);
    let vault = env.collateral_vault(&org.pubkey());
    env.send_units(&holder, &vault, 1);

    borrow(&mut env, &org, 1_000 * UNIT, 500 * USDC, 4);

    // They're pledged with the units borrowed against, and go back to the
    // borrower with them.
    assert_eq!(env.loan_state(&org.pubkey()).units, 1_000 * UNIT + 1);
}

#[test]
fn test_proceeds_go_only_to_the_pools_own_vault() {
    let mut env = Env::new();
    env.depositor(2_000 * USDC);
    let org = borrower(&mut env, 1_000 * UNIT);
    borrow(&mut env, &org, 1_000 * UNIT, 500 * USDC, 4);

    // A buy-back's USDC waits in the loan's proceeds account.
    let proceeds = ata(&env.loan(&org.pubkey()), &env.usdc, &spl_token::ID);
    env.fund_usdc(&env.loan(&org.pubkey()), 600 * USDC);
    assert_eq!(env.balance(&proceeds), 600 * USDC);

    // An account the pool's authority owns, but not its vault, is refused.
    let authority = env.authority;
    let side = env.side_usdc_account(&authority);
    let keeper = env.wallet();
    let ix = env.sync_loan_ix_into(&org.pubkey(), side);
    env.send_expecting_error(&[ix], &[&keeper], "ConstraintAssociated");
    assert_eq!(env.balance(&proceeds), 600 * USDC);
}

#[test]
fn test_several_holders_borrow_against_one_token_within_its_cap() {
    let mut env = Env::new();
    env.depositor(2_000 * USDC);
    let a = borrower(&mut env, 1_000 * UNIT);
    let b = borrower(&mut env, 1_000 * UNIT);
    borrow(&mut env, &a, 1_000 * UNIT, 600 * USDC, 4);

    // The cap is 50% of NAV across both loans: 600 + 500 is over it.
    let proven = env.post_token();
    let ix = env.borrow_ix(&b.pubkey(), 1_000 * UNIT, 500 * USDC, 4, proven);
    env.send_expecting_error(&[ix], &[&b], "ExceedsTokenCap");

    borrow(&mut env, &b, 1_000 * UNIT, 400 * USDC, 4);
    assert_eq!(env.exposure_state().principal, 1_000 * USDC);
    assert_eq!(env.pool_state().performing_loans(), 2);
    assert_eq!(env.usdc_balance(&b.pubkey()), 400 * USDC);
}

#[test]
fn test_borrowing_is_refused_without_good_facts() {
    let mut env = Env::new();
    env.depositor(2_000 * USDC);
    let org = borrower(&mut env, 2_000 * UNIT);
    let attempt = |env: &mut Env,
                   proven: ProvenSnapshot,
                   units: u64,
                   principal: u64,
                   maturity: u16,
                   error: &str| {
        let ixs = [
            env.settle_ix(&org.pubkey()),
            env.borrow_ix(&org.pubkey(), units, principal, maturity, proven),
        ];
        env.send_expecting_error(&ixs, &[&org], error);
    };

    // A root posted by a key the pool doesn't trust.
    let stranger = env.wallet();
    let s = env.snapshot();
    let posted = env.post_as(&stranger, &[s]);
    let ix = env.borrow_ix_with_roots(
        &org.pubkey(),
        1_000 * UNIT,
        100 * USDC,
        4,
        posted.proven(0),
        roots_of(&stranger.pubkey()),
    );
    env.send_expecting_error(&[ix], &[&org], "UnknownSigner");

    let mut proven = env.post_token();
    proven.root = [7; 32];
    attempt(
        &mut env,
        proven,
        1_000 * UNIT,
        100 * USDC,
        4,
        "RootNotLatest",
    );

    // A root still kept and fresh enough, but not the latest, can't step
    // around newer facts: here, burn debt that went overdue.
    let kinder = env.post_token();
    let mut s = env.snapshot();
    s.burn_debt_overdue = true;
    env.post_with(s);
    attempt(
        &mut env,
        kinder,
        1_000 * UNIT,
        100 * USDC,
        4,
        "RootNotLatest",
    );

    // A keeper can't change a fact under a real root.
    let mut proven = env.post_token();
    proven.snapshot[156] = 0xff;
    attempt(
        &mut env,
        proven,
        1_000 * UNIT,
        100 * USDC,
        4,
        "InvalidProof",
    );

    let mut s = env.snapshot();
    s.burn_debt_overdue = true;
    let proven = env.post_with(s);
    attempt(
        &mut env,
        proven,
        1_000 * UNIT,
        100 * USDC,
        4,
        "BurnDebtOverdue",
    );

    let mut s = env.snapshot();
    s.network_supply -= 1;
    let proven = env.post_with(s);
    attempt(
        &mut env,
        proven,
        1_000 * UNIT,
        100 * USDC,
        4,
        "MintedSinceSnapshot",
    );

    let mut s = env.snapshot();
    s.network = [0; 16];
    s.network[..3].copy_from_slice(b"sol");
    let proven = env.post_with(s);
    attempt(
        &mut env,
        proven,
        1_000 * UNIT,
        100 * USDC,
        4,
        "WrongNetwork",
    );

    let mut s = env.snapshot();
    s.mint = Pubkey::new_unique().to_bytes();
    let proven = env.post_with(s);
    attempt(&mut env, proven, 1_000 * UNIT, 100 * USDC, 4, "WrongMint");

    // 61-90 days past due counts for nothing, but isn't late until 90.
    let mut s = env.snapshot();
    s.aging_current = 0;
    s.aging_days_61_to_90 = s.outstanding_units;
    let proven = env.post_with(s);
    attempt(
        &mut env,
        proven,
        1_000 * UNIT,
        100 * USDC,
        4,
        "NoEligibleUnits",
    );

    // More than half the units over 90 days past due.
    let mut s = env.snapshot();
    s.aging_over_90 = s.outstanding_units / 100 * 51;
    s.aging_current = s.outstanding_units - s.aging_over_90;
    let proven = env.post_with(s);
    attempt(&mut env, proven, 1_000 * UNIT, 100 * USDC, 4, "TooMuchLate");

    // Due in 20 days and paid 10 days late: a 4-week term ends too soon.
    let mut s = env.snapshot();
    s.weighted_days_beyond_terms = 10;
    let proven = env.post_with(s);
    attempt(
        &mut env,
        proven,
        1_000 * UNIT,
        100 * USDC,
        4,
        "TermTooShort",
    );

    // 1000 units of clean facts advance 80%.
    let proven = env.post_token();
    attempt(
        &mut env,
        proven,
        1_000 * UNIT,
        801 * USDC,
        4,
        "ExceedsAdvance",
    );

    // The cap is 50% of a 2000 USDC pool.
    let proven = env.post_token();
    attempt(
        &mut env,
        proven,
        2_000 * UNIT,
        1_001 * USDC,
        4,
        "ExceedsTokenCap",
    );

    let proven = env.post_token();
    attempt(
        &mut env,
        proven,
        1_000 * UNIT,
        100 * USDC,
        5,
        "MaturityNotOffered",
    );

    // The pool reads roots up to two days old.
    let proven = env.post_token();
    env.set_time(env.now + 2 * DAY + 61);
    attempt(&mut env, proven, 1_000 * UNIT, 100 * USDC, 4, "RootTooOld");

    // A new epoch must be settled first.
    env.set_time(T0 + EPOCH);
    let proven = env.post_token();
    let ix = env.borrow_ix(&org.pubkey(), 1_000 * UNIT, 100 * USDC, 4, proven);
    env.send_expecting_error(&[ix], &[&org], "EpochNotSettled");
}

#[test]
fn test_the_risk_engine_sets_the_advance_and_rate_from_the_facts() {
    let mut env = Env::new();
    env.depositor(2_000 * USDC);
    let org = borrower(&mut env, 1_000 * UNIT);
    let mut s = env.snapshot();
    // 60% current, 20% 1-30 days, 10% 31-60 days (counted half), 10% over
    // 90 days (not counted), and 5% disputed: 80% eligible.
    s.aging_current = 600 * UNIT;
    s.aging_days_1_to_30 = 200 * UNIT;
    s.aging_days_31_to_60 = 100 * UNIT;
    s.aging_over_90 = 100 * UNIT;
    s.disputed_share_bps = 500;

    let proven = env.post_with(s.clone());
    let ix = env.borrow_ix(&org.pubkey(), 1_000 * UNIT, 641 * USDC, 4, proven);
    env.send_expecting_error(&[ix], &[&org], "ExceedsAdvance");

    let proven = env.post_with(s);
    let ix = env.borrow_ix(&org.pubkey(), 1_000 * UNIT, 640 * USDC, 4, proven);
    env.send(&[ix], &[&org]).unwrap();
    let loan = env.loan_state(&org.pubkey());
    // 12% base, plus 6% x 40% past due, plus 10% x 5% disputed.
    assert_eq!((loan.rate_bps, loan.late_rate_bps), (1_490, 1_790));
}

#[test]
fn test_every_loan_is_marked_before_withdrawals_are_paid() {
    let mut env = Env::new();
    let depositor = env.depositor(2_000 * USDC);
    let org = borrower(&mut env, 1_000 * UNIT);
    borrow(&mut env, &org, 1_000 * UNIT, 500 * USDC, 13);
    let shares = env.shares_of(&depositor.pubkey());
    let ix = env.request_withdrawal_ix(&depositor.pubkey(), shares / 4);
    env.send(&[ix], &[&depositor]).unwrap();

    // The epoch opens, but nothing waiting on its marks may run yet.
    env.set_time(T0 + EPOCH);
    env.settle();
    assert_eq!(env.pool_state().reserved, 0, "withdrawals wait for marks");
    let late = env.wallet();
    env.fund_usdc(&late.pubkey(), 100 * USDC);
    let ix = env.deposit_ix(&late.pubkey(), 100 * USDC, None);
    env.send_expecting_error(&[ix], &[&late], "EpochNotSettled");

    // A keeper can't mark from an older root than the signer's latest, even
    // one still kept and fresh enough.
    let mut s = env.snapshot();
    let older = env.post_with(s.clone());
    s.report_hash = [10; 32];
    let newer = env.post_with(s);
    let creator = env.creator.insecure_clone();
    let ix = env.mark_loans_ix(vec![newer], &[&org.pubkey()]);
    env.send(&[ix], &[&creator]).unwrap();
    let ix = env.mark_loans_ix(vec![older], &[&org.pubkey()]);
    env.send_expecting_error(&[ix], &[&creator], "RootNotLatest");

    // Marked, the epoch's withdrawals are paid and deposits reopen.
    env.settle();
    assert!(env.pool_state().reserved > 0);
    let ix = env.deposit_ix(&late.pubkey(), 100 * USDC, None);
    env.send(&[ix], &[&late]).unwrap();

    // A later mark that finds burn debt overdue writes the loan down before
    // its maturity: NAV counts it at the impaired share, 0 here.
    let mut s = env.snapshot();
    s.burn_debt_overdue = true;
    let proven = env.post_with(s);
    let ix = env.mark_loans_ix(vec![proven], &[&org.pubkey()]);
    env.send(&[ix], &[&creator]).unwrap();
    let loan = env.loan_state(&org.pubkey());
    assert!(loan.is_written_down);
    let pool = env.pool_state();
    assert_eq!(pool.nav(env.now).unwrap(), pool.idle);
}

#[test]
fn test_a_loan_nobody_can_mark_is_marked_stale_after_the_window() {
    let mut env = Env::with_params(|p| p.impaired_share_bps = 2_000);
    env.depositor(2_000 * USDC);
    let org = borrower(&mut env, 1_000 * UNIT);
    borrow(&mut env, &org, 1_000 * UNIT, 500 * USDC, 13);
    let creator = env.creator.insecure_clone();

    env.set_time(T0 + EPOCH);
    env.settle();
    let ix = env.mark_stale_ix(&[&org.pubkey()]);
    env.send_expecting_error(&[ix], &[&creator], "MarkWindowOpen");

    // A day in, the window has closed: the loan counts at 20% for the rest
    // of the epoch.
    env.set_time(T0 + EPOCH + DAY);
    let full = env.pool_state().nav(env.now).unwrap();
    let owed = env.loan_state(&org.pubkey()).owed(env.now).unwrap();
    let ix = env.mark_stale_ix(&[&org.pubkey()]);
    env.send(&[ix], &[&creator]).unwrap();
    let ix = env.mark_stale_ix(&[&org.pubkey()]);
    env.send_expecting_error(&[ix], &[&creator], "AlreadyMarked");
    let pool = env.pool_state();
    assert_eq!(pool.nav(env.now).unwrap(), full - (owed - owed / 5));

    // The epoch settles without the token's facts, and deposits reopen.
    env.settle();
    let d = env.depositor(10 * USDC);
    assert!(env.shares_of(&d.pubkey()) > 0);

    // A fresh mark after the deposits doesn't lift NAV this epoch, so
    // nobody can buy shares at the discount and then clear it.
    let low = env.pool_state().nav(env.now).unwrap();
    let proven = env.post_token();
    let ix = env.mark_loans_ix(vec![proven], &[&org.pubkey()]);
    env.send(&[ix], &[&creator]).unwrap();
    assert_eq!(env.pool_state().nav(env.now).unwrap(), low);

    // Read again next epoch, the discount goes.
    env.set_time(T0 + 2 * EPOCH);
    env.settle_and_mark(&[&org.pubkey()]);
    let pool = env.pool_state();
    assert_eq!(pool.stale_discount, 0);
    let owed = env.loan_state(&org.pubkey()).owed(env.now).unwrap();
    assert_eq!(pool.nav(env.now).unwrap(), pool.idle + owed);
}

#[test]
fn test_withdrawals_wait_while_the_pool_is_lent_out() {
    // The whole pool may be lent against one token here.
    let mut env = Env::with_params(|p| p.cap_per_token_bps = 10_000);
    let depositor_b = env.depositor(1_000 * USDC);
    let org_b = borrower(&mut env, 1_000 * UNIT);
    borrow(&mut env, &org_b, 1_000 * UNIT, 600 * USDC, 4);

    let shares = env.shares_of(&depositor_b.pubkey());
    let ix = env.request_withdrawal_ix(&depositor_b.pubkey(), shares);
    env.send(&[ix], &[&depositor_b]).unwrap();

    // Before the epoch's start nothing is settled to claim: its generation
    // has no record yet.
    let ix = env.claim_withdrawal_ix(&depositor_b.pubkey(), 0);
    env.send_expecting_error(&[ix], &[&depositor_b], "AccountNotInitialized");

    // Only 400 is idle at the next epoch's start, so 400 is paid. A keeper
    // claims it for the depositor, which queues the unpaid shares again.
    env.set_time(T0 + EPOCH);
    env.settle_and_mark(&[&org_b.pubkey()]);
    let pool = env.pool_state();
    assert_eq!((pool.idle, pool.reserved), (0, 400 * USDC));
    assert_eq!((pool.withdrawal_generation, pool.pending_shares), (1, 0));
    let keeper = env.wallet();
    let ix = env.claim_withdrawal_ix(&depositor_b.pubkey(), 0);
    env.send(&[ix], &[&keeper]).unwrap();
    assert_eq!(env.usdc_balance(&depositor_b.pubkey()), 400 * USDC);
    let request = env.request_state(&depositor_b.pubkey());
    assert_eq!(request.generation, 1);
    assert_eq!(env.pool_state().pending_shares, request.shares);

    // Once the loan is repaid, the next epoch's start pays the rest.
    env.fund_usdc(&org_b.pubkey(), 10 * USDC);
    let ixs = [env.repay_ix(&org_b.pubkey(), &org_b.pubkey(), u64::MAX)];
    env.send(&ixs, &[&org_b]).unwrap();
    env.set_time(T0 + 2 * EPOCH);
    env.settle();
    assert_eq!(env.pool_state().withdrawal_generation, 2);
    let ix = env.claim_withdrawal_ix(&depositor_b.pubkey(), 1);
    env.send(&[ix], &[&depositor_b]).unwrap();
    assert!(env
        .svm
        .get_account(&env.request(&depositor_b.pubkey()))
        .is_none_or(|a| a.lamports == 0));
    // 1000 back, plus a week's interest on 600 at 12% (1.380821), less rounding.
    let paid = env.usdc_balance(&depositor_b.pubkey());
    assert!(
        (1_001_380_819..=1_001_380_821).contains(&paid),
        "paid {paid}"
    );
}

#[test]
fn test_withdrawals_paid_nearly_in_full_twice_lose_nothing() {
    // The review's case: settlements just short of the queue, twice, used
    // to wear the queue's index down until a later request claimed 0.
    let mut env = Env::with_params(|p| p.cap_per_token_bps = 10_000);
    let a = env.depositor(1_000 * USDC);
    let org = borrower(&mut env, 1_000 * UNIT);
    borrow(&mut env, &org, 1_000 * UNIT, 600 * USDC, 4);
    let keeper = env.wallet();

    // A asks for a little more than the 400 idle.
    let shares = env.shares_of(&a.pubkey()) * 401 / 1_000;
    let ix = env.request_withdrawal_ix(&a.pubkey(), shares);
    env.send(&[ix], &[&a]).unwrap();
    env.set_time(T0 + EPOCH);
    env.settle_and_mark(&[&org.pubkey()]);
    let ix = env.claim_withdrawal_ix(&a.pubkey(), 0);
    env.send(&[ix], &[&keeper]).unwrap();
    assert_eq!(env.usdc_balance(&a.pubkey()), 400 * USDC);

    // Half a dollar comes back: short again.
    env.fund_usdc(&org.pubkey(), USDC / 2);
    let ixs = [env.repay_ix(&org.pubkey(), &org.pubkey(), USDC / 2)];
    env.send(&ixs, &[&org]).unwrap();
    env.set_time(T0 + 2 * EPOCH);
    env.settle_and_mark(&[&org.pubkey()]);
    let ix = env.claim_withdrawal_ix(&a.pubkey(), 1);
    env.send(&[ix], &[&keeper]).unwrap();
    assert_eq!(env.usdc_balance(&a.pubkey()), 400 * USDC + USDC / 2);
    assert!(env.request_state(&a.pubkey()).shares > 0);

    // C deposits 300 and asks for all of it; the loan is repaid, so the next
    // start pays everyone in full.
    let c = env.depositor(300 * USDC);
    let ix = env.request_withdrawal_ix(&c.pubkey(), env.shares_of(&c.pubkey()));
    env.send(&[ix], &[&c]).unwrap();
    env.fund_usdc(&org.pubkey(), 10 * USDC);
    let ixs = [env.repay_ix(&org.pubkey(), &org.pubkey(), u64::MAX)];
    env.send(&ixs, &[&org]).unwrap();
    env.set_time(T0 + 3 * EPOCH);
    env.settle();
    for owner in [&a, &c] {
        let ix = env.claim_withdrawal_ix(&owner.pubkey(), 2);
        env.send(&[ix], &[&keeper]).unwrap();
    }
    let paid_c = env.usdc_balance(&c.pubkey());
    assert!(
        (299 * USDC..=300 * USDC).contains(&paid_c),
        "C paid {paid_c}"
    );
    let paid_a = env.usdc_balance(&a.pubkey());
    assert!(
        (401 * USDC..402 * USDC).contains(&paid_a),
        "A paid {paid_a}"
    );
    assert!(env.pool_state().reserved < 10, "dust only");
}

#[test]
fn test_a_loan_past_maturity_and_grace_is_impaired() {
    let mut env = Env::new();
    env.depositor(2_000 * USDC);
    let org = borrower(&mut env, 1_000 * UNIT);
    borrow(&mut env, &org, 1_000 * UNIT, 500 * USDC, 4);

    // Maturity is epoch 4 and grace 2 epochs, so epoch 6's start impairs it.
    env.set_time(T0 + 5 * EPOCH);
    env.settle_and_mark(&[&org.pubkey()]);
    assert!(env.pool_state().nav(env.now).unwrap() > 2_000 * USDC);
    env.set_time(T0 + 6 * EPOCH);
    env.settle();
    let pool = env.pool_state();
    assert_eq!(
        pool.nav(env.now).unwrap(),
        1_500 * USDC,
        "counted at a share of 0"
    );

    // A later buy-back adds what it pays to NAV.
    env.buy_back(&org.pubkey(), 1_000 * UNIT, 1_000 * USDC);
    let creator = env.creator.insecure_clone();
    let ix = env.sync_loan_ix(&org.pubkey());
    env.send(&[ix], &[&creator]).unwrap();
    let owed = env.pool_state().idle - 1_500 * USDC;
    assert!(owed > 500 * USDC && owed < 510 * USDC, "owed {owed}");
    assert!(!env.loan_state(&org.pubkey()).is_open);
}

#[test]
fn test_create_pool_refuses_a_rate_mode_not_built() {
    let mut env = Env::without_pool();
    let mut params = devnet_params(env.oracle.pubkey());
    params.rate_mode = RateMode::Variable;
    let ix = env.create_pool_ix(params);
    let creator = env.creator.insecure_clone();
    env.send_expecting_error(&[ix], &[&creator], "RateModeNotSupported");

    // The devnet config as committed creates a pool, trusting a key nobody
    // holds until it is replaced.
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../pools/devnet.toml");
    let ix = env.create_pool_ix(pool_config::load(path).unwrap());
    env.send(&[ix], &[&creator]).unwrap();
    let pool = env.pool_state();
    assert_eq!(pool.version, receivables_pool::ACCOUNT_VERSION);
    assert_eq!(pool.risk.max_advance_bps, 9_000);
}

/// A SAS attestation account for `wallet`, expiring at `expiry`.
fn attestation_data(credential: &Pubkey, schema: &Pubkey, wallet: &Pubkey, expiry: i64) -> Vec<u8> {
    let mut data = vec![2u8];
    data.extend_from_slice(wallet.as_ref());
    data.extend_from_slice(credential.as_ref());
    data.extend_from_slice(schema.as_ref());
    data.extend_from_slice(&4u32.to_le_bytes());
    data.extend_from_slice(&[0, 1, 2, 3]);
    data.extend_from_slice(Pubkey::new_unique().as_ref());
    data.extend_from_slice(&expiry.to_le_bytes());
    data.extend_from_slice(Pubkey::default().as_ref());
    data
}

#[test]
fn test_a_pool_can_require_depositors_to_be_attested() {
    let credential = Pubkey::new_unique();
    let schema = Pubkey::new_unique();
    let mut env = Env::with_params(|p| {
        p.depositor_attestation = Some(DepositorAttestation { credential, schema })
    });
    let d = env.wallet();
    env.fund_usdc(&d.pubkey(), 100 * USDC);

    let ix = env.deposit_ix(&d.pubkey(), 100 * USDC, None);
    env.send_expecting_error(&[ix], &[&d], "AttestationRequired");

    let address = Pubkey::find_program_address(
        &[
            b"attestation",
            credential.as_ref(),
            schema.as_ref(),
            d.pubkey().as_ref(),
        ],
        &SAS_PROGRAM_ID,
    )
    .0;
    let put = |env: &mut Env, expiry: i64| {
        let data = attestation_data(&credential, &schema, &d.pubkey(), expiry);
        env.svm
            .set_account(
                address,
                solana_account::Account {
                    lamports: 1_000_000_000,
                    data,
                    owner: SAS_PROGRAM_ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
    };

    put(&mut env, T0 - 1);
    let ix = env.deposit_ix(&d.pubkey(), 100 * USDC, Some(address));
    env.send_expecting_error(&[ix], &[&d], "InvalidAttestation");

    put(&mut env, T0 + 365 * DAY);
    let ix = env.deposit_ix(&d.pubkey(), 100 * USDC, Some(address));
    env.send(&[ix], &[&d]).unwrap();
    assert!(env.shares_of(&d.pubkey()) > 0);
}
