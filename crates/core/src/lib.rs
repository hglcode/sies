// TODO(#38): Test code lint exemptions - remove when fixing pre-existing clippy issues
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreadable_literal
    )
)]

pub mod bayesian;
pub mod belief;
pub mod decay;
pub mod engine;
pub mod scoring;
pub mod swarm;

#[allow(dead_code)] // TODO(#38): serde_helpers reserved for future big-int/bytes fields
pub(crate) mod serde_helpers;
