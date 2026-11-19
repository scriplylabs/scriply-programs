//! Pins docs/snapshot-vectors.json, which a signer or keeper written in
//! another language (Scriply's services are Go) checks itself against.
//! After a deliberate change, regenerate it with
//! `UPDATE_SNAPSHOT_VECTORS=1 cargo test -p token-snapshot --test vectors`.
//! docs/bill-leaf-vectors.json does the same for bill leaves, in a tree
//! that also holds tokens' snapshots, as a signer's daily tree does, and
//! docs/business-leaf-vectors.json for business leaves (layout version 2)
//! beside snapshots in layout version 4 and a bill leaf.

use serde_json::{json, Value};
use token_snapshot::{
    merkle::Tree, AsOfFacts, BillLeaf, BusinessFacts, BusinessLeaf, Snapshot, COUNT_NONE,
    COUNT_ONE, COUNT_TEN_OR_MORE, COUNT_TWO_TO_FOUR, DAYS_OVERDUE_NONE, DAYS_OVERDUE_OVER_90,
    DOCUMENT_PURCHASE_ORDER, DOCUMENT_VENDOR_INVOICE, NEVER_PUBLISHED,
};

fn vector_snapshots() -> Vec<Snapshot> {
    (1..=5u8)
        .map(|i| {
            let mut network = [0u8; 16];
            network[..10].copy_from_slice(b"sol-devnet");
            Snapshot {
                version: 2,
                network,
                mint: [i; 32],
                as_of: 1_800_000_000 + i as i64,
                report_hash: [0xa0 + i; 32],
                network_supply: 1_000_000 * i as u64,
                outstanding_units: 2_000_000 * i as u64,
                burn_debt: 1_000 * i as u64,
                burn_debt_overdue: i == 3,
                aging_current: 100 * i as u64,
                aging_days_1_to_30: 200 * i as u64,
                aging_days_31_to_60: 300 * i as u64,
                aging_days_61_to_90: 400 * i as u64,
                aging_over_90: 500 * i as u64,
                concentration_index_bps: 1_000 + i as u16,
                disputed_share_bps: 10 * i as u16,
                weighted_days_to_due: 10 + i as u16,
                weighted_days_to_pay: 30 + i as u16,
                weighted_days_beyond_terms: i as u16,
                paid_on_time_share_bps: 9_000 - 100 * i as u16,
                history_share_bps: 8_000 + 100 * i as u16,
                weighted_days_in_burn_debt: 2 * i as u16,
                days_burn_debt_overdue: i as u16,
            }
        })
        .collect()
}

fn build_vectors() -> Value {
    let snapshots: Vec<Vec<u8>> = vector_snapshots().iter().map(Snapshot::to_bytes).collect();
    let tree = Tree::build(&snapshots).unwrap();
    let leaves: Vec<Value> = snapshots
        .iter()
        .enumerate()
        .map(|(i, s)| {
            json!({
                "snapshot": hex::encode(s),
                "leaf": hex::encode(token_snapshot::merkle::hash_leaf(s)),
                "proof": tree.prove(i).iter().map(hex::encode).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({
        "description": "Snapshots in layout version 2, their leaves, the tree's root and each leaf's proof (siblings from the bottom up). All hex.",
        "root": hex::encode(tree.root()),
        "leaves": leaves,
    })
}

#[test]
fn test_vectors_match_docs() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/snapshot-vectors.json"
    );
    let built = build_vectors();
    if std::env::var("UPDATE_SNAPSHOT_VECTORS").is_ok() {
        let text = serde_json::to_string_pretty(&built).unwrap() + "\n";
        std::fs::write(path, text).unwrap();
        return;
    }
    let pinned: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(built, pinned);
}

fn vector_bill_leaves() -> Vec<BillLeaf> {
    (1..=3u8)
        .map(|i| {
            let mut network = [0u8; 16];
            network[..10].copy_from_slice(b"sol-devnet");
            let mut currency = [0u8; 16];
            currency[..8].copy_from_slice(b"FIAT_USD");
            BillLeaf {
                version: 2,
                network,
                commitment: [0x10 + i; 32],
                as_of: 1_800_000_000,
                currency,
                amount: 1_000_000_000 * i as u64,
                due_date: 1_801_000_000 + i as i64,
                repay_on: 1_803_000_000 + i as i64,
                documents_verified: if i == 2 {
                    DOCUMENT_VENDOR_INVOICE
                } else {
                    DOCUMENT_VENDOR_INVOICE | DOCUMENT_PURCHASE_ORDER
                },
                documents_hash: [0xc0 + i; 32],
                vendor_tenure: i,
                vendor_volume: i + 1,
                vendor_history_vouched_bps: 0,
                anchor: Some([0xd0 + i; 32]),
            }
        })
        .collect()
}

fn build_bill_vectors() -> Value {
    let mut leaves: Vec<Vec<u8>> = vector_snapshots()
        .iter()
        .take(2)
        .map(Snapshot::to_bytes)
        .collect();
    leaves.extend(vector_bill_leaves().iter().map(BillLeaf::to_bytes));
    let tree = Tree::build(&leaves).unwrap();
    let out: Vec<Value> = leaves
        .iter()
        .enumerate()
        .map(|(i, l)| {
            json!({
                "kind": if i < 2 { "snapshot" } else { "bill" },
                "bytes": hex::encode(l),
                "leaf": hex::encode(token_snapshot::merkle::hash_leaf(l)),
                "proof": tree.prove(i).iter().map(hex::encode).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({
        "description": "Two token snapshots (layout version 2) and three bill leaves (layout version 2) in one tree, in this order: each leaf's bytes, its hash, the tree's root and each leaf's proof (siblings from the bottom up). All hex.",
        "root": hex::encode(tree.root()),
        "leaves": out,
    })
}

#[test]
fn test_bill_vectors_match_docs() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/bill-leaf-vectors.json"
    );
    let built = build_bill_vectors();
    if std::env::var("UPDATE_SNAPSHOT_VECTORS").is_ok() {
        let text = serde_json::to_string_pretty(&built).unwrap() + "\n";
        std::fs::write(path, text).unwrap();
        return;
    }
    let pinned: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(built, pinned);
}

fn vector_business_facts(i: u8) -> BusinessFacts {
    BusinessFacts {
        period_end_exclusive: 1_798_761_600 + 86_400 * i as i64,
        commitment: [0xe0 + i; 32],
        current_ratio_bps: Some(15_000 + 1_000 * i as u32),
        quick_ratio_bps: Some(11_000 + 500 * i as u32),
        cash_to_current_assets_bps: Some(3_000 + i as u32),
        cash_to_current_liabilities_bps: (i != 2).then_some(4_000 + i as u32),
        liabilities_to_assets_bps: Some(5_000 - 100 * i as u32),
        operating_margin_bps: (i != 2).then_some(-300 * i as i32),
        receivables_turnover_bps: Some(60_000 + 10_000 * i as u32),
        cash_agreed_to_bank_bps: Some(9_900 + i as u32),
        receivables_tied_to_invoices_bps: (i != 2).then_some(10_000),
        receivables_confirmed_bps: (i != 2).then_some(5_000 + i as u32),
        as_of_facts: Some(vector_as_of_facts(i)),
    }
}

/// The first business repaid on time and has nothing open; the second has a
/// listing written off and unrepaid, long overdue, and no bank cash now.
fn vector_as_of_facts(i: u8) -> AsOfFacts {
    if i == 1 {
        return AsOfFacts {
            bank_cash_now_to_current_liabilities_bps: Some(6_500),
            supplier_finance_to_current_liabilities_bps: Some(1_200),
            established_vendor_share_bps: Some(8_000),
            overdue_unpaid: COUNT_NONE,
            most_days_overdue: DAYS_OVERDUE_NONE,
            written_off_unrepaid: COUNT_NONE,
            written_off_in_year: COUNT_NONE,
            repaid_in_year: COUNT_TEN_OR_MORE,
            repaid_late_in_year: COUNT_NONE,
        };
    }
    AsOfFacts {
        bank_cash_now_to_current_liabilities_bps: None,
        supplier_finance_to_current_liabilities_bps: Some(4_100),
        established_vendor_share_bps: Some(0),
        overdue_unpaid: COUNT_ONE,
        most_days_overdue: DAYS_OVERDUE_OVER_90,
        written_off_unrepaid: COUNT_ONE,
        written_off_in_year: COUNT_ONE,
        repaid_in_year: COUNT_TWO_TO_FOUR,
        repaid_late_in_year: COUNT_ONE,
    }
}

fn build_business_vectors() -> Value {
    let mut network = [0u8; 16];
    network[..10].copy_from_slice(b"sol-devnet");
    // Two tokens' snapshots in layout version 4: the first's business
    // published, the second's never did (every fact 0).
    let mut leaves: Vec<Vec<u8>> = vector_snapshots()
        .into_iter()
        .take(2)
        .enumerate()
        .map(|(i, s)| {
            let mut b = Snapshot { version: 4, ..s }.to_bytes();
            if i == 0 {
                b.extend_from_slice(&vector_business_facts(1).to_bytes());
            } else {
                b.extend_from_slice(&NEVER_PUBLISHED);
            }
            b
        })
        .collect();
    leaves.push(vector_bill_leaves()[0].to_bytes());
    leaves.extend((1..=2u8).map(|i| {
        BusinessLeaf {
            version: 2,
            network,
            wallet: [0x40 + i; 32],
            as_of: 1_800_000_000,
            facts: vector_business_facts(i),
        }
        .to_bytes()
    }));
    let kinds = ["snapshot", "snapshot", "bill", "business", "business"];
    let tree = Tree::build(&leaves).unwrap();
    let out: Vec<Value> = leaves
        .iter()
        .enumerate()
        .map(|(i, l)| {
            json!({
                "kind": kinds[i],
                "bytes": hex::encode(l),
                "leaf": hex::encode(token_snapshot::merkle::hash_leaf(l)),
                "proof": tree.prove(i).iter().map(hex::encode).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({
        "description": "Two token snapshots in layout version 4 (the second's business never published), a bill leaf (layout version 2) and two business leaves (layout version 2) in one tree, in this order: each leaf's bytes, its hash, the tree's root and each leaf's proof (siblings from the bottom up). All hex.",
        "root": hex::encode(tree.root()),
        "leaves": out,
    })
}

#[test]
fn test_business_vectors_match_docs() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/business-leaf-vectors.json"
    );
    let built = build_business_vectors();
    if std::env::var("UPDATE_SNAPSHOT_VECTORS").is_ok() {
        let text = serde_json::to_string_pretty(&built).unwrap() + "\n";
        std::fs::write(path, text).unwrap();
        return;
    }
    let pinned: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(built, pinned);
}
