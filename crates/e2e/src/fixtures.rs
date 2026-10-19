#![allow(clippy::unwrap_used, clippy::expect_used)]
use rust_decimal::Decimal;
use sies_core::belief::Belief;
use sies_storage::LedgerEntry;

/// Helper: construct a Belief from Decimal slice, panicking on invalid input.
/// Only for test fixtures where values are known-good by construction.
fn belief(vals: &[Decimal]) -> Belief {
    Belief::new(vals.to_vec()).expect("fixture belief must be valid")
}

/// Create a minimal valid [`LedgerEntry`] for testing.
#[must_use]
pub fn sample_entry(seq: u64, round: u64, user_id: u64, alpha: u64, beta: u64) -> LedgerEntry {
    LedgerEntry::new(
        seq,
        round,
        user_id,
        // ✅ 归一化: [0.7, 0.3] 代替原来的 [0.7]
        belief(&[Decimal::new(7, 1), Decimal::new(3, 1)]),
        belief(&[Decimal::ONE]),
        Decimal::from(alpha),
        Decimal::from(beta),
        Decimal::new(85, 2),
    )
}

/// Generate `n` sample entries with sequential seq starting from 1.
#[must_use]
pub fn sample_entries(n: u64, round: u64) -> Vec<LedgerEntry> {
    (0..n)
        .map(|i| sample_entry(i + 1, round, 0, 1, 1))
        .collect()
}

/// Build a batch of sequential entries for multiple users/rounds.
#[must_use]
pub fn sample_batch(count: u64) -> Vec<LedgerEntry> {
    (1..=count)
        .map(|i| sample_entry(i, (i - 1) / 3 + 1, i % 10 + 1, i + 1, 1))
        .collect()
}

// ─── Log-score determinism fixtures ─────────────────────────────────────────
#[must_use]
pub fn log_score_entries() -> Vec<LedgerEntry> {
    vec![
        // Round 1 — User 1: confident correct prediction
        LedgerEntry::new(
            0,
            1,
            1,
            belief(&[Decimal::new(7, 1), Decimal::new(3, 1)]),
            belief(&[Decimal::ONE, Decimal::ZERO]),
            Decimal::from(2u64),
            Decimal::ONE,
            Decimal::new(-356_674_943_938, 12),
        ),
        // Round 1 — User 2: moderate confidence, wrong outcome
        LedgerEntry::new(
            0,
            1,
            2,
            belief(&[Decimal::new(15, 2), Decimal::new(85, 2)]),
            belief(&[Decimal::ONE, Decimal::ZERO]),
            Decimal::ONE,
            Decimal::from(2u64),
            Decimal::new(-1_897_119_984_886, 12),
        ),
        // Round 2 — User 1: high-precision allocation
        LedgerEntry::new(
            0,
            2,
            1,
            belief(&[Decimal::new(3333, 4), Decimal::new(6667, 4)]),
            belief(&[Decimal::ZERO, Decimal::ONE]),
            Decimal::from(3u64),
            Decimal::from(2u64),
            Decimal::new(-405_465_108_108, 12),
        ),
        // Round 2 — User 2: extreme skew → ε boundary
        LedgerEntry::new(
            0,
            2,
            2,
            belief(&[Decimal::new(1, 6), Decimal::new(999_999, 6)]),
            belief(&[Decimal::ONE, Decimal::ZERO]),
            Decimal::from(2u64),
            Decimal::from(3u64),
            Decimal::new(-13_815_510_557_964, 12),
        ),
    ]
}

#[must_use]
pub fn log_score_boundary_entries() -> Vec<LedgerEntry> {
    vec![
        // p = 0 (exact zero) → ε-smoothing mandatory
        LedgerEntry::new(
            0,
            1,
            100,
            belief(&[Decimal::ZERO, Decimal::ONE]),
            belief(&[Decimal::ONE, Decimal::ZERO]),
            Decimal::ONE,
            Decimal::from(5u64),
            Decimal::new(-27_631_021_115_928, 12),
        ),
        // p = ε (1e-12) exactly → smoothing no-op
        LedgerEntry::new(
            0,
            1,
            101,
            belief(&[Decimal::new(1, 12), Decimal::new(999_999_999_999, 12)]),
            belief(&[Decimal::ONE, Decimal::ZERO]),
            Decimal::ONE,
            Decimal::from(3u64),
            Decimal::new(-27_631_021_115_928, 12),
        ),
        // Uniform distribution — scale-4 trailing zeros
        LedgerEntry::new(
            0,
            2,
            100,
            belief(&[Decimal::new(5000, 4), Decimal::new(5000, 4)]),
            belief(&[Decimal::ZERO, Decimal::ONE]),
            Decimal::from(2u64),
            Decimal::from(6u64),
            Decimal::new(-693_147_180_560, 12),
        ),
    ]
}

/// Stress batch: 20 entries across 5 users × 4 rounds.
/// ✅ All allocations now go through `Belief::new()`, guaranteeing validity.
#[must_use]
pub fn log_score_stress_batch() -> Vec<LedgerEntry> {
    (0..20u64)
        .map(|i| {
            let user_id = (i % 5) + 1;
            let round = (i / 5) + 1;
            // ✅ 将 p 钳制到 [0.01, 0.99] 范围内，避免负值和非归一化
            let raw_p = (i * 7 + 1).cast_signed();
            let clamped_p = raw_p.clamp(1, 99); // ✅ 使用 .clamp() 替代 .min().max()
            let p = Decimal::new(clamped_p, 2);
            let q = Decimal::ONE - p;

            let outcome_idx = (i % 2) as usize;
            let mut outcome_vec = vec![Decimal::ZERO; 2];
            outcome_vec[outcome_idx] = Decimal::ONE;

            LedgerEntry::new(
                0,
                round,
                user_id,
                belief(&[p, q]),
                belief(&outcome_vec),
                Decimal::from(i + 2),
                Decimal::from(i + 1),
                -Decimal::new(((i + 1) * 12345).cast_signed(), 4),
            )
        })
        .collect()
}
