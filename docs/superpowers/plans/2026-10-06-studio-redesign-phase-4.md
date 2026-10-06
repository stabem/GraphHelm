# Studio Redesign Phase 4: Journey Screens, Captures, Folding and Route Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A journey contract can name the screen behind each step; any agent records a screenshot of a step (`jpd.screen_captured`) and a walked transition (`jpd.transition_walked`) as ordinary signals; the Runtime folds them per contract into steps with the newest capture and arrows, each marked `fresh`, `stale` (naming the changed files) or `unknown` (naming the cause), and serves that map identically over HTTP, MCP and the CLI.

**Architecture:** Pure folding plus a git-backed freshness oracle live in one new module of an existing core crate, `core/execution/src/journeys.rs` (crate `graphhelm-execution`), behind a small `ScopeHistory` trait so the fold is testable and the git adapter is a fixed-argument `std::process::Command`. Scope matching reuses Keel's `graphhelm_policy::keel::check_scope` semantics (exact or `/`-prefix, no globs). The CLI layer (`apps/cli/src/commands/journeys.rs`) reads contracts from `<project>/.graphhelm/journeys/<contractId>.json`, validates them against the JPD schema, opens the sealed signal envelopes of the run with the keyring, decodes the two description documents, and calls the fold. `GET /v1/executions/{id}/journeys`, the MCP `journeys` tool and `graphhelm journeys` all call that one function (the `execution::briefing` parity pattern). The producers `graphhelm journey capture|walked` build the envelope and call the shared signal core `execution::signal::execute` with the phase-3 `ImageAttachment` path.

**Tech Stack:** Rust 1.97.1. Crates touched: `graphhelm-execution` (gains a dependency on `graphhelm-policy`, already a workspace crate; no external dependency), `graphhelm-cli`. JSON Schema 2020-12 for the extension.

**Spec:** `docs/specs/2026-10-05-studio-live-team-and-proven-journeys-design.md` §5.4, §6.1–§6.4, §7, §10 phase 4. ADR: ADR-044 in `docs/reference/REFERENCE_STACK_AND_ADRS.md`.

## Rulings (made by the orchestrator; do not reopen)

1. **Identifiers.** Contract ids, step ids and screen ids used by the fold, the route, the producers and file names must match `^[a-z0-9][a-z0-9._-]{0,127}$` and must not contain `..`. The schema's `$defs/id` allows `/` and `:`; that stays (existing contracts stay valid), but such an id is refused by every phase-4 consumer before it touches the filesystem, git or a record ("path-like ids are refused"). Revisions must be 40 or 64 lowercase hex characters.
2. **Description documents.** Both are JSON text in the envelope's `description` with a `protocol` field, like `graphhelm-actor-alias-v1`:
   - `{"protocol":"graphhelm-screen-capture-v1","contractId","stepId","revision","dirty","viewport":{"width","height"},"observer", "pr"?, "phase"?}` — `pr` integer ≥ 1, `phase` `before|after`, `viewport` integers 1..=16384, `observer` 1..128 chars.
   - `{"protocol":"graphhelm-transition-walked-v1","contractId","fromStepId","toStepId","revision","observer","fromCaptureId","toCaptureId"}`.
   Schemas: `extensions/builtin/graphhelm-jpd/schemas/screen-capture.schema.json` and `.../transition-walked.schema.json` (`additionalProperties: false`), registered as `schema` contributions in `extension.json` with their `sha256`.
3. **Where validation happens.** The producers build only valid documents. The Runtime's generic signal path does not gain a new check (signals stay ordinary). The fold ignores a record whose description does not parse into the document, whose ids fail Ruling 1, or (capture) whose event does not carry exactly one image evidence ref after the envelope; ignored records are counted in the response (`ignoredRecords`). Captures from any actor count (spec: "recorded by any agent or observer").
4. **Which capture a step shows.** The newest (highest event sequence) non-dirty capture; when every capture of the step is dirty, the newest dirty one, marked `unknown` with cause `dirty`.
5. **Freshness.** `head = git rev-parse --verify HEAD^{commit}`; revision present = `git cat-file -e <rev>^{commit}`; changed = `git diff --name-only -z <rev> <head>` filtered in Rust with Keel scope semantics (`check_scope` from `core/policy/src/keel.rs`; a path is in scope when it is not reported as outside). No pathspec reaches git, so pathspec magic cannot be smuggled. Every invocation: `Command::new("git")` with `-C <project>` plus a fixed argument list, `GIT_OPTIONAL_LOCKS=0`, stdin null. Causes for `unknown`, checked in this order: `dirty`, `no_scope_paths` (step has no `screen` or empty `scopePaths`), `no_git` (git missing, or the project is not a repository / has no HEAD), `revision_missing`.
6. **Arrows.** For each consecutive step pair, the newest transition whose `fromStepId`/`toStepId` match: none → `never_walked`; both referenced captures exist in the run, belong to the matching steps, and are `fresh` → `walked`; otherwise `stale`.
7. **Cache.** Within one fold, results are memoized per revision (HEAD is read once per fold). No cross-request cache: HEAD moves and a stale cache would report `fresh` wrongly; the cost is one `git diff` per distinct revision. This is a deliberate deviation from the spec's "caches per (revision, HEAD) pair" and is stated in ADR-044.
8. **Contracts on disk.** Every `*.json` directly under `<project>/.graphhelm/journeys/`, sorted by file name. A file whose stem fails Ruling 1, that does not parse, fails the `journey-contract` schema, or whose `contractId` differs from the stem is listed under `refusedContracts: [{file, reason}]` and not folded. Missing directory → empty list. Files over 1 MiB are refused. Symlinked files are refused (`symlink_metadata`).
9. **Response shape** (`data` of the envelope, identical on all three surfaces):
   ```json
   {"head":"<sha>|null","journeys":[{"contractId","title","steps":[{"stepId","screen":{"screenId","title","scopePaths"}|null,
     "capture":{"signalId","imageEvidenceId","revision","dirty","viewport","observer","pr"?,"phase"?,
       "freshness":"fresh|stale|unknown","changedFiles":[...],"unknownCause":"dirty|no_scope_paths|no_git|revision_missing"?}|null}],
     "arrows":[{"fromStepId","toStepId","state":"walked|never_walked|stale","transitionSignalId"?}]}],
    "refusedContracts":[...],"ignoredRecords":N}
   ```
   `changedFiles` sorted, present (possibly empty) on every capture.
10. **Route requirements.** `GET /v1/executions/{id}/journeys` needs `--project` and a keyring on the Runtime (envelopes are sealed); otherwise `execution_state` 409 naming `/project` or `/keyring`. Owner credentials only: confirm `agent_route_allowed` in `apps/cli/src/commands/serve/mod.rs` does not admit it, and pin that with a test. The project directory is the one the Runtime was started with; no path from the request or a record is used.
11. **CLI surfaces.** `graphhelm journeys --events <dir> --execution <id> --project <dir> --keyring <dir> --key-id <id>`; `graphhelm journey capture --events --execution --keyring --key-id --project <dir> --contract <id> --step <id> --image <file> [--viewport WxH] [--pr N] [--phase before|after]`; `graphhelm journey walked --events --execution --keyring --key-id --project <dir> --contract <id> --from <step> --to <step>`. `--project` defaults to the current directory. `capture` reads the revision with `git rev-parse HEAD` and dirty with `git status --porcelain` (non-empty → dirty) in the project; refuses when git is unavailable, when the contract/step does not exist in the project's contract file, or when the image is not PNG/JPEG/WebP (phase-3 validator `attachments::from_files`). `--viewport` is required unless the image is PNG, whose IHDR width/height are read. `walked` picks the newest capture of each step in this run (dirty allowed, Ruling 4 applies at fold time), refuses when either is missing or when the steps are not consecutive in the contract. Envelope: `type` the kind, `source {"type":"test","id":"journey-observer"}`, `severity "low"`, `evidence` one line naming contract/step(s)/revision, fresh signal id from the same generator other CLI-built envelopes use. Observer = the recording actor's id.
12. **Schema pin.** The `journey-contract` schema is pinned by git blob id in `apps/cli/tests/development_contract_schemas.rs` (`the_bound_closed_artifacts_are_byte_identical_to_their_pins`). The additive `screen` change updates that pin and the `extension.json` sha256 in the same commit, with the reason in the commit message. `contributionCount` in `apps/cli/tests/jpd_plugin.rs` rises by the two new schemas.

## Global Constraints

- Docs in English. Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Commit and push (`git push -u origin issue-315-journey-captures`) after every green step.
- One implementer at a time in this worktree. No `git stash`, no checkout of other branches, never touch `F:\github\ml-saas\.graphhelm`.
- Cargo target: `CARGO_TARGET_DIR=F:/t315` for every cargo command.
- Before each Rust commit: `cargo +1.97.1 fmt --all -- --check`, `cargo +1.97.1 clippy --locked -p <touched crate> --all-targets --all-features -- -D warnings`, and `cargo +1.97.1 test --locked -p graphhelm-protocols --test authored_strings_across_the_workspace`.
- Tests: fixed ids, no wall clock, no network; temp git repos via `tempfile` with `git -c user.name=t -c user.email=t@t commit` (no global config), and every test that needs git skips with a printed reason only if `git --version` fails (it does not on this host).

## Task 1: Schemas (§6.1, §6.2)

**Files:** modify `extensions/builtin/graphhelm-jpd/schemas/journey-contract.schema.json` (`$defs/step` gains optional `screen` → `$defs/screen {screenId: id, title: 1..160, scopePaths: array 0..64 of strings 1..512 not starting with "/" and without "\\" or ".."}`, `additionalProperties:false`); create the two schemas of Ruling 2; modify `extensions/builtin/graphhelm-jpd/extension.json` (sha256 of the changed schema; two new `schema/screen-capture`, `schema/transition-walked` contributions, family `journey-verifier`); `apps/cli/tests/development_contract_schemas.rs` (pin); `apps/cli/tests/jpd_plugin.rs` (count; new test).

- [ ] New test in `jpd_plugin.rs` `journey_screens_and_capture_records_validate` using `package_schemas`: a contract with no `screen` (built from an existing contract-shaped value; there is no contract fixture, so build a minimal valid contract in the test) validates; the same contract with `screen` on a step validates; `screen` with an unknown property, an absolute `scopePaths` entry, and a missing `screenId` are refused; one valid and one invalid document for each of the two new schemas (invalid: `phase: "during"`, `dirty` missing, extra property). It fails on the parent (schemas absent / `screen` refused by `additionalProperties:false`).
- [ ] Run `graphhelm extension validate extensions/builtin/graphhelm-jpd` (through `jpd_plugin` test `built_in_jpd_extension_is_a_closed_digest_bound_package`), `development_contract_schemas`, and the whole `jpd_*` and `development_*` integration tests of `graphhelm-cli`. Commit; push.

## Task 2: Fold and freshness in `graphhelm-execution` (§6.4)

**Files:** create `core/execution/src/journeys.rs`; modify `core/execution/src/lib.rs` (module + re-exports), `core/execution/Cargo.toml` (`graphhelm-policy = { path = "../policy" }`); create `core/execution/tests/journeys_freshness.rs`.

**Interfaces (produces):**
```rust
pub fn valid_journey_id(id: &str) -> bool;            // Ruling 1
pub fn valid_revision(rev: &str) -> bool;
pub struct ContractInput { pub contract_id: String, pub title: String, pub steps: Vec<StepInput> }
pub struct StepInput { pub step_id: String, pub screen: Option<ScreenInput> }
pub struct ScreenInput { pub screen_id: String, pub title: String, pub scope_paths: Vec<String> }
pub struct CaptureRecord { pub signal_id: String, pub sequence: u64, pub image_evidence_id: String,
    pub contract_id: String, pub step_id: String, pub revision: String, pub dirty: bool,
    pub viewport: Viewport, pub observer: String, pub pr: Option<u64>, pub phase: Option<String> }
pub struct TransitionRecord { pub signal_id: String, pub sequence: u64, pub contract_id: String,
    pub from_step_id: String, pub to_step_id: String, pub revision: String, pub observer: String,
    pub from_capture_id: String, pub to_capture_id: String }
pub enum ChangedSince { Files(Vec<String>), NoGit, RevisionMissing }
pub trait ScopeHistory { fn head(&self) -> Option<String>; fn changed_since(&self, revision: &str, head: &str) -> ChangedSince; }
pub struct GitHistory { /* project dir */ } impl GitHistory { pub fn new(project: &Path) -> Self }
pub fn fold_journeys(contracts: &[ContractInput], captures: &[CaptureRecord],
    transitions: &[TransitionRecord], history: &dyn ScopeHistory) -> JourneysView;
```
`JourneysView` (and nested types) derive `Serialize` with `rename_all = "camelCase"` producing exactly Ruling 9 minus `refusedContracts`/`ignoredRecords` (the CLI layer adds those). `freshness`/`unknownCause`/`state` serialize snake_case.

- [ ] Tests (`core/execution/tests/journeys_freshness.rs`, real temp git repo with `GitHistory`): fresh (capture at HEAD, no change); stale naming the changed file (commit touching `web/cart/Line.tsx` after the capture revision, scope `web/cart/`) with `changedFiles == ["web/cart/Line.tsx"]`, and a change outside scope stays fresh; prefix semantics (`web/cart` does not match `web/cartography.ts`); unknown `no_git` (plain temp dir, not a repo); unknown `revision_missing` (well-formed sha not in repo); unknown `no_scope_paths` (step without screen); unknown `dirty` (only capture is dirty) and newest non-dirty wins over a newer dirty one; arrows: `walked` (both captures fresh), `never_walked` (no transition), `stale` (one capture stale), and `stale` when the transition names a capture id not in the run. Unit tests in `journeys.rs` for `valid_journey_id` refusing `../x`, `a/b`, `a:b`, `A`, `""`, `a..b`, 129 chars, and accepting `cart`, `checkout.v2`, `step-1_a`; `valid_revision`. Each test fails on the parent (module absent).
- [ ] fmt; clippy `-p graphhelm-execution`; `cargo test -p graphhelm-execution --locked`; source guard. Commit; push.

## Task 3: CLI read, route, MCP, parity (§5.4)

**Files:** create `apps/cli/src/commands/journeys.rs` (contract loading per Ruling 8, record extraction, `pub(crate) fn read(events, execution, project, keyring) -> Result<serde_json::Value, Failure>` and `pub fn run(...) -> Outcome` with command name `journeys.read`); modify `apps/cli/src/commands/mod.rs` (module, dispatch), `apps/cli/src/args.rs` (top-level `Journeys` and `Journey` per Ruling 11 — `Journey` subcommands land in Task 4, declare only `Journeys` here), `apps/cli/src/commands/serve/routes.rs` (handler `journeys`, `off_reactor`, like `briefing` ~1651), `apps/cli/src/commands/serve/mod.rs` (route registration ~626), `apps/cli/src/commands/mcp/tools.rs` (`ToolSpec` `journeys`, `execution_only_schema`, GET forwarding like `briefing` ~1408); any route/tool inventory that a test enumerates (find with `grep -rn '"briefing"' apps/cli/tests apps/cli/src`); create `apps/cli/tests/journeys_surfaces.rs` (harness from `apps/cli/tests/signal_image_evidence_http.rs`).

- Record extraction: replay the run (`execution::resolve_stream`), for each `EventKind::SignalRecorded` with `kind` `jpd.screen_captured` / `jpd.transition_walked`, open the envelope evidence (first evidence ref, `signal-<id>`) with `execution::signal::open_sealer` exactly as `documents.rs` `envelope` (~201) does, parse `description` per Ruling 2/3. Image evidence id = the second evidence ref. Sequence = the event's sequence in the stream.
- Contracts: validate with `graphhelm_schema::OfflineSchemaSet` compiled from the JPD schemas embedded with `include_str!` (the set `jpd_plugin.rs` `package_schemas` compiles: compile the whole `schemas/` directory list that `journey-contract` references; if it references none, only that file).

- [ ] Tests (`journeys_surfaces.rs`): a temp project that is a git repo with `.graphhelm/journeys/cart.json` (3 steps with screens), a run with two captures and one transition recorded through `execution signal --attach` (real sealed store, keyring per the phase-3 harness); `graphhelm journeys` (CLI), `GET /v1/executions/{id}/journeys` (owner token) and MCP `journeys` return the same `data` byte-for-byte; the route with a scoped agent credential is refused (401); a contract file named `bad..id.json` and one whose `contractId` differs from its stem appear under `refusedContracts`; a capture record with `contractId: "../etc"` is counted in `ignoredRecords` and never folded; route without `--project` refuses with `/project`.
- [ ] fmt; clippy `-p graphhelm-cli`; `cargo test -p graphhelm-cli --locked --test journeys_surfaces` plus `mcp_stdio`, `api_http`, `development_surface_parity`, `signal_image_evidence_http`; source guard. Commit; push.

## Task 4: Producers `journey capture` / `journey walked` (§6.3)

**Files:** create `apps/cli/src/commands/journey.rs` (or extend `journeys.rs` if it stays under ~600 lines — one file per responsibility: producers in `journey.rs`); modify `args.rs` (`Journey { Capture, Walked }`), `commands/mod.rs` dispatch; create `apps/cli/tests/journey_producers_cli.rs`.

- [ ] Tests: `capture` in a clean temp repo records `jpd.screen_captured` whose decoded description has `revision == HEAD`, `dirty == false`, PNG viewport from IHDR, `pr`/`phase` when given, and exactly one image ref; an uncommitted change makes `dirty == true`; `--contract ../x` and `--contract a/b` are refused before any file read and the stream is unchanged; unknown step refused; `walked` between two captured consecutive steps records a transition citing both newest capture ids, and the fold then reports the arrow `walked`; `walked` with a step that has no capture refuses with no append; non-consecutive steps refused. End-to-end: capture, commit a change in scope, `graphhelm journeys` reports `stale` naming the file.
- [ ] fmt; clippy; tests; source guard. Commit; push.

## Task 5 (orchestrator): docs

- [ ] ADR-044 section in `docs/reference/REFERENCE_STACK_AND_ADRS.md` after ADR-043, carrying Rulings 1, 3, 5, 7. `docs/keel/RECORDS.md` and `docs/harness/JOURNEY_PROVEN_DEVELOPMENT.md`: the two records and the screen field.

## Review Focus

1. A contract, step or record id containing `/`, `\`, `:` or `..` never reaches a file path or a git argument.
2. A dirty capture is never reported `fresh`, even when no file changed since its revision.
3. A capture whose revision was rewritten away (force-push) reports `unknown`/`revision_missing`, not `fresh`.
4. A scope path that is a prefix of a longer file name (`web/cart` vs `web/cartography.ts`) does not mark the capture stale.
5. The three surfaces return identical `data` for the same run.
