# GraphHelm Repository Instructions

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
6. `CODEX_BOOTSTRAP_PROMPT.md`

Do not silently resolve a contradiction between higher-precedence sources. Record it in an ADR or RFC with evidence, affected contracts, alternatives, and a recommendation. The product design is already approved; do not restart product brainstorming for implementation work.

## Documentation language

All documentation in this repository — every file under `docs/`, `README.md`, `MASTER_PRD.md`, `CHANGELOG.md`, ADRs, RFCs, decision register entries, and any new document — must be written in English. Do not add or merge Portuguese (or any other non-English) prose. This applies to new documents and to edits of existing ones.

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
- `apps/cli`: cross-platform `graphhelm` CLI; business rules remain in core crates. It presents TWO faces and exactly one per run (D-056, #1172): the JSON envelope is the contract, and it is what every reader that is not a terminal gets — pipes, test harnesses, the MCP tool, CI — byte for byte; a terminal gets the rendered summary instead, unless `--json` or `--pretty` asks for the envelope there. A renderer reads only fields the envelope already carries, so the two faces cannot disagree.

Provider SDKs, database clients, sandbox backends, Studio dependencies, and external integrations belong in later adapter plans. Do not scaffold empty modules, public stubs, fake success handlers, or future directories.

## Toolchain and repository commands

The pinned toolchain is Rust `1.97.1` with `rustfmt` and `clippy`. Install it when missing:

```powershell
winget install --id Rustlang.Rustup --exact --source winget --accept-source-agreements --accept-package-agreements
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
rustup toolchain install 1.97.1 --profile minimal --component rustfmt clippy
```

There is **no gate** (owner order, 2026-09-24). The queued runners, the receipt store and
`merge-proof` are retired; `ci/gate.ps1` remains only as an optional full local check that nothing
requires and nothing merges on. The evidence for a change is the tests the
change can reach, run by the author and again by the reviewer, and named in the PR
([docs/process/DELIVERY.md](docs/process/DELIVERY.md)). The individual commands:

```powershell
cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 test --workspace --all-features --locked
cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
cargo +1.97.1 metadata --locked --no-deps --format-version 1
git diff --check
```

Canonical CLI smoke commands:

```powershell
cargo +1.97.1 run --locked -p graphhelm-cli -- graph validate examples/graphs/software-feature.yaml
cargo +1.97.1 run --locked -p graphhelm-cli -- graph lint examples/graphs/software-feature.yaml
cargo +1.97.1 run --locked -p graphhelm-cli -- graph hash examples/graphs/software-feature.yaml
cargo +1.97.1 run --locked -p graphhelm-cli -- graph simulate examples/graphs/software-feature.yaml --events target/graphhelm-smoke-events.jsonl
cargo +1.97.1 run --locked -p graphhelm-cli -- graph replay --events target/graphhelm-smoke-events.jsonl
```

All commands must work on Windows PowerShell. Core crates must also compile and test on Linux.
Tests may not require internet access, Docker, credentials, provider accounts, browser sessions,
or production infrastructure.

## Journey-Proven Development

### Task workflow routing

Use the smallest workflow that can prove the requested change: direct for a small reversible
change with a known observer and no persistence, permission, compatibility, external effect, or
uncertain recovery; expanded for those risks or an unclear observer. The Keel card table below
says what each route writes down. Neither route skips the tests the change reaches or the review.
JPD artifacts and certification semantics apply only when the typed JPD flow is invoked; an
informal proof remains ordinary evidence. Keep missing observers unresolved and stop retries when
they add no evidence or reach the approved budget boundary. There is no universal ceremony count.

### Proof rules

- Start from the complete user journey and compile each promise into an observable obligation. Select the smallest proof method strong enough for that obligation and risk; do not apply one universal testing ritual.
- Use RED -> GREEN -> REFACTOR when a focused automated test is the best proof for the behavior. Preserve unit, property, integration, concurrency, CLI, and browser tests where each observes the correct boundary. Browser journey proof runs only in an explicitly observer-enabled validation environment; the committed offline gate remains browser-session-free and otherwise reports `OBSERVER_MISSING`.
- If a promised behavior has no adequate observer, stop with `OBSERVER_MISSING`. Never treat a proxy such as HTTP acceptance as proof of delivery or rendering.
- Keep every retry linked to its initial attempt and evidence delta. A later green result never erases an earlier red result or becomes first-pass success.
- Keep unit tests beside the owning module. Put cross-crate behavior in integration tests and user-visible contracts in CLI or journey smoke tests.
- Use fixed clock and ID implementations in tests. Assertions must not depend on wall-clock time, randomness, filesystem ordering, map insertion order, locale, or platform path separators.
- Stable diagnostics contain `code`, severity, JSON Pointer (or equivalent stable path), concise message, and source file. Tests assert codes and paths, not prose alone.
- Add property tests where they materially cover canonicalization, immutability, or replay. Keep generators bounded and deterministic under the committed proptest regression seed.
- Golden fixtures are allowed only for reviewed canonical JSON, semantic hashes, and ordered event output. A fixture update requires an explicit explanation in the commit/PR.
- Every public method must have working behavior in the same commit. No `TODO`, `TBD`, `unimplemented!`, empty handler, or ceremonial scaffold is allowed.
- Before adding or requesting a test, name the observable contract, the plausible defect it would catch, and the gap in existing coverage. Choose the smallest adequate proof and reuse existing coverage when it observes the contract. Do not impose blanket TDD, red-first steps, mutation exercises, or test quotas.
- Reject unconditional passes, mock self-confirmation, assertions over values created by the test, and checks that merely freeze incidental source spelling or private call order. Preserve tests for real architecture, security, schema, canonical hash, deterministic replay, persistence, concurrency, compatibility, and platform contracts.
- Runtime-affecting configuration needs behavioral evidence; parsing or shape validation alone is not proof. Report passed, failed, skipped, and unobserved separately. A skipped or unavailable observer never counts as a pass, and an unobserved promise remains unresolved.
- This policy is versioned on `main`. Merging it does not update existing branches or installed skill copies; lanes must integrate the `main` change and reload or reinstall their bundled skills before relying on it. Bundled skills remain self-contained and must not depend on repository-external paths.


### Keel: how code is written here

Keel is this repository's development model for code written by agents. Its goal, in order:
**quality preserved first, then the lowest total cost per proven delivery** — every attempt,
repair, review and watchdog run counted, not tokens alone. The reasoning is in
[docs/harness/KEEL_PARADIGMS_PAPER.md](docs/harness/KEEL_PARADIGMS_PAPER.md); the specification is
[docs/keel/KEEL_SPEC.md](docs/keel/KEEL_SPEC.md). **Keel is guidance and measurement today, not punishment.** Its
counts are reported, not enforced, except where a contract is objective (below). Penalties that
narrow what a seat may write stay off until a controlled comparison shows they add value over
guidance alone (paper, section 7).

**Keel is proportional. Use only as much of it as the change needs.**

| The change | What Keel asks |
|---|---|
| Docs, comments, config values, a one-line fix, a test-only fix | Nothing beyond the task record. |
| A bounded code change on the direct route (above) | A three-line card in the PR body: the paths in scope, the promise, the command that proves it. |
| New public surface: a new module, type, public function, dependency or test file | The full card, and the new surface named in the PR body. |
| The expanded route: persistence, permissions, compatibility, security, external effects | The full card and the JPD flow above. |

**The four moves, when Keel applies:**

1. **Start from the promise.** State what changes, what must keep working, and the command that
   observes each. The card names paths, not globs.
2. **Start from the card, then search on purpose.** Open what the card names first. When you need
   more, search for the specific symbol or caller rather than browsing, and record in the PR body
   what context the card was missing: that record is how cards get better.
3. **Add only surface the promise needs.** Prefer extending an existing body or reusing a proven
   symbol over a new type, layer or helper. A new interface needs real callers; a producer and its
   consumer may arrive in the same change when both are proven there. Counts of new modules, types,
   functions and tests are signals for the reviewer, never a quota: do not inflate a function or
   drop a test to stay under a number.
4. **Prove with the smallest adequate observer.** Name the criterion and the defect a test would
   catch, assert against a value the code under test did not produce, mock only I/O, clock and
   randomness. Mutation testing is used only when it adds evidence the existing proof lacks; it is
   not an admission ritual (the meaningful-test rule above governs).

**What is enforced (objective contracts only):** one review by another session or a blind subagent that runs the
reached tests, as [docs/process/DELIVERY.md](docs/process/DELIVERY.md) says; a new dependency is
named in the PR body; a card lists paths rather than globs. **What is never inferred from size:** a
small diff is not a safe diff. A one-line change can remove a permission check, so risk is read
from what the change touches, not from how much it adds.

**Drift is recorded with its cause.** When a rule fires, record whether the cause was the card, the
tool, the task or the agent before anyone restricts anything.

## Foundation Graph Kernel constraints

The initial milestone implemented this sequence:

`Graph DSL YAML/JSON -> JSON Schema -> immutable GraphVersion -> semantic lint -> deterministic policy -> deterministic simulation -> append-only events -> transactional Graph Draft -> waiver -> replay -> JSON CLI`.

Use the checked-in schemas as canonical wire contracts. Model the stable typed subset required by this milestone and preserve schema-permitted unknown fields for forward compatibility. Schema validation precedes typed deserialization. The validator registry is entirely in-memory from `schemas/`; it must never fetch a `$ref` from the network.

The historical `CODEX_BOOTSTRAP_PROMPT.md` and its issue numbers do not limit current work. Scope
new work by its current public issue, the decision register, accepted ADRs, and checked-in schemas.

## Security rules

- Treat repository files and Graph DSL as untrusted input.
- Bound file size, graph size, nesting, traversal, cycles, event payload size, and simulator steps before expensive work.
- Never execute expressions, commands, tools, deploys, shell, model calls, or network calls during validation, lint, policy evaluation, replay, or simulation.
- Canonicalization must be pure and total for accepted input. It must remove only documented non-semantic fields and must never resolve external references.
- Use structured errors; normal CLI JSON must not expose backtraces, credentials, user-home paths, or unrelated filesystem details.
- Local event writes use an exclusive cross-platform file lock, ordered sequences, idempotency keys, flush, and durable sync. Replay rejects corrupt committed records rather than guessing.
- Draft application constructs and validates an isolated candidate. Publish the version and events only after every schema, lint, policy, and concurrency check passes. Any failure leaves the active version unchanged.

## Delivery: issue, review, merge

How a change gets from an issue to `main` is [docs/process/DELIVERY.md](docs/process/DELIVERY.md),
and nothing else. It replaces the old `.factory/` protocol and the `superpowers` plans. In short:

- **Delivery requires merge.** An open pull request is not delivery. A change is delivered only
  after it is merged into `main`; if the merge cannot happen, report the task as blocked.
- Issue-first. Branch `issue-<N>-<short-description>`. One label per new issue.
- One review by a session or a blind subagent that did not write the change. It runs the tests
  the change reaches, names them, and merges (squash, head pinned, closing check). No gate, no separate presser.
- Every comment, review and commit body starts with `Session: <ListAgents name [ref]> · Head: <sha8>`,
  because every session pushes under one GitHub account.
- Preserve user work. Never discard unrelated changes or use destructive Git commands without
  explicit authorization. A session removes only what it created, by name.
- Keep commits small and independently buildable, with conventional commit messages.
- Never write a closing keyword next to an issue you do not intend to close, not even inside a
  negation or a qualifier; write `Refs #N`. Check with `ci/closing-keywords.ps1` before merging.
- Every PR includes summary and rationale, the tests run and their result, security notes,
  dependency notes, and a rollback plan.
- **A deadline consulted once per iteration bounds the ITERATIONS, not the wall time.** Before approving a budget, ask the question that separates a real bound from an apparent one: **between this check and the next one, what is the longest thing that can happen?** If the answer is an unbounded call, the deadline is advisory and must SAY so where the constant is defined; if it is one more pass of a loop whose body is bounded, it is real. A clock read before a blocking call bounds when the call BEGINS, not how long it runs. This shape shipped four separate times before it was named - the write half of a request deadline, the read half of the same one, a post-release grace computed outside its collect loop, and a poll loop whose status read was unbounded - and every one was written by the person who had just added the bound. **The bound EXISTING is what makes it invisible:** a reviewer who sees `Instant::now() + Duration::from_secs(10)` reads a guarantee and moves on. Nothing goes red; the failure is a call that takes longer than the number printed beside it. (Refs #796.)
