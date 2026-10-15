//! SIES Storage Abstraction Layer
//!
//! Defines `StateStore` and `Ledger` traits to decouple core algorithms
//! from underlying storage implementations.

use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use serde::{Deserialize, Serialize};
use sies_core::bayesian::BetaState;
use thiserror::Error;

const DECIMAL_SCALE: u64 = 1_000_000_000_000_000_000; // 10^18

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("serialize error: {0}")]
    Serialize(String),
    #[error("deserialize error: {0}")]
    Deserialize(String),
    #[error("key not found: {0}")]
    NotFound(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("backend error: {0}")]
    Backend(String),
}

pub type Result<T> = std::result::Result<T, StorageError>;

// ---------------------------------------------------------------------------
// Type aliases
// ---------------------------------------------------------------------------

pub type UserId = u64;

// ---------------------------------------------------------------------------
// LedgerEntry (was DecisionEvent)
// ---------------------------------------------------------------------------

/// An immutable, append-only record of a single state transition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerEntry {
    /// Monotonically increasing sequence number assigned by the ledger.
    pub seq: u64,
    /// Round / epoch number in which this transition occurred.
    pub round: u64,
    /// The participant whose state was updated.
    pub user_id: UserId,
    /// Serialized belief allocation vector.
    pub belief_allocations: Vec<Decimal>,
    /// Serialized outcome / ground-truth vector.
    pub outcome_allocations: Vec<Decimal>,
    /// Post-update alpha of the participant's BetaState.
    pub new_alpha: Decimal,
    /// Post-update beta of the participant's BetaState.
    pub new_beta: Decimal,
    /// Score computed by the scoring rule for this transition.
    pub score: Decimal,
}

/// 磁盘/网络序列化用的纯整数表示，兼容 postcard + ZK circuit。
/// 所有 Decimal 字段以 10^18 定点数存储。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawLedgerEntry {
    pub seq: u64,
    pub round: u64,
    pub user_id: u64,
    pub belief_allocations: Vec<u64>,
    pub outcome_allocations: Vec<u64>,
    pub new_alpha: u64,
    pub new_beta: u64,
    pub score: u64,
}

fn decimal_to_fixed(d: Decimal) -> u64 {
    (d * Decimal::from(DECIMAL_SCALE))
        .to_u64()
        .expect("decimal overflow in fixed-point conversion")
}

fn fixed_to_decimal(v: u64) -> Decimal {
    Decimal::from_u64(v).unwrap() / Decimal::from(DECIMAL_SCALE)
}

impl From<&LedgerEntry> for RawLedgerEntry {
    fn from(e: &LedgerEntry) -> Self {
        Self {
            seq: e.seq,
            round: e.round,
            user_id: e.user_id,
            belief_allocations: e
                .belief_allocations
                .iter()
                .map(|d| decimal_to_fixed(*d))
                .collect(),
            outcome_allocations: e
                .outcome_allocations
                .iter()
                .map(|d| decimal_to_fixed(*d))
                .collect(),
            new_alpha: decimal_to_fixed(e.new_alpha),
            new_beta: decimal_to_fixed(e.new_beta),
            score: decimal_to_fixed(e.score),
        }
    }
}

impl From<RawLedgerEntry> for LedgerEntry {
    fn from(r: RawLedgerEntry) -> Self {
        Self {
            seq: r.seq,
            round: r.round,
            user_id: r.user_id,
            belief_allocations: r
                .belief_allocations
                .into_iter()
                .map(fixed_to_decimal)
                .collect(),
            outcome_allocations: r
                .outcome_allocations
                .into_iter()
                .map(fixed_to_decimal)
                .collect(),
            new_alpha: fixed_to_decimal(r.new_alpha),
            new_beta: fixed_to_decimal(r.new_beta),
            score: fixed_to_decimal(r.score),
        }
    }
}

// ---------------------------------------------------------------------------
// StateStore trait (unchanged)
// ---------------------------------------------------------------------------

pub trait StateStore {
    fn get_state(&self, user_id: UserId) -> Result<Option<BetaState>>;
    fn put_state(&mut self, user_id: UserId, state: &BetaState) -> Result<()>;
    fn put_states_batch(&mut self, entries: &[(UserId, &BetaState)]) -> Result<()> {
        for (uid, state) in entries {
            self.put_state(*uid, state)?;
        }
        Ok(())
    }
    fn list_user_ids(&self) -> Result<Vec<UserId>>;
}

// ---------------------------------------------------------------------------
// Ledger trait (was EventLog)
// ---------------------------------------------------------------------------

/// Append-only ledger supporting range reads and deterministic replay.
///
/// The ledger is the single source of truth for reconstructing state
/// via deterministic replay from genesis.
pub trait Ledger {
    /// Append a [`LedgerEntry`] and return its assigned sequence number.
    fn append(&mut self, entry: &LedgerEntry) -> Result<u64>;

    /// Read entries in the half-open range `[start_seq, end_seq)`.
    fn read_range(&self, start_seq: u64, end_seq: u64) -> Result<Vec<LedgerEntry>>;

    /// Return the highest sequence number written so far. Returns `0` when empty.
    fn last_seq(&self) -> Result<u64>;

    /// Retrieve all entries for a given user in a specific round.
    fn read_by_user_round(&self, user_id: UserId, round: u64) -> Result<Vec<LedgerEntry>> {
        let last = self.last_seq()?;
        if last == 0 {
            return Ok(Vec::new());
        }
        let all = self.read_range(0, last + 1)?;
        Ok(all
            .into_iter()
            .filter(|e| e.user_id == user_id && e.round == round)
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Module declarations
// ---------------------------------------------------------------------------

pub mod file_ledger;
pub use file_ledger::FileLedger;

// ---------------------------------------------------------------------------
// Tests (updated to use Ledger / LedgerEntry)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct MemStateStore {
        map: HashMap<UserId, BetaState>,
    }
    impl MemStateStore {
        fn new() -> Self {
            Self {
                map: HashMap::new(),
            }
        }
    }
    impl StateStore for MemStateStore {
        fn get_state(&self, user_id: UserId) -> Result<Option<BetaState>> {
            Ok(self.map.get(&user_id).cloned())
        }
        fn put_state(&mut self, user_id: UserId, state: &BetaState) -> Result<()> {
            self.map.insert(user_id, state.clone());
            Ok(())
        }
        fn list_user_ids(&self) -> Result<Vec<UserId>> {
            let mut ids: Vec<_> = self.map.keys().copied().collect();
            ids.sort();
            Ok(ids)
        }
    }

    struct MemLedger {
        entries: Vec<LedgerEntry>,
    }
    impl MemLedger {
        fn new() -> Self {
            Self {
                entries: Vec::new(),
            }
        }
    }
    impl Ledger for MemLedger {
        fn append(&mut self, entry: &LedgerEntry) -> Result<u64> {
            let seq = self.entries.len() as u64 + 1;
            let mut owned = entry.clone();
            owned.seq = seq;
            self.entries.push(owned);
            Ok(seq)
        }
        fn read_range(&self, start_seq: u64, end_seq: u64) -> Result<Vec<LedgerEntry>> {
            Ok(self
                .entries
                .iter()
                .filter(|e| e.seq >= start_seq && e.seq < end_seq)
                .cloned()
                .collect())
        }
        fn last_seq(&self) -> Result<u64> {
            Ok(self.entries.len() as u64)
        }
    }

    fn test_state(a: u64, b: u64) -> BetaState {
        BetaState::new(Decimal::from(a), Decimal::from(b)).unwrap()
    }

    fn sample_entry(round: u64, user_id: UserId) -> LedgerEntry {
        LedgerEntry {
            seq: 0,
            round,
            user_id,
            belief_allocations: vec![Decimal::new(7, 1), Decimal::new(3, 1)],
            outcome_allocations: vec![Decimal::ONE, Decimal::ZERO],
            new_alpha: Decimal::from(2u64),
            new_beta: Decimal::from(1u64),
            score: Decimal::new(85, 2),
        }
    }

    #[test]
    fn state_store_get_missing_returns_none() {
        let store = MemStateStore::new();
        assert!(store.get_state(42).unwrap().is_none());
    }
    #[test]
    fn state_store_put_and_get() {
        let mut store = MemStateStore::new();
        let state = test_state(3, 2);
        store.put_state(1, &state).unwrap();
        assert_eq!(store.get_state(1).unwrap(), Some(state));
    }
    #[test]
    fn state_store_list_user_ids_sorted() {
        let mut store = MemStateStore::new();
        store.put_state(30, &test_state(1, 1)).unwrap();
        store.put_state(10, &test_state(1, 1)).unwrap();
        store.put_state(20, &test_state(1, 1)).unwrap();
        assert_eq!(store.list_user_ids().unwrap(), vec![10, 20, 30]);
    }
    #[test]
    fn state_store_batch_default_impl() {
        let mut store = MemStateStore::new();
        let s1 = test_state(2, 1);
        let s2 = test_state(1, 3);
        store.put_states_batch(&[(100, &s1), (200, &s2)]).unwrap();
        assert_eq!(store.get_state(100).unwrap(), Some(s1));
        assert_eq!(store.get_state(200).unwrap(), Some(s2));
    }
    #[test]
    fn ledger_append_assigns_sequential_seq() {
        let mut ledger = MemLedger::new();
        assert_eq!(ledger.append(&sample_entry(1, 10)).unwrap(), 1);
        assert_eq!(ledger.append(&sample_entry(1, 20)).unwrap(), 2);
        assert_eq!(ledger.last_seq().unwrap(), 2);
    }
    #[test]
    fn ledger_read_range_half_open() {
        let mut ledger = MemLedger::new();
        for r in 1..=5 {
            ledger.append(&sample_entry(r, 1)).unwrap();
        }
        let range = ledger.read_range(2, 4).unwrap();
        assert_eq!(range.len(), 2);
        assert_eq!(range[0].seq, 2);
        assert_eq!(range[1].seq, 3);
    }
    #[test]
    fn ledger_empty_range_returns_empty() {
        let ledger = MemLedger::new();
        assert!(ledger.read_range(0, 10).unwrap().is_empty());
        assert_eq!(ledger.last_seq().unwrap(), 0);
    }
    #[test]
    fn ledger_read_by_user_round_default_impl() {
        let mut ledger = MemLedger::new();
        ledger.append(&sample_entry(1, 10)).unwrap();
        ledger.append(&sample_entry(1, 20)).unwrap();
        ledger.append(&sample_entry(2, 10)).unwrap();
        ledger.append(&sample_entry(1, 10)).unwrap();
        let entries = ledger.read_by_user_round(10, 1).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|e| e.user_id == 10 && e.round == 1));
    }
    #[test]
    fn ledger_entry_serde_roundtrip() {
        let entry = sample_entry(5, 42);
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: LedgerEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, deserialized);
    }
}
