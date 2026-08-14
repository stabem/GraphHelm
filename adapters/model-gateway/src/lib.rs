//! Impure adapters for the Universal Model Gateway.
//!
//! `core/gateway` is the pure route-manifest and policy crate; this crate holds everything that
//! touches the outside world on top of it (`docs/superpowers/plans/2026-08-14-gateway-slice.md`):
//! the credential broker over the sealed key provider (Task 3), the `HttpTransport` boundary and
//! its `ureq` implementation plus the BYOK Anthropic/OpenAI adapters (Task 4), and native-runtime
//! CLI adapters (Task 5).

pub mod broker;
pub mod byok;
pub mod env;
pub mod runtime;
pub mod transport;
