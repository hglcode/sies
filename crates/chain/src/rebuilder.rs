//! Deterministic SMT state reconstruction from [`Ledger`].

use sies_core::bayesian::BetaState;
use sies_storage::{Ledger, LedgerEntry, StateStore, StorageError};
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::smt::SmtStateStore;

#[derive(Debug, Error)]
pub enum RebuildError {
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    #[error(
        "root hash mismatch: expected {}, got {}",
        hex::encode(expected),
        hex::encode(actual)
    )]
    RootHashMismatch {
        expected: [u8; 32],
        actual: [u8; 32],
    },

    #[error("sequence gap: expected {expected}, found {found}")]
    SequenceGap { expected: u64, found: u64 },
}

pub struct Rebuilder<L: Ledger> {
    ledger: L,
}

impl<L: Ledger> Rebuilder<L> {
    pub fn new(ledger: L) -> Self {
        Self { ledger }
    }

    pub fn rebuild(&self) -> Result<SmtStateStore, RebuildError> {
        let total = self.ledger.last_seq()?;
        info!(total_entries = total, "Starting SMT rebuild from ledger");

        let mut store = SmtStateStore::new();
        let mut processed: u64 = 0;
        const BATCH: u64 = 4096;
        let mut cursor: u64 = 1;

        while cursor <= total {
            let end = (cursor + BATCH).min(total + 1);
            let batch = self.ledger.read_range(cursor, end)?;
            for entry in &batch {
                apply_entry(&mut store, entry)?;
                processed += 1;
            }
            if processed.is_multiple_of(10_000) {
                debug!(processed, "Rebuild progress");
            }
            cursor = end;
        }

        let root = store.root_hash();
        info!(
            entries_processed = processed,
            root_hash = hex::encode(&root[..8]),
            "SMT rebuild completed"
        );
        Ok(store)
    }
}

pub struct RootHashVerifier;

impl RootHashVerifier {
    pub fn verify(store: &SmtStateStore, expected: [u8; 32]) -> Result<(), RebuildError> {
        let actual = store.root_hash();
        if actual == expected {
            info!(
                root_hash = hex::encode(&actual[..8]),
                "Root hash verification passed"
            );
            Ok(())
        } else {
            warn!(
                expected = hex::encode(&expected[..8]),
                actual = hex::encode(&actual[..8]),
                "Root hash verification FAILED"
            );
            Err(RebuildError::RootHashMismatch { expected, actual })
        }
    }
}

fn apply_entry(store: &mut SmtStateStore, entry: &LedgerEntry) -> Result<(), StorageError> {
    let state = BetaState::new(entry.new_alpha, entry.new_beta)
        .map_err(|e| StorageError::Deserialize(e.to_string()))?;
    store.put_state(entry.user_id, &state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use sies_storage::FileLedger;
    use tempfile::TempDir;

    fn make_entry(seq: u64, round: u64, user_id: u64, alpha: u64, beta: u64) -> LedgerEntry {
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

    #[test]
    fn rebuild_empty_ledger() {
        let dir = TempDir::new().unwrap();
        let ledger = FileLedger::open(dir.path()).unwrap();
        let store = Rebuilder::new(ledger).rebuild().unwrap();
        assert_eq!(store.root_hash(), SmtStateStore::new().root_hash());
    }

    #[test]
    fn rebuild_single_entry() {
        let dir = TempDir::new().unwrap();
        let mut ledger = FileLedger::open(dir.path()).unwrap();
        ledger.append(&make_entry(1, 1, 42, 3, 2)).unwrap();
        let store = Rebuilder::new(ledger).rebuild().unwrap();
        let state = store.get_state(42).unwrap().unwrap();
        assert_eq!(state.alpha(), Decimal::from(3u64));
        assert_eq!(state.beta(), Decimal::from(2u64));
    }

    #[test]
    fn rebuild_deterministic() {
        let entries = vec![
            make_entry(1, 1, 10, 2, 1),
            make_entry(2, 1, 20, 3, 1),
            make_entry(3, 2, 10, 4, 1),
        ];
        let build = || {
            let dir = TempDir::new().unwrap();
            let mut ledger = FileLedger::open(dir.path()).unwrap();
            for e in &entries {
                ledger.append(e).unwrap();
            }
            Rebuilder::new(ledger).rebuild().unwrap()
        };
        assert_eq!(build().root_hash(), build().root_hash());
    }

    #[test]
    fn rebuild_matches_manual_construction() {
        let entries = vec![
            make_entry(1, 1, 10, 2, 1),
            make_entry(2, 1, 20, 3, 1),
            make_entry(3, 2, 10, 4, 1),
        ];
        let dir = TempDir::new().unwrap();
        let mut ledger = FileLedger::open(dir.path()).unwrap();
        for e in &entries {
            ledger.append(e).unwrap();
        }
        let rebuilt = Rebuilder::new(ledger).rebuild().unwrap();

        let mut manual = SmtStateStore::new();
        for e in &entries {
            let state = BetaState::new(e.new_alpha, e.new_beta).unwrap();
            manual.put_state(e.user_id, &state).unwrap();
        }
        assert_eq!(rebuilt.root_hash(), manual.root_hash());
    }

    #[test]
    fn verify_pass() {
        let dir = TempDir::new().unwrap();
        let mut ledger = FileLedger::open(dir.path()).unwrap();
        ledger.append(&make_entry(1, 1, 1, 2, 1)).unwrap();
        let store = Rebuilder::new(ledger).rebuild().unwrap();
        assert!(RootHashVerifier::verify(&store, store.root_hash()).is_ok());
    }

    #[test]
    fn verify_fail() {
        let dir = TempDir::new().unwrap();
        let mut ledger = FileLedger::open(dir.path()).unwrap();
        ledger.append(&make_entry(1, 1, 1, 2, 1)).unwrap();
        let store = Rebuilder::new(ledger).rebuild().unwrap();
        assert!(matches!(
            RootHashVerifier::verify(&store, [0xffu8; 32]),
            Err(RebuildError::RootHashMismatch { .. })
        ));
    }

    #[test]
    fn overwrite_produces_latest_state() {
        let dir = TempDir::new().unwrap();
        let mut ledger = FileLedger::open(dir.path()).unwrap();
        ledger.append(&make_entry(1, 1, 42, 2, 1)).unwrap();
        ledger.append(&make_entry(2, 2, 42, 5, 3)).unwrap();
        let store = Rebuilder::new(ledger).rebuild().unwrap();
        let state = store.get_state(42).unwrap().unwrap();
        assert_eq!(state.alpha(), Decimal::from(5u64));
        assert_eq!(state.beta(), Decimal::from(3u64));
    }
}
