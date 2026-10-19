//! SIES Internal RPC Contracts

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
