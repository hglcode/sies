//! ed25519 后端（feature `ed25519`；任务 #45，`tech_stack` 选型兑现）
//!
//! RFC 8032 确定性签名（nonce 由消息与私钥共同派生），纯 Rust 无 unsafe、
//! 无浮点——跨平台比特一致（ADR #2）。

use crate::signature::{SignatureError, Signer, Verifier};

use alloc::string::ToString;
use core::fmt;
use ed25519_dalek::{
    Signature as DalekSignature, Signer as _, SigningKey, Verifier as _, VerifyingKey,
};

/// ed25519 后端单元（与 [`crate::blake2b_backend::Blake2bHasher`] 同构的零成本标记类型）
pub struct Ed25519Backend;

/// ed25519 私钥；`Debug` 恒定脱敏——#45 验收：密钥不出现在日志/输出
pub struct Ed25519SecretKey(SigningKey);

impl fmt::Debug for Ed25519SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Ed25519SecretKey(<redacted>)")
    }
}

impl Ed25519SecretKey {
    /// 从 32 字节种子导入私钥（任何字节串都是合法种子）
    #[must_use]
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self(SigningKey::from_bytes(bytes))
    }

    /// 导出私钥字节（写盘/迁移用；调用方负责文件权限 0600）
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    /// 导出对应公钥（32 字节，可安全入日志）
    #[must_use]
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.0.verifying_key().to_bytes()
    }

    /// 对消息签名（确定性）
    #[must_use]
    pub fn sign_bytes(&self, message: &[u8]) -> [u8; 64] {
        self.0.sign(message).to_bytes()
    }
}

#[cfg(feature = "keygen")]
impl Ed25519SecretKey {
    /// 由操作系统熵源生成新私钥（需 feature `keygen`）
    pub fn generate() -> Result<Self, SignatureError> {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).map_err(|e| SignatureError::Entropy(e.to_string()))?;
        Ok(Self(ed25519_dalek::SigningKey::from_bytes(&seed)))
    }
}

impl Signer for Ed25519Backend {
    type Signature = [u8; 64];
    type PublicKey = [u8; 32];
    type SecretKey = Ed25519SecretKey;

    fn public_key(secret: &Self::SecretKey) -> Self::PublicKey {
        secret.public_key_bytes()
    }

    fn sign(secret: &Self::SecretKey, message: &[u8]) -> Self::Signature {
        secret.sign_bytes(message)
    }
}

impl Verifier for Ed25519Backend {
    fn verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> Result<(), SignatureError> {
        let pk: &[u8; 32] =
            public_key
                .try_into()
                .map_err(|_| SignatureError::MalformedPublicKey {
                    got: public_key.len(),
                })?;
        let sig: &[u8; 64] =
            signature
                .try_into()
                .map_err(|_| SignatureError::MalformedSignature {
                    got: signature.len(),
                })?;
        let verifying =
            VerifyingKey::from_bytes(pk).map_err(|_| SignatureError::VerificationFailed)?;
        verifying
            .verify(message, &DalekSignature::from_bytes(sig))
            .map_err(|_| SignatureError::VerificationFailed)
    }
}
