use sies_chain::smt::SmtStateStore;
use sies_e2e::fixtures::sample_entry;
use sies_e2e::harness::TestNode;

/// Cold-start: an empty ledger must produce the same SMT root as a fresh
/// in-memory SmtStateStore (genesis state).
#[test]
fn cold_start_produces_genesis_smt() {
    let node = TestNode::start().expect("cold start");

    // Empty ledger → seq 0
    assert_eq!(node.last_seq().unwrap(), 0);

    // SMT root must match a brand-new empty store
    let genesis_root = SmtStateStore::new().root_hash();
    assert_eq!(
        node.smt_root(),
        genesis_root,
        "cold-start SMT root diverges from genesis"
    );
}

/// Restarting a node with an empty ledger must also yield genesis state.
#[test]
fn restart_empty_ledger_stays_genesis() {
    let mut node = TestNode::start().expect("cold start");
    node.restart().expect("restart");

    assert_eq!(node.last_seq().unwrap(), 0);
    assert_eq!(
        node.smt_root(),
        SmtStateStore::new().root_hash(),
        "post-restart empty ledger SMT root diverges from genesis"
    );
}

/// Append entries, rebuild SMT, then restart and verify the rebuilt SMT
/// root matches the pre-restart root (incremental persistence round-trip).
#[test]
fn incremental_append_then_restart_preserves_smt_root() {
    let mut node = TestNode::start().expect("cold start");

    // Append 5 sample entries in one batch
    let entries: Vec<_> = (0..5u64).map(|i| sample_entry(i + 1, 1, 0, 1, 1)).collect();
    node.append_entries(&entries).expect("append batch");
    assert_eq!(node.last_seq().unwrap(), 5);

    // Rebuild SMT from current ledger
    node.rebuild_smt().expect("rebuild after append");
    let root_before = node.smt_root();

    // Restart (drops in-memory SMT, re-reads ledger from disk)
    node.restart().expect("restart");
    assert_eq!(node.last_seq().unwrap(), 5, "ledger seq lost after restart");

    // Rebuild again — must produce identical root
    node.rebuild_smt().expect("rebuild after restart");
    assert_eq!(
        node.smt_root(),
        root_before,
        "SMT root diverged after restart + rebuild"
    );
}

/// Multiple rebuild cycles without new appends must be idempotent.
#[test]
fn repeated_rebuild_is_idempotent() {
    let mut node = TestNode::start().expect("cold start");

    let entries: Vec<_> = (0..3u64).map(|i| sample_entry(i + 1, 1, 0, 1, 1)).collect();
    node.append_entries(&entries).expect("append batch");

    node.rebuild_smt().expect("first rebuild");
    let root = node.smt_root();

    node.rebuild_smt().expect("second rebuild");
    assert_eq!(node.smt_root(), root);

    node.rebuild_smt().expect("third rebuild");
    assert_eq!(node.smt_root(), root, "rebuild not idempotent");
}
