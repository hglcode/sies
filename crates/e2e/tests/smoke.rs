use sies_chain::smt::SmtStateStore;
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
