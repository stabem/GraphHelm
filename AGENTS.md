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
machine (measured 2026-09-05), so the ceiling is **one gate on the HDD plus one on the SSD** (`E:\_agent-scratch\graphhelm\<lane>\target`
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

## Disk hygiene (owner order, 2026-09-13)

Measured 2026-09-13 ~14:00 local, on the owner's machine, with `Get-ChildItem D:\ -Force` and
`cmd /c dir /s /-c` per directory: `D:\` had 416 directories and 893 loose files at its root
(177 directories with `target` in the name, none written to for 48 h except two; the rest lane
worktrees, `*-runs`, `pr-*`, `gh-*` scratch and log files) and 18 GB free. `F:\github\GraphHelm`
weighed 70 GB: 47 GB in `.worktrees\`, 23 GB in `.claude\worktrees\`, and 22 of those worktree
directories carried a `target\` dir untouched for 49 h to 18 days. `F:` had 11 GB free. No rule
said where a lane's scratch goes; ED-10 said "each owner deletes their own when the lane closes"
and it was not done; `lane-loop.md` ("no wide deletes, no `worktree prune`") and
`.factory/MERGE-CHECKLIST.md` item on targets ("`Remove-Item` by name, never a sweep") forbid the
only other way it could have happened. The rule now:

- **One scratch root per lane:** `D:\_agent-scratch\graphhelm\<lane>\` (a lane letter or an issue
  number - one directory per lane, sub-directories per branch if the lane has two). Never a new
  directory at the root of `D:\`, never inside a worktree, never on `F:` (`F:` holds code only).
  The cargo target dir is `CARGO_TARGET_DIR=D:\_agent-scratch\graphhelm\<lane>\target`, written
  **on the same line as the `cargo` call** (ED-11). Run logs, gate logs and probes go in the same
  lane directory. ED-5's `D:\gh-check\<letter>\<issue>` and MERGE-CHECKLIST's `E:\<lane>-targets`
  and `C:\<lane>-targets` allowances keep their conditions but move under the same shape:
  `<drive>\_agent-scratch\graphhelm\<lane>\...`. The queued runner's managed roots are the one
  exemption (Codex on #1068): `ci/gate-runner.ps1` takes `-BenchRoot`/`-TargetRoot` (defaults
  `D:\runner-ssd|hdd`, `E:\runner-targets\ssd`, `D:\runner-targets\hdd`; the board runs it with
  `D:\orch-runner-benches-ssd` and `E:\orch-runner-targets`), creates `<TargetRoot>\pr<N>` per pull
  request and removes the previous run's target of the same PR before a re-run; whoever presses the
  merge removes `<TargetRoot>\pr<N>` afterwards (measured 2026-09-15: four gates filled `E:` to
  11 GB and the fifth went RED with `os error 112`). A hand-run gate bench (`.factory/lane-loop.md`
  section 1, `E:/<lane>-<n>`) is a short-lived bench on the SSD for the same reason and is removed
  by the lane that made it when the manifest is published.
- **Worktrees live under `<repo>\.worktrees\<branch>`.** `.claude\worktrees\` (Claude Code's own
  root) and `D:\codex\worktrees\` (Codex's own root) are accepted because the tool chooses them, not
  the session; a throwaway detached worktree created and removed in the same command (the
  merge-proof recipe below) is not a lane worktree. No new lane worktree at the root of `D:\`.
- **Whoever opens a lane closes it.** When its PR merges or the lane is abandoned, the same session
  runs `git worktree remove <path>`, `git branch -D <branch>` (a squash never makes the branch an
  ancestor of `main`, so `-d` refuses every merged lane branch; before `-D`, confirm the merge with
  `gh pr view <N> --json state,mergeCommit,headRefOid` AND that the local tip is that `headRefOid` or an
  ancestor of it — the same predicate the sweeper applies, because a reused branch name with new local
  commits still reads MERGED on GitHub), and removes its own
  `D:\_agent-scratch\graphhelm\<lane>`. Evidence lives in the PR or the issue; a gate log that exists
  only on `D:\` is not evidence of anything.
- **The only sweeper is `F:\github\Dale\dale-ci\disk-sweep.ps1`** (not part of this repository;
  dry-run by default, `-Apply` to act). It removes, in this order: (1) cargo target dirs idle
  > 48 h - a directory counts as a target only if it (or every direct child) carries a cargo
  marker (`CACHEDIR.TAG`, `.rustc_info.json`, `debug`, `release`), and the main checkout's own
  `target` is never touched; (2) worktrees that are clean AND idle > 7 days AND merged - "merged" =
  the branch is an ancestor of `origin/main` OR GitHub lists a merged PR whose head is the local tip
  or a descendant of it, because a squash never makes the branch an ancestor and a reused branch
  name with new local commits is not merged - or, failing that, fully contained in `origin/<branch>`
  and idle > 14 days; "clean" ignores `tools/ci-canary/src/nonce.rs`, which `ci/gate.ps1` rewrites
  on every run (#152) and which is restored before removal;
  it runs `git worktree prune` for registered worktrees whose directory is gone; (3) `D:\` root
  directories whose NAME matches the agent-scratch patterns fixed in the script (`gh-*`, `pr-*`,
  `<letter>-<2-4 digits>*`, `*-runs`, ...) idle > 14 days - an unclassified name is left alone.
  Declared residual: `<letter>-<2-4 digits>` IS the lane shape, so a human directory named like
  `f-16` or `b-52` at the root of `D:\`, idle > 14 days, would be swept; measured 2026-09-13 against
  every root name and 55 human-shaped probes, only that shape collides. Do not name a personal
  directory that way at the root of `D:\`, or add it to the script's keep list; (4) loose
  `*.log|txt|json|...` files at the root of `D:\` idle > 14 days; (5) Dale desktop installers beyond
  the newest three and a `win-unpacked` idle > 7 days; (6) `.tmp-*`, `.codex-work`, `.review-*`, `.probe` at a repo root idle > 7 days;
  (7) `D:\codex\scratch\*` idle > 7 days; (8) `D:\_agent-scratch\<repo>\<lane>` idle > 14 days.
  In (3) and (8) a directory that holds a registered worktree, or any `.git` within three levels,
  is skipped whatever its name (measured: six worktrees live under `D:\*-runs\`, e.g.
  `D:\m-runs\w827`) - only (2) may act on a worktree. It never touches the main checkout, a worktree on `main`, a dirty or locked worktree, one whose
  `git status` failed, or a branch with commits absent from both `origin/main` and
  `origin/<branch>`; it unlinks junctions inside a worktree before removal and verifies the link
  target survived; it calls `git worktree remove` without `--force` and keeps the worktree if git
  refuses; and it keeps a named list (`Steam`, `codex`, `graphhelm-slot`, ...). Log in
  `D:\_sweep-logs\`. The owner runs it by hand until the scheduled task `dale-disk-sweep` is
  registered on the machine (the command is in `F:\github\Dale\dale-ci\DISK-HYGIENE.md`); this
  section does not claim the task exists.
- **Still forbidden by hand:** `git worktree prune`, deleting another lane's worktree, target or
  scratch (`lane-loop.md`, MERGE-CHECKLIST). A session removes only what it created, by name. The
  sweeper is the single exception those two rules now name, and it is the only thing that may act on
  another lane's leftovers - by the criteria above, never by judgement.
- Before writing "disk full" anywhere: run the sweeper's dry-run and read its log.

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
- **When the board is too small for two different non-author lanes, the two passes may come from
  two subagents of ONE non-author lane (owner order, 2026-09-11).** This is TWO normative changes,
  not one: a clause is dropped, and a check is retired (the second is the last bullet below).
  A **subagent** is a reader a session spawns; it has no address of its own, so its pass belongs to
  the LANE of the session that spawned it, exactly as item 8 of `.factory/MERGE-CHECKLIST.md`
  states - the lane is the unit, never the session. **What closed the lane set was an INSTRUCTION,
  not an accident**, and the distinction decides how long this form lives: the owner told one lane,
  directly, that no other session was to be used ("n vai usar outra sessao e so vcs dois e seus
  subagents"), and that lane relayed it to the other - owner to one lane directly, lane to lane
  relayed; the lane that received it FROM THE OWNER attests it, and nothing on the board records it,
  which is the weakest link in this recital and is named rather than dressed up. A machine
  reboot the same day is NOT the cause
  and must not be cited as one: a third lane posted an admissible pass forty-seven minutes after the
  boot and stayed active for another hour. A rule justified by an accident invites waiting for the
  accident to pass; a rule justified by an instruction does not. With one non-author lane the
  arithmetic is an impossibility rather than a queue - "two passes from two different lanes, neither
  the author" is unreachable by construction, so NO pull request authored by either lane could ever
  merge. **What is dropped is the "two different lanes" clause and nothing else.** It was always a
  PROXY for two independent readings; on this board the proxy became unsatisfiable while the thing
  it proxies for did not, and what made every reading independent was fresh context rather than a
  different account: second readings caught at least twelve defects in a single day's board activity
  on 2026-09-11, including one in the author lane's own code found by a subagent of that
  same lane.
  - **The author exclusion is NEVER relaxed.** A pass from a subagent of the session that authored
    the pull request is a reading, not a pass, however it is labelled. The owner's order settles HOW
    a lane produces a pass; the author exclusion settles WHICH lane may produce one for a given pull
    request. Answering the second question with the first lets an author's own subagents satisfy a
    rule written to exclude the author, and that substitution is what made this amendment necessary.
    Precisely: "the session that authored" means the CONTEXT that wrote the diff. When the diff was
    written by the session's own context, every subagent of that session is the author's and produces
    readings only. When the diff was written by a subagent, that subagent is the author and is excluded,
    and the spawning session's OTHER fresh-context subagents are governed by the amendment below and
    by nothing else in this bullet (the reconciliation Codex asked for on #1081). A MIXED diff — any
    line written by the session's own context, however small — is the first case, not the second.
    The pull request body names the implementing subagent, so that a presser can tell the two cases
    apart from the record and not from the session's word. Item 8 of `.factory/MERGE-CHECKLIST.md`
    carries the same exception in the presser's terms.
  - **Amendment (owner order, 2026-09-13): an ORCHESTRATING session's subagents may produce the two
    passes on a pull request that other subagents of the same session implemented.** The owner's
    words, given in chat to the MVP orchestrator lane when no other lane on the board could take the
    passes (the two remaining Claude sessions had been told by the owner to work only with each other):
    in English paraphrase (verbatim on #1080, as #901 carries the 2026-09-11 order): "spawn two
    subagents; you may change that AGENTS.md so that a subagent also does the review". This clause is the one exception the preceding bullet names, and it governs exactly the
    case where the diff was written by a subagent and not by the session's own context. What this changes and
    what it keeps: the AUTHOR is the subagent that wrote the diff, and it still never reviews; the
    orchestrating session that planned, wrote no line of the diff and only relayed findings is the
    non-author lane, and two FRESH-CONTEXT subagents it spawns are that lane's two passes. FRESH
    CONTEXT means: a subagent spawned new for the pass, never a resumed one; its brief carries the
    pull request number, the gated head and its own reading angle, and NO finding from the
    implementing subagent, from the orchestrator's own reading or from the other pass - a brief
    that pastes a report is a seeded brief and its output is a reading. A reader spawned before
    the gate is never resumed to write the pass. Because the lane set is one, the spawning session
    is also the presser: the press is the checklist run, and the merge comment names the
    implementing subagent and both pass subagents so the whole chain is on the record. Every other
    condition above stands unchanged: different briefs, neither seeded with the other's findings, the
    disclosure line in the lane field naming the subagent and the spawning session, the gate GREEN at
    the head before the passes, and the presser reading the head that carries the manifest. A reading
    the orchestrator's subagent produced BEFORE the gate (to find defects early) is a reading, not a
    pass; the pass is written against the gated head. This clause exists because the previous bullet
    answered the question "which lane" with "not the one whose subagents wrote it", and a session
    whose subagents wrote it and whose other subagents read it with fresh context is, in every
    measurable respect that matters for independence (fresh context, no shared findings), the same
    reader the previous bullet trusted from a different account.
  - **The two readers MUST be given different briefs, neither may see the other's brief, and neither
    may be seeded with the other's findings.** A pass produced from the other reader's brief, or
    after being shown the other reader's findings, or from a brief that differs only cosmetically,
    **is one reading with two signatures and DOES NOT COUNT as the second pass.** The invoking lane
    states both briefs' scopes in the pull request, so the difference is checkable by the presser
    rather than asserted by the lane that benefits from it.
  - **Every pass DISCLOSES, in the first line the census reads, that it came from a subagent and of
    which session - and a pass whose first line does not carry it IS NOT A PASS.** The disclosure
    goes in the LANE field, not only in `Session:`: item 8 instructs a census to treat `Session:` as
    an address for a reply and not an identity for a count, so provenance recorded only there is
    provenance a census is told to skip. The form is
    `Lane: <letter> (subagent <name> of <session> [ref], <author|non-author> for this PR)`, and the
    parenthetical is what makes it decodable by a reader who was not told what to expect. **What
    disclosure buys is AUDITABILITY, not prevention, and the narrower claim is the true one.** On
    2026-09-11 it did prevent one thing: a pass disclosing "the author session" let the other lane
    refuse to count it. The second case did not stay prevented - a presser disclosing its own lane's
    history refused a pull request its lane had reviewed, and twenty-four minutes later, the eligible
    set having genuinely emptied, the SAME lane lawfully pressed the SAME pull request under the
    exhaustion clause. Disclosure did not stop that press and was never going to; it is the only
    reason a reader can tell from the board alone that a reviewing lane pressed. Claim the property
    it has.
  - **"Too small" is a MEASUREMENT, published, not a judgement made privately by the lane it
    unblocks.** The invoking lane runs a census at the moment of invoking, and writes into the pull
    request every live lane, its role on that pull request, and the instant the census was run. A
    lane that is merely busy, or silent, or offline is NOT absent - that distinction is the whole
    difference between an exhausted set and an unanswered one. **This form expires the moment a
    census finds a second non-author lane**; the ordinary two-lane rule governs the next pull
    request, and nothing carries forward from the last invocation. A rule that exists because the
    board is small must say what makes it big again, or it outlives its reason unnoticed.
  - **DECLARED LIMIT on the brief condition, so it is not read as settled practice.** The evidence
    for "different briefs, neither reader seeing the other's" is ONE observed pair (2026-09-11,
    core crates against surfaces and records, disjoint findings). n = 1. It is the right condition
    on the reasoning above and it is not an established one; a later board with more pairs should
    revisit whether the difference must be in SCOPE or whether an angle of attack suffices.
- **After correcting a measured value, sweep EVERY surface for values derived from it - the file, the
  COMMIT MESSAGES, and the pull request body - before pushing.** Which surface actually reaches
  `main` was measured rather than assumed, and the assumption everyone had been repeating was wrong:
  this repository is configured `squash_merge_commit_message: COMMIT_MESSAGES`, so a squash body is
  built from the COMMIT MESSAGES and the pull request body never enters `main` at all. Verified
  against a real merge - `3eb2058f` (#1041) carries its commits as `*` bullets and not one line of
  that pull request's body -- the setting is `squash_merge_commit_message: COMMIT_MESSAGES`, named
  here rather than borrowed, for the reason below. `.factory/MERGE-CHECKLIST.md:414` and `:470`
  ALREADY RECORD THE FACT, in the file a presser is told to open, and two lanes re-derived it from the
  API and from a squash commit without opening it. **But the checklist gives the WRONG REASON for it:**
  `:471` says the squash message comes from the commit bodies "and `squash_merge_commit_title:
  COMMIT_OR_PR_TITLE` is why", and that setting governs the SUBJECT line; `squash_merge_commit_message`
  appears NOWHERE in that file. So a reader who checks the cited reason finds a setting saying the
  title may come from the PR title, and can reasonably conclude the body does too. **The checklist is a
  second publisher of the false premise**, beside `60cc7d79`. Cite it for the fact, never for the
  reason. The body still has to be swept, for a
  different reason: it is what
  `closingIssuesReferences` is computed from, so it decides which issues CLOSE, and it is the record
  every later reader cites. Two surfaces, two distinct consequences - a stale number in a commit
  message is permanent in `main`; a stale number in a body misleads every reader and can close the
  wrong issue.
  **The remedy costs nothing at press time and does not need the head to move:** `gh pr merge
  --squash` accepts `--body`, so the presser COMPOSES the squash message instead of accepting the
  default concatenation, carrying the closing keywords and one sentence on what the change
  discharges. Measured 2026-09-11: neither #1036 nor #1037 had a closing keyword in ANY commit
  message, so both would have landed in `main` with no record of the issues they close - while
  closing them anyway through `closingIssuesReferences` in #1036's case, which is why nothing would
  have complained. For #1037 not even that holds - its field is EMPTY, for the reason the next
  sentence gives.
  **And that field has a trap of its own: a closing keyword is inert while the PR's base is not the
  default branch.** #1037 carries `Closes #107` in its body with an EMPTY `closingIssuesReferences`,
  because its base is another pull request's branch; the field is recomputed when the base changes
  under it. **Re-read it AFTER a retarget and before the press** - reading it before says nothing
  about what it will be, and reading it before is what makes it look handled.
  This is not a counsel of care: the edit site
  is where attention already is, and the stale value sits where it is not, usually spelled
  differently - a duration instead of the instant it came from, a bare `:NNN` instead of
  `file.md:NNN`, a rounded span instead of the raw timestamps. The remedy is mechanical: regex the
  artifact for every number, instant, duration, count and sha, and audit each against its source,
  including the ones nobody reported. Measured 2026-09-11: six STALE VALUES across four episodes and
  four pull requests - the unit is VALUES, not episodes, because two of the four episodes left two
  behind each - every one a correct fix leaving a neighbour holding the old value - an instant
  corrected while the interval computed from it survived one bullet above; a citation re-pin that
  fixed four `file.md:NNN` coordinates and missed the two bare `:NNN` continuations below them; a
  figure dropped from a file and kept twice in the body of the same push; a remedy for stale lanes
  left in a body after the lanes were withdrawn. The rule was itself stated only in chat for most
  of that day, which is why it is here.
- **A conclusion that checks out does not certify the reasons given for it.** The bullet above is one
  member of a wider family, and the family is the more dangerous half: a TRUE conclusion resting on
  FALSE premises survives review, because the reviewer verifies the claim, finds it correct, and
  never re-derives what it was inferred from. A wrong conclusion is caught by the next reader; a
  right one with rotten reasons is ratified and cited. Measured 2026-09-11, six instances in one
  day, each verified against its source before being written here:
  - "no poller since 2026-09-08" for a queue directory - true that nothing would drain it, and the
    date came from a DIFFERENT directory's mtime; the queue itself carried a status file written
    13:22 that same day.
  - An empty listing read as an idle slot - true that the slot was dead, from a path
    (`queue-hdd`) that does not exist. The error was not the wrong directory: it was reading a
    directory at all to answer a question about a PROCESS. No listing of any path could have
    answered it.
  - A board sweep finding that most open pull requests carry no gate receipt for their current head
    - the finding was real and the stated cause was not: `ci/gate-queue.ps1` and `ci/gate-runner.ps1`
    default to the SAME directory, so nothing in the repository points a writer at an unread queue.
    The drift was a launch-time override on one process against every client's compiled-in default.
  - A gate receipt recorded as a third RED inside a list of REDs, when its own status field reads
    GREEN, 61 stages, zero failed - the receipt is
    `.factory/gate-runs/ba3c2f093586-20260911T175816.412Z-e6feb79c.json` on #1036's branch, named
    here because a measurement a reviewer cannot locate is not one. It had been superseded when the
    head moved, not failed - and the
    sentence was introduced by the edit that corrected the stale coordinates around it.
  - "The body is the squash message" - repeated across two pull request bodies, a review comment and
    the bullet immediately above this one, load-bearing for the rule that the body must be swept,
    and **false**. This repository is configured `squash_merge_commit_message: COMMIT_MESSAGES`;
    `3eb2058f` (#1041) proves it, carrying its commit messages and not one line of that PR's body.
    The conclusion it was cited for - sweep the body - is still right, for a different reason, which
    is exactly why nobody re-derived it. **This is the member to remember: not a number that went
    stale, but a claim about how the repository WORKS that everyone repeated and no one asked the
    API. It was caught only because a closing-keyword check required knowing what a squash carries.**
    **And the false premise had a PUBLISHER: `main` itself.** Commit `60cc7d79` sits there titled
    "the body is a squash message, and nobody re-reads it" - its BODY is about including commit
    headlines in the merge reading, but the TITLE reads as a claim about where squash text comes
    from, and it is permanent and prominent while the correction is four hundred lines into another
    file. That is how two independent lanes acquired the same wrong premise without either inventing
    it: ask where a belief was PUBLISHED, not only how it survived.
  - The same claim again, in the one place it had already been DISPROVED. A merge checklist run on
    #1041 printed `closing keywords IN THE SQUASH BODY: (NONE - issue will not auto-close)` beside
    `issue 1040 state=CLOSED`, while that PR's body carried `Closes #1040`. The disconfirmation was
    on screen and was explained away as "two independent mechanisms close an issue" - which is TRUE,
    and which preserved the false premise intact. The question asked was *how did it close anyway*;
    the question that would have broken it was *why is the body's text not here*.
  **How a premise like that survives contact with its own disproof, since "nobody re-derives" does
  not cover the last case: the disconfirmation arrives wearing an innocent explanation.** A true
  local account of the anomaly is always available, and taking it costs nothing in the moment. So
  the discipline is not only to re-derive premises nobody checked - it is to distrust the
  explanation that leaves your prior belief standing, ESPECIALLY when that explanation is correct.
  **What this asks of a writer:** state what you MEASURED beside what you concluded, so a reader can
  re-derive rather than agree. **What it asks of a reviewer:** re-derive the premise even when the
  conclusion is obviously right - especially then, because that is when nobody else will. A
  conclusion you agree with is not evidence that its reasons were checked.
- **What this amendment RETIRES, named here rather than discovered later.** Under the exhaustion
  clause above, the presser is one of the reviewers by construction, so the check "may this lane
  press here" can no longer fire. **The plain form of the loss: a lane that reviewed will press, and
  nothing in the procedure will stop it.** That is not a forecast. One pull request shows both faces
  of the check on the same day: it made a presser refuse #1012 at 18:50Z while two eligible lanes
  still existed, and at **19:14:57Z**, the set genuinely empty, the exhaustion clause DIRECTED that
  same lane to press - which it lawfully did. (19:21:37Z is when the merge COMMENT was posted; a
  comment's instant is not the act's, which is the distinction this whole file exists to keep.)
  **The residual this bullet names materialised while the amendment naming it was still in draft:**
  that merge landed first, and every commit that has carried this paragraph was COMMITTED after it -
  the committer date, not the author date, which `git log` shows by default and which is frozen at
  the first draft (here 2m59s BEFORE the merge). Stated without a duration on purpose: a figure
  derived from the committer date cannot survive the next amend, since that field is rewritten
  while the author date is not. Read that as the evidence it is - the clause working as written,
  and the loss being
  real at the same time. It is retired BY DESIGN. **Its mitigation is NOT that the check is
  moot, and the record refutes that
  reading:** on 2026-09-11 a presser ran the four-class census, named TWO eligible lanes, wrote
  "this is not the exhaustion case", and refused - and it observed in the same comment that under
  exhaustion the rule disqualifying it is the rule that would hand it the button. So where
  exhaustion is CLAIMED but not true, retiring the check changes the action: the pull request gets
  pressed by a lane that reviewed it. The only thing standing between those two cases is the
  published census required above, and that census is self-reported by the lane the form unblocks.
  That is the residual risk, stated plainly rather than reassured away.
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
