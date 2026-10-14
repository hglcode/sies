//! SIES Internal RPC Contracts

/// All generated gRPC types and client/server traits for `sies.v1`
pub mod v1 {
    tonic::include_proto!("sies.v1");
}

pub use v1::*;
