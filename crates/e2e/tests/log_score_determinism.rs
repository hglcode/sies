//! E2E golden-vector tests for **Log Score bit-level determinism**.
//!
//! ## What these tests verify
//!
//! The complete pipeline from `LedgerEntry` construction → postcard binary
//! serialization → CRC32-guarded file storage → SMT rebuild → SHA-256 root
//! hash produces **identical bytes** across:
//!
//! * repeated runs on the same machine
//! * restart (simulated crash + cold recovery)
//! * multiple rebuild cycles (idempotency)
//! * incremental vs batch append ordering
//!
//! ## Why this is sufficient for cross-platform safety
//!
//! Every component in the pipeline is already inherently deterministic:
//!
//! | Layer | Mechanism | Safety guarantee |
//! |-------|-----------|-----------------|
//! | Decimal construction | `Decimal::new(i64, u32)` / `Decimal::from(u64)` | Exact `(i128, u32)` representation, no f64 |
//! | Serialization | **postcard** binary format | Canonical byte-for-byte encoding |
//! | Storage | `[len_le32][payload][crc32_le32]` | Platform-independent LE encoding |
//! | SMT hashing | SHA-256 over sorted key-value pairs | Cryptographic determinism |
//!
//! These tests lock in the current correct output as **golden vectors**.
//! Any future change that introduces non-determinism (e.g. accidental `f64`,
//! `HashMap` iteration order, `SystemTime` leaks) will fail immediately.
//!
//! ## Golden value capture workflow
//!
//! 1. Set `GOLDEN_*` constants to all-zeros placeholder
//! 2. Run tests — they fail with the actual hex printed
//! 3. Copy the `const GOLDEN: &str = "..."` from the panic output
//! 4. Replace the placeholder → re-run → green
//!
//! ## Cross-platform verification (requires CI matrix — task #26)
//!
//! Once golden values are captured on x86_64-linux, run these same tests on
//! aarch64-linux in CI. Identical golden values across both targets confirm
//! true bit-level determinism.

use sies_e2e::asserts::{
    assert_ledger_seq, assert_round_trip_smt, assert_smt_root_eq, assert_smt_root_hex,
};
use sies_e2e::fixtures::{log_score_boundary_entries, log_score_entries, log_score_stress_batch};
use sies_e2e::harness::TestNode;

// ─────────────────────────────────────────────────────────────────────────────
// Golden vectors
//
// Each value was captured by running the test on the reference platform,
// copying the hex from the panic output, and pasting it here.
//
// If you change LedgerEntry fields, postcard version, or SMT hashing logic,
// these MUST be re-captured and reviewed for correctness (not just blindly
// updated).
// ─────────────────────────────────────────────────────────────────────────────

/// Golden SMT root after appending `log_score_entries()` (4 entries).
///
/// TODO(#29): Run once to capture, then replace:
///   cargo test -p sies-e2e --test log_score_determinism log_score_golden_cold_start -- --nocapture
const GOLDEN_LOG_SCORE_ROOT: &str =
    "c5c70ac3240c64f606a7630e11d8354b5c83bb9de058c48981ad455582682d58";

/// Golden SMT root after appending `log_score_boundary_entries()` (3 entries).
/// Exercises ε-smoothing paths: p=0, p=ε, and uniform 50/50.
const GOLDEN_LOG_SCORE_BOUNDARY_ROOT: &str =
    "731a21e329529c63c2ed3cc5a5bf789f9a1a92aa53fd4da49e75ffe6bf268ad5";

/// Golden SMT root after appending `log_score_stress_batch()` (20 entries).
const GOLDEN_LOG_SCORE_STRESS_ROOT: &str =
    "9a9153b78e22f6de88c07a64f16e215ff17e17999db6b0ed07c4992af3474392";

// ─────────────────────────────────────────────────────────────────────────────
// Golden vector tests
// ─────────────────────────────────────────────────────────────────────────────

/// Cold-start: append log-score entries and verify the SMT root matches the
/// golden vector. This is the **primary determinism gate** — if this test
/// passes on two different platforms, bit-level determinism is confirmed.
#[test]
fn log_score_golden_cold_start() {
    let mut node = TestNode::start().expect("cold start");

    let entries = log_score_entries();
    node.append_entries(&entries)
        .expect("append log-score entries");
    assert_ledger_seq(&node, 4, "after 4 log-score entries");

    node.rebuild_smt().expect("rebuild after append");
    assert_smt_root_hex(&node, GOLDEN_LOG_SCORE_ROOT, "golden log-score cold-start");
}

/// Restart + rebuild preserves the golden SMT root.
/// Verifies the persistence round-trip: disk → postcard deserialize → SMT
/// produces the same root as the in-memory first build.
#[test]
fn log_score_golden_restart_preserves_root() {
    let mut node = TestNode::start().expect("cold start");

    node.append_entries(&log_score_entries())
        .expect("append log-score entries");
    node.rebuild_smt().expect("rebuild");

    // Restart simulates crash + recovery
    assert_round_trip_smt(
        &mut node,
        hex_to_bytes(GOLDEN_LOG_SCORE_ROOT),
        "golden log-score post-restart",
    );
}

/// Multiple rebuild cycles produce the same golden root (idempotency).
#[test]
fn log_score_rebuild_idempotent() {
    let mut node = TestNode::start().expect("cold start");

    node.append_entries(&log_score_entries()).expect("append");
    node.rebuild_smt().expect("first rebuild");
    let root = node.smt_root();

    for i in 2..=5 {
        node.rebuild_smt()
            .unwrap_or_else(|_| panic!("rebuild #{i}"));
        assert_smt_root_eq(&node, root, &format!("rebuild #{i} idempotent"));
    }

    // Also match golden
    assert_smt_root_hex(&node, GOLDEN_LOG_SCORE_ROOT, "golden after 5 rebuilds");
}

/// Boundary entries (ε-smoothing edge cases) produce a stable golden root.
///
/// This specifically targets the `max(p, ε)` logic in `Scorer::log_score`:
/// * Entry 1: p = 0 → smoothed to ε = 1e-12 → ln(ε) ≈ −27.63
/// * Entry 2: p = 1e-12 = ε → no smoothing → same ln(ε)
/// * Entry 3: p = 0.5000 (scale 4) → ln(0.5) ≈ −0.693
#[test]
fn log_score_boundary_golden() {
    let mut node = TestNode::start().expect("cold start");

    let entries = log_score_boundary_entries();
    node.append_entries(&entries)
        .expect("append boundary entries");
    assert_ledger_seq(&node, 3, "after 3 boundary entries");

    node.rebuild_smt().expect("rebuild");
    assert_smt_root_hex(
        &node,
        GOLDEN_LOG_SCORE_BOUNDARY_ROOT,
        "golden boundary cold-start",
    );
}

/// Boundary entries survive restart + rebuild with golden root preserved.
#[test]
fn log_score_boundary_restart_preserves_root() {
    let mut node = TestNode::start().expect("cold start");

    node.append_entries(&log_score_boundary_entries())
        .expect("append boundary entries");
    node.rebuild_smt().expect("rebuild");

    assert_round_trip_smt(
        &mut node,
        hex_to_bytes(GOLDEN_LOG_SCORE_BOUNDARY_ROOT),
        "golden boundary post-restart",
    );
}

/// Stress test: 20 entries (5 users × 4 rounds) produce a stable golden root.
///
/// Values are deterministically derived from loop indices using only
/// `Decimal::new` and `Decimal::from` — no floating-point involved.
#[test]
fn log_score_stress_golden() {
    let mut node = TestNode::start().expect("cold start");

    let entries = log_score_stress_batch();
    assert_eq!(entries.len(), 20, "stress batch should have 20 entries");

    node.append_entries(&entries).expect("append stress batch");
    assert_ledger_seq(&node, 20, "after 20 stress entries");

    node.rebuild_smt().expect("rebuild");
    assert_smt_root_hex(
        &node,
        GOLDEN_LOG_SCORE_STRESS_ROOT,
        "golden stress cold-start",
    );
}

/// Stress batch survives restart.
#[test]
fn log_score_stress_restart_preserves_root() {
    let mut node = TestNode::start().expect("cold start");

    node.append_entries(&log_score_stress_batch())
        .expect("append stress batch");
    node.rebuild_smt().expect("rebuild");

    assert_round_trip_smt(
        &mut node,
        hex_to_bytes(GOLDEN_LOG_SCORE_STRESS_ROOT),
        "golden stress post-restart",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Non-golden structural tests
// ─────────────────────────────────────────────────────────────────────────────

/// Append entries one-by-one vs all-at-once produces the **same** SMT root.
///
/// This verifies that `FileLedger`'s seq assignment and postcard serialization
/// are order-independent at the entry level (each entry is self-contained).
#[test]
fn log_score_incremental_vs_batch_same_root() {
    let entries = log_score_entries();

    // --- Batch append ---
    let mut batch_node = TestNode::start().expect("batch cold start");
    batch_node.append_entries(&entries).expect("batch append");
    batch_node.rebuild_smt().expect("batch rebuild");
    let batch_root = batch_node.smt_root();

    // --- Incremental (one-by-one) append ---
    let mut incr_node = TestNode::start().expect("incr cold start");
    for entry in &entries {
        incr_node
            .append_entries(std::slice::from_ref(entry))
            .expect("incremental append");
    }
    incr_node.rebuild_smt().expect("incr rebuild");

    assert_smt_root_eq(
        &incr_node,
        batch_root,
        "incremental vs batch must produce identical SMT root",
    );
}

/// Restart preserves root consistency for the stress batch across 3 cycles.
///
/// Each cycle: restart → rebuild → assert root unchanged.
#[test]
fn log_score_stress_multiple_restarts() {
    let mut node = TestNode::start().expect("cold start");

    node.append_entries(&log_score_stress_batch())
        .expect("append stress batch");
    node.rebuild_smt().expect("rebuild");
    let root = node.smt_root();

    for cycle in 1..=3 {
        node.restart()
            .unwrap_or_else(|e| panic!("restart cycle {cycle}: {e}"));
        node.rebuild_smt()
            .unwrap_or_else(|e| panic!("rebuild cycle {cycle}: {e}"));
        assert_smt_root_eq(&node, root, &format!("stress restart cycle {cycle}"));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn hex_to_bytes(hex: &str) -> [u8; 32] {
    let bytes = hex::decode(hex).expect("golden hex must be valid");
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    arr
}
