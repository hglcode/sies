use rust_decimal::Decimal;
use sies_storage::LedgerEntry;

/// Create a minimal valid [`LedgerEntry`] for testing.
/// `seq` is ignored by `FileLedger::append` (it assigns its own), but kept
/// for readability in test code.
pub fn sample_entry(seq: u64, round: u64, user_id: u64, alpha: u64, beta: u64) -> LedgerEntry {
    LedgerEntry {
        seq,
        round,
        user_id,
        belief_allocations: vec![Decimal::new(7, 1)],
        outcome_allocations: vec![Decimal::ONE],
        new_alpha: Decimal::from(alpha),
        new_beta: Decimal::from(beta),
        score: Decimal::new(85, 2),
    }
}

/// Generate `n` sample entries with sequential seq starting from 1.
pub fn sample_entries(n: u64, round: u64) -> Vec<LedgerEntry> {
    (0..n)
        .map(|i| sample_entry(i + 1, round, 0, 1, 1))
        .collect()
}

/// Build a batch of sequential entries for multiple users/rounds.
pub fn sample_batch(count: u64) -> Vec<LedgerEntry> {
    (1..=count)
        .map(|i| sample_entry(i, (i - 1) / 3 + 1, i % 10 + 1, i + 1, 1))
        .collect()
}
