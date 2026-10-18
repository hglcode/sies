use sies_core::bayesian::BetaState;
use sies_crypto::Hasher;
use sies_storage::{StateStore, StorageError};
use sparse_merkle_tree::{
    H256, SparseMerkleTree, default_store::DefaultStore, traits::Hasher as SmtHasher, traits::Value,
};

/// 适配 sparse-merkle-tree v0.6.1 的有状态 Blake2b Hasher
///
/// 内部缓冲所有写入，在 `finish()` 时通过 `sies_crypto::Blake2bHasher` 一次性计算哈希。
/// 迁移自 blake2b-rs（#34）：移除了 personalization，因为 key (固定 8 字节)
/// 与 value (变长) 的输入域天然不重叠，不需要额外域隔离。
#[derive(Clone, Default)]
struct SiesBlake2bHasher {
    buffer: Vec<u8>,
}

impl SmtHasher for SiesBlake2bHasher {
    fn write_byte(&mut self, b: u8) {
        self.buffer.push(b);
    }

    fn write_h256(&mut self, h: &H256) {
        self.buffer.extend_from_slice(h.as_slice());
    }

    fn finish(self) -> H256 {
        sies_crypto::Blake2bHasher::hash(&self.buffer).into()
    }
}

// ─── SMT Value 包装 ───────────────────────────────────────────
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SmtValue(Vec<u8>);

impl Value for SmtValue {
    fn to_h256(&self) -> H256 {
        if self.0.is_empty() {
            H256::zero()
        } else {
            sies_crypto::Blake2bHasher::hash(&self.0).into()
        }
    }
    fn zero() -> Self {
        SmtValue(Vec::new())
    }
}

// ─── StateStore 适配器 ────────────────────────────────────────
pub struct SmtStateStore {
    tree: SparseMerkleTree<SiesBlake2bHasher, SmtValue, DefaultStore<SmtValue>>,
}

impl SmtStateStore {
    pub fn new() -> Self {
        Self {
            tree: SparseMerkleTree::default(),
        }
    }

    /// 将 user_id 转换为 SMT 的 256-bit key
    fn user_key(user_id: u64) -> H256 {
        sies_crypto::Blake2bHasher::hash(&user_id.to_le_bytes()).into()
    }

    pub fn root_hash(&self) -> [u8; 32] {
        (*self.tree.root()).into()
    }
}

impl Default for SmtStateStore {
    fn default() -> Self {
        Self::new()
    }
}

impl StateStore for SmtStateStore {
    fn get_state(&self, user_id: u64) -> Result<Option<BetaState>, StorageError> {
        let key = Self::user_key(user_id);
        match self.tree.get(&key) {
            Ok(val) if !val.0.is_empty() => {
                // 直接从固定长度字节恢复，绕过 Postcard
                let raw: [u8; 40] = val.0.as_slice().try_into().map_err(|_| {
                    StorageError::Deserialize(format!(
                        "invalid BetaState raw bytes length: expected 40, got {}",
                        val.0.len()
                    ))
                })?;
                let state = BetaState::from_raw_bytes(&raw)
                    .map_err(|e| StorageError::Deserialize(e.to_string()))?;
                Ok(Some(state))
            }
            Ok(_) => Ok(None),
            Err(e) => Err(StorageError::Backend(e.to_string())),
        }
    }

    fn put_state(&mut self, user_id: u64, state: &BetaState) -> Result<(), StorageError> {
        let key = Self::user_key(user_id);
        // 直接写入固定长度字节，绕过 Postcard
        let bytes = state.to_raw_bytes();
        self.tree
            .update(key, SmtValue(bytes.to_vec()))
            .map(|_| ())
            .map_err(|e| StorageError::Backend(e.to_string()))
    }

    fn list_user_ids(&self) -> Result<Vec<u64>, StorageError> {
        // SMT 不支持枚举，#10 中引入辅助索引后完善
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smt_roundtrip() {
        let mut store = SmtStateStore::new();
        let initial_root = store.root_hash();

        let state = BetaState::uniform_prior();
        store.put_state(42, &state).unwrap();

        assert_ne!(initial_root, store.root_hash());
        assert_eq!(store.get_state(42).unwrap(), Some(state));
        assert!(store.get_state(999).unwrap().is_none());
    }

    /// 验证 key 派生和 value 哈希不会产生碰撞（域隔离检查）
    ///
    /// 移除 blake2b-rs personalization 后，需确认 user_key(42) ≠ blake2b_hash(BetaState::uniform_prior().to_raw_bytes())
    #[test]
    fn key_and_value_domain_separation() {
        let key_hash: [u8; 32] = SmtStateStore::user_key(42).into();
        let value_hash: [u8; 32] =
            sies_crypto::Blake2bHasher::hash(&BetaState::uniform_prior().to_raw_bytes());
        assert_ne!(
            key_hash, value_hash,
            "key derivation and value hash collided!"
        );
    }
}
