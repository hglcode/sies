//! SIES Internal RPC Contracts
//!
//! 契约层（任务 #30 冻结基线）：
//! - gRPC 契约：`proto/v1`（包名 `sies.v1`）
//! - REST 契约：`openapi/v1.yaml`（`OpenAPI` 3.1，与 gRPC 同源）
//! - 定点转换：[`conversion`] 模块（10^18 定点 + 十进制字符串，ADR #2）

/// All generated gRPC types and client/server traits for `sies.v1`
#[allow(
    clippy::all,
    clippy::doc_markdown,
    clippy::must_use_candidate,
    clippy::default_trait_access,
    clippy::too_many_lines,
    missing_docs
)]
pub mod v1 {
    tonic::include_proto!("sies.v1");
}

pub use v1::*;

pub mod conversion;

pub use conversion::{
    AuthenticatedLedgerEntry, BELIEF_VECTOR_VERSION, ConversionError, ED25519_PUBLIC_KEY_LEN,
    ED25519_SIGNATURE_LEN, FIXED_POINT_SCALE, belief_to_vector, decimal_from_string,
    decimal_to_string, ledger_entry_from_proto, ledger_entry_to_proto, vector_to_belief,
};
