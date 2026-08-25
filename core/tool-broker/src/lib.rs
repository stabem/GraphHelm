//! Pure effect taxonomy and tier rule for the Tool Broker.
//!
//! This crate is total and side-effect free: no clock, no randomness, no filesystem, no network
//! and no subprocess. It defines the declared tool-effect classes and the rule that maps each
//! effect to the isolation tier it requires — or to a typed refusal for every effect the 05c
//! broker does not support. The impure half (process supervision, workspace I/O, the broker
//! runtime itself) lives in its adapter crate, which depends on this crate rather than the other
//! way around.

pub mod call;
pub mod effect;
pub mod lease;
pub mod mcp_capability;
pub mod path;
pub mod record;
