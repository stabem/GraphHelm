# GraphHelm Repository Instructions

## Scope and authority

These instructions apply to the entire repository. GraphHelm is an open-source agent operating system and control plane. The local Studio is the control plane; the Runtime and project data live on infrastructure controlled by the user.

The approved documentation baseline is commit `72c376499e4fc92f7a1097432c703d73c1b2f6b0`. When sources conflict, use this order:

1. `docs/DECISION_REGISTER.md`
2. accepted ADRs and normative schemas under `schemas/`
3. normative subsystem specifications
4. `docs/product/PRODUCT_REQUIREMENTS.md` and `docs/product/ROADMAP_AND_ACCEPTANCE.md`
5. examples under `examples/`
6. `CODEX_BOOTSTRAP_PROMPT.md`

Do not silently resolve a contradiction between higher-precedence sources. Record it in an ADR or RFC with evidence, affected contracts, alternatives, and a recommendation. The product design is already approved; do not restart product brainstorming for implementation work.

## Documentation language

All documentation in this repository — every file under `docs/`, `README.md`, `MASTER_PRD.md`, `CHANGELOG.md`, `DOCUMENTATION_MANIFEST.md`, ADRs, RFCs, decision register entries, and any new document — must be written in English. Do not add or merge Portuguese (or any other non-English) prose. This applies to new documents and to edits of existing ones.

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
- `apps/cli`: cross-platform `graphhelm` CLI and JSON presentation only; business rules remain in core crates.

Provider SDKs, database clients, sandbox backends, Studio dependencies, and external integrations belong in later adapter plans. Do not scaffold empty modules, public stubs, fake success handlers, or future directories.

## Toolchain and repository commands

The pinned toolchain is Rust `1.97.1` with `rustfmt` and `clippy`. Install it when missing:

```powershell
winget install --id Rustlang.Rustup --exact --source winget --accept-source-agreements --accept-package-agreements
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
rustup toolchain install 1.97.1 --profile minimal --component rustfmt clippy
```

This project runs **no hosted CI**. `ci/gate.ps1` is the authoritative gate and nothing verifies a
change unless it is run locally. Treat a red gate exactly as a red pipeline: do not merge. Run it
from the repository root:

```powershell
./ci/gate.ps1
```

It runs rustfmt, Clippy with `-D warnings`, workspace tests, the CLI suites, the schema catalog,
baseline compatibility and conformance commands, locked metadata, a whitespace check, and the
ignored PostgreSQL matrix twice - once in the C locale and once under a real collation, because the
C locale cannot reveal collation-dependent ordering defects. `-SkipPostgres` exists for changes that
cannot touch persistence; a run using it is not a full gate and must be reported as such.

**When you need to check the exit code, redirect to a file and check the exit code of that same
command - never pipe the live run.** From PowerShell: `./ci/gate.ps1 > gate.log 2>&1; echo $LASTEXITCODE`.
From bash invoking the PowerShell host directly: `powershell.exe -File ci/gate.ps1 > gate.log 2>&1; echo $?`.
Either way, read the file separately afterward if you need to page it (`Get-Content gate.log -Tail 50`
or `tail -50 gate.log`) - that read has no bearing on the exit code you already captured.

**Never pipe the gate's output when the exit code matters** (`./ci/gate.ps1 | tail`, `| head`,
`| grep`, and so on). In bash, `$?` after a pipe belongs to the last command in it, not to
`gate.ps1` - a genuinely red run reads back as success. This cost two silent false-greens in one
day before it was named (issue #97).

A branch cut before #100 carries the pre-#100 `Invoke-Stage` (the `& $Body` that swallows a native
tool's own stdout — issue #97/#98's log-completeness finding) and will keep producing gates with no
per-test failure text until it rebases onto a #100-or-later `main`, regardless of how the gate is
invoked. A red gate on such a branch is real; its missing diagnostic text is not evidence of a new
capture bug — check the branch's base before treating swallowed text as a fresh regression.

The individual commands, if you need to run one in isolation:

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

All commands must work on Windows PowerShell. Core crates must also compile and test on Linux in CI. Tests may not require internet access, Docker, credentials, provider accounts, browser sessions, or production infrastructure.

## Journey-Proven Development

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

## Foundation Graph Kernel constraints

Issue `#1` and `CODEX_BOOTSTRAP_PROMPT.md` define the first milestone. Implement only:

`Graph DSL YAML/JSON -> JSON Schema -> immutable GraphVersion -> semantic lint -> deterministic policy -> deterministic simulation -> append-only events -> transactional Graph Draft -> waiver -> replay -> JSON CLI`.

Use the checked-in schemas as canonical wire contracts. Model the stable typed subset required by this milestone and preserve schema-permitted unknown fields for forward compatibility. Schema validation precedes typed deserialization. The validator registry is entirely in-memory from `schemas/`; it must never fetch a `$ref` from the network.

Explicitly out of scope: Studio/Tauri/React, SSH bootstrap, Docker/Podman orchestration, PostgreSQL adapter, provider/model integrations, Agent Registry, Context Compiler, Knowledge Graph, Tool Broker, adaptive isolation, Dreams, hosted services, collaboration, marketplace, billing, and telemetry export.

## Security rules

- Treat repository files and Graph DSL as untrusted input.
- Bound file size, graph size, nesting, traversal, cycles, event payload size, and simulator steps before expensive work.
- Never execute expressions, commands, tools, deploys, shell, model calls, or network calls during validation, lint, policy evaluation, replay, or simulation.
- Canonicalization must be pure and total for accepted input. It must remove only documented non-semantic fields and must never resolve external references.
- Use structured errors; normal CLI JSON must not expose backtraces, credentials, user-home paths, or unrelated filesystem details.
- Local event writes use an exclusive cross-platform file lock, ordered sequences, idempotency keys, flush, and durable sync. Replay rejects corrupt committed records rather than guessing.
- Draft application constructs and validates an isolated candidate. Publish the version and events only after every schema, lint, policy, and concurrency check passes. Any failure leaves the active version unchanged.

## Git, review, and completion

- Issue-first is mandatory. The Foundation Graph Kernel is tracked by `#1`.
- The required implementation branch/worktree is `feat/foundation-graph-kernel`.
- Preserve user work. Never discard unrelated changes or use destructive Git commands without explicit authorization.
- Keep commits small and independently buildable. Use conventional commit messages and include `Closes #1` in the final implementation commit/PR body.
- **Never write a closing keyword next to an issue number you do not intend to close - not inside a negation, not inside a QUALIFIER, and not in a commit message.** The squash carries COMMIT text, and `closingIssuesReferences` reads only the PR BODY: they are different texts and GitHub links from both. Write `Refs #N` or `Scope: #N stays open` instead. Every one of these fires: `close`, `closes`, `closed`, `fix`, `fixes`, `fixed`, `resolve`, `resolves`, `resolved`. This has closed the wrong issue twice (#675, and #746 where the offending sentence was the negation written to prevent it), and a third was caught by hand on #781: `this closes #710's second finding only` narrows for a human and closes for the parser. A qualifier is not a guard either.
- Before pressing merge, run `ci/closing-keywords.ps1 -Number <pr> -Closes <intended...>`. It reads BOTH texts and refuses unless their union equals the intent. A body-scoped instrument agreeing is not the same question and must not retire this check.
- Every PR includes summary and rationale, validation evidence, security review, dependency notes, rollback plan, and the exact out-of-scope list.
- Before declaring completion, run every verification command from a clean state, inspect the output, review the full branch diff, and verify all automated acceptance scenarios. Code inspection alone is not completion evidence.

## Issue labels (owner directive, 2026-08-24)

Every new issue is created with exactly one of these labels — pass it at creation time
(`gh issue create --label <label>`), never leave an issue unlabeled:

- `current-wave` — a task of the active milestone, with an assigned agent.
- `in-flight` — a lane with work in progress right now.
- `tech-debt` — a measured finding placed on record; real, not yet scheduled. This is the default
  for defects, flakes, and gaps found while doing other work.
- `product-vision` — a capability that does not exist yet; future milestone, owner decision.

When an issue's situation changes (a debt gets scheduled, a wave task ships), swap the label —
one label per issue, and the label states the drawer, not the severity.
## Lane identity and multi-agent merge protocol

Several sessions work this repository at once and **all of them push, comment, and merge under one
GitHub account**. Nothing in the API says which session did what. The rules below are the only
attribution that exists; they are read at session start and are not re-explained per task.

### Identity line — first line of everything you write

Every issue or PR comment, review, merge comment, and commit body starts with:

```
Session: <your ListAgents name [ref]> · Head: <sha8 you measured>
```

The session name is the line "This session is …" that `ListAgents` prints — it is the only address
another session can message, and a hook cannot know it, so run `ListAgents` once and keep the line.
(`local_<session_id>` from the hook JSON is **not** deliverable — measured.) A lane letter, when the
board has given you one, goes in front: `Lane: <letter> · Session: … · Head: …`; it is a label for the
board, never a substitute for the session name. A comment without this line cannot be addressed,
re-asked, or audited; a carry without the head sha covers nothing.

### Peer review and peer merge (owner order, 2026-09-04)

- A PR needs **two passes from two different lanes, neither the author**, each naming the sha it
  measured. Say "no open roots **against `<sha>`**", never "no open roots".
- Approval is the reviewer's **word in the text**. GitHub cannot record it — self-approval is
  refused on a shared account — so every review is `COMMENTED`, and a review whose body declines
  to verify ("not a pin", "I will not record this as verified") is **not** a pass.
- Read each carry's **body** for declared limits and roots without a verdict. Each is closed by
  another pass, marked non-blocking by its author, or written into the merge comment as accepted
  risk with an issue number. Thread counts do not see findings written in review bodies.
- While the Codex sweep is unavailable, two passes are a **substitution, not an equivalence**: the
  merge comment names what the mechanical sweep would have caught (prose contradicting code,
  form-vs-instance matching, the third actor).

### Before you press

Run `.factory/MERGE-CHECKLIST.md` and cite its item numbers in the merge comment. The three that
each caught a real loss: closing keywords read in **PR title, body, commit messages, and
`closingIssuesReferences`** with the union equal to intent (the title is the squash's first line) — on #746 the body was fixed and the
parser came back clean, and the squash `34cced9c` still shut issue #717 through the negated sentence
left in a COMMIT body (the instrument that frees the PR is blind to the field that shuts the issue);
carry bodies read, not counted (#754, #758); and `gh pr list --base <branch> --state all` before
`--delete-branch` (#713's delete shut the stacked #729).

### After you press — read the output, never your intent

`gh pr view N --json mergedAt,mergeCommit`; `git log -1 --format=%B origin/main` for what the
squash actually carried; `gh issue view` for every issue it names; the branch endpoint returns
`404` with the text `Branch not found`. Corrections go in the same PR as a follow-up comment.

### Tooling traps that produced false readings (each measured; long form in the checklist)

- A failed `git fetch` leaves the ref at its **old** value and the next `rev-parse` succeeds —
  check the fetch's exit code immediately, or fetch into a **new** ref name every time.
- MSYS mangles `<ref>:<path>` for some path shapes (`origin/main:.factory/x`, `origin/main:/core/x`)
  and not others (`origin/main:core/x`); git then says "ambiguous argument" or "invalid object name".
  Do not learn the rule — use the sha instead of the ref, or `MSYS_NO_PATHCONV=1`.
- Bash: `$?` after a pipe is the last command's. PowerShell: after `| Select-Object -First N` the exit
  code is not the command's — different PATHs read `-1` in different quadrants. Test the value.
- `git ls-tree` without `--full-tree` is scoped to the CWD prefix and returns empty with rc=0.
- A zero produced by a filter you wrote (`grep`, `head`, a regex calibrated on the old format) is
  a measurement of the filter. Put a known positive in the same command.
