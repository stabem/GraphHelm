# Specification Changelog

## Answering a node that waits for you, #1186 - 2026-09-21

The Studio could say a node needed a person and gave them nowhere to say so. The panel turned an
`attentionReasons` entry of kind `waiting_input_node` into the debt "your answer", and the only way
to pay it was to leave the Studio for a terminal.

- **Measured.** `runtime/client.ts` had no `claim` and no `clear` method, and no component
  referenced either verb. Both routes have existed on the server since M11; what was missing was
  the door.
- **Two steps, claim then clear,** because that separation is the customs design: a claim is
  quarantined testimony that releases nothing, and only a clearance releases it. One button would
  make a rejected countersignature look like a failure of the click.
- **The artefact never leaves the browser.** A claim presents `{kind, contentHash, size}` — a
  digest and a size — so the file is hashed where it already is and only its fingerprint travels.
  A cell asserts that neither the file's name nor its bytes appear in what is sent.
- **The verdict is read from the journal, not from the HTTP result.** Both verbs answer 200 when
  they refuse: a refused claim is a `completion_refused` event, and a rejected clearance is an
  entry in `customs.clearances`. `MutationEvidence.result` says "succeeded" for both, so a surface
  reading it would tell a person their claim went through when the journal says otherwise.
  `runtime/customs.ts` holds those readers, and an unrecognised shape answers `unknown` rather than
  either verdict.
- **`MUTATION_DECISION_KIND` holds a LIST per action.** `claim` appends `completion_claimed` OR
  `completion_refused`; one name per action made every refused claim read as "no attributable
  event", which is the reading reserved for a mutation that never landed.
- **The wait sequence is sent, always.** Omitting it asks the Runtime to answer whichever wait is
  open now, which is the stale rendezvous the field exists to prevent. A status that carries no
  sequence renders no controls instead of guessing.
- **Refusals print in the Runtime's own vocabulary,** and a code this build has not caught up with
  prints as itself rather than as a generic error.

Declared limit: the screen cannot say WHAT proof the node asked for. The topology route publishes
endpoint identities only, and the status payload carries the open wait but not the node's
declaration, so nothing the Studio reads names a node's `proofKinds`. The person names the kind,
and a bundle that does not satisfy the declaration is refused by the Runtime with
`evidence_budget_unmet`.
## A node can wait for a person, #1184 - 2026-09-21

A node that declares `completion.customs.proofKinds` now PARKS when its work succeeds, instead of
completing itself. The customs pipeline was built from the fold outwards and one link was missing
at the runtime edge: nothing ever produced the park.

- **Measured before the change.** A node declaring `proofKinds: ["test_report"]` reached
  `succeeded` and its execution completed with `customs: {"clearances":{},"nodes":{}}`. A control
  run with `completion.requires` present behaved identically - the declaration was inert on every
  real path. The only producer of `NodeOutcome::NeedsInput` in the workspace was the FIXTURE
  executor answering for a node with no fixture; `serve/mod.rs` already said in as many words that
  the real executor "is deliberately built to never return `NeedsInput`".
- **The park happens after the work, never instead of it,** and the outcome's sealed material
  travels with it. Proof kinds are evidence that the work HAPPENED, so a gate that skipped the work
  would ask for proof of nothing, and a park that dropped the sealables would ask for evidence it
  had just discarded. Only a SUCCESS is converted: a failure keeps its own outcome and stays in the
  retry path that owns it.
- **An empty `proofKinds` list is not a gate.** It is the field's own default, so treating it as one
  would park every node that declared budgets and nothing else. `examples/graphs/customs-acting.yaml`
  was authored this way already - `implementation` names a proof kind and `release_notes` names
  none - and it now behaves as it reads.
- **Nothing else was needed.** `(Running, NeedsInput) -> WaitingInput`, the open-wait map, the
  `Parked` scan, `attention`'s `needs_you`, `claim`, and the `CompletionCleared` arm that writes
  `Succeeded` back were all already built and tested. Because the clearance writes the completion
  directly, a released node never re-runs its work.
- **An unreadable `completion.customs` block now refuses the execution before any node effect**
  (`GHG017_CUSTOMS_DECLARATION_INVALID` at `/spec/nodes/<id>/completion`), the same shape `GHG016`
  uses. By the time the park decision runs, the node's work has already happened and there is no
  honest answer left: completing it would spend the declaration silently, parking it would invent a
  gate nobody declared.

Declared limit, not fixed here: the wait carries NO deadline on the `start` path. `stage_deadline`
reads customs from `current_graph`, which `start` never fills - it appends `execution_form_declared`
into `declared_form`, and a declaration is deliberately not a publication. So `waitWithinSeconds`,
required by its own schema, stays inert and the overdue sweep will not fire on a wait this change
creates. That is the function's documented "no graph published yet" arm, not a regression.

## The red banner decides the failed stages, #1149 - 2026-09-18

`graphhelm gate classify-red`'s excerpt took `failedStages` from every `[gate] FAILED: <stage>`
line. The gate's own self-test fixtures (`ci/gate-*.tests.ps1`) fail on purpose, to prove the gate
reports failures, and they print the gate's own vocabulary into the gate's own log - so those lines
count the gate testing itself as stages that failed.

- **Measured on the first live System One call.** `1128-20260917T005738.log` carries EIGHT
  `[gate] FAILED:` lines and exactly ONE stage failed; seven are self-test fixtures. The judge was
  shown "eight unrelated stages failed" for a single flaky test, and the classification hedged.
- **`[gate] RED - failed stages:` now decides whenever it is present**, and the printed lines
  remain the evidence for the one shape that never reaches a banner: a run that dies before any
  verdict line, the canary abort included (#1140 deviation 2). The banner's presence is tracked as
  its own flag, not as "did it yield anything", so a banner naming nothing reports nothing rather
  than falling back to the noise this excludes.
- Nothing else changes: `failedTests`, the panic, the first error line and the tail are untouched,
  and the classification is still shadow-only - it changes no verdict, selects no stage, counts no
  pass and re-queues nothing.

## A human summary on stderr, #1150 - 2026-09-18

`graphhelm init`, `graphhelm gateway setup` and `graphhelm gateway probe` now write a short human
summary to STDERR when stdout is a terminal. Before it, the one-command setup answered a person who
had just pasted a secret with 1.5 KB of single-line JSON.

- **Stdout is unchanged, byte for byte.** `AGENTS.md` keeps `apps/cli` at JSON presentation only,
  and the summary goes to a different stream behind `std::io::stdout().is_terminal()`. A pipe, a
  test harness, the MCP tool and every other reader see exactly what they saw before:
  `apps/cli/tests/human_output.rs` asserts on the raw bytes that piped stdout is one compact JSON
  document with exactly one trailing newline, and that no summary phrase reaches either stream.
- **There is no flag.** The person who needs this is the one who has not read the documentation
  yet, so the terminal check is the whole gate.
- **Every renderer reads only from `output.data`, and only fields it names.** Nothing is
  re-derived and nothing is dumped: the setup envelope stands next to a secret, and
  `a_field_the_renderer_does_not_name_is_not_printed` plants an unnamed field that must not appear
  while a named one must. A sentinel key on stdin reaches neither stream, on the success path and
  on the rotation path, swept over raw bytes with a control string that IS found.
- **It says what the JSON only implies**: what is now true (route, provider, where the key was
  sealed, manifest state), what to run next in the shell this binary was built for (the `next`
  array already carried a `bash` and a `powershell` spelling and buried both), and what the command
  did NOT do - `gateway probe` places no model call, which is the thing a person otherwise assumes
  was tested.
- **A refusal renders its diagnostics** as plain lines with code and pointer, not the success
  shape.

## graphhelm gateway setup, #1139 - 2026-09-17

`graphhelm gateway setup --provider <typesafe|anthropic|openai> [--project <dir>] [--route-id <id>]
[--model <name>] [--base-url <url>] [--replace] [--key-id <id>]`: one command that provisions a
provider route and stores its key, where the operator fills in only the key. Before it, wiring a
provider after `graphhelm init` (#1062) was four hand steps across three documents.

- **Reuses what `init` made.** `<project>/.graphhelm/serve.key` is the passphrase and
  `<project>/.graphhelm/keyring` the keyring, through `init`'s own provisioning function
  (`ensure_sealing_keyring`, `apps/cli/src/commands/init.rs`): created when absent, reported
  `existing` otherwise, never rotated. `init` after `setup` finds both `existing`.
- **Writes or merges the route** into `<project>/.graphhelm/manifest.json` (typesafe →
  `https://api.typesafe.ai`, `jev-latest`, route `judge`; anthropic → `https://api.anthropic.com`,
  route `anthropic`; openai → `https://api.openai.com`, route `openai`; `--model` is required for
  the last two, setup does not guess a model; `credentialRef` = `secret_<provider>`). The whole
  document passes `RouteManifest::from_json` before a byte is written; the write is a temporary
  file renamed into place, LF, no BOM. A same-id route is refused before the key is asked for and
  leaves the file byte-identical; `--replace` swaps it.
- **Asks for the key once.** Terminal: `Paste the <provider> API key (input hidden):` on stderr
  with echo off (termios on Unix, `SetConsoleMode` on Windows, through the `libc`/`windows-sys`
  dependencies the crate already had; a terminal that refuses the mode change is warned, not
  refused). Pipe: one trimmed line. Never an argument, never printed, never logged; stored only in
  the Credential Broker (`.graphhelm/broker`) through the same `CredentialBroker::store` path
  `gateway credential set` uses, usable by that route alone. An empty key is refused
  (`GHCLI009_GATEWAY_INVALID`, `/stdin`).
- **Probes and prints the next commands.** `gateway probe`'s own function (`probe_loaded`) runs on
  the route with the passphrase from `serve.key`; `data.probe` carries its reply verbatim, and
  `data.next` the `gateway probe` and `graph synthesize` commands with the paths filled in —
  `--judge-route judge` for typesafe, `--route <id>` for a chat provider. Setup wires a route and
  never selects one.
- **`.gitignore`** gains `.graphhelm/` when missing, through `init`'s helper.
- **Broker fix on the way.** `CredentialBroker::open_or_create`
  (`adapters/model-gateway/src/broker.rs`) reached `create` whenever the store did not exist yet,
  and `SealedKeyProvider::create` refuses a keyring that already holds the key — so the first
  credential stored against an `init`-made keyring failed with "the sealed keyring could not be
  used". It now opens the existing keyring and starts the empty index over it
  (`open_or_create_starts_a_store_over_a_keyring_that_already_holds_the_key`,
  `adapters/model-gateway/tests/broker.rs`).
- **Cells.** `apps/cli/tests/gateway_setup.rs`: the typesafe route after `init` with a piped
  sentinel key against a silent loopback provider (the probe is quota-free and dials nothing),
  the sentinel in no output and readable in no file under the project; refusal without
  `--replace` with a byte-identical manifest and an untouched store; `--replace`; the empty key;
  the anthropic provider (`--model` required, `--route anthropic` in `next`); setup before
  `init`; a manifest whose other routes survive the merge. Unit cells on the prompt/reader split
  with a fake reader (`apps/cli/src/commands/gateway/setup.rs`).
- **Docs.** `docs/install/GETTING_STARTED.md` (the one-command path at the end of §2),
  `docs/reference/PROVIDER_AND_LICENSE_REFERENCES.md` (the TypeSafe section's four manual steps
  replaced), `docs/acceptance/architect-judgments-recipe.md` (prerequisites 1-2 become the one
  command).

## Shadow classification of a red gate, #1138 - 2026-09-17

Edge 1 of #1138, in SHADOW MODE: `graphhelm gate classify-red` puts a bounded excerpt of a RED
gate log and the known-flake list to the typed judge (`class` over
`known_flake | environment_void | real_defect | harness_broke`, one `same_as:<issue>` Noul per
known flake) and records what it said beside the manifest. It never changes a verdict, never
re-queues a head, and never exits non-zero for a classification; the thresholds are the named
constants of `judgment/policy.rs`. Record `docs/harness/GRAPH_ARCHITECT.md` §10.10.

- **`core/architect/src/judgment/red.rs`** (pure): `excerpt` (≤ 20 failed tests, ≤ 40 tail
  lines, ≤ 400 characters per string), `request`, `read`, `RED_CLASSES`, `excerpt_sha256`.
- **`apps/cli/src/commands/gate/classify_red.rs`**: the `gate` group's one subcommand; reads
  UTF-8 or UTF-16 logs (BOM-detected), opens the judge door `graph synthesize` opens, writes
  `--out` through `create_new`.
- **Fixtures** under `apps/cli/tests/fixtures/classify-red/`, authored from the shape of two real
  runner transcripts and re-recorded with `ARCHITECT_RECORD=1`.
- Not here: the runner hook (`ci/gate-runner.ps1`), flake dedup (edge 2), finding triage (edge 3).

## Typed judgments in the Graph Architect, #1109 - 2026-09-16

The Graph Architect gains a SECOND model port: a typed judge (TypeSafe's Jev, System One) that
answers closed questions over state the compiler hands it and can never draft. Design
`docs/specs/2026-09-16-architect-judgments-design.md`; record
`docs/harness/GRAPH_ARCHITECT.md` §10; decision D-054. An absent judge is today's bytes: the
first-compile golden did not change.

- **Judgment wire types (#1114).** `core/gateway/src/judgment.rs`: `Question` (`Noul | Choice |
  Score`), `JudgeRequest`, `Answer`, `JudgeReply`, `request_sha256`, serializing to the documented
  `POST /v1/systemone` body and pinned against the documented examples.
- **The judge port and its recorded door (#1116).** `JudgeModel` beside `DraftModel`;
  `RecordedJudgeModel` keyed by the request digest, `ARCHITECT_RECORD=1` records judge replies
  beside draft replies; `judgeMissing { requestSha256 }` names the request a recording lacks.
- **The System One adapter and the `typesafe` provider (#1118).**
  `adapters/model-gateway/src/systemone.rs` over the existing transport, key leased from the
  Credential Broker; the manifest admits provider `typesafe` for `direct_api`; a `typesafe`
  route on the draft door and a chat route on the judge door both refuse
  `UnsupportedCapability` and send nothing.
- **Site 3, per-node judgments as repairable diagnostics (#1120).** `on_goal` (`Noul`) and
  `kind` (`Choice`) per node; `GHA005_NODE_OFF_GOAL` and `GHA006_NODE_KIND_MISMATCH` go back to
  the draft model like any lint error; the judge is asked only about a draft that passed every
  deterministic check; thresholds are named constants in `judgment/policy.rs` (`0.80`, `0.35`,
  `0.65`) with a cell on each side; an answer under the threshold does nothing and is reported
  `unresolved`.
- **Site 2, rank up to three stance drafts (#1125).** `--drafts 1..=3`; the three fixed stances
  `minimal`, `verified`, `explicit` append one fenced `<stance>` block and leave the single-draft
  prompt byte-identical; composite `coverage - waste`, ties to the lower index, low confidence
  keeps draft 1. Non-goal 10 of `GRAPH_ARCHITECT.md` §5 ("multi-draft judge panels") is retired
  by this PR.
- **Sites 4 and 1, the graph library, the road decision and typed parameters (#1126).** A
  caller-supplied directory of templates with `.template.json` sidecars declaring closed-set
  parameters; the judge chooses `reuse` / `adapt` / `create` before any draft is asked; `reuse`
  fills without a draft model and validates through the same chain a draft takes; `adapt` seeds
  the prompt inside a fenced `<seed>` block; no default directory, no bundled template.
- **Three doors, one execute (#1127).** CLI `--judge-route` / `--judge-fixture`, `--drafts`,
  `--library`; HTTP `judgeRoute` / `judgeFixture`, `drafts`, `library`; the MCP `synthesize` tool
  the same; all through `commands/architect.rs::execute`, byte-identical `data` with a judge.
- **Records (#1124).** `GRAPH_ARCHITECT.md` §10 with the cell map, D-054, the Tier B recipe
  `docs/acceptance/architect-judgments-recipe.md`, and `docs/milestones/runtime.md` counting six
  route families.

## TypeSafe System One route and skill, #1106 - 2026-09-16

GraphHelm was accepted into TypeSafe AI early access. Documentation only; no Runtime code.

- **A sixth route family.** `docs/models/UNIVERSAL_MODEL_GATEWAY.md` §2.6 records System One
  judgment models (TypeSafe's Jev): typed `Choice` / `Noul` / `Score` answers with a probability,
  for the decisions GraphHelm today pays a chat completion for. Classify-and-propose side only;
  the Policy Engine keeps no dependency on it; never an automatic paid fallback.
- **Verified reference.** `docs/reference/PROVIDER_AND_LICENSE_REFERENCES.md` carries the docs
  index, primitives, skill and install commands, and the published price at acceptance.
- **The skill is loaded per project.** `.claude/settings.json` enables `typesafe@typesafe-ai`
  for every Claude Code session on this repository; `docs/agents/AGENTS_SKILLS_PLUGINS.md` §10.5
  names externally maintained skills as ordinary advisory skills.

## Scanner hardening and execution-worktree retrieval, #1086 - 2026-09-14

The declared gaps of the context chain #1078 landed, closed one by one.

- **The excerpt read cannot be redirected by an ancestor.** The containment walk runs over
  paths, so a parent directory renamed to a link between the walk and the open used to send the
  open outside the root while both identity checks passed. The open no longer trusts the walk.
  Unix: `openat` one component at a time from the root's own handle, `O_DIRECTORY | O_NOFOLLOW`
  on every ancestor. Windows: every ancestor opened as itself (`FILE_FLAG_OPEN_REPARSE_POINT`),
  refused if it is a reparse point, and held with a share mode that denies delete, so nothing
  checked can be renamed while the file is read; the final handle must resolve
  (`GetFinalPathNameByHandleW`) inside the canonical root. Declared residuals: on Unix a directory
  already held open and then moved OUT of the root is caught by the post-read path identity check,
  not before the read (`openat2(RESOLVE_BENEATH)` is Linux-only); on Windows another process
  renaming a directory on the path gets a sharing violation for the length of one bounded read,
  and a regular file carrying a non-link reparse tag (a cloud placeholder) is refused.
- **No digest exemption at the cut.** A hex run of 32+ that touches the end of a clipped excerpt
  is refused even behind `sha256:`: a whole digest ending exactly at the 16 KiB boundary may be
  the head of a longer run nobody read.
- **Credential keys are identifiers, not a word list.** The assignment scanner finds every `=`
  and `:` and reads the key back from it: `AWS_SECRET_ACCESS_KEY=`, `DB_PASSWORD=`,
  `SLACK_BOT_TOKEN=`, `stripe_api_key =`, `service.access_key =`, `gcp-private-key =` (any
  segment beginning `secret`; a last segment `token`, `password`, `passwd` or `authorization`;
  `api_key`, `access_key`, `private_key` anywhere). A JSON- or dict-quoted key (`{"password":"x"}`)
  counts, `authorization:` headers and `Bearer <credential>` (16+ characters with a digit) are
  refused, and `tokenizer`, `token_count`, `password_policy`, `mytoken` are not credential keys.
- **Configuration files are judged as configuration.** Behind `:` a short bare value is a type
  in source (`token: String` ships) and a value in `.yml .yaml .toml .ini .env .properties .json
  .conf .cfg` files (`client_secret: xyz` is refused there, `null`/`true`/`false`/`~` and a nested
  `{`/`[` are not). A YAML block scalar (`password: |` / `>-` followed by an indented block) is
  refused, and so is its head when the block lies past the cut.
- **The node path refits framing in rank order.** `retrieve_and_compile` now fits the rendered
  capsule the way `compile_items` does: an item that overflows by framing is dropped and counted
  and the walk continues, so a small later item still ships; popping from the tail used to drop
  it first and then the large item, and ship nothing.
- **A source path must be the schema's shape before it is recorded.** Not only its length: an
  empty segment, `.`, `..` or a backslash is refused and counted as dropped before the read.
- **Context is read from the execution's own tree when it has one.** On a drive with a model and
  a tool half, a cognitive node compiled while the execution's Tier 1 tree exists (a tool node ran
  in this drive, or `refs/graphhelm/executions/<id>` was landed by an earlier one) reads THAT tree,
  so it sees the tool's work instead of pre-tool excerpts. The compile holds the execution's slot
  lock, which every tool call of the execution holds for its whole duration, so no tool writes
  while the compile reads. A tree that exists and cannot be opened is a counted
  `search_unavailable`, never a silent fallback to the checkout. `context.nodes.<node>.root` and
  the sealed record say which tree was read: `project` or `execution`. A cancelled drive never
  waits on an abandoned scan (Codex P1 on #1092): the driver sets the scan's `ScanCancel` token
  when it drops the compile, the channel and the reader check it between entries and before every
  open and read, the host polls for the tree with `try_lock` (never blocking on it), and
  `ToolHost::release` waits at most 2 s for the tree before deferring its removal to whoever holds
  it, who removes it under the lock as it lets go.
- **`GHG016_CONTEXT_BUDGET_INVALID` is asked only of nodes that receive context** — plain cognitive
  nodes on a drive that has context ports. A graph with an unreadable `context.budgetBytes` that ran
  on a fixture drive or a tools-only server before #1065 runs again; on a drive with a model half
  it is refused before any effect, as #1078 shipped.
- **The sealed record is opened, not counted.** `GET /v1/executions/{id}/evidence/{evidenceId}`
  renders structured `application/*+json` evidence (the `context-provenance@1` record and the
  accounting receipt); the journey opens the record and compares `sources`, `digest` and
  `capsuleBytes` with the drive reply.
- **Tests that measure the machine no longer gate.** The workspace channel skips agent and tool
  state directories by name at any depth (`.claude`, `.codex`, `.cursor`, `.windsurf`, `.aider`,
  `.worktrees`, `.idea`, `.vscode`), and the live-tree quality run prints fallbacks and source
  counts instead of asserting them. The frozen-corpus floor (hit rate@3 = 0.90) is unchanged and
  still asserted.
- Real sealed records — a capsule, a `digest: null` fallback, a secret-shaped fallback, an
  `execution` root — are validated with the repository's offline validator against
  `schemas/context-provenance.schema.json`.
## Getting started on a clean machine, Studio included, #1094 - 2026-09-14

- **A clean-machine run of `docs/install/GETTING_STARTED.md` is recorded, PARTIAL (`OBSERVER_MISSING` for the reordered page as published and for an authenticated clone)**
  (`docs/acceptance/clean-machine-2026-09-14.md`): a fresh `ubuntu:24.04` Podman container at
  `34208846`, through build, `init`, `serve`, the first fixture execution, `npm ci` and the Studio
  dev server, the browser half (connect, pause, approve, resume) from the Windows host,
  `events verify` and a byte-identical double replay.
- **The page now says what that run needed.** The repository is private: cloning needs a GitHub
  invitation and an authenticated git (`gh auth login` + `gh repo clone`, or a token over HTTPS).
  The bare-Ubuntu apt block (with `apt-get update` first, and no `sudo` in a root container) now
  comes before the rustup line, which needs the `curl` it installs. Node 22 has
  install commands (the checksum-verified nodejs.org tarball on Linux, `winget` on Windows). A
  container or VM starts the Studio dev server with `-- --host`; the Runtime stays on loopback.
- **§6 names the real MCP tools.** `graphhelm mcp` exposes `list`, `status`, `approve`, … and
  refuses `graphhelm_list_executions`, which is a Studio WebMCP page tool; the page told a chat
  harness to call the refused name.
## MVP closing record and links, #1082 - 2026-09-14

- **`docs/acceptance/mvp-integrated-2026-09-14.md`**: promises 1, 3 and 4 of #302 and the main
  journey re-run by a stranger over the integrated `main` (`533df7b3`), documents only; findings
  F1-F13, of which F4 (the keyring directory must exist before `keyring init`) is fixed in
  `docs/operations/TOOLS_ONLY_RUNTIME.md` and `docs/acceptance/useful-change-2026-09-13.md`, and the Studio/Runtime ones are tracked in #1083.
- `README.md`, `docs/install/GETTING_STARTED.md`, `docs/product/PROVIDER_LESS_MODE.md` and
  `docs/INDEX.md` link the resume briefing (#1071) and the tools-only Runtime (#1073);
  `docs/product/ROADMAP_AND_ACCEPTANCE.md` §3.2.1 rows 1 and 2 state what `main` holds.
- Corrections from the passes on #1078: `docs/context/CONTEXT_KNOWLEDGE_DREAMS.md` says an invalid
  `context.budgetBytes` refuses the execution before start (`GHG016`), not the node; ADR-029 says
  where `compiled_input_tokens` is produced since #1065; the #1065 entry below names the
  rank-order skip in `compile-context` and the scope of the protected-project refusal.
- **D-053: the MVP install bar is a clean local machine** (owner order 2026-09-14 on #302). GraphHelm runs
  locally and publishes to a VPS (#114). Roadmap §3.2.1 row 1 measures GETTING_STARTED on a clean machine,
  Studio included, and stays partial until #1094 records that run. D-001, D-002, FR-001 and the §3.4
  scenario are unchanged.
## Studio and Runtime findings from the integrated MVP verification, #1083 - 2026-09-14

- **An unknown execution id is a 404, not a calm run (F1).** `GET /v1/executions/{id}` and
  `GET /v1/executions/{id}/briefing` for a well-formed id that names no stream answer `404` with
  `GHCLI028_EXECUTION_NOT_FOUND` at `/execution` (new code, registered in
  `apps/cli/src/error_codes.rs`), where they answered `200` with every field null and
  `attention: can_sleep`. `graphhelm execution status|briefing --execution <unknown>` refuse with
  the same diagnostic, and the MCP `status`/`briefing` tools relay it as `isError`. The evidence
  route refuses an unknown execution with the same code once its sealing check passes. Unchanged
  on purpose: the events tail (`/events`) still answers an empty page for an unknown id
  (`gate_http.rs` pins that), and the render a mutation replies with after it commits never
  refuses. The Studio's start treats this refusal as "the run does not exist yet" and proceeds
  guarded at head 0; every other verb keeps it as a refusal.
- **The Studio's resume can name a fixture file (F2).** On a demonstration run the connect row
  offers `Fixture file for resume (optional)…`, sent as the API's existing `fixtures` field.
  `GETTING_STARTED.md` §5 step 8 names both outcomes: with a fixture it decides; without one the
  resumed node parks `waiting_input`.
- **GETTING_STARTED §5 steps 1–4 describe the current Studio (F3):** `LIVE` mark, runs named by
  objective with the id beneath, the verdict as a mark, the blocking node in the conversation
  column's first block, the folded lifecycle chips.
- **Enter sends in every chat box (F5).** One predicate (`components/keys.ts`) serves the new-task
  composer and the run's message box, which had no key handler at all: Enter sends, Shift+Enter
  breaks the line, and nothing sends during an IME composition (`isComposing` or `keyCode 229`);
  `code` `Enter`/`NumpadEnter` count. In a real browser
  (the Browser pane, 1280×720) the Enter keydown reached the handler — `key: "Enter"`,
  `keyCode 0`, `code: ""`, `isComposing: false`, default prevented — and called the send; the
  failure the record describes was not reproduced there. Cells dispatch native `keydown` events.
- **A fixture-only Runtime's route listing is an answer (F6).** `GET /v1/gateway/routes` on a
  server with no `--manifest`, asked without the `manifest` query, answers `200` with
  `{"configured": false, "routes": [], "reason": "no gateway manifest is configured on this
  server"}` instead of a `400` that every connecting browser logged as a red console error. A
  manifest that is named (flag or query) but unusable keeps its refusal; `probe` without a
  manifest still answers `400`. `docs/milestones/runtime.md` updated. The Studio reads
  `configured`, keeps the `400` fallback for older Runtimes, and asks once per client.
- **Runs are named by their objective (F7).** `execution list` / `GET /v1/executions` rows gain
  an additive `objective` field: the objective the run declared at start (`ExecutionFormDeclared`),
  bounded by `MAX_DECLARED_OBJECTIVE_CHARS`, `null` when none was declared. It equals the
  briefing's `objective` for the same execution. The rail names every run from the row, CLI- and
  HTTP-started runs included, with the id as secondary text, and issues no per-row briefing read
  (the pre-existing briefing prefetch runs only for generated ids from a Runtime whose rows lack
  the field). A run with no objective is named by its id.
- **Free canvas framing (F8).** First framing and `fit` frame the work cards' bounds — not the
  People and Conversations lanes — inside the band the floating chrome leaves (toolbar, header
  rows, lint strip, docks, measured at framing time), at 75% when the first rank fits and down to
  60% so that it fits whole; the rest is a pan away. On a scene 480–700px wide the canvas header
  shares rows instead of stacking four, giving the sheet the height it lost. A demonstration run's
  canvas lint reads `Demonstration run · N log notes` in the overview's neutral voice; a real run
  keeps the amber `N log disagreements`.
- **Ended runs (completed, failed, cancelled).** The dock drops pause, resume and cancel and says
  why; sweep and messages stay, because `sweep.rs` and `signal.rs` refuse no terminal state. A
  message sent into an ended run reads `delivered — this run is <status>, so nobody is working on
  it to reply` instead of waiting for a reply.
- **The overview scrolls out from under the dock (F9).** The scroll box runs to the scene's
  bottom and pads its content by the docks' measured height (`--dock-reserve`, read by a
  ResizeObserver on the dock elements, never on render). A demonstration
  run's log findings read as a neutral `Demonstration run · N log notes`; the amber
  `Evidence needs attention` banner stays for real runs.
- **Review follow-ups (Codex on PR #1091).**
  - Only the fixture-explained kind (`done-without-evidence`) reads as a neutral demonstration note. `reopened-after-done` and `orphan-edge` keep the amber attention treatment on any run, in the overview and on the canvas, and each is counted on its own.
  - The dock reserve drops a detached dock by its React 19 ref cleanup, so `--dock-reserve` returns to its base value when the remedies go away.
  - `execution status --execution <unknown> --html <path>` refuses with `GHCLI028_EXECUTION_NOT_FOUND` before writing, leaving an existing snapshot byte-identical. Briefing, the evidence route and the MCP tools write nothing before their check.
## Studio test startup stays outside peak gate load, #1095 and #1102 - 2026-09-14

- **One reused worker thread runs the Studio suite.** `apps/studio/vite.config.ts` keeps one worker,
  selects the threads pool, and disables per-file worker isolation. Vitest 4.1.11 has a fixed
  60-second startup wait; this reduces one run from one worker start per file to one start total.
- **The gate starts Studio after competing work.** A gate for #1097 proved that one thread could
  still miss the same wait while Rust and both PostgreSQL matrices ran beside it. `ci/gate.ps1`
  now starts the same fail-closed Studio stage after those jobs join. It does not retry failures.

## Context reaches the node, #1065 - 2026-09-13

- **A capsule reaches every plain cognitive node.** Before assembly the driver derives query
  terms from the objective (`objective-terms/v1`: lowercase, split on non-alphanumerics, drop
  terms under 3 characters and a 40-word stop-list, dedupe, cap 12), searches the project root
  through the bounded workspace channel (8 results, 50,000 entries / 20,000 files / 256 MiB
  ceilings), reads at most 8 candidates as 16 KiB prefixes within 64 KiB total, fits them to the
  node's `context.budgetBytes` (default 32 KiB) and compiles an `evidence` section of
  `source://<path>` items. The capsule is `AssembledPrompt.context` and enters the prompt digest
  as a third length-prefixed field: a prompt with an empty capsule has a different digest from
  the pre-#1065 prompt of the same system and task, by construction.
- **Bounds refuse and count; fallbacks count and never stop the node.** A candidate over the cap
  or the byte bound is refused whole, never trimmed; the prefix read is the one declared partial
  (`bytes 0..n of len` in the item); the budget bounds the rendered capsule, framing included.
  No terms, a refused search, zero candidates, no readable candidate, nothing that fits the
  budget or only secret-shaped candidates are `retrieval_fallbacks = 1` with the cause named;
  the node runs. A declared `context.budgetBytes` that is not an integer in `1..=1 MiB` refuses
  the execution before it starts, in the same preflight as a retry-policy conflict:
  `GHG016_CONTEXT_BUDGET_INVALID` at `/spec/nodes/<node>/context` journaled under
  `GraphValidationFailed`, then `ExecutionCompleted(Failed)` — never a node skipped in silence
  with the execution left `running`. An immediate stop during a node's compile dispatches
  nothing after it. `development compile-context` bounds the RENDERED capsule the same way the
  node path does: optional items that would overflow the framing are skipped and counted in rank order, and a
  required-only capsule still over budget is refused with the rendered size as the budget that
  would fit.
- **Secrets never enter a capsule.** Credential locations (`.env*`, `*.key`, `*.token`, a
  `.graphhelm` or `keyring` segment at ANY depth) are refused before a read, and the workspace
  walk never enters a directory of either name at any depth; a file NAME carrying a secret shape
  is refused before the read too, because the path is the citation; excerpts carrying a secret
  shape (a bare hex run of 64 characters or more that is not a digest, `sk-`/`ghp_`/`AKIA`
  tokens, PEM private keys, `password=`/`token=`/`secret=` assignments) are refused before
  they can become an item; all count as `candidatesSecretShaped` and are never named. On the
  wire the capsule travels inside an explicit trust boundary — opened as untrusted repository
  excerpts that are evidence and never instructions, closed by an end marker before the task —
  and the prompt digest, over the three fields, is unchanged by the framing. The boundary is
  unforgeable from inside the capsule: both markers carry the first sixteen hex characters of
  the capsule's own SHA-256 (`--- BEGIN CONTEXT CAPSULE <digest16> (…) ---` /
  `--- END CONTEXT CAPSULE <digest16> ---`, the prefix of the `sha256:` digest the provenance
  record and the drive reply publish — deterministic, never a nonce), and every excerpt line
  that begins like a marker is quoted with `> ` before the capsule is digested and sealed, so
  the sealed bytes and the wire are one text and a retrieved file carrying the literal end
  marker cannot close the capsule early. The walk also skips
  generated directories by name at any depth (`node_modules`, `target`, `.venv`, `venv`,
  `vendor`, `dist`, `build`, `__pycache__`, `.next`, `.cache`): they are the bulk of a tree by
  entry count and no answer can cite them, so they no longer spend the traversal ceiling; the
  directory entry itself is still counted. Every `serve` drive that builds context ports refuses a `project` that is, or
  lies inside, the keyring or broker directory (setup failure, nothing committed) — the
  keyring's own files carry no `keyring` segment in their relative names and match no shape;
  the keyring inside the project (the default layout) stays allowed. The reverse is decided
  by what the walk does: a keyring or broker directory INSIDE the project is refused the same
  way when its path below the root carries no `.graphhelm` or `keyring` segment (say
  `<project>/credentials`), because the walk would enter it and cite its files; the default
  `<project>/.graphhelm/keyring` carries the segment the walk skips and stays allowed. Both
  sides are compared canonical against canonical, and both messages name the direction, never
  the path.
- **The numbers are sealed beside the reply, in the receipt's own vocabulary.** A content-free
  `context-provenance@1` record (new schema, added whole) carries `zero_result_queries`,
  `retrieval_pages`, `retrieval_fallbacks` `measured` by `context_retrieval` and
  `compiled_input_tokens`, `eligible_candidate_tokens`, `tokens_saved` `derived` under
  `bytes-div-4/v1` with the arithmetic in the note (roadmap §9.2's v1 estimator, recorded as an
  estimator), beside the paths, counts and capsule digest. The accounting receipt's own six
  context lines stay `unavailable`: the schema-evolution guards admit only comparator-compatible
  changes to a schema the frozen release never held, and none of the receipt's positional lines
  can change that way — `schemas/CHANGELOG.md` records the attempt and the rule. They move at
  the next frozen baseline.
- **The cut of a clipped prefix is checked too, and the path is re-checked after the read.** A
  16 KiB excerpt that ENDS in the head of a secret shape — a trailing hex run of 32 or more
  that is not a digest, a final `sk-`/`ghp_`/`AKIA` token however short, a `-----BEGIN ` block
  with no `-----END `, a `password=`/`token=`/`secret=` whose value lies past the cut — is
  refused and counted as `candidatesSecretShaped`, so a 64-hex key straddling the boundary no
  longer ships 63 of its characters under the whole-shape rule; the workspace excerpt reader
  repeats its identity check after `read_to_end`, as the search channel does, and refuses a
  path that no longer names the opened file; a `"project"` that is present but not a string is
  a 400 at `/project`, never the default tree; a declared candidate length that would carry
  `eligibleCandidateBytes` past the schema's `9007199254740991` is refused as dropped before the
  provenance record is built.
- **The drive reply says what each node ran with.** `context.nodes.<node>` carries `sources`,
  `capsuleBytes`, `eligibleCandidateTokens`, `compiledInputTokens`, `tokensSaved`, the three
  counters, `fallback`, `estimator`, `digest` — paths and numbers, never content. The ledger
  line is written after the dispatch gate: a node refused at assembly or held back by a pause
  that landed since the plan was read is never recorded as having run with a capsule. Context
  ports are built only beside a real model half — a tools-only server answers its cognitive
  nodes from fixtures that read no prompt and seal no provenance, so it publishes no
  `context.nodes`. Every door that holds only the projection (`status` on CLI, HTTP and MCP)
  publishes `nodes: null` with the reason, so the CLI/API parity story sees one value on both
  sides. The `context-provenance@1` `sources` pattern is segment-aware: `.` and `..` segments,
  empty segments, a leading or trailing slash and a backslash are refused (`.env` and `..x`
  remain names); `conformance/schemas/invalid/context-provenance-traversal-source.json` pins it.
- **`development compile-context` runs the same producer** (`context::compile_items`): its digest
  is now the digest of the capsule the required items compile to. #724's producer half; the
  adapter half stays open there.
- **Measured over a frozen corpus** (`adapters/tool-host/tests/context_quality.rs`, ten pairs
  from real files; `adapters/tool-host/tests/fixtures/context-quality/tree`, the ten target files
  and five decoy documents copied from `1ac2438e` and pinned by sha256 in `MANIFEST.md`):
  hit rate@3 (success@3: the one relevant file is in the top three — with one relevant document
  per query, precision@3 would max out at 1/3, so that is not the number measured) = 9/10 =
  0.90, the floor the test asserts; tokens eligible 902,886 / shipped 55,782
  / saved 847,104. The live repository tree is measured and printed with no floor: 6/10 = 0.60 at
  `0e398e75` (eligible 1,629,696 / shipped 60,713 / saved 1,568,983; 1,556,434 / 61,810 /
  1,494,624 after the secret-shape refusal), 4/10 = 0.40 at `1ac2438e` after three merges of
  documentation moved what outranks what — a floor over a moving corpus is not a deterministic
  test. Entity RRF, graph-neighbour RRF, vectors, authority, the Knowledge Graph and Dreams remain
  open under #302 / #111.
## A useful change lands: tools without a model credential, #1066 - 2026-09-13

- **`serve` runs real tools with no model route.** The real-executor wiring is now two
  independent all-or-none halves: `{--manifest, --broker, --route}` wires the model port,
  `{--staging, --allow-program}` wires the tool host; either, both, or neither, and any real half
  requires `--keyring`/`--key-id`. With the tool half alone, tool nodes run on the real host and
  cognitive nodes are answered by node fixtures exactly as in fixture-only mode
  (`graphhelm_runtime::executor::SplitExecutor`, dispatching by `NodeWorkKind`; `PortExecutor`
  is unchanged for the all-real case). A half-given group refuses at startup under
  `GHCLI006_SERVE_INVALID`, naming flags and never programs.
- **One Tier 1 workspace per execution.** `ToolHost::invoke_for_execution` keys one detached
  worktree on the execution id and reuses it across that execution's tool calls; the driver
  releases it when the drive ends (`ToolHost::release`). `ToolHost::invoke` keeps its per-call
  contract, and `--keep-workspace` keeps a tree past release as it kept one past a call.
- **The commit lands as a ref.** A completed `commit` runs
  `git update-ref refs/graphhelm/executions/<execution id> <commit>` in the project — a plain
  ref, never a branch, never the operator's checkout — and a later drive of the same execution
  provisions its tree from that ref. `ToolCallRecord` gains `commit` and `landedRef`, both
  optional and content-free; an id git cannot spell in a ref takes
  `sha256-<32 hex>` (`graphhelm_tool_broker::record::execution_ref`).
- **Hooks never run on an execution's behalf; a dead server's tree is reclaimed; landing is a
  compare-and-swap** (review of #1073). Every spawn through the tool host carries
  `core.hooksPath` at a hook-free directory via `GIT_CONFIG_*`, not only provisioning; a stale
  tree at an execution's root is removed and re-provisioned from the ref, recorded as
  `recoveredWorkspace: true`; `update-ref` passes the expected old value. `serve.started`
  publishes `executors: {model, tools}`, and GHCLI021/022 name which node kinds fixtures answer.
- Proof: `apps/cli/tests/runtime_http.rs` (`a_useful_change_lands_with_tools_and_no_model_credential`,
  `a_half_given_tool_half_is_refused_at_startup`), `adapters/tool-host/tests/execution_workspace.rs`,
  and the hand-run record `docs/acceptance/useful-change-2026-09-13.md`. Operator doc:
  `docs/operations/TOOLS_ONLY_RUNTIME.md`.

## Provider-less mode as a declared guarantee, #1064 - 2026-09-13

- **The promise is written down and held.** `docs/product/PROVIDER_LESS_MODE.md` states that a
  clean installation with no gateway manifest, no keyring, no credentials and no network runs a
  complete execution, shows it on the monitor and the Studio, and exports it; it lists what
  works, what refuses and in which words, and the exact commands.
  `apps/cli/tests/providerless_journey.rs` runs those commands from an empty directory with the
  process environment scrubbed of every `GRAPHHELM_*` variable, then reads the document back
  and fails if its fenced commands and the journey's commands are not the same set.
- **A fixture run says so on every view.** The executor declared at start (#1063's
  `ExecutionFormDeclared.executor`, published as `data.executor`) now reaches the monitor page,
  the `execution status --html` snapshot and the Studio's run panel as one sentence at the top
  of the run: "Demonstration run — started under the fixture executor: outcomes at start were
  supplied by a fixture file, not produced by a model or a tool." The label reads the executor
  DECLARED AT START; a resume through a server with real wiring is not recorded on the form
  today, and the sentence says "at start" for exactly that reason. `execution list` rows carry
  `executor` too, so the Studio rail marks a demonstration beside its name. A `gateway` run and
  a stream recorded before the field existed carry nothing.
- `docs/product/ROADMAP_AND_ACCEPTANCE.md` §3.2.1 row 4 moves from "not declared" to declared
  and held. README and QUICKSTART link the page.

## The resume briefing: continuity across harnesses, #1063 - 2026-09-13

- **A second harness picks an execution up from the store alone (MVP promise 3 of #302).**
  `core/execution::briefing_view(projection, answer, history)` folds ONE typed `Briefing`:
  `name` and `objective` (declared at start), `executor`, `graphHash`/`graphVersion` (so the
  file `resume --file` needs can be verified first), `decisions[]` in sequence order with the
  actor the envelope recorded (approval, waiver, budget_amended, mode_changed, paused, resumed,
  claim, clearance, rejection, mutation_accepted, cancelled), `workDone[]` (every terminal node
  with attempts, last outcome and recorded reason), `pending` and `unevaluated` (COPIES of the
  attention answer, never a second computation), `nextStep` (`finished` > `answer` with the
  node and the verb, ahead of the resume because `approve` and `claim` are legal under a pause >
  `resume_held` with the resume template and the held nodes > `diagnose` carrying any remaining
  reason whole - a SILENT node included, since raising a bound the node already exceeded is
  purchased calm, not an answer - so a need is never listed beside `nothing` > `dispatch` >
  `nothing`) and `asOfSequence`. No clock anywhere.
- **One rendering, three doors.** `graphhelm execution briefing --events --execution`,
  `GET /v1/executions/{id}/briefing` (same token and read budget as status) and the MCP
  `briefing` tool all call the same read; the two-harness journey (`claude-code` approves and
  pauses over MCP, `codex` reads the briefing from a fresh stdio process) asserts byte-for-byte
  equality with the CLI and that every decision names `claude-code`.
- **The objective is persisted at start.** `execution_form_declared` gains three optional,
  bounded (2000 chars, truncated on a char boundary, never refused) fields: `name`
  (`metadata.name`), `objective` (the first entrypoint's own objective - the operator's words,
  where the Studio's draft keeps them) and `executor` (`fixture` | `gateway`, read off the same
  predicate that decides the fixture-only warning). Old journals replay unchanged: the fields
  are `skip_serializing_if`, so no hash chain moves. `render()` publishes `executor` beside
  `mode` on every status-shaped reply.
- **The digest reads the fold, not the event name.** A `completion_cleared` the fold recorded
  as `Refused` (machine-replay hash mismatch) is a `rejection` with the fold's code;
  `completion_refused` is a `refusal` with the registry code; a parked node whose claim is
  already open is answered by `clear` (with `claimSeq`), never a second `claim`; declared nodes
  the driver never touched are `Draft` by absence and count as work to dispatch (or as held, for
  a `--held` start); a verb-less hazard is diagnosed BEFORE a resume is offered.
- **What the log must not carry is left off, not refused.** A `name`/`objective` the store's
  durable-content scan would reject (a `secret://` reference, a token-shaped run) is omitted
  from the declaration and the start proceeds; a held start declares no executor, since nothing
  drives it yet.
  The inline `name`/`objective` contradict ADR-022 §3 / D-036 on their face; ADR-022 amendment
  A1 (`docs/reference/REFERENCE_STACK_AND_ADRS.md`) records the decision: a bounded, scanned label
  in the journal, never evidence - the sealed slots remain the objective's only authoritative home.
- **An approval is a decision only when it readied a `Blocked` or `Ghost` node.** The driver
  records the same `Approved` outcome for its own `Draft -> Ready` hop under the system actor;
  the digest folds each node's prior state and leaves those hops out.
- Not in this change: `resume` without `--file`; parity beyond CLI, HTTP and MCP.

## The first compile: the Graph Architect, #107 - 2026-09-11

- **Something in the tree now turns a prompt into a graph.** `core/architect` is a compiler
  with a model in the middle: `synthesize(profile, catalog, model)` assembles a deterministic
  prompt, asks for ONE draft, wraps the model's `spec` in the compiler's own metadata, and
  validates through the SAME chain every authored graph takes -- `load_graph_json` -> `lint` ->
  executor viability. One road: the document it emits enters the system through
  `execution start --file` exactly as an authored file does. It never publishes and never
  starts.
- **K=2 repairs, then a refusal with the diagnostics attached.** A draft that fails schema,
  lint or viability is fed back with its diagnostics verbatim (`code`, pointer, message) for at
  most two more rounds; a third invalid draft is `invalid { rounds: 3, diagnostics }`. The
  compiler adds four repairable codes of its own -- `GHA001_NOT_JSON`,
  `GHA002_NODE_TYPE_NOT_EXECUTABLE`, `GHA003_TOOL_CALL_MISSING`,
  `GHA004_BUDGET_EXCEEDS_PROFILE` -- each at the pointer the model must correct.
- **Every synthesized graph is born completable (issue #183, D-051).** The compiler stamps
  `completion.customs` (`proofKinds: []`, the profile's two budgets) onto every node the
  runtime dispatches as work that can park, reports the stamped ids as `stampedCustoms`, and
  treats a `GHG102_UNBOUNDED_CUSTOMS` that survives the stamp as `notCompletable` -- a failure
  of synthesis, never a warning. The stamp set is derived from `classify::work_kind`, and a
  cross-crate witness holds it against the lint's park set for every `NodeType` variant.
- **The catalog is the runtime's, and a goal outside it is refused (issue #184, D-052).**
  `CapabilityCatalog::from_runtime(programs)` lists the node types `work_kind` executes, the
  three tool families, and the operator's program allowlist -- the same surface
  `serve --allow-program` feeds the tool lease, with no default. A draft naming a program
  outside it is `capabilityMissing { node, program }`: not repaired, never widened, naming what
  the operator would have to authorize.
- **Metadata is the compiler's.** The model is asked for `spec` only; `apiVersion`, `kind` and
  `metadata { id: arch_<sha8 of goal>_v1, executionId, version: 1, labels { origin, template } }`
  are written by the compiler, so two runs on one goal produce one identity and the golden is
  byte-stable.
- **A content-hashed template and a keyless door.** The prompt template is versioned by its
  own sha256, substituted into every prompt, so editing it moves every recorded reply's key at
  once and the suite fails with `fixtureMissing { promptSha256 }` naming the hash to record --
  a named event, never drift. `RecordedDraftModel` (`--fixture`) is the model in every test and
  is available to operators; the gateway door (`--manifest --route`, or the Runtime's wiring on
  `serve`) reuses `gateway probe`'s lease construction and `serve`'s adapters. No new credential
  path; a gateway error surfaces as the taxonomy's static text, never a path or a key.
- **Three surfaces, one JSON.** `graphhelm graph synthesize --goal --out [--allow-program P]*`,
  `POST /v1/graphs/synthesize`, and the MCP tool `synthesize` return the same
  `{ document, rationale, stampedCustoms, templateSha256, rounds, promptSha256s, usage? }`;
  the CLI additionally writes `--out` and refuses to overwrite. A compiler refusal is ONE
  diagnostic, `GHCLI026_ARCHITECT_REFUSED` at `/goal`, carrying the `kind`-tagged refusal.
- **The reviewer's findings landed with the slice.** Every `tool.call` is parsed at the trust
  boundary by the tool broker's own checked parser (`ToolCall::from_json`), so a call the
  compiler accepts is a call the driver will assemble and a stray field is a diagnostic at
  compile time rather than a refusal mid-run; the repository writes the template never offered
  are refused there too. The park set has a witness (`tests/park_witness.rs`). The node count is
  read off the parsed reply and refused as `tooManyNodes` BEFORE the schema walk and the lint,
  so a thousand-node draft costs one map length. The goal and the previous draft are fenced in
  the prompt and declared data, not instructions.
- **Declared gaps, on record.** Tier B (semantic quality by a paid judge) is unmeasured; the
  golden suite proves determinism and validity, not usefulness. The Task Profiler and
  Capability Discovery are not built -- the profile is caller-supplied and the catalog is
  code-derived. Synthesized `agent` nodes are ephemeral; the registry seam (#110) is untouched.
  The run, its hashes and the cell-to-test map: `docs/acceptance/m11-first-compile-2026-09-11.md`;
  the harness note: `docs/harness/GRAPH_ARCHITECT.md`.
## The acting half: claim, clear, and the scan history, #159 - 2026-09-11

- **A parked node could be described and never finished.** The customs event family, the fold
  that keeps every node's scan history, the clearance verdicts and the sweep all existed, and no
  verb on any surface appended `completion_claimed` or `completion_cleared`: #132 measured
  informing 4/4, acting 0/4. Two verbs land - `execution claim` and `execution clear` on the
  CLI, `POST /v1/executions/{id}/claim` and `/clear` on HTTP, `claim` and `clear` as MCP tools -
  and every one of them replies with the same `render()` envelope every other mutation replies
  with, plus a `claim` or `clearance` object naming the outcome and the sequence it landed at.
- **A claim is decided against the replayed fold, first match wins, and a refusal is itself a
  journal event.** `not_waiting` (the node is not at `waiting_input`), `stale_rendezvous` (a
  named wait that is a `parked` scan of this node but no longer the open one), `unknown_wait` (a
  named sequence no scan of this node ever parked at), `duplicate_completion` (an open claim
  already names the node), `evidence_budget_unmet` (a declared `proofKinds` entry is missing
  from the presented bundle; extra kinds are never stronger proof). Each code is produced by
  exactly one arrangement and the arrangement is asserted before the refusal is demanded, so a
  decision that reaches the wrong code first goes red on the cell that names the right one. A
  refused claim appends `completion_refused` with the registry code and touches no state.
- **Countersign is refused at the door.** The wire carries no signature to verify until D-047 /
  #529, and the `#527` trap goes red the moment a production surface appends a `Countersign`
  clearance. `--verifier countersign` is a `GHCLI005_EXECUTION_STATE` diagnostic that names #529
  and opens no store: the journal's head sequence is unchanged, and the cell holds it by
  reading the journal before and after.
- **A clearance that clears drives; a rejection drives nothing.** After `completion_cleared`
  folds to `Cleared` the node is `succeeded` and its dependents are dispatchable, but nothing in
  the tree dispatches them: `resume_preconditions` refuses a non-paused execution
  (`core/execution/src/recovery.rs`), and `start` refuses a started stream. So `clear` runs the
  same drive `resume` runs, with an empty release set, and the graph reaches `completed` in the
  same reply. A wrong digest folds as `rejected`/`hash_mismatch`, spends the claim, and leaves
  the downstream exactly where it was.
- **The scan history is one typed value, rendered by the one `render()`.**
  `graphhelm_execution::CustomsView` is built from the fold's own maps - `customs_scans`,
  `open_waits`, `open_claims`, `clearances` - with no second derivation; `quarantinedNodes` is
  the deduplicated set of the NODES `open_claims` names (that map is keyed by claim
  sequence). `execution status`, `GET /v1/executions/{id}`, and every
  mutation's reply carry it as `data.customs`, and the parity cells hold the CLI and HTTP
  readings byte-identical to the verb's own reply.
- **Both verbs take the graph the execution started from, through the seam `resume` already
  has.** The projection cannot supply a node's declared `proofKinds` on the start path, and the
  drive after a clearance needs the spec; a second way to carry those facts would be a second
  trust seam. The supplied file's content hash must equal the recorded `graph_hash`, and a
  foreign graph is refused before any APPEND - the store is opened and the history replayed
  first, which is what that check reads.
- **The append is pinned to the sequence the deciding read saw.** The verb replays, decides,
  and appends with `PreparedAppend` at `next_after(&history)` - `1` for an empty stream, last +
  `1` otherwise - so a stream that moved between the read and the write answers
  `SequenceConflict` instead of accepting a decision made against a world that no longer exists.
  The first version derived the position from a second store read (`next_sequence`) taken after
  the history it decided against; the reviewer found it, and the guard now reads
  `core/events/src/customs.rs` at run time and refuses any `next_sequence(` call, with
  `append_atomic(` as the positive control so an empty read cannot pass.
- **Declared gap: deadlines are `null` on every stream `execution start` creates**, so
  `clearance_expired` has no producer and the sweep raises no `overdue_exception` on those
  streams. Cause, measured: `current_graph` is set only by `GraphVersionPublished`, which only
  the governor's draft publication emits; `execution start` publishes a `GraphVersion`, never a
  `PersistedGraphVersion`. The clearance-sweep mechanism is proven at fold level
  (`core/events/tests/sweep_verb.rs`); putting a `PersistedGraphVersion` on the start path is the
  D-036 externalizer's own lane and is not attempted here. Rejection, DLQ redrive and return,
  and the identity registry verbs ride the same deferral as countersign; the Studio does not yet
  read `data.customs`; #153 stays open.
- **The lanes were subagents, and the mapping is now in the file that sequences lanes.** The
  arrangement this change was built under, stated as an arrangement rather than as a finished
  event: implementer subagents author, reviewer subagents with no implementation context read,
  the registered runner gates, and a lane that wrote no line of the diff presses. What the
  reviews found, and who pressed, is on the pull request rather than here; a changelog that
  narrates its own review as already done is the tense defect this entry would otherwise ship. `.factory/lane-loop.md` section 0 carries the clause and the
  identity-line shape (`Lane: <letter> · Session: subagent-<name> of <ListAgents name> [ref] ·
  Head: <sha8>`), because a rule that lives only in chat is the defect that file exists to end.
  Acceptance record: `docs/acceptance/m11-acting-2026-09-11.md`; milestone note:
  `docs/milestones/acting-half.md`.

## A registered gate runs the gate it was registered as, #668 - 2026-09-03

- **Certification said yes and the consumer path could only say no.** `quality certify` stamped
  `GateCertified` for `gate-retry-lineage` and `gate-journey-contract`, and a node using either
  was refused as uncertified: the drive carried ONE suite digest (geometry's) and the driver
  compared every gate's receipt against it, while the executor evaluated every dispatched gate
  with geometry's evaluator. Two production sites, one consequence - the registry could admit a
  gate that no running graph could reach.
- **The runtime now asks a registry, per gate id.** `GateRegistryPort` (`core/runtime/src/ports.rs`)
  answers both questions from one object: the digest of THAT gate's own suite, and that gate's own
  evaluation of the node's evidence. The binary supplies it (`RegisteredGates` in
  `apps/cli/src/commands/quality.rs`), so `core` still names no pathogen suite and no gate's
  evaluator.
- **A gate node carries the evidence its gate judges.** `GateCheckWork` no longer names geometry's
  three fields; it carries `gateId` plus the contract's remaining keys, and the registered
  evaluator parses what it needs. Geometry's `deny_unknown_fields` strictness moved WITH it, into
  the geometry evaluator.
- **Evidence a gate cannot READ refuses the node; it does not verdict it.** The registry answers
  either a verdict or `Unreadable`, and `Unreadable` becomes the same `Unassemblable` refusal an
  unparseable contract always produced: the node is never dispatched and NOTHING is appended. The
  distinction is the append-only store's, not a taste in error shapes -- a `GateVerdict` is
  permanent evidence that a delivered surface was examined and refused, so emitting one for a
  misspelled `gate.check` block would leave a High-severity claim about a surface nothing looked
  at, and fixing the typo could not retract it. A gate that PARSES the node's evidence and rejects
  it for being the wrong kind (the journey-contract gate's `GHJPD000_WRONG_EVIDENCE_KIND`) is a
  verdict: it looked. (Found by L reviewing the first version of this change, which returned
  findings for both.)
- **Fail-closed did not move.** No registry, no entry for the named gate, or a gate this build can
  no longer certify all refuse to dispatch exactly as an absent digest did. A gate id the registry
  does not hold produces no verdict at all rather than another gate's opinion under its name.
- **`core/runtime` no longer depends on `core/quality`.** Geometry became one registered evaluator
  among three, wired by the binary; the dependency survives only as a dev-dependency of the gate
  cells that choose it.

## Local Studio MVP and the execution index, #105 - 2026-08-27

- **The Studio stopped being a specification with no code behind it.** `apps/studio` is a
  local-first operator surface: it lists the runs a store holds, answers which one needs the
  operator and why, shows the sixteen node states and the append-only timeline, and performs
  three mutations - pause, approve a node, resume. It is NOT the Studio `docs/ux/STUDIO_SPEC.md`
  specifies: no graph canvas, no DSL editor, no chat, no collaboration, no cloud.
  `docs/ux/STUDIO_MVP.md` records the line between the two.
- **D-040 is unchanged.** The monitor stays local, read-only, and simple; it gained no
  operational action. The Studio is a separate application, and it may use only public Runtime
  contracts - it imports no crate and reads no event file.
- **The board draws real edges, and only ones it can prove.** `POST /v1/graph/topology` reads a
  graph file on the Runtime host and returns its entrypoints, nodes and edges together with the
  `semanticHash` that says WHICH graph it is. The Studio compares that hash against the one the
  run itself recorded in `execution_started` and draws on a match and only on a match: mismatched
  or unverified yields an EMPTY edge list by construction, so a component that forgets to branch
  cannot render the wrong graph. Sabotaging that single expression turns two guards red. It grants
  no reach the API lacked - `start` and `resume` already load a file by path - and it returns node
  identity only, never objectives or agent blocks. Reachable from all three surfaces.
- **`GET /v1/executions` exists because scraping `/monitor` was the alternative.** A presentation
  surface for a human browser was the only way to discover which executions a store held. The new
  index is authenticated like every other `/v1` route, ordered by execution id, bounded at 100
  rows, and paged by an EXCLUSIVE cursor; an over-large limit is refused rather than clamped,
  because a caller that asked for 500 and silently received 100 cannot tell a clamp from a short
  store. Each row is a key subset of the same `render` value `execution status` replies with, so
  the index cannot disagree with the detail view - checked field by field on a live store, not
  asserted in prose. Reachable from all three surfaces: `graphhelm execution list`, the route, and
  the MCP tool `list`.
- **Seven WebMCP site tools over the SAME client the buttons use.** The adapter builds no request,
  chooses no actor, and interprets no diagnostic; it calls `RuntimeClient`. `cancel` is
  deliberately absent - it is the destructive verb, and the journey does not need it.
- **No write reports success it has not verified.** Every mutation reads the head, mutates with
  `If-Match`, then reads the status and the appended events back, and answers `succeeded`,
  `refused`, or `unknown`. The third value is the point: reporting an unverifiable write as
  succeeded makes the one state an operator must act on look exactly like the one they can ignore.
- **A browser confirmation is consent, not authorship.** A mutation an agent chose is recorded as
  actor type `agent`, id `studio-webmcp-adapter`, even though the person approved it in the
  browser. Recording it as the owner would destroy the only distinction the audit log exists for.
- Declared gap: the Studio's own suite is not part of `ci/gate.ps1`, which covers the Rust
  workspace, schemas, and PostgreSQL. Running it is an explicit step, named in
  `apps/studio/README.md`, rather than a risky pipeline change made to show a green check.

## Name the state you were true of, M10 — 2026-08-20

- The milestone's product sentence, repeated eighteen times: **make the answer name which world it is in.** `GHCLI016` answered a setup refusal and a mid-drive failure with one value, so "the call failed" could not tell an operator whether their hold survived (#96); a restore step that ran out of time reported itself as a corrupt archive, with elapsed scattered across four variants and three codes, none of which named timing (#81); `wake-wait`'s timeout answered from a snapshot the store already contradicted (#88). Each fix is the same shape — the answer now names its world.
- The operator's verbs stopped disagreeing with themselves. A refused `resume` no longer commits `ExecutionResumed` before the drive's setup can fail, so a 500 and an intact hold stop being two facts where the operator reads one (#83). A `resume` names what it held and the drive releases each node when its edges allow, ending the round-forever loop where `pause` re-held on bare state and `resume` force-started (#123); the release is recorded under the OWNER's actor, because releasing work the owner paused is the owner's act (D-019), which partially reverses an earlier deliberate decision and says so at the site. MCP `start`/`resume` gained a deployer-level `--project` default, so an operator confined to that surface can resume without knowing a workspace path (#82).
- Dispatch waits for its edges (#80), and the flagship story that used to credit `deploy.userOverrideAllowed` was rewritten around the recorded path that exists — that field has no execution-lane consumer anywhere. Attention was deliberately NOT made edge-aware (deferred, with in-code signposts at the two sites whose change would make the deferred population non-empty).
- The gate stopped discarding its own evidence: a failing tool's stdout was swallowed at every call site, and the per-suite pass ran off a hardcoded list that had drifted from the filesystem (#97, #98). The reported exit-code defect was investigated and REFUTED — it is what piping the run through `tail` does to an exit status — and saying so is the finding; the lane's real yield was the swallowed-output defect nobody had filed.
- Instruments gained edges they lacked. `ServerGuard` now drains the spawned server's stderr, so a server panic and a slow server stop producing identical client evidence (#140) — an instrument boundary, not a fix: it does not say which storm hypothesis is true, it makes one visible for the first time. The wake belt's headline moved from an aggregate a dead recorder satisfies to a per-arming identity assertion read from the raw journal (#118).
- Performance work landed and immediately taught its own lesson: the verified prefix stopped re-proving history on every request (#87), then readers serialized again until clean opens shared the lock (#143, #146). **A performance change that had been measured, and measured well, still broke a property no measurement was watching** — the rule that measurement never substitutes for the gate arrives with its receipt attached.
- One terminality predicate and one parallelism policy, not five (#101), with the dedup re-verified AFTER the merge because eight commits had landed in between: "there is exactly one" is a claim about a tree, and the tree moved.
- The storm regression the owner saw was attributed by interleaved comparison within one session and fixed (#123). Its MECHANISM stays OPEN with four candidates named side by side, including dead-server, which the instrument was structurally unable to see for the whole milestone; the rate is recorded INDETERMINATE, declared rather than restored; and a storm rate is a property of commit × session, which voids any standalone characterization that does not name its session.
- The milestone found the same defect in its own records nine times — a record that was true when written, read later by someone who cannot see what moved — and recorded the two cases that cost nothing beside the seven that cost something. `docs/milestones/name-the-state.md` carries the account, the defect classes, the traps, the deferrals with their four distinct reasons, and what is NOT claimed.

## Arming the alarm, M09 — 2026-08-19

- Seed 3 from `m09-seeds.md` closed on `main` (#70, `efd85d0`): silence is keyed on whether a
  node has been REACHED, not on `state == Running` — a node retried into `Queued` after a
  declared bound now counts. `Invalidated` (a completed node returned to the queue with no
  failure anywhere in its history) was the case a failure-keyed rule would have left mute
  forever; found by reading the outcome vocabulary, not by the failing run alone.
- Seed 4 ("the doorbell cannot ring for silence"): arming declares how long quiet may last; a
  bound nobody could live to see is refused rather than answered with a date (a trillion-second
  horizon overflowed into a year-33715 date before this fix); `wake-wait` reads its own lease's
  deadline instead of a caller-supplied `--timeout`, closing the two-numbers-one-question gap on
  the sleep surface; the MCP half of `wake_wait` now matches the CLI half (one definition, not
  two); shortening a re-armed horizon is accepted and named, never silently late; reads take a
  shared lock instead of the same exclusive lock as writes (measured: eight concurrent reads
  cost the same wall time as eight sequential ones, before the change).
- Three flakes named at milestone open; two fixed and landed on `main`, one still open at
  close. `concurrent_sweeps_never_double_consume_a_lease`'s window (a validate/`next_sequence`
  gap) closed by pinning the consume's sequence from the read that judged the lease, not a
  second store read — the seam survives the fix and stays sabotage-testable, unlike the
  alternative design considered and rejected. The belt test sharing this guard's name was
  separately measured hollow: green with 14 of 15 consumptions missing, by construction — its
  own oracle upgrade is tracked as #74. `a_sleeper_wakes_on_a_peer_append_with_zero_requests_
  in_the_window` fixed by a condition-wait replacing a timing-dependent immediate read (#73);
  base rate 9/10 isolated, 3/3 in-suite before the fix. `the_storm_holds_under_eight_
  concurrent_agents` stays OPEN: a same-disk paired re-baseline (ten runs at the pre-M09
  commit, ten at the current tip, three minutes apart, identical free disk on every row) found
  ZERO failures at both — the same code produced 4/10 failures and 0/10 failures on the SAME
  machine at two different disk states, which is this lane's central finding, not a caveat:
  code is EXONERATED, but any before/after comparison that is not a fresh paired baseline in
  the same session and disk state reproduces today's confound while looking clean (conditions
  recorded with the numbers so a later pairing is checkable: free disk ~19G, fsync 1.5-2.0ms/op,
  commit `ef51193`, 2026-08-19, n=1521 store opens). An instrumented headroom measurement, on
  today's disk, found ≈8x headroom on the storm's 8-request convoy at the median (29.1-32.6ms
  per store-open x 2.40-2.56 opens per request x 8 requests = 0.56-0.64s against the 5s budget)
  but only ≈1.8x at the p99 (the SAME 8-deep convoy at p99 per-open latency totals 2.81s) and
  CROSSES the budget by 4% at the observed maximum — a non-firing median with a
  tail this close to the budget is a live finding, not a clearance. The composition bound
  narrows it further: reaching the budget needs a burst-average around 250ms, which requires
  broad degradation (roughly 20% of opens slowed to ~1.1s) — a few slow outliers cannot get
  there. Verdict: CONSISTENT WITH A GENUINELY SICK VOLUME, INCONSISTENT WITH MILD PRESSURE.
  Quantified for M10: each store open costs ~30ms of structural work serializing on an
  exclusive lock regardless of handler threading, at ~2.5 opens per request — removing ONE open
  saves ~30ms per request and ~244ms off the 8-deep convoy (an 8x amplification, because the
  convoy is where an open's cost is spent). Fewer opens, not more threads. No fix lands with
  M09; all instrumentation used to measure this was reverted before commit.
- A consumption now names the arming it burns (#74, merged with one Postgres-adapter stage RED
  under an explicit owner decision to land on the evidence rather than re-roll — see the
  milestone record): the rendezvous-equal-burn discriminator closes the #55 family's remaining
  silent-loss window going forward. The fold-side check stays forward-only by design (inventing
  a mismatch from a field absent in pre-fix history would make all committed history look
  defective); whether the defect ever fired in already-committed history is permanently
  unanswerable, because only the live side of that comparison was ever written down.
- The second judge story (M08's own coverage gap: seven of fourteen MCP tools never touched
  across nine M08 runs) ran paid and FAILED (`passed: false`, 8 findings, 2 critical). The
  coverage goal was MET — all seven tools fired, verified against the audit middleware's own
  route-registration order, which records a refused request too. The redesign's own forcing
  mechanism (`MAX_IDENTICAL_OUTCOMES`) fired correctly in the real run and the judge triaged the
  resulting incident correctly; the release did not ship, on real MCP-surface defects the
  story's own design surfaced rather than a story-design flaw — a `resume` that refuses on a
  workspace/staging collision with no MCP parameter able to satisfy it (#82), and that SAME
  refused `resume` still committing state and dropping the operator's pause hold before
  reporting failure (#83). Archived as `docs/acceptance/m09-judge-run-2026-08-19/`. Method
  lesson: rehearse on the EXACT surface the real actor uses — the free rehearsal supplied a
  `project` parameter on every `resume` call that the real MCP tool schema never exposes at
  all, so the rehearsal proved the state machine's mechanics thoroughly and could not have
  caught #82/#83 by construction — a gap in which LAYER was rehearsed, not in how carefully.
- A second method lesson, from the storm lane's own falsifier discipline grading its own
  author: a pre-registered no-referral prediction held 10 out of 10, but it was derived from a
  headroom model wrong by roughly an order of magnitude (40-100x predicted, ≈8x measured at the
  median) — right only because both values landed on the same side of the trigger, a
  near-boundary result would have flipped it. Recorded as RIGHT-FOR-WRONG-REASON, not a
  successful forecast, on the predictor's own principle: a number that is right for a reason its
  author does not have is not a measurement. The weight-bearing findings of that lane are the
  measurements themselves, not the prediction that happened to survive them.
- Three more fixes landed on `main` this milestone, main-based rather than part of the M09
  branch itself: cited evidence must open, not just hash (#75/#77) — two committed acceptance
  stores had answered `GHE005_INTEGRITY_FAILURE` on open for their entire committed life while
  every checksum stayed green (`SHA256SUMS` hashes files; two EMPTY DIRECTORIES have no file to
  hash), closed by a test that opens and replays every committed store by directory rather than
  by binding. Store-layout recovery (#76/#84) — `.tmp/`/`active/` are recoverable on open
  (transient workspace, nothing the journal does not already carry), `blobs/` stays strict (a
  blob is a tracked file; its absence means evidence is actually gone). A CLI that can print a
  schema digest (#78/#85) — a ritual that previously needed a throwaway test and a manual run to
  get the number a schema change requires now has one canonical command for it.
- PENDING, not a close blocker: a status-code question on `resume`'s failure surface (D) and
  the prediction ledger's final scoring pass (M) remain open rows, owners named, carried into
  M10 rather than resolved here.

## Ask Once and Sleep, M08 — 2026-08-18 (backfilled 2026-08-19, during M09's close — this
section was omitted at M08's own ship time; written now, dated then)

- `node_silence_seconds` is the single place elapsed time is computed; the monitor's private
  `last_event_per_node` and its three staleness thresholds are deleted, not deprecated. Silence
  is judged from an INSTANT the surface injects, never a duration a surface computes.
- §8 clause seven: the product now promises no surface recalculates the attention verdict — an
  owner decision, taken after the measurement that would otherwise have made recalculation the
  honest description of the state.
- `wake_wait`, the thirteenth MCP tool: bounded in the schema, content-free by construction,
  refusing any rendezvous the calling session does not hold.
- Task 3 ("the serve cannot serve while it drives") was going to be a fix; measurement refuted
  the premise instead — with a drive parked in a 90-second model call, `/health` answered in
  0.00s, `GET status` in 0.03s, and `POST wake-lease` was ACCEPTED in 0.08s with a live lease.
  Four of five causes proposed for the M07 alarm failure died to measurement; the fifth stands
  recorded as an unadopted hypothesis. The task became a correction of the record and was
  deleted rather than reworded, in both the milestone record and this milestone's own plan.
- One defect shape — an assertion reading one level above what it actually measures — appeared
  eleven times in one milestone, including once inside the very guard written to close a
  previous instance of it. The rule it produced, binding since: assert at the finest grain the
  question has.
- Closed after nine blind-judge runs, on the rule that findings close when NAMED and WITHDRAWN
  — never when the judge simply approves, which the M07 record established he does not. The
  finding carrying F1's number survived and changed KIND, from a claim that the mechanism fails
  to an argument about a default; it opened M09 rather than closing here.

## The one-glance answer, M07 — 2026-08-17

- Scope was the blind judge's four M06 findings and nothing else; the closing rule was that the same judge, on the same story, had to stop making them. Three real subscription runs were needed (`docs/acceptance/m07-run-2026-08-17/` — transcripts, not stores: the committed `journal.jsonl` has no `format.json` and no `blobs/`, so it cannot be opened or replayed, and the evidence its batches reference was never committed): 8 findings with one critical, then 7 with two criticals, then 7 with none — the last crediting a fix in its own words ("wake_status does correctly separate contentHead (12) from head (14)").
- F1: the sleep question is decided ONCE. `graphhelm_execution::attention` is the single pure predicate; `render` and the monitor both call it, and the monitor's private copy (the third in the codebase) is gone. `attentionRequired` is derived from its reasons, never declared beside them. The monitor states the verdict in words.
- F2: all sixteen lifecycle states are emitted as zero-filled buckets, reversing a documented guarantee — the assert that pinned the omission was inverted with its reasoning rewritten, not deleted.
- F3: `NodeOutcomeRecorded` carries an optional `reason` from a closed vocabulary (the fourteen route classes plus empty reply, malformed judgment, judge/gate refusals, four tool dispositions, fixture-scripted). Closed by design: the class rides the event, the text seals to Evidence (D-036). The gateway-error arm — which knew the most and sealed the least — now seals too. `reason` is OMITTED when absent because replay re-serializes each envelope and recomputes its hash: an always-emitted null breaks the chain of every pre-M07 event, proven by sabotage (a committed store stopped opening).
- F4: the fold keeps the last consumption per session, recorded from the consumption event; `wake_status` returns live/cursor/head/contentHead/lastConsumed. `contentHead` exists because the doorbell rings on content only, and publishing the raw head alone made a lost ring look plausible to the judge.
- The closing rule caught two defects both agents had shipped: the wedge arm was dead code in production (a real execution never emits `simulation_started`, so its status is null throughout and the arm demanded `Some(Running)`), and `status` itself was null on every read of a live run. A started execution with no recorded status now reports `running` on every surface.
- Honest limits recorded in `docs/milestones/one-glance.md`, including the four M08 seeds the judge raised: no liveness/time data in the glance, retry flapping invisible, no blocking wait on the MCP surface, and `wake_last_consumed` growing without bound.

## Quality gates, M06 — 2026-08-17

- The verdict vocabulary (kinds 29/30): `GateVerdict` is refusal-with-findings by construction (the envelope schema refuses a bare fail on the wire; the judge parser refuses it in-process); `GateCertified` is the thymus receipt as replayable state — `gate_certifications[gate_id] = suite_digest` in the fold, compared against the CURRENT suite digest so growing the pathogen suite voids old immunity by comparison.
- The thymus: ten bred pathogens (one per uselessness mode, paired per specimen with the plausible gate each fools); `certify()` refuses on any pass naming who was fooled; the correctness battery itself fails all ten (correctness alone certifies nothing) and `reject_everything` certifies (necessary, not sufficient).
- The deterministic evaluators (`core/quality`, pure): spec-derived content manifest + layout grammar over stripped HTML (style bodies survive — geometry declaration, not prose), praise-stuffing sentinel pinning identical findings; the COMPOSED evaluator certified, the layout grammar alone REFUSED — geometry never gates by itself, as a test.
- Gate nodes execute certified-or-not-at-all: `NodeWorkKind::GateCheck` (no model port, panicking-port-proven), one refusal arm died, fifteen node types byte-identical to 05d; failing verdicts are `TerminalFailure` with findings sealed beside the verdict event.
- The blind judge: no new work kind — cognitive transport with blindness as input discipline (type diet + assembler signature + source fence, each test-pinned); refusal-with-findings with stepsOverPar/stallPoints; fence/prose-tolerant parsing learned from the live run without loosening the contract.
- Demonstrations as the acceptance map's third binding: recorded journeys replayed against the current build, seed frozen at recording from injected entropy; the artifact verifier gained tracked-vs-named (the 05f journal lesson as machinery).
- The dogfood run (2026-08-17, committed with checksums): uncertified refusal live → `graphhelm quality certify` (GHCLI018 debut, closed registry) → certified geometry pass → the blind judge on the owner subscription probed the live system via MCP and REFUSED the one-glance story with four findings — the recorded M07 backlog seed. The gate-freeze rule ships as a pure check (gate machinery and gated code never move in one PR).
- `gate_http` is the twelfth gate suite (22 stages). Hotfix #55/#56 (wake sweep double-consume) landed mid-milestone from the pair loop's own dogfooding.

## Wake doorbell, 05g — 2026-08-16

- The wake primitive: a session sleeps at zero cost and is woken by another actor's append — one content-free byte, no payload, no polling anywhere (D-036/D-037; the 05f-era divergent pass's attacker traps are binding refused scope). `WakeLease`/`WakeLeaseConsumed` kinds with fold-pinned invariants: one live lease per session (arming replaces — anti fork-bomb), consumption burns, consuming unarmed corrupts replay, and a replay never rings (the fold speaks no transport, source-scanned).
- The serve-side ring fires only AFTER the trigger append is durable — the test's sleeper snapshots the store at the instant the byte arrives (the first sabotage exposed a blind detector; it was hardened before the guard was trusted). Two-phase consumption records the TRUE reason (rung / stale_rendezvous); only non-wake appends ring; a burned lease never rings twice; ring failure never fails the route.
- The sleeper-only surface: MCP tools `wake_arm`/`wake_status` (twelve exactly — no ring tool exists, the thirteenth-tool sabotage failed the closed list; the session can only arm ITSELF, its identity injected inside the dispatch, never an argument) over `POST/GET /v1/executions/{id}/wake-lease`, one path never two. The rendezvous derives from an OPAQUE id under a fixed local prefix — a hostile lease points nowhere.
- `graphhelm wake-wait`: the sidecar blocks for free and exits by code (0 rung / 3 timeout-as-routine / 2 GHCLI017); hostile ring bytes die in its sink — content never crosses, sentinel-proven.
- §5 measured, not promised: two real sessions through a counting TCP proxy — ZERO connections from the sleeper in the arm→ring window; degradation pinned (dead serve → routine timeout → a plain read still true: slow, never wrong).
- `wake_http` joined the gate (eleventh CLI suite, red-proven). Honest limits recorded: the Unix arm compile-shaped, CLI-direct appends ring nothing (dead-man covers), spurious wakes possible across a crash (content-free, so slow never wrong), the driver does not sleep on leases yet, and the 05f gitignore'd-evidence lesson with its named hardening candidate.

## Monitor and Milestone 05 close, 05f — 2026-08-16

- The read-only monitor (D-040): `GET /monitor[/{id}]` on serve, server-side-rendered ZERO-JavaScript HTML over the same `ExecutionProjection` the status command folds — the medium enforces the refusal (no script for a button to hook into), GET-only structurally (405 to every mutating verb, CSP on every 200), cookie bootstrap through the one shared constant-time verifier (token never in a Location or a page byte), meta-refresh with a `since` cursor making the delta strip stateless.
- Silence as signal and remediation as text: per-node staleness clocks from event gaps (per-kind bounds when the stream carries a graph, honest "unknown" when it does not), blast radius via pure reachability, and the EXACT approve command beside each triaged node — rendered from the clap definition itself and parse-round-trip-tested so page and CLI cannot drift.
- The negative proof: every monitor route hammered with every verb, store bytes fingerprinted bit-identical; `execution status --html` writes the same renderer frozen (byte-equal minus exactly the refresh line) as the incident artifact.
- The Milestone 05 close: `m05-clauses.toml` binds each §8 clause to named provers with assert fingerprints; `acceptance_map_is_grounded` (gate-listed) verifies fn existence, gate membership, fingerprints, D-citations, the generated map's bytes, and the committed run evidence's checksums in both directions. The real-work run happened once on the owner-subscription route (native_runtime, claude CLI) — completed, replayed byte-identically, identical state via CLI/HTTP/MCP — and is never re-run, only re-hashed.
- `monitor_http` joined the gate (tenth CLI suite, red-proven). Honest limits recorded: staleness vs event granularity, graph-publication absence on CLI/serve streams, the cookie as a named second door, the 2s refresh as the whole update contract, remediation without If-Match, the native adapter's cwd sensitivity, 401-as-needs_capacity inherited from 05b, the double-duty keyring biting once, and the single-store monitor index.

## Chat surface, 05e — 2026-08-16

- `graphhelm mcp` added: a stateless stdio MCP server whose tools map 1:1 onto Public Runtime API requests (D-039). Hand-rolled minimal JSON-RPC 2.0 per ADR-026 — the declined rmcp footprint measured and recorded (328→342 packages, 14 crates) — with a bounded 1 MiB line reader, protocol revision pinned to 2025-06-18 (the handshake model the targeted hosts speak; the meta-versioned 2026-07-28 spec noted as a revisit candidate), and a conformance suite pinning id echo, notification silence, and oversized-line resync.
- The ten tools: start, status, events, signal, approve, pause, resume, cancel, routes, probe — a closed list with closed schemas, guard-tested (no credential tool is representable; omission is the enforcement). Every mutation carries the optional `ifMatch` head pin; idempotency keys follow the logical act (`mcp-{nonce}-{s|n}{id}`, type-marked, digested past 32 chars) with retry-reuse and 409 divergence behaviorally proven; a notification-form tools/call never executes.
- Config fail-closed before any protocol byte: loopback-only URL under the post-#36 userinfo rule, token via file or env never argv (sentinel-scanned across a real transport attempt), GHCLI015_MCP_INVALID as reserved.
- The serve layer grew `GET /v1/gateway/routes|probe` calling the same command-layer functions as the CLI (parity test-pinned, explicit 401 asserts, server manifest preferred with query override) so the MCP tools never become a second path.
- Parity and choreography as tests: the 05a story via MCP equals direct HTTP with an empty exception list; two chat sessions coordinate through events alone and resolve an If-Match race with one re-read retry.
- Packaging thin and deletable: Claude Code plugin (.mcp.json with --token-file, skills operate-execution/observe-agents with `tool:`-marked choreography) and the Codex snippet, all validated in the suite; both READMEs carry §7's deletability sentence.
- `mcp_stdio` joined the gate as the ninth CLI suite, red-proven.
- Honest limits recorded: eight skills deferred each with its dependency named, pull-only notifications, tools-only MCP surface, the narrow secret-prefix heuristic, the aging protocol pin, probe's inherited spawn surface, verbatim gateway-read queries, and the shared stdout.

## Runtime, 05d — 2026-08-16

- `core/runtime` added: the `AsyncNodeExecutor` seam, dependency-inverting `ModelPort`/`ToolPort` (adapter crates implement them in `apps/cli`'s wiring — the arrow is pinned from both sides by source invariants), deterministic prompt assembly from the node contract, and the closed cognitive/tool classification with typed refusals the driver never dispatches.
- Evidence-before-append generalized to node work: seal first, then one atomic append naming exactly the sealed references; a sealing failure appends nothing (sabotage-proven). The 05c `ReuseDecision` obligation has its first producer — the ledger entry rides the same `PreparedAppend` as its outcome. Signal envelopes seal beside the operator copy on both the CLI (mandatory keyring) and the configured HTTP path.
- The M04 ledger closed: attempt-fair deterministic dispatch, edge-aware readiness for the decidable subset (literal-false conditions ungate; Failure edges release on Failed and only Failed, both deltas property-pinned), and resume cross-checking the supplied file's hash against the recorded graph before any recovery append — with the empirical discovery that the CLI path's record is the `execution_started` hash, not the M03 publication field.
- `drive_to_quiescence_async`: the 04f sequencing event-for-event, concurrent work up to the budget bound, every write serialized through the single writer inside `spawn_blocking`, and §12 immediate stop composed from the 04e pieces — aborted futures, real children killed and proven dead, `Interrupted → Blocked`, resume gated on triage.
- The API drives async while the CLI stays byte-identical (`execute_prepared` split; the 05a parity test unchanged through the swap). Serve gains grouped runtime flags, `pause` `{"mode":"immediate"}`, and the milestone's §8 acceptance sentence is one named green test: agent (sealed reply) + tool (sealed record and streams) to completion over HTTP with byte-identical double replay. `runtime_http` joined the gate, red-proven.
- Honest limits recorded: the viability gate's mixed-graph quiescence-without-completion case, the double-duty serve keyring, `ReuseDecision` silent on the serve path pending a host accessor, HTTP calls not abortable mid-flight, prompt assembly without a Context Compiler, one route, empty `artifact_refs`, and throughput recorded in its own unit (≈0.17 full stories/second vs 05a's ≈3 raw requests/second — not comparable, both kept as baselines).

## Runtime, 05c — 2026-08-16

- `core/tool-broker` (pure) and `adapters/tool-host` (impure) added: the closed repository/shell/tests call vocabulary with per-action effects and capabilities, the effect→tier rule with `SecretUse` structurally refused, lexical path/program rules, the deny-by-default capability lease, and the pure `authorize` pipeline in pinned order — decisions in the pure crate, enforcement in the host, purity pinned by a source-invariant scan over all six sources.
- Tier 1 executions run inside an ephemeral detached `git worktree` provisioned with hooks disabled and removed under retry/backoff + prune; the process primitive is argv-only with `env_clear()` plus a six-name allowlist, redirected homes, a synthetic git identity, and an `extra_env` deny-list covering every host-defined name. Provision runs git under the SAME scrubbed config posture as execution — config consistency proved to be the correctness condition when a user-level `autocrlf` smudge made `git apply --index` refuse everything.
- The register's hard constraint is one named green test: `credentials_are_demonstrably_absent_from_the_tier_1_workspace` — sentinels in the parent environment and a protected keyring, real invokes including a write, streams/workspace-scan/separation/record asserts, sabotage-proven in both directions.
- The 05c amendment landed: `FreshnessClass` in `core/protocols`, a clean-tree-gated snapshot-keyed `ReadCache` (Tier 0 + `SnapshotClosed` only; erasure invalidation deletes bytes), and `ReuseDecision` as the 26th event kind through the full D-037 ritual — identity-only payload (`node_id`, `key_digest`, closed enums), an explicit ledger-not-state fold arm, and no producer until the 05d executor.
- `graphhelm tool invoke` added (GHCLI012–014, mandatory `--capture-out`, digest-only record in the envelope) and `tool_cli` joined the gate as a named stage, proven able to go red.
- Honest limits recorded: Tier 1 is worktree+scrub not a container; the Policy Engine step is fixed rules; redaction is caps + structural env emptiness; leases have no lifecycle; records are not yet Evidence; `apply_patch`/`commit` never compose across calls (the 05d `ToolPort` reconciliation names the composition decision); the read cache persists under staging by design.

## Runtime, 05b — 2026-08-14

- `core/gateway` (pure) and `adapters/model-gateway` (impure) added: the Universal Model Gateway's route manifest, error taxonomy, capacity policy, credential broker, and BYOK/native-runtime adapters, with purity enforced by a source-invariant test pinning `core/gateway`'s dependency table to exactly `graphhelm-protocols`/`serde`/`serde_json` and forbidding it from ever naming the adapter crate.
- `RouteManifest::from_json` makes billing mode and authentication a single structural fact tied to transport (§20): a `direct_api` route requires `api_key`/`per_token`/`baseUrl`/`model`/`credentialRef` and a provider in `{anthropic, openai}`; a `native_runtime` route requires `account_subscription`/`subscription_quota`/`runtime`/`command` and structurally forbids `credentialRef`/`baseUrl`, so the manifest cannot route a broker secret into a runtime that owns its own auth. Cleartext `http://` is refused except to loopback.
- The fourteen-kind `GatewayError` taxonomy (§17) maps to `NodeOutcome` through one exhaustive match with no wildcard arm: quota/rate/auth failures park the node as `NeedsCapacity` (§12, no automatic paid fallback), provider/timeout/crash/malformed trouble retries, a request the gateway can never satisfy is terminal, cancellation passes through.
- The credential broker invents no new cryptography — it wraps the existing `EvidenceProtector<SealedKeyProvider>`, persisting sealed parts atomically, enforcing `usable_by` route scoping and durable revocation at `lease()`, and never caching plaintext.
- ADR-025 pins `ureq =3.4.0` for the BYOK adapters' outbound HTTPS, rustls-only, confirmed to share `sqlx`'s existing `rustls`/`ring` versions rather than adding a second TLS stack. Anthropic and OpenAI adapters map fixed status tables onto the taxonomy; native-runtime adapters spawn Claude Code/Codex under `env_clear()` plus a fixed allowlist and a stdin-only prompt — the isolation proved load-bearing when a sabotage run leaked a live `SENTRY_AUTH_TOKEN` into a fake child before the guard was restored.
- `graphhelm gateway routes|probe|credential set|remove` added, with a quota-free probe (§18) and `gateway_cli` as a new named gate stage.
- Honest limits recorded: JSON manifests where the spec shows YAML, router scoring and the broker's access audit deferred, three of the five route types deferred, session management and gateway-native tool calls out of scope, host-CLI JSON shapes are fixtures pending 05e's live re-verification, and the quota-detection marker list is a documented heuristic rather than a wire contract.

## Runtime, 05a — 2026-08-14

- `graphhelm serve` added: the Public Runtime API as the multi-agent concurrency contract. Loopback-only fail-closed bind, sibling-path bearer token with constant-time comparison, the CLI's four-key envelope on every route.
- Endpoints one-to-one with the seven execution commands plus status (now carrying `headSequence`) and a paged events tail that refuses rather than truncates. A fresh mutation's own reply now carries `headSequence` too (a serve-layer enrichment; CLI mutation output is unchanged), closing the extra-GET gap on `If-Match`-chained writes.
- Every mutation attributed from headers (`owner`/`agent`; `system` reserved for the driver's hops) and idempotent under full retry: decision-event keys derive from the caller's command key plus a fixed suffix plus a content digest, and a pre-flight classifies them Absent/Complete/Partial/**Divergent** against the stream history it already has in hand. **Fixed in final review**: the pre-flight was content-blind — key presence alone, no check that a committed key's event actually carried the same request — so reusing an `Idempotency-Key` across two different bodies got silently absorbed as a completed retry of the first, its real effect never applied. The digest closes that: a byte-identical retry still re-derives the same key (Complete, unchanged), a divergent reuse now derives a same-prefix-different-digest key and is refused 409 before the store is touched. The header is capped at 64 characters to keep the longer derived key within `OpaqueId`'s limit. `If-Match` gives optimistic concurrency with the current head in every 409.
- The eight-agent storm holds across consecutive runs — no 500s, coherent replay, byte-identical double replay, full attribution — and its sabotage was caught synchronously by the fold rejecting the incoherent history inside the causing request.
- CLI-API parity pinned with an empty exception list; `api_http` is a named gate stage proven able to fail. axum `=0.8.9` recorded as ADR-024.
- Honest limits recorded: no documentation surface yet (Living Docs arrives into this same API), ~3 req/s on the current-thread runtime as the baseline 05d must beat, unknown executions read as empty, mTLS deferred.

## Graph Engine and Governor, 04f — Milestone 04 complete — 2026-08-13

- The driver ships in `apps/cli`: drive-to-quiescence over every pure piece, with `Queued` nodes unioned into the dispatch candidates so retries redispatch, every `next_state` from `apply_transition`, every append through the production store.
- JSON-only `execution start|status|signal|approve|pause|resume|cancel` with redaction-safe codes. Evidence is written before any event that references it; an unrecordable signal preserves its envelope and says so; approval is the triage act and never auto-drives; resume redispatches only what pause held.
- Two semantic corrections landed first: `Started` no longer touches run-length accounting, making `MAX_IDENTICAL_OUTCOMES` fire for retry loops, and resume refuses untriaged interruptions and only them.
- The operator story runs end to end through the binary and the final stream replays byte-identically; `execution_cli` is a gate stage, proven able to fail.
- The acceptance map in the milestone document ties all eight §8 criteria to named tests, with the gaps in the same table: file-based signal evidence, undesigned signal-to-draft translation, unbudgeted ghost births, the resume file-trust seam, and the intentional simulate/executor divergence.

## Graph Engine and Governor, 04e — 2026-08-13

- `NodeOutcome::{Paused, Interrupted}` and `SimulationStatus::Cancelled` added, appended so no existing wire name moves; `execution_paused` and `execution_resumed` grow the closed event set from 23 to 25, with the envelope schema corrected in place under D-037 and both catalog digests recomputed. Cancel is a final status per §13, not a new event kind.
- Three transition arms close long-named gaps: `Blocked` gains its owner resume path (open since 04a), graceful pause holds `Ready`/`Queued` work, and an interrupted running node can only become `Blocked` — a crash is not an outcome the executor reported, and anything but blocking would authorize a retry nobody judged safe.
- Pause and resume fold with coherent-history guards; an incoherent pause or resume is corrupt.
- Pure `recovery_plan` and `resume_preconditions` added; §11.4's undecidable items are named, not approximated. The checkpoint is the `ProjectionGeneration` 04b already ships — no second checkpoint type exists.
- The composed lifecycle test drives every pure piece since 04a through pause, crash, recovery, owner approval, resume and completion, and the full history replays byte-identically, including split through `apply_page`. Three findings for 04f are recorded in the milestone document: resume does not demand triage of blocked nodes, the honest crash-recovery order is pause-recover-approve, and `MAX_IDENTICAL_OUTCOMES` is structurally unreachable for retry loops, a design defect 04f must resolve.

## Graph Engine and Governor, 04d — 2026-08-13

- Three governance event kinds added — `signal_recorded`, `ghost_node_proposed`, `mutation_accepted` — growing the closed set from 20 to 23; the `1.0.0` envelope schema corrected in place under D-037 with both copies byte-identical and both catalog digests recomputed. `signal_recorded` carries no free-form content per D-036: typed fields plus a digest binding the record to the raw envelope bytes destined for encrypted Evidence.
- `SignalSeverity` and `SignalSourceKind` moved to `graphhelm-protocols` as wire vocabularies, gaining the `Serialize` their new role requires; `core/execution` re-exports both.
- The projection folds `signals_recorded` and `accepted_mutations` by counting history, and folds a ghost's birth: a proposal for a node that already has any state is corrupt, and an acceptance recorded under a mode the execution was not in is corrupt — decision 5.5 enforced at replay.
- Pure governance decisions added in `core/governor`: `admit_signal` blocks at the signal budget rather than dropping evidence; `decide_mutation` maps D-022's modes with Manual and no-mode rejecting, blocks at `MAX_ACCEPTED_MUTATIONS` in every mode, and rejects an unrecognized kind at every mode and counter value; `override_with_waiver` reuses the M03 waiver verbatim, node-scoped, refusing an empty risk acknowledgement. Id and clock are injected, never read.
- Bounded concurrency added as `dispatch_plan`: a deterministic prefix of the ready set, `ZeroParallelism` surfaced loudly. The function that will read `max_parallel_model_calls` now exists; wiring the field to it is the 04f driver's work.
- Nothing appends these events or drives intake yet; the decisions await the 04f driver. Ghost approval reuses the existing `Approved -> Ready` path unchanged.

## Graph Engine and Governor, 04c — 2026-08-13

- `ready_set` added: which nodes may be dispatched now. Only `Ready` nodes are dispatchable, and a test pins that every dispatchable state accepts a `Started` outcome, so the scheduler cannot propose work the state machine rejects. Dependencies are fail-closed — every incoming edge gates, whatever its type — and a predecessor releases its dependent only when `Succeeded`, `Waived` or `Skipped`. Exceeding `MAX_READY_SET` blocks rather than truncating, per decision 5.7.
- `NodeState::Ghost` is excluded from the ready set by construction and never releases a dependent, making "consumes no tokens" structural rather than conventional. Covered by a property test sampling randomised assignments of the other nodes' states.
- `classify_progress` added: retry exhaustion and repeated identical outcomes, read against the counters the projection derives. It reads the run length through `identical_outcomes_for`, so a run belonging to a different outcome cannot block a node on its first failure.
- Five of the seven `OBSERVABILITY_AND_RECOVERY.md` §15 no-progress conditions are deliberately not detected; they need signal intake or real tool calls, and none is approximated.
- `FixtureExecutor` added in `core/simulation`: the first and only milestone-04 `NodeExecutor`, consulting a fixture table and nothing else. Its answer does not depend on the attempt number. It has no callers yet — `simulate()` still drives its own transitions, so the seam is defined but simulation is not yet a consumer of it; the divergences between the two are tabulated in the milestone document and tracked as outstanding work.
- `MAX_PROJECTION_NODES` separated from decision 5.7's domain bounds as a resource guard, published, and pinned above `MAX_READY_SET` by a module-scope `const` verified to fail `cargo build`. It is not compared against `MAX_SIGNALS_PER_EXECUTION`, which counts a different dimension.
- 04a's purity invariant narrowed to what it protects — adapters, clocks and randomness, not sibling core crates — with the exact dependency set pinned by a second test.

## Graph Engine and Governor, 04a/04b — 2026-08-13

- `core/execution` added: a pure crate with no I/O, no clock, no randomness, and no adapter dependency, enforced by a source invariant test rather than documented alone.
- `NodeState::Ghost` added to the shared vocabulary; its only legal exit is approval to `Ready`, proven by property test across every outcome and counter value.
- Bounds fixed as counters, never durations: `MAX_NODE_ATTEMPTS` 8, `MAX_IDENTICAL_OUTCOMES` 3, `MAX_ACCEPTED_MUTATIONS` 64, `MAX_READY_SET` 1024, `MAX_SIGNALS_PER_EXECUTION` 10,000.
- The closed Graph Signal typed subset added; an unrecognized kind is recorded as evidence but can never propose a mutation.
- `apply_transition` added: total and property-tested for totality, determinism, and ghost-safety. The `NodeExecutor` trait added as the Milestone 05 seam, with no implementor yet.
- `NodeOutcome` and `ExecutionMode` added to `graphhelm-protocols` as closed vocabularies. The four execution event kinds — `execution_started`, `execution_mode_changed`, `node_outcome_recorded`, `execution_completed` — added to the closed event set, growing it from 16 to 20; the `1.0.0` envelope schema corrected in place under D-037, both schema copies kept byte-identical, and both catalog digests recomputed.
- `ExecutionProjection` extended with execution ID, mode, and per-node attempt and identical-outcome counters, all derived by folding history rather than read from a payload. A generation predating these fields loads with them empty; one missing a pre-existing field still fails.
- Replay proven identical across runs, and a discarded generation rebuilt through `apply_page` proven to land on the same state as a direct replay.
- `GHPROJ001_WATERMARK_MISMATCH`, shipped in Milestone 03 with no observed test, now has one: it drives the guard in `ProjectionRebuilder::rebuild` through a test-double repository, since the production PostgreSQL adapter cannot structurally reach it.
- Scheduling, in-flight governance, pause/resume/cancel, and the operator CLI remain out of scope; they are Milestones 04c through 04f.

## Production Event and Evidence Store — 2026-08-12

- Decisions D-035, D-036, and D-037 accepted with ADR-021 through ADR-023: authoring and persistence are distinct representations, the Governor externalizes free-form content as encrypted Evidence, and required content that is unavailable blocks execution.
- Safe persistence projection design and a focused Event/Evidence Store threat model added.
- Single pre-release schema baseline `1.0.0` rebuilt with 15 contracts, adding `PersistedGraphVersion`, event envelope, Evidence record, artifact reference, repository scope, and sensitivity; the intermediate release `1.1.0` removed.
- Eleven typed content positions registered, including the `context_path`, `permission_path`, and `isolation_path` authoring scopes.
- Local JSONL repository and PostgreSQL adapter implemented against one wire contract, with forced row-level security, transaction-local scope, authenticated stream heads and integrity checkpoints, and least-privilege runtime roles.
- Encrypted Evidence, sealed local key provider, authenticated revocation journal, legal holds, and auditable cryptographic erasure implemented.
- Disposable projection generations, fail-closed executable materialization, encrypted streaming backup, and verified restore implemented.
- Operator commands `events verify`, `events rebuild`, `events backup`, and `events restore` added, with bounded JSON configuration and out-of-band key material.
- No legacy compatibility layer: superseded event formats, importers, dual readers, and fallback branches removed before the first public release. Existing developer repositories and databases must be deleted and recreated.
- Milestone documentation and operational procedures published; the specification version remains 0.1.1.

## 0.1.1-spec — 2026-08-08

- Product name selected: GraphHelm.
- Naming decision and brand architecture documented.
- Initial Codex prompt for the Foundation Graph Kernel.
- Manual override example fixed with explicit deploy target.

## 0.1.0-spec — 2026-08-08

- Full PRD for Programação 5.0.
- Decisions on topology, autonomy, harness, agents, context, Dreams, models, isolation, UI, open source, and licensing.
- Studio functional specification.
- Harness Compiler and Graph Governor.
- Complete Graph Engineer guide.
- Graph DSL v1 and JSON Schemas.
- Event Store, Knowledge Graph, Living Documentation, and Context Capsules.
- Universal Model Gateway with BYOK, official subscriptions, and local models.
- Threat model and isolation tiers.
- Observability, checkpoints, replay, and recovery.
- MIT license and open governance.
- Graph and manifest examples.
