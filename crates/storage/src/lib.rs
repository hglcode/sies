//! SIES Storage Abstraction Layer
//!
//! Defines `StateStore` and `Ledger` traits to decouple core algorithms
//! from underlying storage implementations.

use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use serde::{Deserialize, Serialize};
use sies_core::bayesian::BetaState;
use sies_core::belief::Belief;
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
    pub belief: Belief,
    /// Serialized outcome / ground-truth vector.
    pub outcome: Belief,
    /// Post-update alpha of the participant's BetaState.
    pub new_alpha: Decimal,
    /// Post-update beta of the participant's BetaState.
    pub new_beta: Decimal,
    /// Score computed by the scoring rule for this transition.
    pub score: Decimal,
}

impl LedgerEntry {
    /// 唯一构造入口，强制经过 Belief 校验
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        seq: u64,
        round: u64,
        user_id: u64,
        belief: Belief,
        outcome: Belief,
        new_alpha: Decimal,
        new_beta: Decimal,
        score: Decimal,
    ) -> Self {
        Self {
            seq,
            round,
            user_id,
            belief,
            outcome,
            new_alpha,
            new_beta,
            score,
        }
    }

    /// 兼容现有序列化/测试代码的只读访问
    pub fn belief_allocations(&self) -> &[Decimal] {
        self.belief.allocations()
    }
    pub fn outcome_allocations(&self) -> &[Decimal] {
        self.outcome.allocations()
    }
}

/// 磁盘/网络序列化用的纯整数表示，兼容 postcard + ZK circuit。
/// 概率字段以 u64 存储 (范围 [0, 10^18])；
/// alpha/beta/score 以 i128 存储 (支持负数和大值)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawLedgerEntry {
    pub seq: u64,
    pub round: u64,
    pub user_id: u64,
    pub belief_allocations: Vec<u64>,
    pub outcome_allocations: Vec<u64>,
    pub new_alpha: i128,
    pub new_beta: i128,
    pub score: i128,
}

//fn decimal_to_fixed_unsigned(d: Decimal) -> u64 {
//    let scaled = d * Decimal::from(DECIMAL_SCALE);
//    scaled
//        .to_u64()
//        .expect("decimal overflow in unsigned fixed-point conversion")
//}
fn decimal_to_fixed_unsigned(d: Decimal) -> u64 {
    let clamped = d.max(Decimal::ZERO).min(Decimal::ONE);
    let scaled = clamped * Decimal::from(DECIMAL_SCALE);
    scaled
        .to_u64()
        .expect("clamped decimal still overflowed u64 fixed-point")
}

fn decimal_to_fixed_signed(d: Decimal) -> i128 {
    let scaled = d * Decimal::from(DECIMAL_SCALE as i128);
    scaled
        .to_i128()
        .expect("decimal overflow in signed i128 fixed-point conversion")
}

fn fixed_to_decimal_unsigned(v: u64) -> Decimal {
    Decimal::from_u64(v).unwrap() / Decimal::from(DECIMAL_SCALE)
}

fn fixed_to_decimal_signed(v: i128) -> Decimal {
    Decimal::from_i128(v).unwrap() / Decimal::from(DECIMAL_SCALE as i128)
}

impl From<&LedgerEntry> for RawLedgerEntry {
    fn from(e: &LedgerEntry) -> Self {
        RawLedgerEntry {
            seq: e.seq,
            round: e.round,
            user_id: e.user_id,
            belief_allocations: e
                .belief_allocations()
                .iter()
                .map(|&d| decimal_to_fixed_unsigned(d))
                .collect(),
            outcome_allocations: e
                .outcome_allocations()
                .iter()
                .map(|&d| decimal_to_fixed_unsigned(d))
                .collect(),
            // ✅ 使用 signed 定点编码
            new_alpha: decimal_to_fixed_signed(e.new_alpha),
            new_beta: decimal_to_fixed_signed(e.new_beta),
            score: decimal_to_fixed_signed(e.score),
        }
    }
}

impl From<RawLedgerEntry> for LedgerEntry {
    fn from(r: RawLedgerEntry) -> Self {
        let belief_allocs: Vec<Decimal> = r
            .belief_allocations
            .iter()
            // ✅ 修正函数名
            .map(|&v| fixed_to_decimal_unsigned(v))
            .collect();
        let outcome_allocs: Vec<Decimal> = r
            .outcome_allocations
            .iter()
            .map(|&v| fixed_to_decimal_unsigned(v))
            .collect();

        // ✅ 反序列化出非法值 → panic（损坏数据不应被静默接受）
        let belief =
            Belief::new(belief_allocs).expect("deserialized belief must satisfy invariant");
        let outcome =
            Belief::new(outcome_allocs).expect("deserialized outcome must satisfy invariant");

        LedgerEntry::new(
            r.seq,
            r.round,
            r.user_id,
            belief,
            outcome,
            fixed_to_decimal_signed(r.new_alpha),
            fixed_to_decimal_signed(r.new_beta),
            fixed_to_decimal_signed(r.score),
        )
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

/// Blanket impl: allow `&L` to be used as a read-only [`Ledger`].
/// Only read methods are delegated; `append` panics at runtime if called
/// through a shared reference (which should never happen in practice since
/// `Rebuilder` only calls read methods).
impl<L: Ledger + ?Sized> Ledger for &L {
    fn append(&mut self, _entry: &LedgerEntry) -> Result<u64> {
        Err(StorageError::Backend(
            "cannot append through shared reference".into(),
        ))
    }

    fn read_range(&self, start_seq: u64, end_seq: u64) -> Result<Vec<LedgerEntry>> {
        (**self).read_range(start_seq, end_seq)
    }

    fn last_seq(&self) -> Result<u64> {
        (**self).last_seq()
    }

    fn read_by_user_round(&self, user_id: UserId, round: u64) -> Result<Vec<LedgerEntry>> {
        (**self).read_by_user_round(user_id, round)
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
        LedgerEntry::new(
            0, // seq 由 append 自动分配，这里填占位值即可
            round,
            user_id,
            Belief::new(vec![Decimal::new(7, 1), Decimal::new(3, 1)]).unwrap(),
            Belief::new(vec![Decimal::ONE, Decimal::ZERO]).unwrap(),
            Decimal::from(2u64),
            Decimal::ONE,
            Decimal::new(-356_674_943_938i64, 12),
        )
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
        // ✅ sample_entry(round, user_id) — seq 由 append 自动分配
        ledger.append(&sample_entry(1, 10)).unwrap(); // seq=1, round=1,  user_id=10
        ledger.append(&sample_entry(2, 10)).unwrap(); // seq=2, round=2,  user_id=10
        ledger.append(&sample_entry(1, 20)).unwrap(); // seq=3, round=1,  user_id=20
        ledger.append(&sample_entry(1, 10)).unwrap(); // seq=4, round=1,  user_id=10
        let entries = ledger.read_by_user_round(10, 1).unwrap(); // 查 user_id=10, round=1
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

#[cfg(kani)]
mod verification {
    use super::*;

    #[kani::proof]
    #[kani::unwind(3)]
    fn verify_ledger_entry_belief_is_always_valid() {
        // TODO: 待 Kani 环境就绪后补充完整 proof
        // 当前仅作为 cfg(kani) 占位，cargo check 不会编译此块
    }
}
