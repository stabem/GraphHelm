//! `graphhelm tool` — the operator surface of the Tool Broker (Milestone 05c Task 9). One
//! command, `invoke`: authorize → route by tier → execute → record, with the captured stream
//! bytes written to the operator's `--capture-out` directory (the `signal --evidence-out`
//! precedent: free-form bytes go to the operator, never into the envelope) and the digest-only
//! [`graphhelm_tool_broker::record::ToolCallRecord`] in the reply.
//!
//! Three failure classes, the sibling-family `Failure` pattern:
//! - `GHCLI012_TOOL_INVALID` — malformed arguments or a request that fails checked
//!   deserialization (`ToolCall::from_json`); messages name the violated rule, never request
//!   bytes.
//! - `GHCLI013_TOOL_DENIED` — `authorize` refused; the message carries the refusal's stable
//!   rule name (`Denied`'s own vocabulary), never call content.
//! - `GHCLI014_TOOL_HOST` — the host failed around the tool; the message carries the host's
//!   stable internal code (`GHTOOL...`), never a path or environment value.

pub(super) mod invoke;

pub(super) const INVALID_CODE: &str = crate::error_codes::GHCLI012_TOOL_INVALID;
pub(super) const DENIED_CODE: &str = crate::error_codes::GHCLI013_TOOL_DENIED;
pub(super) const HOST_CODE: &str = crate::error_codes::GHCLI014_TOOL_HOST;
