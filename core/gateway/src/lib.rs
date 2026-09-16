//! Pure route-manifest and policy types for the Universal Model Gateway.
//!
//! This crate is total and side-effect free: no clock, no randomness, no filesystem, no network and
//! no adapter dependency. It defines the route manifest schema and its structural validation (this
//! task), plus the error taxonomy, health states, capacity-to-outcome mapping and minimal candidate
//! filtering added alongside it. The impure credential broker, BYOK HTTP adapters and native-runtime
//! subprocess adapters live in `adapters/model-gateway`, which depends on this crate rather than the
//! other way around.

pub mod call;
pub mod eligibility;
pub mod judgment;
pub mod manifest;
pub mod taxonomy;
