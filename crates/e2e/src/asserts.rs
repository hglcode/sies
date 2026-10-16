use crate::harness::TestNode;

/// Assert SMT root equals expected, printing hex on failure.
#[track_caller]
pub fn assert_smt_root_eq(node: &TestNode, expected: [u8; 32], msg: &str) {
    let actual = node.smt_root();
    assert_eq!(
        actual,
        expected,
        "{}: SMT root mismatch\n  expected: {}\n  actual:   {}",
        msg,
        hex::encode(expected),
        hex::encode(actual),
    );
}

/// Assert ledger last_seq equals expected with diagnostic context.
#[track_caller]
pub fn assert_ledger_seq(node: &TestNode, expected: u64, msg: &str) {
    let actual = node.last_seq().expect("last_seq should not fail");
    assert_eq!(actual, expected, "{}: ledger seq mismatch", msg);
}

/// Restart node, rebuild SMT, and assert root matches `expected`.
/// Encapsulates the common restart→rebuild→assert pattern.
#[track_caller]
pub fn assert_round_trip_smt(node: &mut TestNode, expected: [u8; 32], msg: &str) {
    node.restart().expect("restart should not fail");
    node.rebuild_smt()
        .expect("rebuild after restart should not fail");
    assert_smt_root_eq(node, expected, msg);
}
