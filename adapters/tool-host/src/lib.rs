//! The impure half of the Tool Broker: process supervision, Tier 1 workspace provisioning and
//! the builtin tools. Everything here executes what `graphhelm-tool-broker`'s pure `authorize`
//! already decided — this crate never re-decides policy, it enforces the decided plan against
//! the real machine (scrubbed environment, deadlines, output caps, physical containment).

pub mod cache;
pub mod documents;
pub mod host;
pub mod keel_source;
pub mod process;
pub mod session;
pub mod snapshot;
pub mod source_channel;
pub mod source_reader;
pub mod tools;
pub mod verified;
pub mod workspace;
