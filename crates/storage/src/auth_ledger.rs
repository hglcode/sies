//! 认证写入层（任务 #45，审计 P1-7 + §9.2-8）
//!
//! [`Ledger`] 的装饰器：写入路径完成「seq 预分配 → 规范负载签名 → 验签 → 委托落盘」。
//! - `signing` 模式（单定序者，ADR #8）：以本层私钥对最终条目签名后落盘；
//! - `verifying` 模式（验证节点）：只接受自带有效签名的条目，未签名/错签名分别报
//!   [`LedgerAuthError::UnsignedEntry`] / [`LedgerAuthError::SignatureMismatch`]。
//!
//! 签名对象 = [`RawLedgerEntry::signing_payload`]（auth 置空的 postcard 编码，#30 冻结），
//! 覆盖落盘最终形态（含 seq）。签名仅证明「条目来自其声称的签名者」（自洽验证）；
//! 签名者准入与密钥体系属 #67 身份层，不在此实现。

use crate::{EntryAuth, Ledger, LedgerAuthError, LedgerEntry, RawLedgerEntry, Result};
use sies_crypto::{Ed25519SecretKey, Verifier};

/// 认证账本装饰器（#45）
pub struct AuthenticatedLedger<L: Ledger> {
    inner: L,
    signer: Option<Ed25519SecretKey>,
}

impl<L: Ledger> AuthenticatedLedger<L> {
    /// 验证节点模式：不持有私钥，只接受自带有效签名的条目
    #[must_use]
    pub fn verifying(inner: L) -> Self {
        Self {
            inner,
            signer: None,
        }
    }

    /// 定序者模式：以给定私钥对每条写入自动签名（单定序者写路径，ADR #8）
    #[must_use]
    pub fn signing(inner: L, signer: Ed25519SecretKey) -> Self {
        Self {
            inner,
            signer: Some(signer),
        }
    }
}

/// 用给定私钥为「seq 已定稿」的条目生成认证材料
/// （写入路径内部使用；#65 executor / #39 运行时壳同码路径复用）
pub fn sign_entry(entry: &LedgerEntry, signer: &Ed25519SecretKey) -> Result<EntryAuth> {
    let payload = RawLedgerEntry::from(entry).signing_payload()?;
    Ok(EntryAuth {
        public_key: signer.public_key_bytes(),
        signature: signer.sign_bytes(&payload).to_vec(),
    })
}

/// 对条目验签（#45 验收：未签名 / 错签名错误可区分）
pub fn verify_entry(entry: &LedgerEntry) -> std::result::Result<(), LedgerAuthError> {
    let Some(auth) = &entry.auth else {
        return Err(LedgerAuthError::UnsignedEntry);
    };
    let payload = RawLedgerEntry::from(entry).signing_payload()?;
    sies_crypto::Ed25519Backend::verify(&auth.public_key, &payload, &auth.signature)
        .map_err(|_| LedgerAuthError::SignatureMismatch)
}

impl<L: Ledger> Ledger for AuthenticatedLedger<L> {
    fn append(&mut self, entry: &LedgerEntry) -> Result<u64> {
        // seq 预分配：签名必须覆盖落盘最终形态（含 seq）。
        // FileLedger::append 按相同规则（last_seq + 1）再分配一次，二者一致，
        // 因此「已验签的条目 == 落盘的条目」。
        let next_seq = self.inner.last_seq()? + 1;
        let mut candidate = entry.clone();
        candidate.seq = next_seq;
        if let Some(signer) = &self.signer {
            candidate.auth = Some(sign_entry(&candidate, signer)?);
        }
        verify_entry(&candidate)?;
        self.inner.append(&candidate)
    }

    fn read_range(&self, start_seq: u64, end_seq: u64) -> Result<Vec<LedgerEntry>> {
        self.inner.read_range(start_seq, end_seq)
    }

    fn last_seq(&self) -> Result<u64> {
        self.inner.last_seq()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::FileLedger;
    use crate::StorageError;
    use rust_decimal::Decimal;
    use sies_core::belief::Belief;
    use tempfile::TempDir;

    const SEED_A: [u8; 32] = [1; 32];
    const SEED_B: [u8; 32] = [2; 32];

    fn key_a() -> Ed25519SecretKey {
        Ed25519SecretKey::from_bytes(&SEED_A)
    }

    fn sample(user_id: u64) -> LedgerEntry {
        LedgerEntry::new(
            0, // seq 由认证层预分配，这里填占位值
            1,
            user_id,
            Belief::new(vec![Decimal::new(7, 1), Decimal::new(3, 1)]).unwrap(),
            Belief::new(vec![Decimal::ONE, Decimal::ZERO]).unwrap(),
            Decimal::from(2u64),
            Decimal::ONE,
            Decimal::new(-356_674_943_938i64, 12),
        )
    }

    /// 经 signing 模式落盘并读回的已签名条目（供 rejecting 测试构造输入）
    fn signed_entry() -> (TempDir, LedgerEntry) {
        let dir = TempDir::new().unwrap();
        let mut signing =
            AuthenticatedLedger::signing(FileLedger::open(dir.path()).unwrap(), key_a());
        signing.append(&sample(42)).unwrap();
        let entries = signing.read_range(1, 2).unwrap();
        assert_eq!(entries.len(), 1);
        (dir, entries[0].clone())
    }

    #[test]
    fn signing_mode_appends_authenticated_entry() {
        let (_dir, signed) = signed_entry();
        assert_eq!(signed.seq, 1);
        assert!(signed.auth.is_some());
        assert!(verify_entry(&signed).is_ok());
    }

    #[test]
    fn stored_auth_covers_full_payload() {
        let (_dir, signed) = signed_entry();
        let mut tampered = signed.clone();
        tampered.user_id += 1;
        assert!(matches!(
            verify_entry(&tampered),
            Err(LedgerAuthError::SignatureMismatch)
        ));
    }

    #[test]
    fn verifying_mode_accepts_presigned_entry() {
        let (_dir, signed) = signed_entry();
        let dir = TempDir::new().unwrap();
        let mut verifying = AuthenticatedLedger::verifying(FileLedger::open(dir.path()).unwrap());
        assert_eq!(verifying.append(&signed).unwrap(), 1);
        let entries = verifying.read_range(1, 2).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(verify_entry(&entries[0]).is_ok());
    }

    #[test]
    fn verifying_mode_rejects_unsigned_entry() {
        let dir = TempDir::new().unwrap();
        let mut verifying = AuthenticatedLedger::verifying(FileLedger::open(dir.path()).unwrap());
        let err = verifying.append(&sample(42)).unwrap_err();
        assert!(
            matches!(
                err,
                StorageError::Authentication(LedgerAuthError::UnsignedEntry)
            ),
            "未签名条目应以 UnsignedEntry 被拒，实际: {err:?}"
        );
    }

    #[test]
    fn verifying_mode_rejects_tampered_entry() {
        let (_dir, signed) = signed_entry();
        let mut tampered = signed.clone();
        tampered.new_beta += Decimal::ONE;
        let dir = TempDir::new().unwrap();
        let mut verifying = AuthenticatedLedger::verifying(FileLedger::open(dir.path()).unwrap());
        let err = verifying.append(&tampered).unwrap_err();
        assert!(
            matches!(
                err,
                StorageError::Authentication(LedgerAuthError::SignatureMismatch)
            ),
            "篡改条目应以 SignatureMismatch 被拒，实际: {err:?}"
        );
    }

    #[test]
    fn verifying_mode_rejects_signature_from_wrong_key() {
        let (_dir, signed) = signed_entry();
        // 签名本身有效（由 B 签出），但公钥声称是 A —— 签名者与声称主体不符
        let mut forged = signed.clone();
        forged.auth = Some(EntryAuth {
            public_key: key_a().public_key_bytes(),
            signature: sign_entry(&signed, &Ed25519SecretKey::from_bytes(&SEED_B))
                .unwrap()
                .signature,
        });
        let err = AuthenticatedLedger::verifying(MemAppendLedger::default())
            .append(&forged)
            .unwrap_err();
        assert!(
            matches!(
                err,
                StorageError::Authentication(LedgerAuthError::SignatureMismatch)
            ),
            "错签名应以 SignatureMismatch 被拒，实际: {err:?}"
        );
    }

    /// 纯内存账本：拒绝路径不需要磁盘
    #[derive(Default)]
    struct MemAppendLedger {
        count: u64,
    }
    impl Ledger for MemAppendLedger {
        fn append(&mut self, _entry: &LedgerEntry) -> Result<u64> {
            self.count += 1;
            Ok(self.count)
        }
        fn read_range(&self, _start_seq: u64, _end_seq: u64) -> Result<Vec<LedgerEntry>> {
            Ok(Vec::new())
        }
        fn last_seq(&self) -> Result<u64> {
            Ok(self.count)
        }
    }
}
