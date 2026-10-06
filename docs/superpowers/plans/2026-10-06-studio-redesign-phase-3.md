# Studio Redesign Phase 3: Image Evidence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Any agent or the owner can attach up to four PNG, JPEG or WebP images to a signal; each image seals as its own Confidential evidence item in the same atomic append as the envelope; the owner (never a scoped agent) reads it back as raw bytes under locked-down headers; CLI, HTTP and MCP accept the same attachments under the same rules.

**Architecture:** One new pure module `apps/cli/src/commands/execution/attachments.rs` (strict base64, magic-byte sniffing, caps, `ImageAttachment`) used by the shared signal core `execute_authenticated_inner` (`apps/cli/src/commands/execution/signal.rs`). Validation runs before the `evidenceOut` write, the seal and the append, so a refusal writes nothing. The sealed attachments join the envelope's `PreparedAppend` (`signal.rs`, the `match sealed` block) so the append is all or nothing. Transports only translate: the CLI reads `--attach <file>` (repeatable), HTTP and MCP carry `attachments: [{mediaType, base64}]` as a sibling of `signal` in the body, so the signal envelope schema does not change. The read route `GET /v1/executions/{id}/evidence/{evidenceId}` (`apps/cli/src/commands/serve/routes.rs`, `evidence`, ~4914) gains a raw-bytes branch for the three image types. Owner-only read is already enforced by `agent_route_allowed` (`apps/cli/src/commands/serve/mod.rs` ~815: a GET with six path segments is not on the agent allowlist); this phase pins it with a test.

**Tech Stack:** Rust 1.97.1 (`graphhelm-cli`). No new dependency.

**Spec:** `docs/specs/2026-10-05-studio-live-team-and-proven-journeys-design.md` §5.1, §7, §10 phase 3. ADR: ADR-043 in `docs/reference/REFERENCE_STACK_AND_ADRS.md` (the spec's "ADR-042" collided with the reuse ADR already on `main`; journeys become ADR-044).

## Rulings (made by the orchestrator; do not reopen)

1. **Where attachments ride.** HTTP body and MCP arguments: `attachments` beside `signal`, never inside the envelope. The envelope schema and `admit_signal` are untouched; no schema or conformance pin changes.
2. **Sealing required.** A signal with attachments and no keyring is refused (`/attachments`): an operator file cannot hold images, and the spec's guarantee is "sealed Confidential at rest".
3. **Evidence ids.** `signal-<signalId>-image-<n>`, `n` = 1..=4 in request order. Deterministic so the envelope's `evidence` strings can cite them before the call. The reply gains `"attachments": [{"evidenceId", "mediaType", "bytes"}]` (absent key when none, so replies without attachments stay byte-identical).
4. **Caps and checks, in this order, all before any write:** array length ≤ 4 (`/attachments`); each item an object with exactly `mediaType` and `base64` (`/attachments/<i>`); `mediaType` in `image/png | image/jpeg | image/webp` (SVG and everything else refused, `/attachments/<i>/mediaType`); encoded length ≤ `4 * ceil(8 MiB / 3)` checked BEFORE decoding; strict RFC 4648 standard alphabet with padding, no whitespace; decoded length 1..=8 MiB (8_388_608); decoded bytes start with the declared type's magic: PNG `89 50 4E 47 0D 0A 1A 0A`; JPEG `FF D8 FF`; WebP `RIFF????WEBP` (bytes 0..4 = `RIFF`, 8..12 = `WEBP`). Any failure refuses the whole signal with `GHCLI003` (signal invalid) and nothing is written or appended.
5. **Base64 is hand-written** (decoder and, for the CLI's own use only where needed, nothing else): no crate in the workspace provides it and a ~40-line strict decoder with unit tests is cheaper than a new dependency audit.
6. **Body limits.** The HTTP signal route gets its own `DefaultBodyLimit` of 48 MiB (the four-image worst case is 42.7 MiB of base64); the global 5 MiB limit stays for every other route. The MCP stdio line cap (`apps/cli/src/commands/mcp/rpc.rs` `MAX_LINE_BYTES`, 1 MiB) rises to 48 MiB so MCP parity is real, not nominal. The 64 MiB evidence batch cap still applies (4 × 8 MiB + envelope fits).
7. **CLI type.** `--attach <file>` infers the media type from the magic bytes; the extension is ignored. A file over 8 MiB is refused from its metadata before it is read. The CLI then builds the same `ImageAttachment` values the HTTP path decodes, so the validator is shared.
8. **Read route.** For `image/png | image/jpeg | image/webp` the route serves the opened bytes (200) with exactly: `Content-Type: <mediaType>`, `X-Content-Type-Options: nosniff`, `Content-Security-Policy: default-src 'none'`, `Cache-Control: private, no-store`. It re-checks the magic bytes after opening and refuses (409 `GHCLI023`) on mismatch. JSON and text keep their current JSON reply. Every other type, `image/svg+xml` included, stays refused.
9. **Studio rendering is not this phase** (spec phase 5).

## Global Constraints

- Docs in English. Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Commit and push after every green step.
- One implementer at a time in this worktree. No `git stash`, no checkout of other branches.
- Before each commit on Rust files: `cargo +1.97.1 fmt --all -- --check` and `cargo +1.97.1 clippy -p graphhelm-cli --all-targets --all-features --locked -- -D warnings`.

## Task 1: Shared validator, sealed append, CLI `--attach`

**Files:** create `apps/cli/src/commands/execution/attachments.rs`; modify `apps/cli/src/commands/execution/mod.rs` (module), `signal.rs`, `apps/cli/src/args.rs` (`Signal` gains `#[arg(long = "attach")] attach: Vec<PathBuf>`), `apps/cli/src/commands/mod.rs` (~283 dispatch); create `apps/cli/tests/signal_image_evidence_cli.rs` (harness copied from `apps/cli/tests/owner_records_cli.rs`).

- [ ] `attachments.rs`: `pub(crate) struct ImageAttachment { media_type: &'static str, bytes: Vec<u8> }`, `pub(crate) fn parse_json(value: &serde_json::Value) -> Result<Vec<ImageAttachment>, Failure>`, `pub(crate) fn from_file(path) -> Result<ImageAttachment, Failure>`, `fn sniff(bytes) -> Option<&'static str>`, `fn decode_base64(&str) -> Option<Vec<u8>>`, constants `MAX_ATTACHMENTS = 4`, `MAX_ATTACHMENT_BYTES = 8 * 1024 * 1024`. Unit tests: each type sniffed; wrong magic; SVG; base64 round cases (padding, bad char, whitespace refused); count 5 refused; 8 MiB + 1 refused; oversized encoded refused before decode.
- [ ] `signal.rs`: `execute_authenticated` / `execute` / inner gain `attachments: &[ImageAttachment]` (native observer passes `&[]`). In the inner function, refuse non-empty attachments without sealing right after the existing keyring guard; caps are enforced by construction (parse) and re-asserted (`len() <= 4`). After the envelope seal, seal each attachment (`EvidenceInput::new(format!("signal-{id}-image-{n}"), media_type, Confidential, "standard", ...)`), push its reference after the envelope's in `evidence_refs`, and add the sealed items to the `PreparedAppend` vector. Reply adds `attachments` when non-empty.
- [ ] CLI integration tests: PNG, JPEG, WebP each record with the right `evidenceRefs` order and media type; wrong magic, SVG file, 8 MiB + 1 file, five files, and a valid + invalid pair each refuse with no new event in the stream (count before == after) — the last one is the no-partial-append proof.
- [ ] fmt, clippy, `cargo +1.97.1 test -p graphhelm-cli --locked --test signal_image_evidence_cli` and the unit tests; commit; push.

## Task 2: HTTP, read route, MCP, parity

**Files:** modify `apps/cli/src/commands/serve/routes.rs` (`signal` ~2115, `evidence` ~4914), `apps/cli/src/commands/serve/mod.rs` (route layer ~630/690), `apps/cli/src/commands/mcp/tools.rs` (`signal_schema` ~478, forwarding ~1477), `apps/cli/src/commands/mcp/rpc.rs` (`MAX_LINE_BYTES`); create `apps/cli/tests/signal_image_evidence_http.rs` (harness from `apps/cli/tests/runtime_http.rs` / `api_http.rs` for serve with keyring and `GRAPHHELM_AGENT_CREDENTIALS`, and `mcp_stdio.rs` for MCP).

- [ ] HTTP `signal`: `attachments` parsed with `attachments::parse_json` before the idempotent mutation; non-array → 400 `/attachments`. Route-level body limit 48 MiB.
- [ ] `evidence`: raw-bytes branch per Ruling 8 using `axum::response::Response::builder`.
- [ ] MCP: schema `attachments: {type: array, maxItems: 4, items: {type: object, required: [mediaType, base64], properties: {mediaType: {enum: [...]}, base64: {type: string}}, additionalProperties: false}}`; forwarded only when given. `MAX_LINE_BYTES` 48 MiB.
- [ ] Tests: owner reads each type with the four headers and the exact bytes; SVG and wrong-magic over HTTP refused with no append; a scoped agent credential can signal with an image but gets 401 reading it; the same PNG sent through the CLI `--attach` and through MCP `signal` yields the same `mediaType`, `contentSha256`, and served bytes (parity); MCP SVG refused like the CLI.
- [ ] fmt, clippy, tests; commit; push.

## Task 3 (orchestrator): docs

- [ ] ADR-043 in `docs/reference/REFERENCE_STACK_AND_ADRS.md` (§48), spec renumbered (ADR-042 → ADR-043, ADR-043 → ADR-044), the signal and evidence route docs where they describe the body and the JSON/text-only rule.

## Review Focus

1. A refused attachment anywhere in the list leaves the stream unchanged (no envelope event, no blob reference).
2. An image whose sealed type is PNG but whose bytes are not is never served.
3. A scoped agent credential cannot read any evidence, image or text.
4. Replies and envelopes of signals without attachments are byte-identical to `main`.
