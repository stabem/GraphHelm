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
cannot touch persistence; a run using it is not a full gate and must be reported as such. The slot
lock (`SLOT.lock`) exists for a **shared** `CARGO_TARGET_DIR`; with an isolated target directory
per lane there is nothing to contaminate — but `D:` is one platter and five concurrent gates stalled the
machine (measured 2026-09-05), so the ceiling is **one gate on the HDD plus one on the SSD** (`E:/<lane>-targets`
while `E:` keeps >30 GB free — `Get-PSDrive E` first); `C:` (the system SSD) may hold ONE build or review
target per lane, at most TWO on the board, only while ≥ 100 GB stay free, removed by its creator, never a gate
target — a full `C:` takes the machine down, so the floor is the rule; and never `F:` (the repository disk,
15 GB free). The long form is `.factory/MERGE-CHECKLIST.md` item 2. Before launching, count LAUNCHES, not cargos — one
gate is 2–9 cargo processes: live `powershell.exe` launching `ci/gate.ps1` (by `-File` or by `-Command … &`;
the regex in `.factory/MERGE-CHECKLIST.md`, tested against real command lines) with at least one descendant;
at most one other live gate, on the other spindle. Proof of life is a CPU delta on the
compiling descendant read 30 s apart (dead = read fails, wedged = equal, progressing = greater). The package-cache lock
(`$CARGO_HOME/.package-cache`, one per machine, unchanged by `CARGO_TARGET_DIR`) is still shared: on
`Blocking waiting for file lock on package cache`, wait — it frees itself and the gate proceeds; it is not a
lost gate; do not relaunch (see #833's matcher).

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
- **Run the merge proof from `origin/main`'s copy, never from the candidate checkout (#733).** Run from the branch being judged, the pull request supplies the predicate that judges it: replacing `ci/merge-proof.ps1` with `exit 0` certifies anything, and the self-check it performs against `origin/main` is removed by the same edit. Self-attestation has no fixed point, so the control belongs to the PROCEDURE rather than to the file. The supported invocation obtains the runner from main as well:

  ```powershell
  git fetch --quiet origin main
  git worktree add --detach --quiet $env:TEMP\mp-main origin/main
  powershell -NoProfile -ExecutionPolicy Bypass -File $env:TEMP\mp-main\ci\merge-proof-from-main.ps1 -PullRequest <N> -RepositoryRoot .
  git worktree remove --force $env:TEMP\mp-main
  ```

  **Not `git show ... > file`.** On Windows PowerShell 5.1 `>` is `Out-File` and re-encodes: measured, that spelling turns main's 63908-byte LF file into 64901 bytes with a BOM and a different blob id, and the result parses with zero errors. `git worktree` has git write the bytes, so no encoding decision exists to get wrong.

  `-RepositoryRoot` names the checkout **under judgement**: the verifier otherwise defaults to the repository it sits in, which once extracted is a temporary directory. `ci/merge-proof-from-main.ps1` additionally compares the extracted file against `git rev-parse origin/main:ci/merge-proof.ps1` and refuses on a mismatch, because a decode that mangles a byte still parses and still runs. Whatever automation presses the button uses main's copy too, so the human instruction is not the only thing standing between a candidate and its own predicate.
- Every PR includes summary and rationale, validation evidence, security review, dependency notes, rollback plan, and the exact out-of-scope list.
- Before declaring completion, run every verification command from a clean state, inspect the output, review the full branch diff, and verify all automated acceptance scenarios. Code inspection alone is not completion evidence.
- **A deadline consulted once per iteration bounds the ITERATIONS, not the wall time.** Before approving a budget, ask the question that separates a real bound from an apparent one: **between this check and the next one, what is the longest thing that can happen?** If the answer is an unbounded call, the deadline is advisory and must SAY so where the constant is defined; if it is one more pass of a loop whose body is bounded, it is real. A clock read before a blocking call bounds when the call BEGINS, not how long it runs. This shape shipped four separate times before it was named - the write half of a request deadline, the read half of the same one, a post-release grace computed outside its collect loop, and a poll loop whose status read was unbounded - and every one was written by the person who had just added the bound. **The bound EXISTING is what makes it invisible:** a reviewer who sees `Instant::now() + Duration::from_secs(10)` reads a guarantee and moves on. Nothing goes red; the failure is a call that takes longer than the number printed beside it. (Refs #796.)

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
re-asked, or audited; a carry without the head sha covers nothing. **Put it in the OPENING of the
comment, not in the footer.** A line at the bottom of a long body is read as absent by whoever is
counting passes: measured 2026-09-08, an identity line that was present as the last of 49 lines
behaved exactly like a missing one.

### Peer review and peer merge (owner order, 2026-09-04)

- A PR needs **two passes from two different lanes, neither the author**, each naming the sha it
  measured. Say "no open roots **against `<sha>`**", never "no open roots". **Whoever presses is a
  third lane** — neither the author nor either reviewer. **When no such lane exists** — every live
  lane other than the author holds a verdict on the PR — **the reviewer whose pass is EARLIEST at
  the current head, or carried to it under THE TWO MANIFEST QUESTIONS (canonical block at the top of
  `.factory/MERGE-CHECKLIST.md`, which this line does not restate), presses**, and writes in the merge comment that the third-lane set was empty, naming
  every lane and the sha of its verdict. The earliest pass is read from the ISSUE-COMMENT surface
  only: a pull-request review can be edited in place too (the update-review endpoint) and its
  listing carries no edit instant, so a review body classifies its lane but is never the earliest
  pass — a lane whose pass is only a review re-posts it as a comment. An exact tie has not occurred
  (0 in 279 bodies across 14 PRs); if two comment passes tie to the second, both lanes re-affirm
  in a new comment and the earlier of those presses. Earliest, not most recent: "most recent" is chosen by
  something a reviewer controls (their own re-pin), so a lane could select itself; the earliest
  pass at a head is fixed the moment it is written — its `created_at` — and **an edited body is not
  a pass for this choice**: GitHub records when a comment appeared and when it last changed, never
  when its verdict was written, so any rule computed from those two fields is moved by an edit in
  one direction or the other; a lane whose pass was edited re-posts it unedited, and a carried pass
  keeps the instant it was written with. A PR with two passes and a manifest for its head
  does not wait on a rule; the rule exists to keep a reviewer from approving their own reading, and
  a reviewer pressing on a *second* lane's independent pass keeps that property. (Owner order,
  2026-09-07, recorded verbatim in the original Portuguese on #901 issuecomment-5572591257 — in
  English: "the orchestrator must decide everything about the project; I only do not want you to
  waste time waiting for my answer" — and applied in #977: the orchestrator decides the protocol;
  #952 sat twelve hours with complete evidence because the eligible set was empty, and that wait
  is the cost this clause removes.)
- **The gate runs before the passes, and nothing is pushed after them.** `ci/gate.ps1` commits its
  manifest onto the branch it judged, so a pass written before the gate names a sha the PR no longer
  has; the order is A plans, B implements, C gates, D passes at the head that CARRIES the manifest (not
  the head it NAMES - a published receipt is itself the head, and names the parent), E presses.
  After the passes, a new finding on an otherwise clean PR is a declared gap in that PR's own body -
  no new issue and no follow-up PR (owner, 2026-09-05), and named without a closing keyword so the
  squash cannot shut a number that must stay open - not a push. What DOES authorise moving a reviewed
  head is the three-condition rule in `.factory/lane-loop.md` section 0, which this line points at
  rather than repeating: a partial copy here listed two of the three, which is how the same rule
  disagreed with itself across two files.
  Sequencing lives in `.factory/lane-loop.md` section 0, which nothing in this file referenced before
  (measured: zero occurrences of `lane-loop` in `AGENTS.md` at `68519ce2`), which is how a restarted
  lane kept reading the old order. (Owner's restructure, 2026-09-08.)
- Approval is the reviewer's **word in the text**. GitHub cannot record it — self-approval is
  refused on a shared account — so every review is `COMMENTED`, and a review whose body declines
  to verify ("not a pin", "I will not record this as verified") is **not** a pass. The identity line
  records the head at the moment of writing, not the verdict: **the sha that holds the button is the
  one in the comment that states the verdict** — not the most repeated, not the most recent.
- Passes live on two surfaces, `issues/N/comments` and `pulls/N/reviews`; count both (#840).
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
  code is not the command's — different PATHs read `-1` in different quadrants. Test the value. A gate's
  log going quiet mid-stage is capture (output written when the stage ends), not death.
- `git ls-tree` without `--full-tree` is scoped to the CWD prefix and returns empty with rc=0.
- A zero produced by a filter you wrote (`grep`, `head`, a regex calibrated on the old format) is
  a measurement of the filter. Put a known positive in the same command.
- A repository-wide sweep measures **the tree you ran it in**, and a stale one under-reports without
  erroring. Seven fleet worktrees were 315-374 commits behind; `ci/find-culture-comparisons.ps1` read
  `3 over 3` in one of them where a fresh checkout of the same head read `354 over 36`, exit 0 both
  times (#835). Measure from a fresh worktree of `origin/main`, and read the `Tree:` line the sweep
  prints before you believe its number. Whether a directory IS the main checkout is decided by
  `--git-dir` against `--git-common-dir` and **never by a file count** -- `git ls-files` there is
  scoped to the CWD prefix and answers 0, which is the same hazard wearing the shape of an answer.
