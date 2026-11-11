//! 契约层定点转换（任务 #30，审计 P1-6 + §9.2-7）
//!
//! - 概率向量：10^18 无符号定点（`BeliefVector`，范本 `RawLedgerEntry`）
//! - 带符号标量（alpha/beta/score）：十进制规范字符串
//! - 全链路无浮点（ADR #2）

use crate::v1::{BeliefVector, EntryAuth, ErrorCode, LedgerEntry as ProtoLedgerEntry};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use sies_core::belief::Belief;
use sies_storage::LedgerEntry;
use thiserror::Error;

/// 定点标度：10^18（与存储层 `RawLedgerEntry` 编码一致）
pub const FIXED_POINT_SCALE: u64 = 1_000_000_000_000_000_000;

/// `BeliefVector` 当前唯一支持的编码版本
pub const BELIEF_VECTOR_VERSION: u32 = 1;

/// ed25519 公钥长度（契约冻结，配合 #45）
pub const ED25519_PUBLIC_KEY_LEN: usize = 32;

/// ed25519 签名长度（契约冻结，配合 #45）
pub const ED25519_SIGNATURE_LEN: usize = 64;

/// 契约转换错误，按统一错误模型映射 [`ErrorCode`]
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ConversionError {
    /// `BeliefVector.version` 不受支持
    #[error("unsupported belief vector version {0}, expected 1")]
    UnsupportedVersion(u32),
    /// 值越界：定点元素必须 ∈ [0, 10^18]
    #[error("value out of range [0, 10^18]: {0}")]
    OutOfRange(String),
    /// 概率值无法用 10^18 定点精确表示（scale > 18 且非整倍）
    #[error("value not representable in 10^18 fixed-point: {0}")]
    NotRepresentable(String),
    /// 归一化/非负等 Belief 不变量不满足
    #[error("invalid belief: {0}")]
    InvalidBelief(String),
    /// 十进制字符串解析失败
    #[error("invalid decimal string: {0}")]
    InvalidDecimal(String),
    /// 认证材料长度不符契约
    #[error("invalid auth length for {field}: got {len} bytes")]
    InvalidAuthLength { field: &'static str, len: usize },
    /// 契约消息缺失必填字段
    #[error("missing required field: {0}")]
    MissingField(&'static str),
}

impl ConversionError {
    /// 统一错误模型映射（契约文档的机器可读形式）
    #[must_use]
    pub fn error_code(&self) -> ErrorCode {
        match self {
            Self::UnsupportedVersion(_)
            | Self::InvalidBelief(_)
            | Self::InvalidDecimal(_)
            | Self::InvalidAuthLength { .. }
            | Self::MissingField(_) => ErrorCode::InvalidArgument,
            Self::OutOfRange(_) | Self::NotRepresentable(_) => ErrorCode::OutOfRange,
        }
    }
}

fn scale_decimal() -> Decimal {
    Decimal::from(FIXED_POINT_SCALE)
}

fn decimal_to_fixed(d: Decimal) -> Result<u64, ConversionError> {
    // RPC 边界对越界报错而非截断（区别于存储层的防御性 clamp）
    if d < Decimal::ZERO || d > Decimal::ONE {
        return Err(ConversionError::OutOfRange(d.to_string()));
    }
    let scaled = d * scale_decimal();
    // 整数性校验：to_u64() 只拦越界，会静默截断小数部分。
    // 落不到 10^-18 格点的值必须显式拒绝（#30 往返无损 / ADR #2）。
    if scaled.fract() != Decimal::ZERO {
        return Err(ConversionError::NotRepresentable(d.to_string()));
    }
    scaled
        .to_u64()
        .ok_or(ConversionError::NotRepresentable(d.to_string()))
}

fn fixed_to_decimal(v: u64) -> Decimal {
    Decimal::from(v) / scale_decimal()
}

/// [`Belief`] → [`BeliefVector`]（10^18 定点）
pub fn belief_to_vector(belief: &Belief) -> Result<BeliefVector, ConversionError> {
    let mut values = Vec::with_capacity(belief.allocations().len());
    for &d in belief.allocations() {
        values.push(decimal_to_fixed(d)?);
    }
    Ok(BeliefVector {
        version: BELIEF_VECTOR_VERSION,
        values,
    })
}

/// [`BeliefVector`] → [`Belief`]（校验版本、值域与归一化）
pub fn vector_to_belief(vector: &BeliefVector) -> Result<Belief, ConversionError> {
    if vector.version != BELIEF_VECTOR_VERSION {
        return Err(ConversionError::UnsupportedVersion(vector.version));
    }
    let mut allocations = Vec::with_capacity(vector.values.len());
    for &v in &vector.values {
        if v > FIXED_POINT_SCALE {
            return Err(ConversionError::OutOfRange(v.to_string()));
        }
        allocations.push(fixed_to_decimal(v));
    }
    Belief::new(allocations).map_err(|e| ConversionError::InvalidBelief(e.to_string()))
}

/// [`Decimal`] → 契约层十进制字符串（alpha/beta/score 编码）
#[must_use]
pub fn decimal_to_string(d: &Decimal) -> String {
    d.to_string()
}

/// 契约层十进制字符串 → [`Decimal`]
pub fn decimal_from_string(s: &str) -> Result<Decimal, ConversionError> {
    s.parse::<Decimal>()
        .map_err(|_| ConversionError::InvalidDecimal(s.to_string()))
}

/// 带认证材料的账本条目（#45 落地前的域侧载体）
#[derive(Debug, Clone, PartialEq)]
pub struct AuthenticatedLedgerEntry {
    pub entry: LedgerEntry,
    pub auth: Option<EntryAuth>,
}

fn validate_entry_auth(auth: &EntryAuth) -> Result<(), ConversionError> {
    if auth.public_key.len() != ED25519_PUBLIC_KEY_LEN {
        return Err(ConversionError::InvalidAuthLength {
            field: "public_key",
            len: auth.public_key.len(),
        });
    }
    if auth.signature.len() != ED25519_SIGNATURE_LEN {
        return Err(ConversionError::InvalidAuthLength {
            field: "signature",
            len: auth.signature.len(),
        });
    }
    Ok(())
}

/// 域侧 [`LedgerEntry`]（+ 可选认证）→ 契约层 `sies.v1.LedgerEntry`
pub fn ledger_entry_to_proto(
    entry: &LedgerEntry,
    auth: Option<&EntryAuth>,
) -> Result<ProtoLedgerEntry, ConversionError> {
    if let Some(a) = auth {
        validate_entry_auth(a)?;
    }
    Ok(ProtoLedgerEntry {
        seq: entry.seq,
        round: entry.round,
        user_id: entry.user_id,
        belief: Some(belief_to_vector(&entry.belief)?),
        outcome: Some(belief_to_vector(&entry.outcome)?),
        new_alpha: decimal_to_string(&entry.new_alpha),
        new_beta: decimal_to_string(&entry.new_beta),
        score: decimal_to_string(&entry.score),
        auth: auth.cloned(),
    })
}

/// 契约层 `sies.v1.LedgerEntry` → 域侧 [`LedgerEntry`]（+ 可选认证）
pub fn ledger_entry_from_proto(
    proto: &ProtoLedgerEntry,
) -> Result<AuthenticatedLedgerEntry, ConversionError> {
    let belief = vector_to_belief(
        proto
            .belief
            .as_ref()
            .ok_or(ConversionError::MissingField("belief"))?,
    )?;
    let outcome = vector_to_belief(
        proto
            .outcome
            .as_ref()
            .ok_or(ConversionError::MissingField("outcome"))?,
    )?;
    if let Some(a) = &proto.auth {
        validate_entry_auth(a)?;
    }
    Ok(AuthenticatedLedgerEntry {
        entry: LedgerEntry::new(
            proto.seq,
            proto.round,
            proto.user_id,
            belief,
            outcome,
            decimal_from_string(&proto.new_alpha)?,
            decimal_from_string(&proto.new_beta)?,
            decimal_from_string(&proto.score)?,
        ),
        auth: proto.auth.clone(),
    })
}
