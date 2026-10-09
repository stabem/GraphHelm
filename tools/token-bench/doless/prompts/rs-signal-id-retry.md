`POST /v1/executions/{id}/signal` answers a recognised idempotent retry (same key, same body, same
actor) with the current status and an `idempotency` block, but without the `signalId` that the first
reply carried, so a client retrying after a lost acknowledgement cannot match it
(`apps/cli/src/commands/serve/mod.rs`, `reply_with_current_status`). Return the original durable
`signalId` on a recognised retry of `execution.signal`; a mismatched actor or a wrong-kind event must
still get no retry proof and no `signalId`. Add the regression assertion to the existing HTTP tests
in `apps/cli/tests/api_http.rs`; it must fail without your change.
