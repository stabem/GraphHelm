# Runtime

Status: Milestone 05 COMPLETE — 05a through 05f: the Public Runtime API, the Gateway slice, the
Tool Broker with Tier 0/1 isolation, the real async executor, the chat surface, and the read-only
monitor with the acceptance close. The acceptance map (`docs/acceptance/M05_ACCEPTANCE_MAP.md`)
binds every §8 clause to running proof and is itself gate-verified. Design: `docs/specs/2026-08-13-runtime-design.md`;
decisions D-039 (chat-first via an official MCP server) and D-040 (the monitor precedes Studio).

Milestone 04 proved governance and durability with an effect-free executor. Milestone 05 makes it
real: model calls, tool calls, isolation — and the surfaces to operate it all. 05a shipped the
first surface, designed as the multi-agent concurrency contract: several agents can work one
project through the same API without torn state, with every act attributed, every conflict
explicit, and every observation flowing through the shared event log. 05b shipped the second
slice: the Universal Model Gateway's route manifest, error taxonomy and capacity policy as a pure
crate, plus the impure adapters — a credential broker, BYOK HTTP adapters, and native-runtime CLI
adapters — that let a node's model call actually reach a provider or an official CLI, with quota
exhaustion parking the node rather than retrying blind or falling back to a paid route on its own. 05c shipped the third slice: tool calls as brokered local
processes — every call passing a pure authorization pipeline, then executing either in place
(Tier 0 reads) or inside an ephemeral, scrubbed git worktree (Tier 1) that is born and dies within
one call, with credentials structurally absent from the workspace and the absence proven by a named
test rather than asserted. 05d made the work real: the fixture behind the executor seam is replaced
by model calls through the gateway and tool calls through the broker, every reply and stream
sealed as Evidence in the same atomic append as its outcome, the API driving through a truly
async single-writer loop with immediate stop — and the CLI's synchronous path byte-identical
throughout, the parity test as the tripwire it was built to be.

## What 05a shipped: `graphhelm serve`

### The server

A `serve` subcommand in `apps/cli`, on axum `=0.8.9` (ADR-024 in
`docs/reference/REFERENCE_STACK_AND_ADRS.md`: chosen over hand-rolled hyper and actix for the tower
ecosystem and the register's replaceability rules; no handler holds state beyond `ServeState`).
Binding is **loopback-only, fail-closed** — a non-loopback `--bind` is refused with
`GHCLI006_SERVE_INVALID` before anything opens, proven able to fail by a test that also caught its
own first version hanging on the regression it guards (`a_non_loopback_bind_is_refused_fail_closed_before_anything_is_opened`).
In a container the guarantee is the same: the shipped `docker-compose.yml` runs with `network_mode: host`
and `GRAPHHELM_BIND=127.0.0.1:8080`, and no flag relaxes the bind (ADR-039, D-055).

Auth is a bearer token: 32 OS-random bytes, hex, stored **beside** the events directory
(`<dir>.token`), never inside it — the store's layout allowlist rejects foreign entries with
`GHE007_UNSUPPORTED_FORMAT`, and the first draft that wrote the token inside the directory poisoned
the whole repository until moved. Comparison is constant-time by a hand-written fold, matching the
D-038 minimalism precedent. `/health` is the only unauthenticated route; unknown routes are 401
before they are 404. mTLS and non-local identity are deferred with the architecture's full §3.1
posture — recorded, not forgotten.

### One store, one truth

The server holds **no repository handle**: every request opens and drops the store exactly as every
CLI command does, because `LocalEventRepository::open` holds an OS-exclusive lock for the handle's
lifetime — a long-lived handle would lock out concurrent CLI processes and kill the multi-agent
premise. The per-request lock serialization *is* the concurrency model; the API surfaces it as HTTP
semantics rather than reinventing it.

### The endpoint contract

All under `/v1`, all replying the same four-key envelope the CLI prints. Reads: `status` (carrying
`headSequence` — the one shared-surface change to the command layer itself, the CLI gained it too)
and a paged events tail (`after` exclusive, `limit` default 100 max 1000, over-limit refused with
400 — never silent truncation) serving raw envelopes, legitimate because D-036 keeps free-form
content off the wire. A fresh mutation's own reply carries `headSequence` too, but that one is an
API-reply enrichment added in the serve layer alone (`run_idempotent_mutation`), not a change to
any command's return shape — the CLI's own mutation output is unchanged.

Mutations require three headers, validated before the store is touched: `Idempotency-Key` (an
`OpaqueId`, capped at 64 characters — see below), `X-GraphHelm-Actor`, and
`X-GraphHelm-Actor-Type: owner | agent` — `human` awaits Studio auth and `system` is reserved for
the driver's own hops, which keep their 04f attribution. `If-Match: <head-sequence>` is optional; a
stale head is refused with 409 carrying `data.currentHead`, checked after the idempotency
pre-flight (a completed command's retry succeeds even with a stale `If-Match`) and immediately
before the command body; the store remains the authority underneath. A fresh mutation success now
also carries `data.headSequence` (the same current head a following status read would report),
closing the gap that previously forced an extra GET per `If-Match`-chained write — CLI output is
unchanged, since this is a serve-layer enrichment of the API reply, not a change to any command's
own return shape.

### Idempotent retry, content-aware

The store **rejects** a duplicate idempotency key at a freshly-advanced sequence
(`GHE003_IDEMPOTENCY_CONFLICT`) — verified empirically before anything was built on it. Retry
semantics rest on a pre-flight: each mutation's *decision event* (the single always-present event
meaning "this command happened") derives its key from the caller's command key plus a fixed suffix
plus a 16-hex-character digest of the request's content (command name, execution id, canonical JSON
body — `{command-key}-{suffix}-{digest16}`), and `run_idempotent_mutation` classifies the decision
keys directly against the stream history `resolve_stream` already loads (no extra store open) as
one of four states:

- **Absent** — genuinely fresh; the command runs.
- **Complete** — a committed event's key matches exactly (suffix *and* digest): the command already
  happened. 200 with current state (`approve`'s own precondition would otherwise reject its own
  retry).
- **Partial** — some but not all of a command's expected keys are committed, possible only if a
  previous attempt died mid-command. 409 naming the stuck key.
- **Divergent** — a committed event's key shares this command's `{command-key}-{suffix}-` prefix but
  carries a *different* digest: the caller reused one `Idempotency-Key` for two different request
  bodies. 409 (`GHE003_IDEMPOTENCY_CONFLICT`) naming the reused key, refusing before the store is
  touched.

The digest is what makes `Complete` and `Divergent` the same check rather than two: presence alone
used to be the whole test, and a caller reusing a key across two different bodies (`approve
{"node":"a"}` then `approve {"node":"b"}` under the same key) was silently absorbed as a completed
retry of the first, the second request's real effect never applied — found in 05a's final review
and closed by folding the digest into the key itself, rather than adding a second check alongside
the old one. It is a divergence *detector*, not a security boundary: a caller can only ever collide
with its own past requests, made under its own bearer token. The header is capped at 64 characters
so the derived key (header + 2 dashes + longest suffix `"cancelled"` (9) + digest (16) = 91 at the
maximum) stays comfortably inside `OpaqueId`'s 128-character limit.

The fan-out a command appends beyond its decision event — pause's per-node holds, the drive loop's
hops — keeps fresh keys, because a variable count cannot be indexed by fixed suffixes; the
pre-flight keying on decision events alone is what makes that safe. Retrying with the same key and
the same body appends nothing, sabotage-proven twice: reverting key derivation to fresh UUIDs
doubles the head on a byte-identical retry, and dropping the digest back out of the derivation lets
a divergent-body reuse be silently absorbed instead of refused.

### Attribution

Owner and agent acts are recorded as the caller (`PersistedActorType::{Owner, Agent}` from the
headers, serialized as the actor's `"type"` field); the driver's hops stay `System`. On the API
path `execution_started` is attributed to the caller while `drive_to_quiescence` runs under a
fresh system actor — start's CLI behaviour is unchanged.

### The storm, and what it proved

`the_storm_holds_under_eight_concurrent_agents`: eight OS threads, per-thread actors, six rounds of
interleaved pause/resume/signal/status, retry-once on 409. Held across three consecutive runs:
never a 500 or hung connection, coherent replay, byte-identical double replay, every mutation
attributed to one of the eight or the system driver. The deliberate sabotage — fresh UUID keys plus
a dropped pause precondition — was caught **synchronously**: the fold's own guard rejected the
incoherent history inside the very request that caused it (`ReplayError::Corrupt` → 500), which is
the oracle working one step earlier than designed.

Two agents also close the information-sharing loop through the API alone: scout signals, builder
observes it in the tail and approves, scout observes the approval — nothing shared but the URL and
the token.

### Parity, pinned

`the_cli_and_the_api_report_identical_status_for_the_same_story` drives one scripted story through
both surfaces against fresh stores and asserts identical final `status` data with an **empty**
exception list — it passed on its first run, meaning D-039's "never a second path" already holds
rather than being aspired to. When 05d swaps the driver under the API, this test is what proves the
surfaces did not drift. The `api_http` suite is a named gate stage, proven able to go red.

## Honest limits, stated (05a)

- **No documentation surface yet.** Checking and writing docs as first-class endpoints is the
  Living Documentation subsystem, reserved in the architecture's API surface
  (`documents/claims/artifacts`, §3.1) and arriving as its own plan into this same API. Until then
  agents coordinate through events, signals and status — which is what the harness defines signals
  for. This is the next coordination increment of the multi-agent vision, not an absence.
- **Throughput.** The server runs the CLI's current-thread tokio runtime with synchronous
  per-request store I/O — measured ≈3 requests/second under eight-thread load, which is why the
  storm is six rounds. Real dispatch concurrency arrives with 05d's async driver; this number is
  recorded so 05d has a baseline to beat, not as an accepted end state.
- **An unknown execution id is a 404 (reversed by #1083, ADR-038).** As shipped in 05a the store
  returned an empty stream for a valid-but-absent id and status/briefing mirrored that as a calm
  `200` with every field null and `attention: can_sleep`, which the integrated MVP verification
  measured as a lie to the operator. Since #1083 `GET /v1/executions/{id}` and `/briefing`, the
  CLI `status`/`briefing`, and the MCP tools refuse a well-formed id that names no stream with
  `GHCLI028_EXECUTION_NOT_FOUND` at `/execution`. The events tail still answers an empty page,
  and the render a mutation replies with never refuses; CLI/HTTP parity is kept because both call
  the same guard. The distinct signal is produced in the command layer, not the store.
- **Bearer-on-loopback only.** mTLS and any non-local identity remain §3.1 work, refused rather
  than half-built: the server will not bind a non-loopback address at all.

## What 05b shipped: the Gateway slice

### The two crates, and the purity boundary

`core/gateway` (`graphhelm-gateway`) is pure — no clock, no randomness, no filesystem, no network,
no subprocess — enforced the same way `core/execution`'s purity is: a source-invariant test
(`core/gateway/tests/source_invariants.rs`) pins the `[dependencies]` table to exactly
`graphhelm-protocols`, `serde`, `serde_json` (Task 1 needed no error-derive crate — `ManifestError`
and `GatewayError` each carry a hand-written `Display`, matching `BrokerError`'s convention
elsewhere in the workspace) and scans every file under `src/` for `std::fs`, `std::time`,
`std::process`, `rand`, and the concrete I/O types `std::net` exposes
(`TcpStream`/`TcpListener`/`UdpSocket`/`ToSocketAddrs` — the bare `std::net` path is allowed because
`manifest.rs` legitimately imports `Ipv4Addr` for a pure loopback-host parse, not a socket). It
holds the route manifest (`manifest.rs`), the error taxonomy and capacity policy (`taxonomy.rs`),
minimal candidate filtering (`eligibility.rs`), and the wire-neutral call/reply types both adapter
families share (`call.rs`).

`adapters/model-gateway` (`graphhelm-model-gateway`) is impure and depends on `core/gateway`, never
the other way — the same source-invariant test explicitly forbids `core/gateway` from naming
`graphhelm-model-gateway` or `adapters/` at all. It holds the credential broker (`broker.rs`), the
`HttpTransport` boundary and its `ureq` implementation (`transport.rs`), the Anthropic/OpenAI BYOK
adapters (`byok.rs`), the native-runtime CLI adapters (`runtime.rs`), and a test-fixture binary that
imitates a host CLI (`src/bin/fake_runtime.rs`, doc-commented as such so it cannot be mistaken for
production code).

### The route manifest: billing and transport as one structural fact

`RouteManifest::from_json` (`core/gateway/src/manifest.rs`) is the only way to obtain a manifest,
and it enforces every rule in one fixed order: a byte bound (`MAX_MANIFEST_BYTES`, 256 KiB) before
anything is parsed, then `serde` with `deny_unknown_fields`, then each route's structural rules,
then a duplicate-id scan (`MAX_ROUTES`, 64). §20's "BYOK and subscription are distinct billing
relationships" is not left as a convention the caller has to hold by hand — it is a structural pair
the manifest enforces per route: a `direct_api` route must declare `authentication: api_key`,
`billingMode: per_token`, `baseUrl`, `model`, and `credentialRef`, must name `provider` as
`"anthropic"` or `"openai"` (the two wire shapes the BYOK adapters actually speak — Task 4 found the
manifest didn't constrain this and closed the gap), and must not carry `runtime`/`command`; a
`native_runtime` route must declare `authentication: account_subscription`,
`billingMode: subscription_quota`, `runtime`, and a non-empty `command.program`, and must not carry
`credentialRef`/`baseUrl` at all — the manifest cannot route a broker secret into a runtime that
owns its own auth, structurally rather than by convention. A route id is 1–64 characters of
`[a-z0-9_]`. `baseUrl` may use `https://` unconditionally; `http://` is refused unless the host
parses as loopback (`localhost`, `127.0.0.0/8`, or `::1`) — cleartext is a misconfiguration anywhere
else. Every route also carries `timeoutSeconds`, defaulted to `DEFAULT_TIMEOUT_SECONDS` (300) via
`serde`, so no existing manifest needs to declare it; only the native-runtime adapter reads it
today. `ManifestError`'s `Display` names a route id and the violated rule (or, for `Oversize`, a
byte count) and never echoes the manifest's own bytes back — the CLI leans on exactly this property
(below). Ten tests in `core/gateway/tests/manifest_contract.rs` cover this, including a
non-loopback `http://` refusal, an unrecognized-provider refusal, a `timeoutSeconds` bound
(`MAX_TIMEOUT_SECONDS`, 86,400s) rejecting an oversized value while accepting the boundary exactly,
and two marker-planting proofs that a parse failure's `Display`/`Debug` carry only its
line/column/category and never `serde_json`'s own message.

### The error taxonomy, route health, and the capacity table

`core/gateway/src/taxonomy.rs` defines `GatewayError`, the fourteen-kind closed vocabulary §17
names verbatim, and `outcome_for_error`, one exhaustive `match` with **no wildcard arm** mapping
each kind to the `NodeOutcome` the execution engine records:

| Outcome | Errors |
|---|---|
| `NeedsCapacity` | `QuotaExhausted`, `RateLimited`, `AuthRequired`, `AuthRevoked` |
| `RetryableFailure` | `ProviderUnavailable`, `Timeout`, `RuntimeCrashed`, `MalformedOutput` |
| `TerminalFailure` | `ContextTooLarge`, `ModelRemoved`, `UnsupportedCapability`, `PolicyDenied`, `ToolDenied` |
| `Cancelled` | `Cancelled` |

The absence of a wildcard arm is deliberate: a fifteenth `GatewayError` variant without a chosen
consequence breaks the build here rather than silently defaulting. `health_for_error` is a separate,
narrower question — what one call's failure implies about the *route* going forward, not the node —
and only four classes answer it: `QuotaExhausted`/`RateLimited` → `WaitingReset`,
`AuthRequired`/`AuthRevoked` → `AuthRequired`, `ProviderUnavailable` → `Degraded`; everything else (a
context that was too large this one time, a cancellation) is `None` and leaves route health where
the caller last observed it. `RouteHealth` itself is the six states §18 names:
`Available`/`Degraded`/`WaitingReset`/`AuthRequired`/`Unavailable`/`Disabled`. Four tests in
`core/gateway/tests/capacity_mapping.rs` cover totality, the capacity-class grouping, health
updates, and `eligible_routes` (`eligibility.rs`): given a manifest, a caller-supplied health map
(the pure crate holds no registry of its own), and `Requirements { profile, subscription_only }`, it
returns the enabled routes whose health is `Available` or `Degraded`, serving the requested
`WorkProfile`, excluding `PerToken` routes when `subscription_only` is true (§19's user control) —
in manifest order, since scoring within that set (§8.3) is deferred.

### The credential broker: `EvidenceProtector` reuse, no new cryptography

`adapters/model-gateway/src/broker.rs`'s `CredentialBroker` invents no cryptography of its own: it
wraps `EvidenceProtector<SealedKeyProvider>` exactly as `core/events` already defines it — `store`
seals a value with `EvidenceProtector::seal`, `lease` opens it with `EvidenceProtector::open` — and
owns only a durable index, `credentials.json` inside `broker_dir`, written atomically (tmp file,
then rename). Each entry persists a `SecretReference` (`id`, `provider`, `usable_by`, all bounded
`[a-z0-9_.-]`/`[a-z0-9_]` tokens), a `revoked` flag, and the full parts of its `SealedEvidence` —
reference, scope, media type (`application/octet-stream`, since a credential is opaque bytes),
sensitivity, retention class, algorithm, nonce, ciphertext, and the wrapped key's own parts — with
the binary fields hex-encoded, reconstructed on every open via `SealedEvidence::new`/
`WrappedKey::new`. Every credential shares one fixed `RepositoryScope` (workspace `gateway`, project
`credentials`): the broker is not multi-tenant within a keyring, so Evidence scoping exists here to
separate records, not to namespace one operator's own store. `lease(id, route_id)` checks
`usable_by` before ever touching the sealed bytes — a route not in that list gets
`BrokerError::NotUsableByRoute`, never a value — and a revoked credential (durable across a broker
reopen) can never be leased again. `list()` returns ids, providers, routes and the revoked flag,
never a value; `CredentialBroker` caches no plaintext at all — `lease` re-derives `SecretBytes`
fresh from sealed storage every call. Five tests in `adapters/model-gateway/tests/broker.rs` prove
this, including `a_tampered_store_file_fails_closed`: flipping one byte of the persisted ciphertext
on disk is caught by AEAD authentication, so `lease` returns `Err` — there is no partially
constructed `SecretBytes` to leak — and `broker_errors_and_listings_never_carry_the_value`, which
formats every reachable error variant and asserts neither the sentinel credential nor the passphrase
ever appears. The broker's own tests need no real async runtime — like `core/events`'s, they drive
`EvidenceProtector`'s plain async fns with a hand-rolled, thread-parking `block_on`; the CLI (below)
bridges the same functions through a real `tokio` current-thread runtime instead, identical in shape
to `commands::events`'s own precedent.

### ADR-025: `ureq`, and the no-second-TLS-stack finding

The BYOK adapters place outbound HTTPS calls, and nothing before this milestone had pinned an
outbound HTTP client — ADR-024 pinned `axum` for 05a's *server* side only. ADR-025
(`docs/reference/REFERENCE_STACK_AND_ADRS.md` §30) pins `ureq = "=3.4.0"`, exact and
workspace-managed. Default features resolve, per `cargo tree -p ureq -e features`, to exactly
`rustls` and `gzip` — no `native-tls`, no OpenSSL, no unused extras. The finding that matters most:
this is **not a second TLS stack** entering the tree. `cargo tree -i ring --locked` confirms `ureq`'s
`rustls` (`0.23.43`) and `ring` (`0.17.14`) resolve to the identical versions `sqlx`'s existing
`tls-rustls-ring-native-roots` feature already pinned for the Postgres event store — one crypto
provider, shared, not two to audit. `UreqTransport` (`transport.rs`) configures its agent with
`Agent::config_builder().http_status_as_error(false).max_redirects(0).build()`, so a non-2xx
response comes back as an `Ok(TransportResponse)`, never an `Err` — status interpretation belongs
entirely to `byok.rs`'s per-provider mapping tables. `max_redirects(0)` is the milestone's final
review closing a real gap: ureq's own redirect handling strips only `Authorization`/`Cookie`/
`Content-Length` from a re-sent request, so Anthropic's `x-api-key`/OpenAI's `Authorization: Bearer`
would otherwise survive to whatever host a 302 named; disabling redirects outright (`/v1/messages`
and `/v1/chat/completions` never legitimately redirect) still returns the 3xx as `Ok`, never a
`TooManyRedirects` error, so `byok.rs`'s own catch-all status mapping is what turns it into
`MalformedOutput`. The constraint ADR-025 records: no code outside `transport.rs` may name `ureq`
directly; `byok.rs` speaks only `TransportRequest`/`TransportResponse`/`HttpTransport`, the same
seam its own tests substitute a local `TcpListener` fake behind.

### BYOK Anthropic and OpenAI adapters

`ByokAdapter::call` (`byok.rs`) dispatches on `route.provider()`. Anthropic gets
`POST {base}/v1/messages` with `x-api-key` and `anthropic-version: 2023-06-01`, body
`{model, max_tokens, messages}`; OpenAI gets `POST {base}/v1/chat/completions` with
`Authorization: Bearer <key>`, body `{model, messages}` — `max_tokens` is deliberately not forwarded
to OpenAI (its chat-completions API splits token-limit parameters by model family in ways this
milestone does not resolve). Status mapping is a fixed table per provider, not a heuristic, with one
genuine judgment call: OpenAI's `429` is ambiguous between hard quota exhaustion and ordinary
throttling, resolved only by sniffing the error body's `type`/`code` for `"insufficient_quota"`
(`openai_error_is_insufficient_quota`) — everywhere else, `401` → `AuthRequired`, `403` →
`PolicyDenied` (both providers), Anthropic's `429` → `RateLimited` and `529` → `ProviderUnavailable`,
any other unrecognized `>=500` → `ProviderUnavailable`, and anything else unmapped or a 2xx body
that does not parse → `MalformedOutput` — a mystery reply must not park capacity (`NeedsCapacity` is
reserved for the classes §12 names explicitly), but it also must not invent a more specific meaning
it cannot support. Usage is always `Option<u64>`, filled only from a figure the provider actually
reported — an absent `usage` object is `None`, never a guessed zero (§11). `TransportRequest`'s
manual `Debug` redacts every header *value* while still naming the header, and
`BYOK_REQUEST_TIMEOUT` is a fixed 60-second constant (the manifest carries no per-route timeout for
`direct_api` routes, unlike native-runtime's `timeoutSeconds`). Fifteen tests in
`adapters/model-gateway/tests/byok_adapters.rs` — a from-scratch `TcpListener` fake-server harness
mirroring `apps/cli/tests/api_http.rs`'s pattern — cover both providers' success paths, every status
above, absent-usage, `the_api_key_never_appears_in_errors_or_debug` (plants the real sentinel key in
a live request and asserts it survives in the captured HTTP headers, proving the test is not
vacuous, while never appearing in any formatted error or `Debug` output), a planted 302 redirect
proving `UreqTransport` never follows it and the redirect target receives no connection at all, and
a `native_runtime` route handed to `ByokAdapter` being refused with `UnsupportedCapability` rather
than panicking on the `baseUrl` it structurally cannot carry.

### Native-runtime adapters: env isolation, stdin-only prompts, deadlines

`RuntimeAdapter::call` (`runtime.rs`) spawns `route.command.program` with `route.command.args` and
speaks to it over stdio only — never a network call the gateway places itself. The child's
environment is built from `env_clear()` plus a fixed `ENV_ALLOWLIST` (`PATH`, `PATHEXT`,
`SYSTEMROOT`, `SYSTEMDRIVE`, `COMSPEC`, `WINDIR`, `TEMP`, `TMP`, `USERPROFILE`, `HOME`, `APPDATA`,
`LOCALAPPDATA`, `PROGRAMDATA`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`) copied verbatim from the parent,
plus whatever the caller passes as `extra_env` — production callers pass nothing beyond the
allowlist; only the test suite uses `extra_env`, to steer `fake_runtime`. `PATHEXT` is a deliberate
addition beyond the plan's own list, needed for Windows' `PATH` program resolution. `HOME` and
`APPDATA` stay on the allowlist on purpose: §6.3's separation is that a native runtime owns its own
authentication — its own login, its own config directory, its own token store — and the gateway
never touches it, so the CLI needs its ordinary config-location variables to find *its own* auth,
not the gateway's. This is not yet the stricter Tier 1 sandbox posture; 05c's tool-broker isolation
is the milestone that tightens this further, and is not built yet. The prompt travels over stdin
only — never argv, never an environment variable — written on its own background thread, spawned
before the reader threads and the deadline loop start and joined only after the child has exited or
been killed: writing directly on the calling thread (the pre-final-review shape) blocks until the
child drains its stdin, and a prompt bigger than the OS pipe buffer against a stalled or
never-reading child (`FAKE_RUNTIME_MODE=hang`) hung there forever, before the deadline logic below
ever ran at all. Stdout/stderr are drained on background threads concurrently with the same 50ms
`try_wait` poll loop against the deadline (`route.timeout_seconds()`), so a child that writes more
than one pipe buffer of output cannot be mistaken for hung either; on deadline the child is killed
and reaped, which unblocks a still-in-flight stdin write with a harmless `BrokenPipe` (joined
alongside the output threads), and the call reports `GatewayError::Timeout` — now true
unconditionally, for an oversized prompt exactly as for a slow-to-exit child.

Successful decoding then depends on `RuntimeKind`. `ClaudeCode` expects one JSON object with a
`result` string and optional `usage`; `subtype`/`is_error` can instead mark that object as an error
report, including on exit `0`. Codex decoding requires complete captured stdout: exceeding the
16 MiB capture bound or failing to read the pipe through EOF refuses the transcript before parsing
(`MalformedOutput` on exit `0`, `RuntimeCrashed` otherwise). Readers still drain overflow bytes
without retaining them, and a complete stream of exactly 16 MiB is not an overflow. A captured
prefix cannot establish success or quota exhaustion while omitted events remain unknown.
`Codex` supports two deliberately separate JSONL formats:

- The current format uses top-level typed events. `thread.started` requires a string `thread_id`;
  `turn.started` has no required payload. A successful reply requires a nonblank
  `item.completed` whose `item.type` is `agent_message`, followed by terminal `turn.completed`.
  All three item lifecycle events (`item.started`, `item.updated`, `item.completed`) require an
  object-valued `item` with a string `type` and optional string `text`. Missing or malformed
  common item payloads invalidate the transcript even when that item is otherwise ignored;
  additional item fields remain forward-compatible and are not executed by this decoder.
  `turn.completed.usage.input_tokens` and `output_tokens` populate the corresponding optional
  usage fields when reported; an absent usage object or field remains `None`. Present usage and
  error payloads must be objects (or null for the existing optional absence case); array-shaped
  objects are refused, and duplicate known fields retain the typed decoder rejection. An
  `item.completed` whose `item.type` is `error` is advisory rather than terminal. A top-level
  `error` may be a reconnect notification: retain its text provisionally and use the subsequent
  terminal outcome. A valid `turn.completed` can succeed after such notices only on process exit
  `0`; `turn.failed` remains a typed failure on either exit status. Every remaining envelope is
  validated before classifying a terminal failure: malformed, unknown, or non-object records
  invalidate the transcript rather than establishing quota exhaustion. Later valid events cannot
  rescue or replace the retained terminal failure. At end of stream without a
  terminal outcome, classify the last provisional error rather than returning a reply. An error
  after terminal completion still fails the one-shot stream. Failed, incomplete,
  malformed, mixed-format, and unknown-current-event streams cannot fall through to a successful
  reply, and an earlier message or completed turn cannot rescue a later failed or incomplete turn.
- The legacy compatibility format keeps the historical tolerant nested-`msg` scan. On exit `0`,
  unrelated or malformed lines, missing-message records, and error records are skipped and the
  last valid `agent_message` wins. On nonzero exit, the last parsed nested error message is used
  for classification. This legacy format does not report usage known to the adapter, so its usage
  fields remain `None`. Unrelated typed log records and unrelated top-level `type` metadata
  do not select the current format or hide a valid nested message. A stream containing both
  recognized current and legacy envelopes is refused; once current events select the strict
  decoder, unknown events remain invalid.

Failure classification scans only those parsed error fields: Claude's `result`/`error`, current
Codex `error.message` or `turn.failed.error.message`, and legacy Codex nested error messages.
`QUOTA_MARKERS` (`["quota", "rate limit", "usage limit"]`) is a documented case-insensitive
substring heuristic over that parsed text. Captured raw stdout and stderr are never scanned for
quota markers. A nonzero exit without a parsed error is `RuntimeCrashed`; an exit-`0` body with no
valid reply is `MalformedOutput`.

At the original Task 5 completion, eight tests in
`adapters/model-gateway/tests/runtime_adapters.rs` covered both then-supported happy shapes,
quota/crash/hang/stdin routing, the stdin-write regression above (a 1 MiB prompt against a child
that never reads stdin at all, which hung indefinitely pre-fix), and — the one requiring the most
care —
`the_child_environment_is_an_allowlist_and_never_carries_broker_material`: since mutating this
process's real environment from inside a test would race every sibling test reading it concurrently,
this test re-execs the compiled test binary as a child with two sentinel variables set only on that
`Command`, and has the grandchild (`fake_runtime`, `env-dump` mode) report over a temp file exactly
what it received — proving the sentinels are filtered rather than merely asserting they were never
present. The guard is not hypothetical: Task 5's own sabotage (commit `86e518a`) removed
`env_clear()` from the spawn path and reran this test, which failed by dumping the shell's entire
ambient environment — including a real, live `SENTRY_AUTH_TOKEN` — into the fake child; restored,
with the test green again. That count is a historical milestone record. The expanded current and
legacy Codex decoder tests and live compatibility evidence are recorded in the
[GPT-6 Astra migration acceptance record](../acceptance/gpt-6-astra-migration.md).

### The quota-free probe

`graphhelm gateway probe` (§18: "health probes must not consume excessive quota") never places a
real model call. For a `direct_api` route it attempts exactly one `CredentialBroker::lease` and
reports only whether it succeeded — no network call, no value ever printed. For a `native_runtime`
route it spawns `command.program --version` (never the manifest's own configured `args` — this is a
liveness probe of the CLI, not a real invocation) under the identical `env_clear()` + allowlist
discipline as `runtime.rs`'s adapter, with a fixed 10-second budget independent of the route's own
`timeoutSeconds`. `health` reports `available` on success, `auth_required` for a denied `direct_api`
lease (revoked, wrong route, or unknown id — a fact about the route, not a CLI failure), or
`unavailable` for a `native_runtime` spawn/exit failure. What it deliberately does not check: whether
the model itself still has remaining quota — that can only be learned by placing a real call, which
a probe by definition never does.

### The CLI surface and codes

`graphhelm gateway routes|probe|credential set|remove` (`apps/cli/src/commands/gateway/`), following
`commands::events`/`commands::execution`'s existing four-key-envelope pattern exactly. Three new
failure codes: `GHCLI009_GATEWAY_INVALID` (manifest fails to load or validate, or an argument is
malformed, missing, or names an unknown route — never echoes manifest bytes, leaning on
`ManifestError::Display`'s own redaction), `GHCLI010_GATEWAY_CREDENTIAL` (the broker itself could not
be used: a missing/malformed `GRAPHHELM_GATEWAY_KEY`, a keyring directory that does not exist yet, or
any `BrokerError`), and `GHCLI011_GATEWAY_PROBE` (a probe-specific refusal neither code above covers
— this milestone's one case, probing a route the manifest marks `enabled: false`). `credential
set`/`remove` read the credential value from stdin as one trimmed line, never as an argument, and
print only `{id, provider, routes}` — never the value; the passphrase comes from
`GRAPHHELM_GATEWAY_KEY` (64 lowercase hex characters), mirroring the `GRAPHHELM_EVENTS_KEY`
precedent in `commands::events::config` byte-for-byte. Seven tests in `apps/cli/tests/gateway_cli.rs`
cover a valid manifest, a manifest refused without leaking a planted marker string, a manifest that
fails to *parse* (rather than merely fails a structural rule) refused the same way, credential
set-then-probe going green, a native-runtime probe spawning the `graphhelm` binary itself as its own
`--version`-answering fixture, credential remove flipping probe to `auth_required`, and a missing
passphrase being refused as `GHCLI010`. `gateway_cli` is now a named stage in `ci/gate.ps1`'s CLI
suite list, alongside `cli_smoke`, `schema_cli`, `event_store_cli`, `execution_cli`, and `api_http`.

## Honest limits, stated (05b)

- **JSON manifests, not YAML.** §4's own example is YAML; this milestone's on-disk manifest is JSON
  (`RouteManifest::from_json`). Revisit when Studio needs to author or edit one directly.
- **Router scoring (§8.3) and local benchmarks (§10) are deferred.** `eligible_routes` filters to a
  candidate set and returns it in manifest order; nothing chooses among several eligible routes yet.
- **The broker's access-audit ledger (§7.1) is deferred.** `CredentialBroker` durably stores,
  leases, and revokes, but keeps no record of which lease happened when or for which node.
- **Three of the six route types in §2 are not built.** Only Direct API/BYOK (§2.2), Native
  runtime (§2.3) and the System One judgment family (§2.6, served over the `direct_api` transport
  by `adapters/model-gateway/src/systemone.rs` on the architect's judge door, #1109) exist; the
  Aggregator (§2.1), OpenAI-compatible endpoint (§2.4), and local embedded runtime (§2.5)
  transports are deferred.
- **Session management (§13) is out of scope.** Every call is stateless — `ModelCall` carries a
  prompt and a token budget, never a session reference — so native-runtime history and session
  resume do not exist yet.
- **Neither tool-use strategy from §14 is built.** §14.1's gateway-native tool calls do not exist —
  a `ModelReply` is text and usage only. Nor does this milestone implement §14.2's requirements for
  a native runtime's own agent tools: when a `native_runtime` route spawns Claude Code or Codex,
  whatever tools that CLI invokes internally are not intercepted, logged, permission-mapped, or
  workspace-confined by anything in `adapters/model-gateway` — the env allowlist bounds what the
  spawned *process* can read from the environment, not what the *CLI itself* does with its own tool
  use once running. That mediation is 05c's Tool Broker and Tier 0/1 isolation.
- **The host-CLI JSON shapes are fixtures, not verified contracts.** `fake_runtime.rs`'s "happy
  shapes" — the Claude Code `result` object, the Codex JSONL `agent_message` — are this repository's
  documented understanding as of this milestone, not a claim about either CLI's real, versioned
  output. 05e re-verifies against the live hosts.
- **`QUOTA_MARKERS` is a text-sniffing heuristic, not a wire contract.** Neither host CLI this
  milestone targets publishes a stable, versioned "out of quota" exit shape to key off instead.
- **`ModelCall::max_tokens` is accepted but never forwarded to a native-runtime CLI.** The route's
  own `command.args`, set once in the manifest, fully controls the invocation; there is no per-call
  channel to carry it through beyond the prompt itself.
- **The BYOK timeout is a hardcoded constant, not manifest-driven.** `BYOK_REQUEST_TIMEOUT` (60s)
  applies to every `direct_api` route alike, unlike native-runtime's per-route `timeoutSeconds`.
- **The probe checks reachability, never remaining model quota.** A successful lease or a clean
  `--version` exit proves the credential or the program is usable; it says nothing about whether the
  next real call would succeed against the provider's own limits — learning that requires placing
  the call, which §18 forbids a health probe from doing.
- **`credentials.json` persists an unsalted SHA-256 of each credential's plaintext.** This is
  `EvidenceProtector`'s own content digest (`content_sha256`, computed at seal time and later
  written verbatim into the broker's persisted entry) — not a hash the broker invented for
  itself — but storing it durably alongside the ciphertext means an attacker who obtains the file
  can run an offline dictionary or rainbow-table check against it for a low-entropy credential.
  Reusing `EvidenceProtector` exactly as `core/events` already defines it (this section's own "no
  new cryptography" framing above) is what keeps the digest unsalted here; revisit if the Evidence
  pipeline itself grows a salted variant.
- **`ContextTooLarge` and `ModelRemoved` currently have no producer.** Both are closed-taxonomy
  members with a chosen outcome (`TerminalFailure`, `core/gateway/src/taxonomy.rs`), but neither
  BYOK provider's status-mapping table (`byok.rs`) nor the native-runtime adapter ever returns
  them: Anthropic's and OpenAI's own 400/404 responses fall through the unmapped-status catch-all
  to `MalformedOutput` instead, so a call against a context the model genuinely cannot accept is
  retried as a transient failure rather than treated as the terminal one it actually is. 05d
  revisits provider-specific body sniffing for these two.
- **`CredentialBroker::store` on an existing id replaces it and un-revokes it.** Calling `store`
  again with a previously-used reference id is a rotation, not a second independent credential: the
  new value overwrites the old sealed entry and resets `revoked` to `false`, even if the credential
  previously stored under that id had been explicitly revoked. Documented on `store` itself
  (`adapters/model-gateway/src/broker.rs`); recorded here too because silent un-revocation on
  rotation is a real behavioral choice an operator relying on `revoke` as a durable kill switch for
  one id should know about.
- **Zeroization at the three secret entry points stops at this process's own heap.** The stdin
  credential read (`apps/cli/src/commands/gateway/credential.rs::read_stdin_secret`), the
  passphrase-from-env read (`apps/cli/src/commands/gateway/mod.rs::passphrase_from_env`), and the
  BYOK header build (`adapters/model-gateway/src/byok.rs`) each zeroize every buffer this process
  controls, but `std::env::var`'s own copy inside the CRT/OS environment block, and any internal
  buffering `ureq`/its TLS stack perform while writing the `Authorization`/`x-api-key` header onto
  the wire, are outside this process's reach — PR review IMPORTANT 5's own honest limit.

## What 05c shipped: the Tool Broker and Tier 0/1

### The two crates, and where decisions live

`core/tool-broker` is pure — no filesystem, no clock, no process, enforced by its own
`source_invariants` scan over all six sources — and holds every *decision*: the closed three-tool
call vocabulary (`call.rs`), the effect taxonomy and the effect→tier rule (`effect.rs`), the
lexical path and program-name rules (`path.rs`), the capability lease and the `authorize` pipeline
(`lease.rs`), and the digest-only `ToolCallRecord` (`record.rs`). `adapters/tool-host` is impure
and holds only *enforcement*: the scrubbed process primitive (`process.rs`), the worktree workspace
(`workspace.rs`), the three builtin tools (`tools.rs`), the snapshot-keyed read cache (`cache.rs`),
and the composed host (`host.rs`). The host never re-decides policy; it executes the decided plan
against the real machine, and re-refuses impossible shapes as defense in depth (`TierViolation` —
which the end-to-end sabotage showed is TWO independent layers deep: disabling only one is
invisible, and the sabotage had to disable both to prove the test could go red).

### `authorize`: the §11.2 pipeline as one pure function

Identity → capability → program allowlist → effect → tier, in a pinned order the tests hold: a
malformed actor (`[a-z][a-z0-9-]{0,63}`) refuses before the mismatch comparison, a mismatch before
capabilities, capabilities before the program allowlist. Deny-by-default is the lease's *shape*,
not a setting — what is not granted does not exist. A malformed program name denies through the
same door as an unlisted one (one refusal, no oracle for probing the allowlist), and `SecretUse`
remains structurally unsupportable: `required_tier` refuses every effect beyond
`ReadOnly`/`ReversibleWrite`, so no tool in this broker can even declare wanting a credential.
Every refusal is content-free — `refusals_never_echo_call_arguments` pins that a denied patch or
argument list never travels into diagnostics.

Trust-boundary deserialization is `ToolCall::from_json`: serde's `deny_unknown_fields` cannot fire
through internal tagging (observed in TDD red, exactly as the plan anticipated), so a per-action
key table refuses unknown fields, and the commit-message bound (at most 512 printable bytes — the
message rides argv) is checked there and re-checked at the spawn site.

### The path rules, twice

Form in the pure crate: `RelativePath` accepts exactly one spelling — forward slashes, no
empty/`.`/`..` components, no absolute/drive/UNC form, no control bytes, a 4096-byte bound — with
a proptest pinning canonical form across arbitrary inputs. Truth in the host: `resolve_within`
walks every existing component with `symlink_metadata`, refuses any symlink or junction on the
chain (junction-escape proven with a real `mklink /J`), and then — belt and braces — canonicalizes
the deepest existing ancestor and requires it inside the canonical root. Both layers exist on
purpose, and the walk is shared: the workspace's `resolve` and the Tier 0 project reads use one
implementation over two roots.

### The process primitive: `run_in_workspace`

Every Tier 1 execution funnels through one function: argv only (never a shell), `env_clear()` plus
a six-name inheritance allowlist, `HOME`/`USERPROFILE`/`TEMP`/`TMP` redirected *into* the
workspace, a fixed git posture including a synthetic commit identity (an empty redirected HOME
leaves git with no `user.name` anywhere), `path_prepend` as host-side PATH composition, and an
`extra_env` deny-list covering every host-defined name — an extra `PATH` would swap program
resolution out from under the lease. Readers always drain (a full pipe with no reader deadlocks
the child) but stop *keeping* at the output cap; the deadline kills and reaps; stdin is piped for
exactly one tool (`git apply`) and written from its own thread. The sentinel test observes the
register's hard constraint from *inside* the child via a re-executed test binary, and its sabotage
(`env_clear` removed) leaked a planted `GRAPHHELM_EVENTS_KEY` — the allowlist is load-bearing.

Note the deliberate asymmetry with 05b: the gateway's allowlist keeps the host `HOME`/`APPDATA`
because official CLIs own their own auth; Tier 1 keeps neither, because a tool workspace has no
auth of its own to keep.

### The Tier 1 workspace: ephemeral, detached, config-consistent

`git worktree add --detach` with `core.hooksPath` pointed at an empty directory (threat model §13:
a hostile repository's `post-checkout` hook must never gain execution from being provisioned —
proven with a planted hook), a shape-checked call id, and removal under retry/backoff with a
`remove_dir_all` fallback plus `worktree prune` — transient Windows lock friction absorbed, a tree
that survives every attempt still an error, because a leaked workspace is a leaked write
capability. The task's hard-won lesson: provision now runs git under the SAME scrubbed config
posture as execution. The original asymmetry — user-level `autocrlf` smudging the checkout, then
scrubbed tools judging it — made `git apply --index` see every text file as dirty ("does not match
index"). Consistency of config is the correctness condition, and it is written on the provision
call.

### The three tools and the composed host

`ToolHost::invoke` is authorize → route by tier → execute → digest → remove → record, and *every*
path ends in a `ToolCallRecord` — denial, timeout and host error included — because 05d must
externalize what happened without a side channel. Tier 0 reads run in-process under the shared
containment walk and never provision (workspace-free by construction, not by cleanup); `Diff` at
Tier 0 spawns `git -C <project>` with its CWD in an ephemeral scratch sibling so a read never
writes a byte into the tree it reads. Tier 1 writes are born and die inside one call: apply lands
in the worktree, the project stays byte-identical, staging returns to empty. `Commit` uses
`--allow-empty` with its reason documented: the ephemeral contract hands every invoke a fresh
workspace, so a commit-after-apply arrives in a tree with no changes of its own — the plan's own
test comment assumed a cross-call persistence the contract forbids, and the test still proves what
matters (the project HEAD never moves; the workspace commit dies with the worktree).

The milestone's §8 criterion is one named test:
`credentials_are_demonstrably_absent_from_the_tier_1_workspace` — sentinels planted in the parent
environment AND in a protected keyring file, two real invokes (including a write, so absence is
not vacuous), asserted across streams, a recursive scan of the still-alive workspaces, canonical
keyring↔staging separation in both directions, and the serialized records. It passed on its first
run — the external proof that the underlying tasks were honest — and its sabotages fail for the
exact reasons the register predicts.

### The read cache and the `ReuseDecision` kind (the 05c amendment)

`FreshnessClass` lives in `core/protocols` (the `NodeOutcome` precedent: wire vocabulary in
protocols, re-exported by the crate that interprets it); only repository reads declare
`SnapshotClosed`, and nothing else is cache-eligible by construction. The host's `ReadCache` is
directory-backed under staging, keyed on a declared subset of §7.2's dependency-hash components —
including the project HEAD — and gated on a *clean working tree*: Tier 0 reads touch the working
tree, which HEAD alone does not pin, so a dirty tree is a recorded `ForcedFresh(DirtyTree)`, never
a stale hit. `invalidate_evidence` deletes the stored bytes, not just the index entry — a cache
must never serve cryptographically erased evidence.

`ReuseDecision` entered the closed event set (25→26) through the full D-037 ritual: envelope
schema corrected in place, the frozen `1.0.0` mirror byte-identical, both catalogs recomputed with
the CANONICAL digest (the release-integrity test itself refused a raw-bytes digest — the machinery
teaching its own lesson), and a serde round-trip. The payload carries identity and nothing
speculative: `execution_id`, optional `node_id`, the `plane` discriminator, a closed
`ReuseOutcome` with a closed `ForcedFreshReason` validated by position, the declared
`key_components` plus the `key_digest` that names the cache entry, an optional evidence ref, and
`provenance_erased`. Cost fields were deliberately left out — additive-optional later, WITH a unit
discriminator, is compatible evolution; unit-less numbers frozen now would be uninterpretable
forever. The fold's arm is an explicit no-op ("ledger, not state"), and no producer exists in this
milestone: the 05d executor appends the first one, the 04a seam-before-implementor precedent.

### The CLI surface and codes

`graphhelm tool invoke` speaks the four-key envelope with `GHCLI012_TOOL_INVALID` (malformed
arguments or a request refused by checked deserialization — rules named, request bytes never),
`GHCLI013_TOOL_DENIED` (the refusal's stable rule name), and `GHCLI014_TOOL_HOST` (the host's
stable `GHTOOL...` internal code). `--capture-out` is mandatory on every invoke — every call
captures; a Tier 0 read's file content IS its stdout — and the captured bytes land as operator
files whose digests equal the record's (the `signal --evidence-out` precedent). A sentinel in the
CLI process's own environment reaches neither stdout, stderr, nor the captured files. `tool_cli`
is a named gate stage, proven able to go red.

## Honest limits, stated (05c)

- **Tier 1 is a git worktree plus process-level scrubbing, not a container.** No kernel network
  deny, no restricted user, no seccomp: a Tier 1 child can still open sockets and read
  world-readable paths outside the workspace. The threat model's own "worktree or snapshot"
  control is what shipped; containers and Tiers 2/3 are deferred, and D-013's dynamic elevation
  waits for the Governor loop.
- **The Policy Engine step (§11.2 step 4) is fixed structural rules.** Effect→tier and the
  program allowlist; nothing dynamic, nothing per-node yet.
- **Redaction is output caps plus a structurally empty child environment, not a content
  scanner.** A tool that *computes* a secret and prints it will be captured to the operator's
  `--capture-out` files (never into the record, which is digest-only).
- **The tests tool's contract is the exit code, nothing else.** The runner is host configuration
  (`tests_runner`, a validated bare name), its declared env is the recorded concession for
  toolchain homes (`CARGO_HOME` and friends must point at credential-free locations — the
  operator's obligation), and the broker does not interpret runner output.
- **Leases have no expiry, no `max_uses`, no revocation-on-pause.** A lease is an input value;
  lifecycle needs the runtime clock and arrives with 05d.
- **Records are not yet Evidence.** `ToolCallRecord` is the durable *shape*; sealing it beside an
  outcome event is 05d's evidence-before-append generalization, and `ReuseDecision` likewise has
  no producer until the 05d executor appends the first one.
- **`apply_patch` and `commit` never compose across calls.** The ephemeral contract hands every
  invoke a fresh workspace — correct for this slice, and the reason `commit` allows empty. The
  real executor needs a composition decision (batched invokes, or a workspace session per node
  attempt); recorded here and flagged for the 05d plan's `ToolPort` `[RECONCILE]` reconciliation.
- **The cache serves exactly one shape: Tier 0, `SnapshotClosed`, clean tree.** Everything else
  is uncached by construction, `Drifting`/`ImmutableByInput` have no members yet (the vocabulary
  is the spec's, held for the registry era — spec-debt queue #35, entries 1–2), and the reuse
  ledger's savings accounting waits for its producer.
- **The read cache persists under staging by design.** The empty-staging contract the end-to-end
  tests pin is about *workspaces* — leaked write capabilities — and their asserts name the one
  cache directory as the deliberate exception rather than filtering broadly.
- **`keep_workspace` reports kept trees as a staging delta.** The host deliberately does not
  surface workspace roots through its API; the CLI's `keptWorkspaces` field is computed by
  diffing staging listings around the call.
- **Fixed CLI execution limits.** `tool invoke` runs with a 300-second deadline and an 8 MiB
  output cap as documented constants; per-call limits are configuration surface that arrives with
  05d's node contract, not before.

## What 05d shipped: the real executor

### `core/runtime`, and the arrow that never flips

The async crate the design's §6.1 promised: the `AsyncNodeExecutor` seam (`NodeWork`/
`WorkOutcome`/`Sealable`/`WorkSummary`, boxed-future form because the driver holds ports as
`Arc<dyn …>`), the dependency-inverting `ModelPort`/`ToolPort` whose stream and reuse-summary
types are runtime-owned (this crate may name the pure vocabularies and never an adapter — pinned
from both sides by source invariants, including the reverse assertion that `core/execution` never
names `graphhelm-runtime` back), deterministic prompt assembly from the node contract (fixed
field table, never map iteration; length-prefixed digest computed at assembly), and the closed
classification: Agent/Planner/Classifier/Evaluator are cognitive, Tool is tool, everything else
is a typed refusal the driver never dispatches — a refusal is never laundered into an outcome the
fold would record.

### The executor, honest to one authority

`PortExecutor` delegates every gateway error to 05b's `outcome_for_error` — one authority on what
parks, retries and terminates, sabotage-proven against a local second mapping. Tool dispositions
map with a lease denial as terminal (it will not heal by retrying the same call). An empty model
reply is a `RetryableFailure`, never a success and never a park: `NeedsInput` would wait for
input nothing in this milestone can deliver — the plan review's finding, now pinned by test. Every
piece of free-form material (the reply, the tool record, both streams) becomes a `Sealable`; the
event-safe summary carries numbers only, and a test scans the serialized summary for planted
reply content.

### Evidence-before-append, generalized — and the ledger's first producer

`record_outcome_with_evidence` is 04f's `record_outcome` with the invariant extended: reread,
`apply_transition`, seal every sealable — and only then one `append_atomic` whose event names
exactly the sealed references (`exec-{id}-{node}-a{attempt}-{suffix}`, attempt-scoped so retries
never collide, `Confidential` because replies and streams are user material). A sealing failure
appends nothing — proven by the sabotage that reordered append-first and left an event with no
evidence. The 05c amendment's obligation lands here too: a tool outcome carrying the port's reuse
summary appends its `ReuseDecision` beside the outcome in the same `PreparedAppend`, hit and miss
both recorded, replay-stable against the ledger's no-op fold arm. The signal command now seals
its envelope beside the operator copy (`--keyring`/`--key-id` mandatory, refusing rather than
silently skipping; `envelopeSha256` equals the sealed plaintext's digest by construction), and
the HTTP signal path seals through the serve keyring when configured.

### The two `core/execution` refinements (M04 ledger)

Dispatch is attempt-fair and deterministic: `dispatch_plan` sorts by `(attempts, node)` so a
persistently retrying, alphabetically earlier node can no longer starve a sibling's first attempt
— with a property pinning both determinism and the ordering invariant. Readiness is edge-aware
for the decidable subset: a literal-`false` condition is statically dead and never gates; a
`Failure` edge releases on `Failed` and only `Failed` — the refinement both GRANTS readiness 04c
never granted and REMOVES its spurious release of failure handlers after success, both deltas
documented on `edge_gates` and the additivity property proving every other shape is exactly the
04c rule.

### The resume cross-check (the last 04f seam)

Resume derives the supplied file's content hash exactly as start does and refuses a mismatch
BEFORE any recovery append — a refused resume leaves the store untouched, proven by an immobile
head sequence under refusal. The load-bearing discovery: `current_graph` is populated only by the
M03-era graph-publication event class the CLI start never appends, so the plan's refuse-on-`None`
would have refused every CLI resume (four pre-existing tests went red on contact). The check
honors `current_graph` when a publication exists and falls back to the `graph_hash` the
`execution_started` payload records; only an execution with neither refuses outright.

### The async driver: one writer, real concurrency, immediate stop

`drive_to_quiescence_async` reproduces the 04f sequencing event-for-event (pinned by a test that
compares the stream shape and replays it twice byte-identically) with work running concurrently
in a `JoinSet` up to `max_parallel_model_calls` — proven at the port's own counters with a
semaphore and zero sleeps — while every write stays serialized through the loop itself: the
driver task is the single writer, each store touch inside `spawn_blocking` so the OS-exclusive
lock never parks an async worker. Immediate stop composes the 04e pieces in order: the ports'
cancel hooks, aborted futures, `Interrupted → Blocked` per aborted node (the silent-retry
sabotage fails exactly as 04e demands), `execution_paused` closing the story, and resume refusing
with `UntriagedInterruption` until an owner approves. The cancelled-tool test kills a real child
process and proves the pid dead before anything records.

### The API drives async — and the CLI does not change

`execute_prepared` splits start/resume into the decision half (through the
`ExecutionStarted`/`ExecutionResumed` append) and the drive; the CLI's `execute` recombines them
with the sync 04f drive — byte-identical, its whole suite the proof — while the serve handlers
drive through `drive_to_quiescence_async`. Serve gains the grouped runtime flags
(`--manifest`/`--broker`/`--route`/`--staging` all-or-none; `--keyring`/`--key-id` all-or-none;
real-executor mode requires both groups, validated fail-fast at startup), the port
implementations construct the borrow-shaped sync adapters inside `spawn_blocking`, `pause`
accepts `{"mode":"immediate"}` through a cancellation registry (the driver appends
`execution_paused`; the route appends nothing), and `FixtureAsyncExecutor` — delegating to the
real `FixtureExecutor`, so the async path cannot drift from the sync fixture meaning — lets a
fixture story exercise the true async path. The 05a CLI–API parity test survived the swap
unchanged, which is exactly what it exists to prove.

An ordinary pause appends `execution_paused`, stops both drivers from dispatching more nodes, and lets work already in flight finish. An immediate pause uses the cancellation registry, interrupts in-flight work, and records interrupted nodes as blocked for triage. Both drivers check pause state before building a dispatch plan and again before each dispatch. The synchronous driver documents one residual window: a concurrent append can arrive after the final pause read and before the dispatch append obtains its sequence.

The milestone's §8 sentence is one named green test:
`an_agent_and_a_tool_node_run_to_completion_with_sealed_evidence` — an agent node against a
replying fake provider (the reply sealed beside its outcome), a tool node running real `git`
inside an ephemeral Tier 1 worktree (record and both streams sealed), completion over HTTP, and
`graph replay` twice, byte-identical. `runtime_http` is a named gate stage, proven able to go
red; the sync-driver sabotage made the immediate-stop test blow its whole deadline — the proof
that the swap matters.

## Honest limits, stated (05d)

- **The viability gate, precisely.** The async driver dispatches only the closed classification
  (cognitive + tool). Graphs carrying other node types fall back to the byte-identical sync path
  on fixture-only servers — but on a server WITH a real executor configured, a mixed graph
  drives async and quiesces WITHOUT completing: unsupported nodes are refused (never dispatched,
  never invented), the plan empties, and no `execution_completed` appends — `status` stays
  `null`/running. Correct for this milestone's acceptance graphs (agent+tool only), named here
  so nobody mistakes quiescence for completion on a mixed graph. Widening the classification is
  future node-type work, not a driver defect.
- **The serve keyring does double duty.** One physical keyring, two passphrase envs:
  `GRAPHHELM_GATEWAY_KEY` opens it for the credential broker, `GRAPHHELM_EVENTS_KEY` for the
  driver's evidence sealer. Divergent envs fail at the provider open — never silent corruption —
  but the shape is a conflation: separate keyrings (gateway vs events domains) are the honest
  refinement, deferred to the 05e-era serve work.
- **`ReuseDecision` never fires on the serve path.** `ToolHost::invoke` exposes only the
  `reused: bool` on the record, not a `ReuseSummary`-shaped accessor, so `ToolPortResult.reuse`
  is always `None` in the serve wiring and the ledger's producer operates only where the driver
  is handed a summary directly. The host-side accessor is the recorded extension.
- **HTTP model calls cannot be aborted mid-flight.** `ModelPort::cancel_all` is a no-op for the
  BYOK path: dropping the future abandons a `spawn_blocking` body, whose bound is the transport
  timeout; a reply arriving after cancel is discarded, recorded as `Interrupted`, never as the
  late reply. The tool side kills real children (proven); the `ToolHost` itself exposes no kill
  surface to the port — the subprocess deadline machinery is the bound.
- **Prompt assembly is the node contract only.** No Context Compiler, no capsules, no retrieved
  context — `objective` plus the `agent.ephemeral` fields in a fixed order.
- **One route, no scoring.** The serve `--route` flag picks the manifest route; 05b's deferred
  scoring stays deferred. Startup resolves the route by ID but does not consult its `enabled`
  field. `gateway probe` refuses a disabled route, and the pure `eligible_routes` helper filters
  disabled routes, but the production serve wiring does not call that helper. Treat `enabled` as
  an operator-side gate for serve until the wiring enforces it.
- **`artifact_refs` stays empty.** Evidence covers replies, records and streams; the artifact
  store design has not landed.
- **Throughput, recorded in its own unit.** The 05a baseline was ≈3 requests/second for single
  API requests on the current-thread runtime. This milestone measures full fixture STORIES —
  start-to-completion, ~10 serialized appends each with per-append store opens — at ≈0.17
  stories/second on the same runtime. The units are not comparable and no comparison is claimed;
  both numbers exist so future work has honest baselines.
- **`project` defaults to the server's working directory.** A start/resume body may carry
  `"project"` for the tool host's worktree source; omitted, the serve process's CWD is the
  project — a local-operator convenience consistent with D-040's local-by-construction premise,
  and a thing to make explicit the moment serve is ever fronted by anything.
- **A fixture-only server fails real stories closed.** Without the keyring group, the driver's
  sealer is a `RefusingSealer`: fixture stories seal nothing and run; any story that produces a
  sealable refuses rather than appending unsealed material.
- **Still absent, still named:** the five no-progress conditions needing signal intake;
  compensation execution (recorded, not compensated); session management; SSE; the local store
  exposes evidence availability rather than a sealed read (the Postgres adapter owns
  `EvidenceRepository`).

## What 05e shipped: the chat surface

### ADR-026 and the layer it declined

The chat surface is `graphhelm mcp`: a stateless stdio MCP server whose tools map 1:1 onto
Public Runtime API requests (D-039; runtime-design §6.4; `CHAT_SURFACE_SPEC` §3). ADR-026
decided the protocol stack by measuring what it declined: rmcp v3.1.2's default features
would have grown the workspace from 328 to 342 packages — fourteen new crates including two
proc-macro stacks — for five methods and a closed tool list. The hand-rolled layer
(`apps/cli/src/commands/mcp/rpc.rs`) is newline-delimited JSON-RPC 2.0 with a bounded line
reader (`MAX_LINE_BYTES` 1 MiB via `take()`-capped reads: an oversized line is refused
naming the bound, never buffered whole, and the loop resyncs on the next line), the four
standard error codes, and the invariant the conformance suite pins: an id-carrying request
gets exactly one reply with its id echoed verbatim, a notification never gets any. The
revisit trigger is named in the ADR: resources, push notifications, `structuredContent`
results, or a non-stdio transport adopt the SDK instead of growing this layer.

### The lifecycle and what the process actually holds

`SUPPORTED_PROTOCOL_VERSION` is pinned to `2025-06-18`, verified against
modelcontextprotocol.io at implementation time: the spec's current revision is `2026-07-28`,
a meta-versioned model without the initialize handshake; handshake revisions remain
interoperable per its backward-compatibility section and are what the targeted hosts speak.
A client asking for a different revision gets ours back — a version mismatch is never an
error (`initialize_negotiates_and_reports_tools_capability`). Statelessness as shipped: the
process holds `SessionState { initialized, nonce, client }` — a lifecycle bit, eight bytes
of OS randomness (the session's one impure input, consumed by the idempotency derivation),
and the API client. Nothing else; every fact lives on the server side of the API.

### The ten tools, and the calls they are

`tools/list` names exactly `start`, `status`, `events`, `signal`, `approve`, `pause`,
`resume`, `cancel`, `routes`, `probe` — and nothing else: no credential tool is
representable, omission is the enforcement and the closed-list test its guard (the sabotage
added an eleventh entry and the suite refused it). Every schema is closed
(`additionalProperties: false`), every description names the API call it maps to (§6 parity
in the tool's own metadata), and every mutation accepts the optional `ifMatch` head pin the
§3 choreography rests on. A tool call is one API request; the result is the API envelope
verbatim as text content with `isError` mirroring `ok` — codes like
`GHE003_IDEMPOTENCY_CONFLICT` reach the chat exactly as the API said them, already
redaction-safe by the API's own contract.

### Config, auth, and the loopback wall

`graphhelm mcp --url --token-file [--actor] [--actor-type]`: `--actor` is optional since #1058
(one `.mcp.json` is shared by every session of a repository, so a literal there made every
session the same actor); the id comes from `--actor` or, failing that, `GRAPHHELM_ACTOR`, and a
session that has neither is refused (`GHCLI015_MCP_INVALID`), never defaulted; the URL is loopback-only
fail-closed under the post-#36 rule (userinfo stripped before host inspection —
`[::1]@evil.com` and `localhost:tok@attacker.example` shapes are pinned refused); the token
arrives via file or `GRAPHHELM_API_TOKEN`, never argv, and a full session's stdout and
stderr are scanned for the sentinel with the real transport attempting a real request;
refusals are `GHCLI015_MCP_INVALID`, entering the registry exactly as reserved since 05c.

### Idempotency per logical act, proven under retry

`derive_key` emits `mcp-{nonce}-{s|n}{norm}`: the rpc id verbatim when already
`[a-z0-9-]{1,32}`, its first 16 SHA-256 hex otherwise (digested, never truncated), with a
fixed type marker so `7` and `"7"` are distinct logical acts — the bound holds by
construction (4+16+1+1+32 = 54 and 4+16+1+1+16 = 38, both ≤ 64). The proof is behavioral:
`a_retried_tool_call_reuses_the_key_and_a_divergent_reuse_travels_as_409` replays the same
line twice (head advances by exactly one call's delta) and the same id with different
arguments surfaces the API's 409 intact. A `tools/call` without an id — the notification
form — is never executed: a mutation with no response channel cannot participate in the
retry choreography, so the server refuses to run it and, per the notification rule, writes
no reply (the head provably does not move).

### The serve gateway read surface, and why it exists

`GET /v1/gateway/routes` and `GET /v1/gateway/probe` (05e Task 4) exist so the MCP `routes`/
`probe` tools never become a second path (rule 3, D-039 read bidirectionally): both handlers
call the same command-layer functions the CLI subcommands call, the envelope is
byte-identical, and parity is test-pinned. The Task 0-reconciled manifest rule: a server
launched with `--manifest` serves it by default, the query parameter is an explicit
override. On a fixture-only server (no `--manifest`) asked without the parameter, `routes`
answers `200` with `{"configured": false, "routes": [], "reason": "no gateway manifest is
configured on this server"}` (#1083: the earlier `400` surfaced in every connecting browser as a
console error for the true answer "no routes"), while `probe`, which needs a manifest to act on,
answers `400` naming the parameter. A manifest that is named but cannot be loaded or validated
is still refused on both. Both endpoints answer
401 to a missing or wrong token — asserted explicitly, so a future router refactor cannot
silently unauthenticate them.

### Parity, and the §5 choreography as tests

`the_mcp_and_the_api_report_identical_status_for_the_same_story` drives the whole 05a story
through MCP tools against one store and through direct HTTP against another, with
`MCP_PARITY_EXCEPTIONS` empty by design — adding an exception is a visible diff. The §5
choreography test runs two chat sessions against one serve coordinating through events
alone: a scout signals, a builder discovers it only from the attributed tail and approves, a
race on the same pinned head resolves with the loser re-reading and a single retry
completing the execution — zero side channels.

### Packaging: thin, deletable, validated

`examples/chat-surface/` ships the Claude Code plugin (`plugin.json`, `.mcp.json`
registering `graphhelm mcp` with `--token-file` and never an inline token) with the first
two skills — `operate-execution` (§4.3: start→watch→triage→act→report, immediate-stop
explicit and confirmed, cancel stating §13's partial-effects consequence before acting, the
credential-refusal instruction verbatim) and `observe-agents` (§4.4: read-only actor
timeline from event attribution, conflicts flagged, never acting as another actor) — plus
the Codex `[mcp_servers.graphhelm]` snippet. Validation runs in the suite: the JSON parses,
the TOML is line-shape-checked (adding a `toml` dependency for a two-line snippet is the
wrong trade, stated in the test), every `tool:`-marked name a skill references exists in the
tool table, and both READMEs carry §7's deletability sentence: deleting the wrappers loses
convenience only.

## Honest limits, stated (05e)

- **Eight skills deferred, each with its dependency named:** `onboard-new-project` and
  `onboard-existing-project` (§4.1/§4.2) await the Living Docs store and Draft flow over the
  API; `invoke-agent` (§4.5) awaits multi-actor session identity beyond one `--actor` per
  process; `share-context` (§4.6) awaits evidence externalization surfaces on the API (the
  transcript-as-Evidence write path); `triage-and-approve` (§4.7) awaits a cross-execution
  read surface (the API is per-execution today); `deploy-and-verify` (§4.8) awaits Deploy
  node execution (05d refuses the node type); `rules` and `document-impact` await Living
  Docs itself.
- **Pull-only notifications.** No SSE and no MCP push: a chat discovers progress by polling
  `events` from its last seen head. The first milestone needing push is a named ADR-026
  revisit trigger.
- **No resources, prompts, or sampling.** The MCP surface is tools-only, deliberately —
  the same revisit trigger owns the expansion.
- **The secret-prefix guard is a narrow, deliberate heuristic** (`sk-ant-`, `sk-proj-`,
  `-----BEGIN `), not a scanner: it catches paste-shaped accidents, and the real enforcement
  is structural — no credential tool exists to receive a secret, and the broker's stdin path
  is the only entry.
- **The protocol pin will age.** `2025-06-18` is the handshake-model revision the targeted
  hosts speak; the spec's current line is the meta-versioned `2026-07-28`. Moving off the
  handshake model is an ADR-026 revisit candidate recorded on the constant itself.
- **`probe` over HTTP inherits the CLI probe's scope AND its spawn surface.** Reachability,
  not model quota — and a probe of a native route spawns the configured program with
  `--version` inside the serve process's context. The manifest is the trust boundary, and
  the API now exercises it on request.
- **Gateway read queries are verbatim.** The `manifest` query parameter is not
  percent-decoded: a path containing `&`, `=`, or a space is inexpressible over HTTP and
  uses the server-configured manifest (the preferred path) or the CLI.
- **stdout is shared.** The protocol stream and the house CLI envelope share stdout; the
  final `CommandOutput` line carries no `jsonrpc` member and hosts ignore it, but the
  streams are interleaved by design rather than separated.

## What 05f shipped: the monitor, and the Milestone 05 close

### The page that cannot grow a button

The monitor (D-040) is `GET /monitor` and `GET /monitor/{id}` on the serve process:
server-side-rendered, **zero-JavaScript** HTML — a page with no script has nothing for
"just one button" to hook into, so the read-only rule is the medium, not review vigilance —
refreshed by `<meta http-equiv="refresh">` whose URL carries the `since` cursor, which makes
the "changed since #N" delta strip stateless: the browser tells the server what the operator
last saw. The renderer (`render_monitor`) is a pure function over the SAME
`ExecutionProjection` the `status` command folds — HTML as a third formatter over one truth
— with every dynamic string passed through a local five-character escape (a hostile
execution id renders escaped, pinned; a header-hostile id in the bootstrap path answers 400,
never a panic — found live in review, fixed in-milestone). The router registers only GET:
POST/PUT/DELETE/PATCH answer 405 structurally, even with a valid cookie, and every 200
carries `default-src 'none'; style-src 'unsafe-inline'`.

### Auth: one authority, one hop

A browser cannot set a bearer header, so the page bootstraps: `?token=` verified against the
serve token through the SAME constant-time comparison `require_token` uses, an
`HttpOnly; SameSite=Strict; Path=/monitor` cookie whose value IS the token (re-verified per
request against the file-loaded bytes — no session store), and a 303 to the clean URL. The
token never rides the redirect Location and never reaches a page byte, both pinned.
Loopback-without-auth was rejected on purpose: loopback is not a trust boundary on a
multi-user machine, and the tail renders operator-grade detail.

### Silence as signal, and remediation as text

Every node row carries a staleness clock folded from the tail's own `nodeId` payloads — a
hung worker's loudest output is nothing, and a RUNNING node silent past its bound renders
red. Where the stream carries a graph publication, bounds are per node kind (tool 30s, agent
120s, default 90s) and failed/blocked nodes show their blast radius (downstream blocked
count; terminal still reachable — pure O(V+E) reachability, `remediation::blast_radius`);
where it does not, the column says "unknown — the stream carries no graph publication" and
never guesses. Triage rows answer scope gravity positively: instead of a button, the EXACT
`graphhelm execution approve ...` command, rendered by `remediation::render_invocation` and
parse-round-tripped through the real clap `Cli` in a test, so the page and the CLI compile
from one source and cannot drift.

### The negative proof, and the snapshot

`hammering_the_monitor_never_changes_a_byte_of_the_store` fingerprints every file under the
events directory, fires six verbs times with/without cookie times malicious bodies at both
paths plus a probe list proving approve/pause/retry/cancel do not exist as surfaces, and
asserts the store bit-identical and the status JSON unmoved. `execution status --html`
writes the same page frozen (no refresh tag) — the incident artifact IS the live renderer,
pinned byte-equal minus exactly the refresh line.

### The Milestone 05 close: a map that cannot rust, and a run that really happened

`docs/acceptance/m05-clauses.toml` binds each §8 clause to named prover tests with assert
fingerprints; `acceptance_map_is_grounded` (in the gate) verifies each fn exists exactly
once, its suite is on the gate surface, the fingerprint appears in the test body, every
D-citation still matches the register, the committed `M05_ACCEPTANCE_MAP.md` is
byte-identical to the generator's output — and, for the one clause no test can prove, that
the committed run evidence still hashes to its `SHA256SUMS` in both directions. That run
(`docs/acceptance/m05-run-2026-08-16/`) happened once, on 2026-08-16: an agent node answered
by the LIVE model over the owner-subscription route (`native_runtime` spawning the claude
CLI, prompt over stdin), a tool node running real git in an ephemeral Tier 1 worktree,
completion over HTTP, double replay byte-identical, and the SAME completed state read back
through CLI, HTTP and MCP. No gate re-runs the call; every gate re-hashes the evidence.

## Honest limits, stated (05f)

- **The staleness clock is only as honest as event granularity.** An agent node streaming
  silently emits nothing and can look stale; per-kind bounds soften this, heartbeat events
  are the named follow-on (a harness contract change, not a monitor feature).
- **Kinds and edges exist only on streams with a graph publication.** The CLI/serve start
  paths do not publish the graph onto the stream (the 05d Task 7 discovery, now a rendering
  reality): without it, per-kind bounds fall back to the default and blast radius states its
  ignorance. Terminals approximate as sink nodes — the completion control's typed expression
  is not evaluated by this slice.
- **The cookie is a second door wearing the same lock.** Its value is the bearer token
  through the one shared verifier — but the monitor is a second authentication path and is
  named as such; the GET-only router assert and the CSP are the fences around it.
- **The 2-second meta refresh is the whole update contract.** No SSE, no push — the wake
  doorbell plan (05g) owns the push story.
- **Remediation strings carry no If-Match.** The CLI has no head-pin flag; the command the
  page hands out can race a concurrent mutation and lose honestly (the API's own refusal).
- **CSP rides every 200, not every response** (401/303 carry none — rendered content is
  what the header defends).
- **The native adapter spawns the CLI in the serve process's cwd.** A cwd inside a
  Claude-configured repository stalls the spawned CLI on that project's own MCP config —
  the acceptance run serves from a neutral cwd, measured both ways. A route-owned working
  directory is a plausible 06-era knob.
- **An invalid direct_api key surfaces as `needs_capacity`.** 05b's `outcome_for_error`
  folds 401 into the capacity class; auth-invalid is not capacity-exhausted, and the
  distinction is deferred to the gateway's own next slice, not patched here.
- **The double-duty serve keyring (05d limit) bit during the run:** one key must serve both
  `GRAPHHELM_GATEWAY_KEY` and `GRAPHHELM_EVENTS_KEY` against a keyring credential-set
  created. Still open, still named.
- **The monitor lists only what one store knows.** No cross-store index; `/monitor` is the
  streams of one events directory.

## Milestone 05, closed

Every §8 clause is bound to running proof in `M05_ACCEPTANCE_MAP.md`; the refused-scope
table names the four banned buttons with D-040's own sentence; the run evidence is
checksummed against the bytes git stores (a fresh-clone simulation is part of the review
record). The runtime now does real work end to end — model calls through gateway or
subscription, tools in isolated worktrees, evidence sealed beside outcomes — and is
operable, with identical observable state, from a terminal, an HTTP client, a chat, and a
read-only page. What Studio inherits is an API that already tells one truth.

## What 05g shipped: the wake doorbell

### The primitive, end to end

A session sleeps at zero cost and is woken by another actor's append — one bit, no payload,
no polling anywhere. The sleeper arms its own wait: `wake_arm` (MCP) or
`POST /v1/executions/{id}/wake-lease` deposits a `WakeLease` event carrying its session, its
last-seen cursor and an OPAQUE rendezvous id — never a path: both sides derive the platform
rendezvous under a fixed local prefix, so a hostile lease cannot point the serve anywhere
(`\\.\pipe\graphhelm-wake-{id}` on Windows; the Unix socket arm is compile-shaped design
until a Linux gate exists). The sleeper then blocks `graphhelm wake-wait` on that
rendezvous: exit 0 rung, exit 3 timeout (routine, not failure — the dead-man design means
timeouts happen), exit 2 (`GHCLI017_WAKE_INVALID`) for unusable arguments.

### The kinds and their invariants

`WakeLease`/`WakeLeaseConsumed` entered by the D-037 ritual with the invariants pinned in
the fold: AT MOST ONE live lease per session (arming again replaces — the waker can never
schedule the sleeper into a loop), consumption burns, consumption without a live lease is a
replay integrity refusal, and **a replay never rings** — the fold crate speaks no transport
vocabulary, pinned by source scan.

### The ring, honest about physics

The serve sweeps after the single success arm every mutation route shares, so the ring can
only happen AFTER the trigger append is durable — and the test's detector is honest about
it: the sleeper snapshots the store's kinds AT THE INSTANT the byte arrives, from inside
its own read completion. (The first sabotage attempt came back green and exposed the
original detector as blind to early rings; it was hardened before the guard was trusted —
the thymus rule lived here before its milestone.) The consume reason is two-phase by
physics: `rung` vs `stale_rendezvous` is only knowable after the ring attempt, so the
consumption is its own follow-up append. Only NON-wake appends ring (arming would otherwise
self-ring). A burned lease never rings twice; a missing rendezvous consumes as honest
cleanup and never fails the route; a hostile ringer's bytes die in the sidecar's sink —
content never crosses, and the sabotage that echoed them failed the sentinel assertion.

### The sleeper-only surface

The MCP tool list grew to exactly twelve: `wake_arm` and `wake_status`, and deliberately
nothing else — **no tool exists to ring another session** (the thirteenth-tool sabotage
failed the closed-list test), and the session can only arm ITSELF: the sessionId is the
session's own nonce injected inside the dispatch, never a tool argument, so arming or
reading a peer's doorbell is unrepresentable.

### §5 measured, not promised

The choreography test puts a counting TCP proxy in front of the serve and drives two real
sessions: A arms through the proxy and blocks a real `wake-wait`; B appends; the serve
rings; A wakes and re-reads its own log, finding B's event attributed. The proxy's count
for the arm→ring window: **zero connections from A** — the token economy that motivated the
milestone, as a number. Degradation is its own test: serve killed, timeout fires routinely,
a plain read still tells the truth — slow, never wrong.

## Honest limits, stated (05g)

- **The Unix rendezvous arm is compile-shaped.** The Windows named pipe is runtime-proven
  (spike and suite); the Unix socket design compiles and follows the same shapes
  (`NotFound`/`AddrInUse`) but earns its runtime proof when a Linux gate exists.
- **Only serve-made appends ring.** `append_atomic` is per-process; the sweep wraps the
  serve's own mutation paths (API and MCP). A CLI-direct append in another process does not
  ring — the sleeper's dead-man timer covers it (accelerator, never correction). The named
  follow-on: a store-level notification file the serve tails, or the lease checker moving
  into the driver era.
- **Two-phase consumption can leave a spurious wake.** A crash between the durable trigger
  and the consumption append leaves a live lease and a possibly-delivered byte: at worst
  the sleeper wakes, re-reads, finds nothing new, and re-arms — content-free by
  construction, so spurious means slow, never wrong.
- **The driver does not sleep on leases yet.** `WaitingInput` nodes still wait on the
  resume path; the driver-side doorbell is the named follow-on, deliberately not this slice.
- **GHCLI017_WAKE_INVALID** entered the registry as reserved.
- **The 05f run-evidence gitignore lesson is recorded**: the journal export survived every
  machine that already had the file and even the fresh-checkout simulation (checkout only
  restores tracked files); the recovery is in `b9b0aa7`, and the named hardening candidate
  is a tracked-vs-named check inside the acceptance map's artifact verification.
