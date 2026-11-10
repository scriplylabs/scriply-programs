//! Pins docs/snapshot-vectors.json, which a signer or keeper written in
//! another language (Scriply's services are Go) checks itself against.
//! After a deliberate change, regenerate it with
//! `UPDATE_SNAPSHOT_VECTORS=1 cargo test -p token-snapshot --test vectors`.

use serde_json::{json, Value};
use token_snapshot::{merkle::Tree, Snapshot};

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
