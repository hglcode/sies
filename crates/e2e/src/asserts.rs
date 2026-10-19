#![allow(clippy::unwrap_used, clippy::expect_used)]
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

/// Assert ledger `last_seq` equals expected with diagnostic context.
#[track_caller]
pub fn assert_ledger_seq(node: &TestNode, expected: u64, msg: &str) {
    let actual = node.last_seq().expect("last_seq should not fail");
    assert_eq!(actual, expected, "{msg}: ledger seq mismatch");
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

/// Assert SMT root equals expected hex string, with full diagnostic on failure.
///
/// The primary assertion helper for golden-vector tests. On failure it prints
/// the actual root as a `const` declaration ready for copy-paste, streamlining
/// the initial golden-value capture workflow:
///
/// ```text
/// panicked at: golden: SMT root mismatch
///   expected: 0000...0000
///   actual:   abcd...1234
///
///   // Update golden value:
///   const GOLDEN: &str = "abcd...1234";
/// ```
#[track_caller]
pub fn assert_smt_root_hex(node: &TestNode, expected_hex: &str, msg: &str) {
    let actual = node.smt_root();
    let actual_hex = hex::encode(actual);
    let expected_hex = expected_hex.to_lowercase();
    assert_eq!(
        actual_hex, expected_hex,
        "{msg}: SMT root mismatch\n  expected: {expected_hex}\n  actual:   {actual_hex}\n\n  // Update golden value:\n  const GOLDEN: &str = \"{actual_hex}\";",
    );
}
