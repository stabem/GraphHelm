//! The async runtime: the seam where real work replaces the fixture, and the ports that keep
//! the dependency arrow pointing the right way.
//!
//! This crate is a *different kind* of crate from its pure siblings — async, I/O-adjacent —
//! and the boundary is enforced from both sides: `core/execution` must never name this crate
//! (its purity suite would stop meaning anything, runtime-design §9), and this crate must
//! never name an adapter crate (`adapters/model-gateway`, `adapters/tool-host`) — the
//! adapters implement this crate's ports in `apps/cli`'s wiring, never the reverse. Every
//! decision rule stays where Milestone 04 proved it: `next_state` comes from
//! `apply_transition`, and the driver never invents an outcome.

pub mod classify;
pub mod context;
pub mod context_accounting;
pub mod context_compiler;
pub mod driver;
pub mod evidence;
pub mod executor;
pub mod fixture;
pub mod judge;
pub mod owner_output;
pub mod ports;
pub mod prompt;
pub mod retrieval;
