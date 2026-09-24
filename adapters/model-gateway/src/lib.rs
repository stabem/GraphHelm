//! Impure adapters for the Universal Model Gateway.
//!
//! `core/gateway` is the pure route-manifest and policy crate; this crate holds everything that
//! touches the outside world on top of it (gateway-slice plan):
//! the credential broker over the sealed key provider (Task 3), the `HttpTransport` boundary and
//! its `ureq` implementation plus the BYOK Anthropic/OpenAI adapters (Task 4), native-runtime
//! CLI adapters (Task 5), and the System One judge adapter over the same transport
//! (architect-judgments plan, Task 3).

pub mod broker;
pub mod byok;
pub mod env;
pub mod runtime;
pub mod systemone;
pub mod transport;
