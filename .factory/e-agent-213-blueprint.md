# Blueprint — #213: per-contribution MCP capability tokens and tool allowlists

**Status: BLUEPRINT ONLY. No code, no fixtures, no policy files.** Written before implementation so
the implementation can be checked against it, per the orchestrator's explicit instruction: threat
assessment first, file scope derived and declared before code, hostile fixtures with a real named
emitter before code.

**Issue uses the OLD template** (Goal/Context/Deliverables/Invariants/Related, no Dependencies or
Files-in-scope section). Scope below is derived from the issue body plus the current codebase, not
copied from a Dependencies block that does not exist.

**Sources read:** issue `#213`; issue `#210` (closed — declared the `permissions`/`effects`/
`requires`/`surfaces` contribution fields this task must finally enforce); issue `#110` (marker —
confirms `PersistedActor` is attribution-only today, no capability semantics); issue `#212` (open,
unimplemented — no package-activation state exists to depend on); issues `#223`/`#225` (downstream
consumers on the critical path); `core/schema/src/extension.rs`; `core/tool-broker/src/lease.rs`,
`call.rs`, `effect.rs`; `apps/cli/src/commands/mcp/session.rs`, `tools.rs`, `client.rs`;
`apps/cli/src/commands/serve/mod.rs` (bearer-token-file pattern); `core/protocols/src/event.rs`,
`actor.rs`.

---

## 1. What exists today, precisely (grounds every design choice below)

Two authorization mechanisms exist and **do not currently meet**:

1. **`core/tool-broker/src/lease.rs::authorize()`** — a pure `identity → capability → program
   allowlist → effect → tier` pipeline over `ToolLease{actor, capabilities: BTreeSet<Capability>,
   programs}`, where `Capability` is a *closed 4-variant enum* (`RepositoryRead`, `RepositoryWrite`,
   `ShellExecute`, `TestsExecute`). It is wired **only** to `adapters/tool-host/src/host.rs` (the
   `graphhelm tool invoke` sandboxed-shell surface). Its `Capability` type has no relationship to MCP
   tool names — reusing it directly for this issue would be a type mismatch, not a fit.

2. **`apps/cli/src/commands/mcp/{session.rs,tools.rs,client.rs}`** — the 14-tool GraphHelm MCP server
   (`start`, `status`, `events`, `signal`, `approve`, `pause`, `resume`, `cancel`, `routes`,
   `wake_arm`, `wake_status`, `amend_budget`, `wake_wait`, `probe`; closed list at
   `tools.rs::TOOLS`). `session.rs::handle()` dispatches `tools/call` straight to `tools::call()`
   after only: lifecycle state (`initialized`) + closed-tool-name membership +
   `contains_secret_shaped()` (a content scanner, not an authorization check). The whole session
   carries **one bearer token** (`ApiClient`, package-level, minted once per `graphhelm serve`
   invocation the same way `serve/mod.rs::ensure_token`/`create_token_file` does). **This is the
   "package-level MCP host token" the issue names.** Any caller holding it may call any of the 14
   tools, regardless of which extension contribution (skill) is driving the call. The caller-supplied
   `X-GraphHelm-Actor` header is **unverified against any grant** — used only to build a
   `PersistedActor` for event attribution (confirms issue #110's framing).

3. **`core/schema/src/extension.rs`** already validates, per contribution, exactly the shape #213
   needs to enforce — and then **throws it away**:
   - `effects: Vec<String>` — closed set `CONTRIBUTION_EFFECTS` (`artifact.local.write`,
     `artifact.propose`, `external.read`, `host.discover`, `runtime.connect`, `runtime.mutate`,
     `runtime.read`).
   - `permissions: Vec<String>` — closed set `CONTRIBUTION_PERMISSIONS` (`network.external`,
     `network.loopback`, `owner.decision.request`, `package.read`, `runtime.read`,
     `token.reference.read`, `workspace.artifact.write`).
   - `surfaces: Vec<String>` — **a skill's declared claim about which MCP tools / CLI commands it
     drives**, checked as a subset of the closed `MCP_TOOLS`/`CLI_COMMANDS` allowlists (the exact
     14-tool list above, plus a CLI command list). This is already the per-contribution tool
     allowlist declaration the issue asks for — it does not need a new schema field.
   - `requires: ContributionRequires{capabilities, observers}`.

   `ValidatedExtensionPackage` — the struct every caller actually receives — carries only `id`,
   `version`, `contribution_count`, `package_digest`. None of the above survives validation. A
   repo-wide grep confirms **zero runtime consumers** of `.permissions`/`.effects`/`.surfaces`/
   `ContributionRequires` outside `extension.rs` itself and its own tests. This is exactly the gap
   the issue's invariant names: *declaration does not equal authorization.*

4. **No package-activation state exists** (#212 unimplemented). §4 below treats package digest as a
   standalone value computed fresh by `validate_extension_package` at token-mint and
   token-verify time — never a persisted "currently active package" registry.

**Consequence for design:** the correct shape of this task is *surface what `extension.rs` already
validated*, not invent a second, parallel declaration mechanism that could drift from the schema's
own reading (`duplicating an oracle` is worse than duplicating a mechanism — a second parser of
`extension.json`'s permission fields is exactly that failure mode). §3 commits to extending
`ValidatedExtensionPackage`, not re-parsing the manifest.

---

## 2. Threat assessment (written before any design line below, per the orchestrator's ordering)

Enumerated as **how a hostile package or a hostile/compromised MCP client would abuse this
mechanism**, matching the four the orchestrator named plus what the current code's own weak points
add:

| # | Abuse | Concretely | Must fail as |
|---|---|---|---|
| T1 | **Token reuse across contributions (confused deputy)** | Contribution A (e.g. `memory-curator`, declared `surfaces: []` per #224's own acceptance criterion — "can emit only advisory candidates/proposals") presents a token minted for contribution B (`code-contract`, which legitimately needs `approve`/`signal`) to call `approve`. Or: a token minted for one package's `schema/owner-output-policy` contribution is replayed against a *different* package that happens to declare a contribution with the same `id` string (ids are unique **within** a package, not globally). | Refused: the token binds `(package_digest, contribution_id)` as one unit, not `contribution_id` alone. A same-id contribution in a different package has a different `package_digest`, so the bound pair fails to match. |
| T2 | **Stale digest** | A package is edited after a token was minted for one of its contributions — e.g. `surfaces` is *narrowed* (declared tool access reduced) in a later edit, but the old token, minted before the edit, still carries the wider allowlist. Or the package is edited at all and the token's bound `package_digest` no longer matches the package's current `package_digest`. | Refused, fail-closed: verification recomputes the package digest fresh (via `validate_extension_package` on the package the MCP server is currently serving) and compares byte-for-byte against the token's bound digest. Any mismatch — widened OR narrowed — refuses. The token does not get "re-interpreted" against new content; it dies with the content it was minted against. |
| T3 | **Capability inherited implicitly** | A contribution declares `surfaces: []` (no MCP tools) but its token is minted with a non-empty `allowed_tools` anyway, e.g. because minting silently unions in a "family" default, a sibling contribution's grant, or the package-level `spec.permissions` block instead of reading the contribution's own declaration. | Refused by construction: minting reads **only** the one contribution's own `surfaces ∩ MCP_TOOLS` (§3.2) — there is no union, no family-level fallback, no package-level default folded in. An empty `surfaces` mints a token whose `allowed_tools` is the empty set, and the empty set denies every tool call (deny by default, mirroring `lease.rs`'s stated §8.2 model: "what is not granted does not exist"). |
| T4 | **Privilege escalation** | A contribution declares `surfaces: ["approve"]` in `extension.json`, but a hostile package author additionally sets `effects: ["runtime.mutate"]` *without* `approve` actually appearing in `MCP_TOOLS ∩ surfaces` — attempting to get mutation authority recognized through the `effects` string rather than the `surfaces` allowlist, since `effects` is a looser, package-level-checked field (`PackageGrants::grants_effect`, checked at parse time, not at the tool-call boundary). Or: a token is minted, then the contribution's `surfaces` list is *widened* in the package after minting, and the (already-refused-by-T2) stale token is somehow still accepted because verification checks `allowed_tools` against a **freshly re-read** manifest instead of the token's own frozen claim. | Refused: `allowed_tools` is read from `surfaces` **only**, at mint time, and frozen into the token's own bytes/claim (never re-derived from a live manifest at verify time — verify time only re-derives the **digest**, to catch staleness, and separately checks the **frozen** `allowed_tools` against the requested tool name). `effects`/`permissions` are not consulted for tool-name gating at all — they remain what `extension.rs` already enforces (package-internal consistency), kept separate so a package cannot buy tool access through a different declared field than the one actually checked. |
| T5 | **Actor spoofing (a gap the current code already has, which this task must not silently inherit)** | The MCP session's `X-GraphHelm-Actor` header is caller-declared and unverified (§1.2) — anyone with the bearer token can claim to be any actor string. A capability token "bound to actor" is only as strong as that binding is verified. | **Named as an accepted residual risk, not silently fixed by this issue and not silently ignored either.** Full actor authentication (verifying the caller really is the actor it claims) is out of scope for #213 — it would require a caller-identity mechanism this codebase does not have (no session cert, no signed actor assertion). What #213 *does* close: the token itself is presented as the credential (an opaque bearer value, like the existing session token), not just an actor-name header — so an attacker must possess the **token bytes**, not merely guess/declare an actor string, to pass `allowed_tools` gating. The actor field inside the token is checked for **consistency logging** (which actor a call is attributed to in the audit event) and as one more bound dimension an attacker must also match if they want the audit trail to lie convincingly, not as the primary secret. The primary secret is token possession. This must be stated in the PR's security review, not discovered by whoever reads this after the fact. |
| T6 | **Token replay after revocation, or after rotation** | A token is revoked (or superseded by rotation) but a client that cached it keeps presenting it, racing the revocation. | Refused: verification checks a revocation record on every call (§3.4), not only at mint or session-init time — no in-memory-only revocation that a long-lived session could outlive. |
| T7 | **Redacted-audit-event leakage** | The audit event for a refused call accidentally carries the tool call's *arguments* (which might contain a secret-shaped string, a file path with sensitive content, etc.), turning the audit trail itself into an exfiltration channel. | Refused by construction: the audit event schema (§3.5) carries only `(actor, contribution_id, package_digest, tool_name, decision, refusal_code?)` — never the call's JSON arguments, mirroring `BrokerRefusal`'s existing discipline ("never a call argument, a patch, or any caller content"; `lease.rs:42-43`) and `owner_output.rs`'s secret-scanning-before-render precedent. |

**Coordination note (per the orchestrator):** B measured (in #224's own work, not yet its own issue
number) that `publication`, `activation`, `composition`, `missingCapabilityResult`, and `hostViews` —
five *package-level* contract fields — currently have no reader in `core/`. This task's reader is
**contribution-level** (`surfaces`, `effects`, `permissions`, `requires`) and MCP-tool-scoped, a
different set of fields entirely — no overlap, no duplication risk against B's future issue. If B's
issue lands first and touches `ValidatedExtensionPackage`'s shape, this task rebases against it
rather than re-deriving package-level fields itself.

---

## 3. Design

### 3.1 Where the token type lives

New module: **`core/tool-broker/src/mcp_capability.rs`**. Rationale: `core/tool-broker` is already
the crate that owns "pure identity → allowlist → decision" pipelines (`lease.rs`) with the exact
discipline this needs (deny-by-default, typed refusals that never echo call content, an `actor`
validity charset). This is a sibling pipeline for a different domain (MCP tool names, not
shell/repository capabilities) — not a reuse of `Capability`/`ToolLease` (type mismatch, §1.1), but
the same crate, same conventions, same test idiom. `apps/cli`'s MCP command module becomes the impure
caller, exactly as `adapters/tool-host/src/host.rs` calls `lease::authorize()` today.

### 3.2 Token shape and minting

```rust
pub struct McpCapabilityToken {
    pub token_id: String,          // opaque, OS-random, the bearer secret (§3.3)
    pub package_digest: String,    // from ValidatedExtensionPackage.package_digest, frozen at mint
    pub contribution_id: String,   // from Contribution.id, frozen at mint
    pub actor: String,             // §T5: bound but not the primary secret
    pub allowed_tools: BTreeSet<String>,  // Contribution.surfaces ∩ MCP_TOOLS, frozen at mint — §T4
    pub effects: BTreeSet<String>, // Contribution.effects, carried for audit/observability only;
                                    // NOT consulted for tool-name gating (§T4)
    pub expires_at: <wire timestamp type already used elsewhere, e.g. RFC3339 string>,
    pub revoked: bool,
}
```

**Minting requires `ValidatedExtensionPackage` to expose per-contribution data it currently drops.**
§1.3's extension is the smallest change that closes this: add
`pub contributions: Vec<ValidatedContribution>` (id, surfaces, effects, permissions,
requires-capabilities) to `ValidatedExtensionPackage`, populated from data `extension.rs` already
parsed and validated — no new parsing logic, just surfacing the existing `Contribution` values that
currently die inside the validator. This is the one required change to `core/schema/src/extension.rs`
this task makes; everything else in that file is read-only from this task's perspective.

Minting itself (`mint(package: &ValidatedExtensionPackage, contribution_id: &str, actor: &str, ttl)`)
reads **one** contribution's own `surfaces`, intersects with the `MCP_TOOLS` allowlist (re-exported
or re-declared — TBD in implementation whether `extension.rs`'s `MCP_TOOLS` becomes `pub(crate)`
across the workspace boundary or `mcp_capability.rs` declares its own copy checked equal by a guard
test, mirroring how `DevelopmentRefusalCode` is checked against its schema by equality rather than
trusted by construction) — never a union across contributions, never a package-level default (§T3).

### 3.3 Where tokens live and how verification finds them

Mirrors `serve/mod.rs::ensure_token`/`create_token_file`'s existing, already-reviewed pattern: one
file per token under a directory scoped to the running `graphhelm serve`/`mcp` invocation, written
`create_new` (atomic, no overwrite), OS-random `token_id`, `0o600` on Unix, the bound claim
(`package_digest`, `contribution_id`, `actor`, `allowed_tools`, `effects`, `expires_at`, `revoked`)
serialized alongside it (not embedded *in* the bearer value itself — the bearer value the MCP client
presents is opaque; the claim it unlocks lives server-side, so revocation is "flip `revoked` / delete
the file," not "wait for a signed JWT to expire").

### 3.4 Verification pipeline (the pure part, testable without I/O)

```rust
pub fn authorize_mcp_call(
    token: &McpCapabilityToken,
    tool_name: &str,
    actor: &str,
    current_package_digest: &str,
    now: <time>,
) -> Result<(), McpCapabilityRefusal>
```

Order (pipeline order is itself a pin, matching `lease.rs`'s own documented contract that a
malformed-actor refusal precedes a mismatch refusal precedes a capability refusal):

1. `token.revoked` → `Revoked` (T6).
2. `now >= token.expires_at` → `Expired` (T6).
3. `actor != token.actor` → `ActorMismatch` (T5, consistency check).
4. `current_package_digest != token.package_digest` → `StaleDigest` (T2) — **the freshly recomputed
   digest, never a cached one**, per this task's own instrument-under-mutation lesson: a digest read
   once at session start and trusted for the rest of the session is exactly the staleness this check
   exists to catch.
5. `!token.allowed_tools.contains(tool_name)` → `ToolNotAllowlisted` (T1, T3, T4).

The impure caller (`apps/cli/src/commands/mcp/tools.rs::call()`) invokes this **before** the existing
closed-tool-name check and secret-shape scan remain as they are — this is an additional gate, not a
replacement for either existing one. On refusal, emit the redacted audit event (§3.5) and return the
existing typed MCP error shape; on success, emit the redacted *allowed* audit event and proceed to
today's dispatch unchanged.

### 3.5 Redacted audit events

New domain event, appended to the existing persisted-event stream (`core/protocols/src/event.rs`,
`core/events/`) rather than a parallel log file — the durable, replayable record this codebase
already trusts, not a second one this task invents:

```rust
pub struct McpCapabilityCallRecorded {
    pub actor: PersistedActor,
    pub contribution_id: String,
    pub package_digest: WireHash,
    pub tool_name: String,
    pub decision: McpCapabilityDecision,   // Allowed | Refused { code: McpCapabilityRefusal }
}
```

No call arguments, no token bytes, no claim beyond what §T7 names. `PersistedActor` is reused as-is
(§1's confirmation that it's attribution-only today is exactly the shape needed here — this task adds
no new authority to that type, only logs it).

### 3.6 Revocation and rotation

- **Revoke:** flip `revoked = true` on the claim file (or delete it — TBD by which is more auditable;
  current lean is flip-in-place so the audit event in §3.5 can still resolve `contribution_id` for a
  refusal-of-a-revoked-token event, rather than refusing with "unknown token").
- **Rotate:** mint a new token for the same `(package_digest, contribution_id, actor)` triple; minting
  **auto-revokes** any prior live token for that exact triple, so at most one live token exists per
  triple at a time — deterministic, no dual-valid window (§T6's replay race is closed by construction,
  not by a timing assumption).

---

## 4. Files in scope (derived, declared here since the issue body has none)

- `core/tool-broker/src/mcp_capability.rs` — new. Token type, pure `authorize_mcp_call`, refusal enum.
- `core/tool-broker/src/lib.rs` — module wiring only.
- `core/schema/src/extension.rs` — extend `ValidatedExtensionPackage` with per-contribution
  `surfaces`/`effects`/`permissions`/`requires.capabilities`, surfacing already-parsed data. No new
  parsing, no schema/`extension.json` format change.
- `apps/cli/src/commands/mcp/session.rs` — wire capability-token presentation into session init.
- `apps/cli/src/commands/mcp/tools.rs` — call `authorize_mcp_call` before dispatch; audit-event
  emission on both branches.
- `apps/cli/src/commands/mcp/client.rs` — if token presentation changes how `ApiClient` attaches
  credentials (TBD during implementation whether this file needs to change at all, or only
  `session.rs`/`tools.rs` do).
- `apps/cli/src/commands/mcp/mod.rs` (or a new `apps/cli/src/commands/mcp/token.rs`) — mint/revoke
  CLI surface (`graphhelm mcp mint-token` / `revoke-token`, exact naming TBD, checked against
  `CLI_COMMANDS`'s own "is this administrative authority I'm handing a skill" policy note at
  `extension.rs:131-150` before landing — a mint/revoke command is operator-only, not something a
  skill's own `surfaces` list may ever claim).
- `core/protocols/src/event.rs` — new `McpCapabilityCallRecorded` event + `McpCapabilityDecision`.
- `apps/cli/tests/development_mcp.rs` or a new `apps/cli/tests/mcp_capability.rs` — hostile fixtures
  (§5).
- `core/tool-broker/tests/` — new test file for the pure pipeline, mirroring `lease.rs`'s own test
  file structure.

**If any of the above turns out to need a file outside this list once implementation starts, that is
a stop-and-request-amendment per the pipeline's scope-discipline rule — not a quiet expansion.**

---

## 5. Test plan (RED first, hostile fixtures with a named real emitter — legal-vs-produced idiom)

Matching `apps/cli/tests/extension_cli.rs`'s established idiom (`valid_package()` fixture, mutate one
field, assert a specific domain code via `assert_domain_code()`):

1. `a_token_authorizes_its_own_allowlisted_tool` — GREEN-path control: the pipeline must say yes to
   something, or every refusal test below is vacuously true (`subset-of-empty-is-false` lesson).
2. `a_token_refuses_a_tool_outside_its_allowlist` (T3/T4 core case).
3. `a_token_minted_for_one_contribution_is_refused_against_a_different_contribution_in_the_same_package`
   (T1, confused deputy — same package_digest, different contribution_id).
4. `a_token_is_refused_after_the_package_digest_changes` (T2 — mint, then mutate the package, then
   verify with the freshly recomputed digest).
5. `a_widened_surfaces_declaration_after_mint_does_not_widen_an_already_minted_token` (T4's second
   half — proves `allowed_tools` is frozen at mint, not re-derived live).
6. `effects_alone_never_grants_a_tool_not_named_in_surfaces` (T4's first half — a contribution with
   `effects: ["runtime.mutate"]` but `surfaces` not containing `approve` still gets refused calling
   `approve`).
7. `a_revoked_token_is_refused_even_before_expiry` (T6).
8. `a_rotated_token_replaces_the_prior_live_token_for_the_same_triple` (T6, rotation).
9. `an_expired_token_is_refused` (boundary + past-expiry).
10. `refusal_events_never_carry_call_arguments_or_token_bytes` (T7 — sabotage-and-revert: deliberately
    leak an argument into the audit event, confirm the test catches it, then fix).
11. `an_actor_mismatch_is_refused_and_logged_under_the_tokens_own_actor_not_the_callers_claim` (T5).

Each hostile fixture names its **real emitter** before being written (legal-vs-produced discipline,
per B's #224 table) — i.e. the test states which line of `authorize_mcp_call` or `tools::call()` is
expected to produce the refusal, not just that "some refusal" occurs.

---

## 6. Out of scope (named explicitly, not left implicit)

- Full actor authentication (§T5) — a caller-identity mechanism beyond token possession. Flagged as
  residual risk in the PR's security review.
- #212's atomic package-activation state — this task computes digests fresh from whatever package the
  MCP server is currently serving; it does not add a persisted "active version" registry.
- Any change to `extension.json`'s schema/format. `surfaces`/`effects`/`permissions`/`requires` are
  consumed as they already exist.
- The five package-level fields B is measuring in #224 (`publication`, `activation`, `composition`,
  `missingCapabilityResult`, `hostViews`) — different fields, different (package, not contribution)
  scope, left for B's own issue.
- Non-MCP tool surfaces (`graphhelm tool invoke`'s `lease.rs` pipeline) — untouched, no shared type.
