//! DWES Storage Abstraction Layer
//!
//! Defines `StateStore` and `EventLog` traits to decouple core algorithms
//! from underlying storage implementations (in-memory, SMT-backed, etc.).

use dwes_core::bayesian::BetaState;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Unified error type for storage operations.
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

/// Opaque user identifier used as storage key.
pub type UserId = u64;

// ---------------------------------------------------------------------------
// Decision Event
// ---------------------------------------------------------------------------

/// An immutable, append-only record of a single decision + weight update.
///
/// Every field is public so that downstream consumers (replay engine,
/// ZK-circuit witness generator, API layer) can inspect events without
/// accessor boilerplate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionEvent {
    /// Monotonically increasing sequence number assigned by the log.
    pub seq: u64,
    /// Round / epoch number in which this decision was made.
    pub round: u64,
    /// The participant who made the decision.
    pub user_id: UserId,
    /// Serialized belief allocation vector (e.g. `[0.7, 0.3]`).
    pub belief_allocations: Vec<Decimal>,
    /// Serialized outcome / ground-truth vector.
    pub outcome_allocations: Vec<Decimal>,
    /// Post-update alpha of the participant's BetaState.
    pub new_alpha: Decimal,
    /// Post-update beta of the participant's BetaState.
    pub new_beta: Decimal,
    /// Score computed by the scoring rule for this decision.
    pub score: Decimal,
}

// ---------------------------------------------------------------------------
// StateStore trait
// ---------------------------------------------------------------------------

/// Read/write access to per-user weight snapshots.
///
/// Implementations may be backed by an in-memory HashMap, a Sparse Merkle
/// Tree, or a remote database. The trait intentionally avoids prescribing
/// transaction semantics — atomicity guarantees are documented per impl.
pub trait StateStore {
    /// Retrieve the current [`BetaState`] for a user.
    ///
    /// Returns `Ok(None)` when the user has never participated.
    fn get_state(&self, user_id: UserId) -> Result<Option<BetaState>>;

    /// Insert or update a user's [`BetaState`].
    fn put_state(&mut self, user_id: UserId, state: &BetaState) -> Result<()>;

    /// Batch insert/update multiple users atomically.
    ///
    /// Default implementation falls back to sequential `put_state` calls.
    /// Backends that support true atomic batches should override this.
    fn put_states_batch(&mut self, entries: &[(UserId, &BetaState)]) -> Result<()> {
        for (uid, state) in entries {
            self.put_state(*uid, state)?;
        }
        Ok(())
    }

    /// List all user IDs that have a stored state.
    fn list_user_ids(&self) -> Result<Vec<UserId>>;
}

// ---------------------------------------------------------------------------
// EventLog trait
// ---------------------------------------------------------------------------

/// Append-only event log supporting range reads and replay.
///
/// The log is the single source of truth for reconstructing state via
/// deterministic replay from genesis.
pub trait EventLog {
    /// Append a [`DecisionEvent`] and return its assigned sequence number.
    fn append(&mut self, event: &DecisionEvent) -> Result<u64>;

    /// Read events in the half-open range `[start_seq, end_seq)`.
    ///
    /// Returns an empty vec when the range contains no events.
    fn read_range(&self, start_seq: u64, end_seq: u64) -> Result<Vec<DecisionEvent>>;

    /// Return the highest sequence number written so far.
    ///
    /// Returns `0` when the log is empty.
    fn last_seq(&self) -> Result<u64>;

    /// Retrieve all events for a given user in a specific round.
    ///
    /// Default implementation scans the full log; optimized backends
    /// should maintain a secondary index.
    fn read_by_user_round(&self, user_id: UserId, round: u64) -> Result<Vec<DecisionEvent>> {
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // -- Minimal in-memory implementations for testing the trait contracts --

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

    struct MemEventLog {
        events: Vec<DecisionEvent>,
    }

    impl MemEventLog {
        fn new() -> Self {
            Self { events: Vec::new() }
        }
    }

    impl EventLog for MemEventLog {
        fn append(&mut self, event: &DecisionEvent) -> Result<u64> {
            let seq = self.events.len() as u64 + 1;
            let mut owned = event.clone();
            owned.seq = seq;
            self.events.push(owned);
            Ok(seq)
        }

        fn read_range(&self, start_seq: u64, end_seq: u64) -> Result<Vec<DecisionEvent>> {
            Ok(self
                .events
                .iter()
                .filter(|e| e.seq >= start_seq && e.seq < end_seq)
                .cloned()
                .collect())
        }

        fn last_seq(&self) -> Result<u64> {
            Ok(self.events.len() as u64)
        }
    }

    // -- Helper to build a valid BetaState for tests --
    fn test_state(a: u64, b: u64) -> BetaState {
        BetaState::new(Decimal::from(a), Decimal::from(b)).unwrap()
    }

    fn sample_event(round: u64, user_id: UserId) -> DecisionEvent {
        DecisionEvent {
            seq: 0, // will be overwritten by append
            round,
            user_id,
            belief_allocations: vec![Decimal::new(7, 1), Decimal::new(3, 1)],
            outcome_allocations: vec![Decimal::ONE, Decimal::ZERO],
            new_alpha: Decimal::from(2u64),
            new_beta: Decimal::from(1u64),
            score: Decimal::new(85, 2),
        }
    }

    // ---- StateStore tests ----

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

    // ---- EventLog tests ----

    #[test]
    fn event_log_append_assigns_sequential_seq() {
        let mut log = MemEventLog::new();
        let e1 = sample_event(1, 10);
        let e2 = sample_event(1, 20);
        assert_eq!(log.append(&e1).unwrap(), 1);
        assert_eq!(log.append(&e2).unwrap(), 2);
        assert_eq!(log.last_seq().unwrap(), 2);
    }

    #[test]
    fn event_log_read_range_half_open() {
        let mut log = MemEventLog::new();
        for r in 1..=5 {
            log.append(&sample_event(r, 1)).unwrap();
        }
        let range = log.read_range(2, 4).unwrap();
        assert_eq!(range.len(), 2);
        assert_eq!(range[0].seq, 2);
        assert_eq!(range[1].seq, 3);
    }

    #[test]
    fn event_log_empty_range_returns_empty() {
        let log = MemEventLog::new();
        assert!(log.read_range(0, 10).unwrap().is_empty());
        assert_eq!(log.last_seq().unwrap(), 0);
    }

    #[test]
    fn event_log_read_by_user_round_default_impl() {
        let mut log = MemEventLog::new();
        log.append(&sample_event(1, 10)).unwrap();
        log.append(&sample_event(1, 20)).unwrap();
        log.append(&sample_event(2, 10)).unwrap();
        log.append(&sample_event(1, 10)).unwrap();

        let events = log.read_by_user_round(10, 1).unwrap();
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|e| e.user_id == 10 && e.round == 1));
    }

    // ---- DecisionEvent serialization round-trip ----

    #[test]
    fn decision_event_serde_roundtrip() {
        let event = sample_event(5, 42);
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: DecisionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, deserialized);
    }
}
