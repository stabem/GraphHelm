# RFC-1951: Coding scope and placeholder retirement

Status: accepted by the owner on 2026-10-23, issue #1951.

## Decision and evidence

The owner authorized removing unused placeholder adapters and aligning D-004 with the current
GraphHelm, Journey-Proven Development and Keel focus, while preserving working functionality.
The README already describes coding-agent orchestration around software requests, issues, pull
requests and journey evidence. D-004's generalist-product scope conflicted with that focus.

GraphHelm focuses on coding-agent work. Keel governs construction and independent review; JPD
connects user-visible promises to observable evidence. The general graph and Runtime foundations
remain reusable, but do not imply separate product commitments for unrelated domains.

## Retired public adapters

Remove `development resolve-contract`, `development present`, and `development accounting`,
their HTTP paths `/v1/development/contract`, `/v1/development/present`, and
`/v1/development/accounting`, and their MCP names `resolve_contract`, `present`, and `accounting`.
Remove their supported extension tool-surface names as well. These experimental adapters accept
no meaningful result/source/execution input: they resolve an empty list, render a fixed success,
or return unavailable accounting without an execution. Hiding them from MCP discovery did not
retire their callable public paths.

The previously removed top-level `gate` command must also leave command-name error recovery.
An unknown command must not be represented as a recognized gate-family invocation.

## Preservation boundary

- Preserve `compile-context`: it compiles caller-supplied required items through the real Runtime
  producer, enforces the budget, and returns the digest. List it in MCP discovery as a working tool.
- Preserve memory policy reads, admission and authenticated durable publication; Dreams shadow;
  Architect; model gateways; monitor/export; deployment; and journal replay. D-011/D-012 and
  other supporting decisions remain applicable within the coding-agent focus.
- Preserve policy resolution's `resolve_once` binary and the execution/context accounting producers.
- Preserve the exported owner-output Rust contract and its security tests. No other in-repository
  production renderer caller was observed after adapter removal; external library consumers are
  unobserved. Removing that public API requires a separate library compatibility decision, not an
  assumption that removing a placeholder adapter makes every underlying declaration disposable.
- Preserve historical receipts, frozen fixtures, event variants and schema baselines. No stored
  event, graph version, semantic hash, credential or persisted record is migrated or rewritten.

## Alternatives

Keeping hidden placeholders preserves apparent availability but still lets callers obtain empty
or fixed answers. Completing the adapters would add public inputs and behavior beyond this
cleanup. Retire the three adapters together; retain the working implementations and contracts.
A broad subsystem purge is rejected because it would remove working consumers and compatibility.

## Compatibility and proof

This is an intentional experimental-surface removal. CLI callers receive an unknown-subcommand
error, authenticated HTTP callers receive 404, and MCP callers receive unknown-tool rejection.
Extension packages declaring the retired tools must remove those unsupported declarations before
validation; no checked-in extension manifest declares them. Capability fixtures switch to a working
tool while preserving allow/refuse coverage. The supported surface inventory must not claim a
removed tool exists.

Real-binary boundary tests cover retirement, surviving development-family parity, capability
enforcement and argument errors. Regenerate the CLI help and emitted MCP catalog fixtures to
record the deliberate public changes. Run the full reached plan and independently review the
pinned head. Rollback restores the adapter change as a whole; no data migration is necessary.
