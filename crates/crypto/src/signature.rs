//! 数字签名零成本抽象（任务 #45，ADR-6：与 [`crate::Hasher`] 同构的原语层）
//!
//! - 纯 `core` 实现（`no_std`），无浮点参与——跨平台比特一致，天然满足 ADR #2
//! - 后端经 feature gate 接入（见 `ed25519_backend`），与 `blake2b` 后端同规则

use alloc::string::String;

/// 签名原语错误（可区分：畸形长度 vs 验签失败）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureError {
    /// 公钥长度不符（ed25519 期望 32 字节）
    MalformedPublicKey {
        /// 实际收到的字节数
        got: usize,
    },
    /// 签名长度不符（ed25519 期望 64 字节）
    MalformedSignature {
        /// 实际收到的字节数
        got: usize,
    },
    /// 验签失败：签名与消息/公钥不匹配，或公钥不是合法曲线点
    VerificationFailed,
    /// OS 熵源不可用（keygen 失败）
    Entropy(String),
}

impl core::fmt::Display for SignatureError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MalformedPublicKey { got } => {
                write!(f, "malformed public key: got {got} bytes, expected 32")
            }
            Self::MalformedSignature { got } => {
                write!(f, "malformed signature: got {got} bytes, expected 64")
            }
            Self::VerificationFailed => write!(f, "signature verification failed"),
            Self::Entropy(reason) => write!(f, "OS entropy source unavailable: {reason}"),
        }
    }
}

impl core::error::Error for SignatureError {}

/// 签名后端零成本抽象（形态对齐 [`crate::Hasher`]：关联函数、无内部状态）
pub trait Signer {
    /// 签名输出（固定大小数组，如 `[u8; 64]`）
    type Signature: AsRef<[u8]> + Copy + Eq + core::fmt::Debug;
    /// 公钥类型（固定大小数组，如 `[u8; 32]`）
    type PublicKey: AsRef<[u8]> + Copy + Eq + core::fmt::Debug;
    /// 私钥类型；实现方必须保证其 `Debug` 输出脱敏（#45 验收：密钥不出现在日志）
    type SecretKey;

    /// 由私钥导出公钥（确定性纯函数）
    fn public_key(secret: &Self::SecretKey) -> Self::PublicKey;

    /// 对消息签名（ed25519 为 RFC 8032 确定性签名，同种子同消息跨平台比特一致）
    fn sign(secret: &Self::SecretKey, message: &[u8]) -> Self::Signature;
}

/// 验签后端零成本抽象（公钥以字节切片传入，便于存储/网络边界直接使用）
pub trait Verifier {
    /// 验签：公钥 + 消息 + 签名 → `Ok(())` 或分类错误
    fn verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> Result<(), SignatureError>;
}
