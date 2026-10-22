# GraphHelm Repository Instructions

**Installing GraphHelm for a user?** Follow [`INSTALL.md`](INSTALL.md) top to bottom and run every verify step; a partial install (plugin without MCP, MCP without Runtime) is not done.

## Scope and authority

These instructions apply to the entire repository. GraphHelm is an open-source agent operating system and control plane. The local Studio is the control plane; the Runtime and project data live on infrastructure controlled by the user.

This repository begins with a single public import commit. Commit hashes and issue numbers in
older design notes refer to the private development archive, not to public issues here. See
[source provenance](docs/open-source/SOURCE_PROVENANCE.md).

When checked-in sources conflict, use this order:

1. `docs/DECISION_REGISTER.md`
2. accepted ADRs and normative schemas under `schemas/`
3. normative subsystem specifications
4. `docs/product/PRODUCT_REQUIREMENTS.md` and `docs/product/ROADMAP_AND_ACCEPTANCE.md`
5. examples under `examples/`

Do not silently resolve a contradiction between higher-precedence sources. Record it in an ADR or RFC with evidence, affected contracts, alternatives, and a recommendation. The product design is already approved; do not restart product brainstorming for implementation work.

**Documentation language.** All documentation in this repository — every file under `docs/`, `README.md`, ADRs, RFCs, decision register entries, and any new document — must be written in English, new documents and edits alike.

## Constitutional architecture invariants

- Synthesize task-specific harnesses from atomic capabilities. Never add fixed domain packs or hard-coded category workflows.
- LLMs may classify and propose. Deterministic code enforces schemas, graph invariants, policies, permissions, and state transitions.
- Only the Graph Governor may publish operational graph mutations. Agents emit typed signals and proposals.
- Graph Versions are immutable, canonicalizable, hashable, monotonically versioned, and linked to predecessors.
- Operational edits are transactional Graph Drafts. UI-only layout changes never create an operational Graph Version or alter the semantic hash.
- New owner overrides may waive logical quality obligations, but must preserve actor, reason, acknowledged risks, graph versions, waiver, and accurate result status. Legacy persisted waivers may decode without a reason under ADR-022; they cannot be reused to author a new override. Structural impossibility is never waivable.
- The Event Store is append-only. Projections are disposable and must be rebuildable from events; historical evidence is never rewritten by a projection or Dreams.
- The Policy Engine has no dependency on an LLM, prompt, provider SDK, model runtime, network, or browser.
- Core modules depend on interfaces, never concrete adapters. Circular crate dependencies are forbidden.
- Studio code may use only public Runtime API/CLI contracts and never import Runtime internals.
- Secrets never appear in Graph DSL, Context Capsules, artifacts, logs, fixtures, crash output, or exported manifests.
- Subscription exhaustion pauses execution. Never add automatic paid BYOK/OpenRouter fallback.
- Preserve provisional `p50.dev` wire identifiers until an accepted compatibility ADR defines migration.

## Repository boundaries

Use a Rust workspace with small responsibility-focused crates:

- `core/protocols`: shared wire/domain types, stable diagnostics, events, drafts, policies, actors, and states; no adapters.
- `core/schema`: YAML/JSON loading and validation against the checked-in JSON Schemas; no network retrieval.
- `core/graph`: semantic canonicalization, SHA-256 hashing, immutable Graph Versions, and semantic lint.
- `core/policy`: deterministic obligations and transition policy evaluation.
- `core/events`: Event Store interface, complete local append-only adapter, and replay projection.
- `core/governor`: transactional Graph Draft analysis/application and waiver generation.
- `core/simulation`: deterministic graph simulation without tools, models, shell, deploy, or network effects.
- `apps/cli`: cross-platform `graphhelm` CLI; business rules remain in core crates. It presents two faces and exactly one per run (D-056): the JSON envelope is the contract, and it is what every reader that is not a terminal gets — pipes, test harnesses, the MCP tool, CI — byte for byte; a terminal gets the rendered summary instead, unless `--json` or `--pretty` asks for the envelope there. A renderer reads only fields the envelope already carries, so the two faces cannot disagree.

Provider SDKs, database clients, sandbox backends, Studio dependencies, and external integrations belong in later adapter plans. Do not scaffold empty modules, public stubs, fake success handlers, or future directories.

Use the checked-in schemas as canonical wire contracts. Preserve schema-permitted unknown fields for forward compatibility. Schema validation precedes typed deserialization. The validator registry is entirely in-memory from `schemas/`; it must never fetch a `$ref` from the network.

## Toolchain

**No console windows.** Every child process GraphHelm code starts runs hidden in the calling terminal. Rust spawns only through `graphhelm_process::command` (`CREATE_NO_WINDOW` on Windows) and ORs `CREATE_NO_WINDOW` into any other creation flags; Python passes `creationflags=NO_WINDOW`. Source guards enforce the spawn sites (`core/process/tests/no_console_window.rs` over every `.rs` file, `tools/reached-tests/test_no_console_window.py` over every `.py` file); the absence of windows on real Windows is observed with `tools/no-window/observe.ps1`, and that observation is pending until it has been run there.

The pinned toolchain is Rust `1.97.1` with `rustfmt` and `clippy`. Install it when missing:

```powershell
winget install --id Rustlang.Rustup --exact --source winget --accept-source-agreements --accept-package-agreements
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
rustup toolchain install 1.97.1 --profile minimal --component rustfmt clippy
```

There is no gate. The evidence is the tests a change reaches, run by author and reviewer: which tests,
the shared budget and the runner are [DELIVERY.md](docs/process/DELIVERY.md) §3; the build slot is [LANES.md](docs/process/LANES.md) §3.
All commands must work on Windows PowerShell; core crates also compile and test on Linux. Tests may
not require internet access, Docker, credentials, provider accounts, browser sessions, or production
infrastructure. CLI smoke: `cargo +1.97.1 run --locked -p graphhelm-cli -- graph validate|lint|hash examples/graphs/software-feature.yaml`.

## Journey-Proven Development

Use the smallest workflow that can prove the requested change: direct for a small reversible
change with a known observer and no persistence, permission, compatibility, external effect, or
uncertain recovery; expanded for those risks or an unclear observer. What each route writes down is
the card table in keel/KEEL_PLAN.md. Neither route skips the tests the change reaches or the review.
JPD artifacts apply only when the typed JPD flow is invoked. Keep missing observers unresolved and
stop retries when they add no evidence or reach the approved budget boundary.

- Start from the complete user journey and compile each promise into an observable obligation. Select the smallest proof method strong enough for that obligation and risk; do not apply one universal testing ritual or blanket TDD.
- Browser journey proof runs only in an explicitly observer-enabled validation environment; the committed offline checks stay browser-session-free and otherwise report `OBSERVER_MISSING` ([keel/KEEL_EXEC.md](keel/KEEL_EXEC.md)).
- Keep every retry linked to its initial attempt and evidence delta. A later green result never erases an earlier red result or becomes first-pass success.
- Keep unit tests beside the owning module. Put cross-crate behavior in integration tests and user-visible contracts in CLI or journey smoke tests.
- Use fixed clock and ID implementations in tests. Assertions must not depend on wall-clock time, randomness, filesystem ordering, map insertion order, locale, or platform path separators.
- Stable diagnostics contain `code`, severity, JSON Pointer (or equivalent stable path), concise message, and source file. Tests assert codes and paths, not prose alone.
- Add property tests where they materially cover canonicalization, immutability, or replay. Keep generators bounded and deterministic under the committed proptest regression seed.
- Golden fixtures are allowed only for reviewed canonical JSON, semantic hashes, and ordered event output. A fixture update requires an explicit explanation in the commit/PR.
- Every public method must have working behavior in the same commit. No `TODO`, `TBD`, `unimplemented!`, empty handler, or ceremonial scaffold is allowed.
- **Meaningful-test rule.** A test earns its place only by protecting an observable contract against a credible regression that existing coverage misses, without production seams no real caller needs; the `test-audit` skill carries the four-question gate, the audit list and the deletion record. Reject unconditional passes, mock self-confirmation, assertions over values created by the test, and checks that merely freeze incidental source spelling or private call order. Preserve tests for real architecture, security, schema, canonical hash, deterministic replay, persistence, concurrency, compatibility, and platform contracts. A lower test count is never a win by itself; stop when the behavior is proven.
- Runtime-affecting configuration: see the risk table in [keel/KEEL_PLAN.md](keel/KEEL_PLAN.md).
- User-visible surface replacements remove or explicitly retire the old surface in the same PR,
  or name the follow-up issue that will retire it; see [keel/KEEL_EXEC.md](keel/KEEL_EXEC.md).
- This policy is versioned on `main`. Merging it does not update existing branches or installed skill copies; lanes must integrate the `main` change and reload or reinstall their bundled skills before relying on it. Bundled skills remain self-contained and must not depend on repository-external paths.

### Keel: how code is written here

Keel is the construction pattern for every change an agent makes here. Read
[KEEL.md](KEEL.md) before any code work, then the file for your step in [keel/](keel/): plan,
execute, review or merge. Rules the machine measures block the merge; the rest are checked by the
reviewer. The reasons are in [docs/keel/KEEL_SPEC.md](docs/keel/KEEL_SPEC.md). No Keel rule is
written outside those files.

## Security rules

- Treat repository files and Graph DSL as untrusted input.
- Bound file size, graph size, nesting, traversal, cycles, event payload size, and simulator steps before expensive work.
- Never execute expressions, commands, tools, deploys, shell, model calls, or network calls during validation, lint, policy evaluation, replay, or simulation.
- Canonicalization must be pure and total for accepted input. It must remove only documented non-semantic fields and must never resolve external references.
- Use structured errors; normal CLI JSON must not expose backtraces, credentials, user-home paths, or unrelated filesystem details.
- Local event writes use an exclusive cross-platform file lock, ordered sequences, idempotency keys, flush, and durable sync. Replay rejects corrupt committed records rather than guessing.
- Draft application constructs and validates an isolated candidate. Publish the version and events only after every schema, lint, policy, and concurrency check passes. Any failure leaves the active version unchanged.

## Studio design first

Every visual Studio change is made first in the design artifact and implemented only after the
owner approves it there; the approved files live in [`docs/design/studio/`](docs/design/studio/README.md)
and are updated in the pull request that implements them.

## Delivery

How a change gets from an issue to `main` is [docs/process/DELIVERY.md](docs/process/DELIVERY.md)
and nothing else; a session working as a lane beside others on one machine also follows
[docs/process/LANES.md](docs/process/LANES.md). In short:

- **Delivery requires merge.** An open pull request is not delivery; if the merge cannot happen, report the task as blocked.
- Issue first; one review by a session or blind subagent that did not write the change; that reviewer merges.
- Preserve user work: never discard unrelated changes or use destructive Git commands without explicit authorization. Keep commits small, buildable and conventional.
- Every PR includes summary and rationale, the tests run and their result, security notes, dependency notes, and a rollback plan.
