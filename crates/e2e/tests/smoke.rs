#![allow(clippy::unwrap_used, clippy::expect_used)]
use sies_chain::smt::SmtStateStore;
use sies_e2e::asserts::{assert_ledger_seq, assert_round_trip_smt, assert_smt_root_eq};
use sies_e2e::fixtures::sample_entries;
use sies_e2e::harness::TestNode;

/// Cold-start: an empty ledger must produce the same SMT root as a fresh
/// `SmtStateStore` (the genesis root).
#[test]
fn cold_start_produces_genesis_smt() {
    let node = TestNode::start().expect("cold start");
    let genesis = SmtStateStore::new().root_hash();
    assert_smt_root_eq(&node, genesis, "cold-start genesis");
}

/// Restart with an empty ledger must still yield the genesis SMT root.
#[test]
fn restart_empty_ledger_stays_genesis() {
    let mut node = TestNode::start().expect("cold start");
    let genesis = SmtStateStore::new().root_hash();

    assert_round_trip_smt(&mut node, genesis, "post-restart empty ledger");
}

/// Append entries, rebuild SMT, then restart and verify the rebuilt SMT
/// root matches the pre-restart root (incremental persistence round-trip).
#[test]
fn incremental_append_then_restart_preserves_smt_root() {
    let mut node = TestNode::start().expect("cold start");

    node.append_entries(&sample_entries(5, 1))
        .expect("append batch");
    assert_ledger_seq(&node, 5, "after append");

    node.rebuild_smt().expect("rebuild after append");
    let root_before = node.smt_root();

    assert_round_trip_smt(&mut node, root_before, "after restart + rebuild");
}

/// Multiple rebuild cycles without new appends must be idempotent.
#[test]
fn repeated_rebuild_is_idempotent() {
    let mut node = TestNode::start().expect("cold start");

    node.append_entries(&sample_entries(3, 1))
        .expect("append batch");
    node.rebuild_smt().expect("first rebuild");
    let root = node.smt_root();

    node.rebuild_smt().expect("second rebuild");
    assert_smt_root_eq(&node, root, "second rebuild");

    node.rebuild_smt().expect("third rebuild");
    assert_smt_root_eq(&node, root, "third rebuild idempotent");
}
