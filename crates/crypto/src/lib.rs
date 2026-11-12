#![no_std] // 确保无隐式 std 依赖，支持嵌入式/Standalone 场景
extern crate alloc;

/// 密码学哈希器的零成本抽象 Trait
///
/// # 设计约束 (ADR-6)
/// - Output 必须是固定大小数组，避免 Vec<u8> 堆分配
/// - 所有方法均为纯函数，无内部状态/IO
/// - 通过 Feature Flag 切换后端实现
pub trait Hasher {
    /// 固定大小的哈希输出类型 (如 [u8; 32])
    type Output: AsRef<[u8]> + Eq + Clone + Copy + core::fmt::Debug;

    /// 对任意字节切片计算哈希
    fn hash(data: &[u8]) -> Self::Output;

    /// 对两个哈希值进行组合哈希 (SMT/Merkle Tree 专用)
    /// 默认实现调用 hash()，后端可提供 SIMD 等优化版本
    fn hash_pair(left: &Self::Output, right: &Self::Output) -> Self::Output {
        let mut buf = [0u8; 64]; // 假设 Output 为 32 字节，编译期可优化
        let out_len = Self::Output::as_ref(left).len();
        buf[..out_len].copy_from_slice(Self::Output::as_ref(left));
        buf[out_len..out_len * 2].copy_from_slice(Self::Output::as_ref(right));
        Self::hash(&buf[..out_len * 2])
    }
}

// === 数字签名原语（#45；ADR-6 与 Hasher 同构的零成本抽象）===

mod signature;

#[cfg(feature = "ed25519")]
mod ed25519_backend;

// === Feature-gated 后端实现 ===

#[cfg(feature = "blake2b")]
mod blake2b_backend;

#[cfg(feature = "blake2b")]
pub use blake2b_backend::Blake2bHasher;

#[cfg(feature = "ed25519")]
pub use ed25519_backend::{Ed25519Backend, Ed25519SecretKey};

pub use signature::{SignatureError, Signer, Verifier};

// 预留未来扩展
// #[cfg(feature = "sha3")]
// pub use sha3_backend::Sha3Hasher;

// #[cfg(feature = "poseidon")]
// pub use poseidon_backend::PoseidonHasher;
