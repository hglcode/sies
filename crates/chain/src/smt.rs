use dwes_core::bayesian::BetaState;
use dwes_storage::{StateStore, StorageError};
use sparse_merkle_tree::{
    H256, SparseMerkleTree, default_store::DefaultStore, traits::Hasher as SmtHasher, traits::Value,
};

/// 适配 sparse-merkle-tree v0.6.1 的有状态 Blake2b Hasher
#[derive(Clone, Default)]
struct DwesBlake2bHasher {
    buffer: Vec<u8>,
}

impl SmtHasher for DwesBlake2bHasher {
    fn write_byte(&mut self, b: u8) {
        self.buffer.push(b);
    }

    fn write_h256(&mut self, h: &H256) {
        self.buffer.extend_from_slice(h.as_slice());
    }

    fn finish(self) -> H256 {
        let mut hasher = blake2b_rs::Blake2bBuilder::new(32)
            .personal(b"dwes-smt-hash!")
            .build();
        hasher.update(&self.buffer);
        let mut buf = [0u8; 32];
        hasher.finalize(&mut buf);
        buf.into()
    }
}

/// 辅助函数：对任意数据执行单次哈希（用于 key 生成和 value 哈希）
fn blake2b_hash(data: &[u8]) -> H256 {
    let mut hasher = blake2b_rs::Blake2bBuilder::new(32)
        .personal(b"dwes-smt-hash!")
        .build();
    hasher.update(data);
    let mut buf = [0u8; 32];
    hasher.finalize(&mut buf);
    buf.into()
}

// ─── SMT Value 包装 ───────────────────────────────────────────
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SmtValue(Vec<u8>);

impl Value for SmtValue {
    fn to_h256(&self) -> H256 {
        if self.0.is_empty() {
            H256::zero()
        } else {
            blake2b_hash(&self.0)
        }
    }
    fn zero() -> Self {
        SmtValue(Vec::new())
    }
}

// ─── StateStore 适配器 ────────────────────────────────────────
pub struct SmtStateStore {
    tree: SparseMerkleTree<DwesBlake2bHasher, SmtValue, DefaultStore<SmtValue>>,
}

impl SmtStateStore {
    pub fn new() -> Self {
        Self {
            tree: SparseMerkleTree::default(),
        }
    }

    /// 将 user_id 转换为 SMT 的 256-bit key（使用独立 personalization 避免与树内哈希冲突）
    fn user_key(user_id: u64) -> H256 {
        let mut hasher = blake2b_rs::Blake2bBuilder::new(32)
            .personal(b"dwes-state-key")
            .build();
        hasher.update(&user_id.to_le_bytes());
        let mut buf = [0u8; 32];
        hasher.finalize(&mut buf);
        buf.into()
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
}
