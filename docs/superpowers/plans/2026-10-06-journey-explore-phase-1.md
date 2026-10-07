# Journey Explore Phase 1 (flow format, validator, compiler) Implementation Plan

> **Execution:** follow the repository's delivery flow (`docs/process/DELIVERY.md`): issue, Keel card, one blind review, merge by the approving reviewer. The task list below is the work breakdown, not a separate workflow.

**Goal:** Ship the `journey-flow` YAML format with a closed schema, a validator with stable diagnostic codes, a deterministic compiler to the frozen journey contract, and file-only approval — no browser, no model.

**Architecture:** A new schema `schemas/journey-flow.schema.json` registered in `schemas/catalog.json`. A new CLI module `apps/cli/src/commands/journey_flow.rs` parses YAML (`serde_yaml_ng`, already a workspace dependency), validates against the schema plus semantic rules, renders canonical YAML, and compiles each path to a contract JSON. `journey validate` gains flow files; new subcommands `journey compile` and `journey approve`. Existing readers (`journeys`, Studio, `keel check`) are untouched because they read the compiled contracts.

**Tech Stack:** Rust (graphhelm-cli), `serde_yaml_ng =0.10.0`, `serde_json`, `graphhelm_schema::OfflineSchemaSet`, `sha2` (check `apps/cli/Cargo.toml`; if absent, use the hashing helper the CLI already uses for evidence digests), `assert_cmd` + `tempfile` for CLI tests.

**Spec:** `docs/specs/2026-10-06-journey-explore-design.md`

## Global Constraints

- The contract schema `extensions/builtin/graphhelm-jpd/schemas/journey-contract.schema.json` is frozen: do not edit it or the jpd package.
- First key of every flow: `schema: graphhelm.journey-flow/1`. Schema `$id`: `https://p50.dev/schemas/journey-flow.schema.json`, document version `1.0.0`, every object `additionalProperties: false`.
- Id rule: exactly `graphhelm_execution::journeys::valid_journey_id` — `^[a-z0-9][a-z0-9._-]{0,127}$`, no `..`, no `/` `\` `:`. Apply it to every composed id too (`<id>.<path>`, `<screen>.visible`); failure is `flow.composed_id_invalid`. The compiler joins only a validated contractId under `.graphhelm/journeys/`.
- Approval preimage: canonical rendering with `status`, `approved`, `drift` **omitted**; one function `approval_digest` serves approve, validation and (phase 2) the cache `flowDigest`.
- Canonical check is a raw byte comparison, no line-ending normalisation: CRLF is `flow.not_canonical`.
- File: `.graphhelm/journeys/<id>.journey.yaml`; max 32 KiB; ≤ 64 screens, ≤ 128 edges, ≤ 16 paths, ≤ 8 acts per edge, 1–8 `expect` pairs.
- `base` host ∈ {`localhost`, `127.0.0.1`, `[::1]`, `*.localhost`, `*.test`}.
- Compiled contract: `main` → `<id>.json`, path `p` → `<id>.<p>.json`; defaults: timeout 30, `visibleError` `screen <id> not reached`, `safeStop` `stop replay`, `recoveryAction` null, `prohibitedSideEffects` [], `requiredFact` `content_rendered`, `requiredEvidenceKinds` [`visual_capture`], `requiredObserverCapability` `browser`, `maxEvidenceAgeSeconds` 604800, `version` 1, `preconditions` [], `outOfScope` [].
- CLI error code `GHCLI034_JOURNEY_FLOW_INVALID`; exit 0 clean, 2 findings, 3 unusable input.
- Diagnostic codes exactly as spec §7; never rename one.
- All docs in English.

## Review Focus

- A flow whose YAML uses anchors/aliases or duplicate keys: expect `flow.not_yaml` or `flow.schema_invalid`, never a silent merge (test in Task 2).
- CRLF line endings, and separately a missing final newline: each is rejected as `flow.not_canonical` (raw byte comparison), and `--fmt` rewrites to LF with one trailing newline (Task 3).
- A `.journey.yaml` beside contracts in `journeys/`: the existing contract reader must not try to parse it (it reads `*.json` only — pin with a test in Task 4).
- A draft flow plus a hand-written contract with the same contractId: `compile` must refuse to overwrite a contract it did not generate unless `--check` shows it identical (`flow.contract_stale`, Task 4).
- Approving a flow, then editing one `expect` name: `flow.approval_stale` (Task 5).

---

## File Structure

- Create `schemas/journey-flow.schema.json` — the closed format.
- Modify `schemas/catalog.json` — register `journey-flow` 1.0.0 with its sha256; `schemas/CHANGELOG.md` — entry.
- Create `apps/cli/src/commands/journey_flow.rs` — parse, semantic checks, canonical render, compile, approve digest. One responsibility: the flow artifact.
- Modify `apps/cli/src/commands/journey_validate.rs` — route `*.journey.yaml` to `journey_flow::check`.
- Modify `apps/cli/src/commands/journey.rs`, `apps/cli/src/args.rs`, `apps/cli/src/commands/mod.rs` — `Compile` and `Approve` subcommands.
- Modify `apps/cli/src/error_codes.rs` — `GHCLI034_JOURNEY_FLOW_INVALID`.
- Create `apps/cli/tests/journey_flow_cli.rs` and fixtures `apps/cli/tests/fixtures/journey_flow/{checkout.journey.yaml,checkout.json,sabotage/*.journey.yaml}`.
- Modify `docs/harness/JOURNEY_PROVEN_DEVELOPMENT.md` (flow section), `docs/DECISION_REGISTER.md` (D-057), `extensions/builtin/graphhelm-jpd/skills/journey-map/SKILL.md` is in the digest-bound package: **do not edit**; mention flows in the JPD doc instead.

### Task 1: The schema

**Files:**
- Create: `schemas/journey-flow.schema.json`
- Modify: `schemas/catalog.json`, `schemas/CHANGELOG.md`
- Test: `apps/cli/tests/journey_flow_cli.rs`

**Interfaces:**
- Produces: `FLOW_SCHEMA_ID: &str = "https://p50.dev/schemas/journey-flow.schema.json"` and `fn flow_schemas() -> Option<&'static OfflineSchemaSet>` in `journey_flow.rs` (same pattern as `journeys::contract_schemas`).

- [ ] **Step 1: Write the failing test** — the spec §3 example is valid; adding an unknown key at root, in a screen, in an act, each fail; `scope: []` fails; `scope: unknown` passes.

```rust
#[test]
fn spec_example_is_valid_and_unknown_keys_are_refused() {
    let dir = project_with_flow(SPEC_EXAMPLE);
    graphhelm().args(["--json", "journey", "validate", "--all", "--project"]).arg(dir.path())
        .assert().code(0);
    for (pointer, patch) in [("/", "extra: 1"), ("/screens/0", "    extra: 1")] {
        let dir = project_with_flow(&insert_after_first(SPEC_EXAMPLE, pointer, patch));
        let out = validate_json(dir.path());
        assert_eq!(out["data"]["diagnostics"][0]["code"], "flow.schema_invalid");
    }
}
```

- [ ] **Step 2: Run it, expect FAIL** — `cargo test -p graphhelm-cli --test journey_flow_cli spec_example` (validate ignores yaml today: exit 0 with zero files, or argument error).
- [ ] **Step 3: Write the schema.** `required` at root: `schema, id, status, approved, base, actors, secrets, risks, screens, edges, paths, drift`; `title` optional. `$defs`: `id`, `screen {id, url, state, expect, scope, title?}`, `expect {role, name}`, `act {kind, role, name, text?, secret?}` with `not: {required: [text, secret]}`, `edge {id, from, to, acts}`, `drift {edge, act, code, seen, at, healed?}`, `approval {revision, digest}`; `scope: oneOf [{const: unknown}, {type: array, minItems: 1, items: path pattern ^[^/.][^*?]*$}]`; `paths: {type: object, required: [main], propertyNames: id, additionalProperties: {array of id, minItems 1}}`; enums copied verbatim from the contract (`kind`, expected states, `risks`). Register it in `catalog.json` (compute `sha256:` with `sha256sum schemas/journey-flow.schema.json`) and add the CHANGELOG entry.
- [ ] **Step 4: Minimal wiring** — `journey_validate.rs`: when a file ends with `.journey.yaml` (or `--all` finds one), parse with `serde_yaml_ng::from_str::<serde_json::Value>`; on error push `flow.not_yaml`; else validate with `flow_schemas()` and map each diagnostic to `flow.schema_invalid`.
- [ ] **Step 5: Run, expect PASS**; also `cargo test -p graphhelm-cli --test journey_validate_cli` still green; run the catalog conformance test (`cargo test -p graphhelm-schema-evolution`) to prove the catalog entry digest.
- [ ] **Step 6: Commit** `feat(jpd): journey-flow schema and validate parses flows` with `Refs #333`.

### Task 2: Semantic validation and finding codes

**Files:**
- Modify: `apps/cli/src/commands/journey_flow.rs`, `apps/cli/src/error_codes.rs`
- Test: `apps/cli/tests/journey_flow_cli.rs`, fixtures `sabotage/*.journey.yaml`

**Interfaces:**
- Produces: `pub(crate) fn check(file: &Path, project: &Path) -> Vec<Finding>` where `Finding { code: &'static str, pointer: String, message: String }` (reuse `journey_validate::Finding`, make it `pub(crate)`).

- [ ] **Step 1: Failing tests, one per code** — table-driven over sabotage fixtures, each fixture differs from the golden flow by one line:

```rust
#[test]
fn every_static_code_fires_on_its_sabotage() {
    for (fixture, code, pointer) in [
        ("id_mismatch", "flow.id_mismatch", "/id"),
        ("duplicate_screen", "flow.duplicate_id", "/screens/1/id"),
        ("edge_to_ghost", "flow.unknown_screen", "/edges/0/to"),
        ("path_ghost_edge", "flow.unknown_edge", "/paths/main/1"),
        ("act_ghost_secret", "flow.unknown_secret", "/edges/1/acts/0/secret"),
        ("path_gap", "flow.path_disconnected", "/paths/main/1"),
        ("path_loop", "flow.path_revisits_screen", "/paths/main/2"),
        ("remote_base", "flow.base_not_local", "/base"),
        ("scope_missing", "flow.scope_path_missing", "/screens/0/scope/0"),
        ("scope_escape", "flow.scope_path_outside_project", "/screens/0/scope/0"),
        ("approved_with_drift", "flow.approved_with_drift", "/drift/0"),
        ("too_large", "flow.too_large", ""),
        ("slash_in_screen_id", "flow.schema_invalid", "/screens/0/id"),
        ("colon_in_path_name", "flow.schema_invalid", "/paths"),
        ("dotdot_edge_id", "flow.schema_invalid", "/edges/0/id"),
        ("composed_too_long", "flow.composed_id_invalid", "/paths/<120-byte-name>"),
    ] {
        let out = validate_fixture(fixture);
        assert_eq!(out.status.code(), Some(2), "{fixture}");
        let d = &json(&out)["data"]["diagnostics"][0];
        assert_eq!((d["code"].as_str(), d["pointer"].as_str()), (Some(code), Some(pointer)), "{fixture}");
    }
}
```

Plus: YAML with an anchor/alias → `flow.not_yaml` (refuse aliases by scanning events: `serde_yaml_ng` resolves them silently, so reject any `&`/`*` anchor token via a pre-scan of the parser events, or by reparsing with `serde_yaml_ng::Deserializer` and checking for `Event::Alias`); duplicate mapping key → `flow.not_yaml`.

- [ ] **Step 2: Run, expect FAIL** (codes absent).
- [ ] **Step 3: Implement** in order: size cap before parse (`flow.too_large`), parse, schema, file-name/id, uniqueness, references, path connectivity (edge n `to` == edge n+1 `from`), revisits, base host (parse with `url::Url` if already a dependency, else split scheme/host by hand and compare against the allowlist), scope paths (reuse the containment logic from `journey_validate.rs` lines ~245–280: no `..`, no absolute, exists under project), `status: approved` with non-empty `drift`. `flow.unreachable_screen` is a warning: emitted with `"severity": "warning"`, does not change the exit code. Add `GHCLI034_JOURNEY_FLOW_INVALID` and use it in the exit-2 envelope when any finding is a flow finding.
- [ ] **Step 4: Run, expect PASS**; `cargo clippy -p graphhelm-cli -- -D warnings`; `cargo fmt --check`.
- [ ] **Step 5: Commit** `feat(jpd): journey-flow semantic checks with stable codes`.

### Task 3: Canonical form

**Files:** Modify `journey_flow.rs`; Test `journey_flow_cli.rs`

**Interfaces:**
- Produces: `pub(crate) fn canonical(flow: &Flow) -> String` (LF, one trailing newline, key order per schema, screens/edges sorted by id, paths `main` first then by name, `act`/`expect`/`approved` in flow style on one line), and typed `Flow` structs (`#[serde(deny_unknown_fields)]`).

- [ ] **Step 1: Failing tests** — golden fixture is canonical (validate clean); the same file with screens reordered, or with block-style `act`, or with CRLF line endings (otherwise identical), or with the final newline missing (otherwise identical) — three separate cases — each yields `flow.not_canonical` at `""`; `journey compile --fmt` rewrites it to bytes equal to the golden fixture; `canonical(parse(canonical(x))) == canonical(x)` on every fixture.
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement** a hand-written emitter (do not rely on `serde_yaml_ng` output style, which is not a stable contract): quote a scalar with double quotes only when it is empty, starts with a YAML indicator, contains `: `/`#`/`,`/`[`/`]`/`{`/`}`, or would parse as bool/null/number. Comparison: raw input bytes against `canonical(parse(input))` bytes, with no normalisation; any difference, CRLF included, is `flow.not_canonical`.
- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `feat(jpd): canonical journey-flow form and --fmt`.

### Task 4: Compile to the frozen contract

**Files:** Modify `journey_flow.rs`, `journey.rs`, `args.rs`, `mod.rs`; fixtures `checkout.json` (golden contract); Test `journey_flow_cli.rs`

**Interfaces:**
- Consumes: `Flow`, `canonical`, `journeys::contract_schemas`, `journeys::contract`.
- Produces: `pub(crate) fn compile(flow: &Flow) -> Vec<(String /*contractId*/, serde_json::Value)>`; CLI `graphhelm journey compile [--project P] [--check] [--fmt] [--include-draft] [ids...]`.

- [ ] **Step 1: Failing tests**
  - approved golden flow → `.graphhelm/journeys/checkout.json` byte-equal to fixture `checkout.json` (serialised with `serde_json::to_string_pretty` + `\n`).
  - the written contract passes `graphhelm journey validate .graphhelm/journeys/checkout.json` (exit 0) **and** `graphhelm --json journeys --execution <empty run>` lists `checkout` with steps `cart, pay, done` (reuse the setup helpers in `apps/cli/tests/journeys_surfaces.rs`).
  - a second path `guest: [...]` writes `checkout.guest.json`.
  - draft flow: nothing written without `--include-draft`; output lists it under `skipped: [{id, reason: "draft"}]`.
  - `--check` after a hand edit of `checkout.json` → exit 2, `flow.contract_stale` at `/journeys/checkout.json`; `journey validate --all` reports the same.
  - a pre-existing hand-written `checkout.json` that differs → compile without `--check` exits 2 with `flow.contract_stale` and does not overwrite; `--force` overwrites. (Adds a `--force` flag; name it in the PR's Keel card.)
  - compile twice → identical bytes (determinism).
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement** per spec §4 and the Global Constraints defaults: steps = `[edges[0].from] + edges.map(to)`; step 0 action `{kind: navigate, target: {strategy: "label", value: url, geometryClaim: false}}`; later steps: entering edge's last act → `{kind, target: {strategy: "accessible_name", value: name, role, geometryClaim: false}}`; `actorId` = `actors[0]`; `title` = flow `title` or `id`; `taskScope` = `journey-flow <id> path <p>`; promise statement `shows <role> "<name>", ...` truncated to 1024 bytes; validate every compiled contract against `contract_schemas()` before writing (a failure is a bug: exit 3 with `flow.compile_invalid`, add the code to the spec list in the same PR).
- [ ] **Step 4: Run, expect PASS**; also `cargo test -p graphhelm-cli --test journeys_surfaces --test journey_validate_cli --test journey_producers_cli` green.
- [ ] **Step 5: Commit** `feat(jpd): compile journey flows to contracts`.

### Task 5: Approve (file-only) and the docs

**Files:** Modify `journey_flow.rs`, `journey.rs`, `args.rs`; `docs/harness/JOURNEY_PROVEN_DEVELOPMENT.md`; `docs/DECISION_REGISTER.md`; Test `journey_flow_cli.rs`

**Interfaces:**
- Produces: `pub(crate) fn approval_digest(flow: &Flow) -> String` (`sha256:` of `canonical` rendered with the keys `status`, `approved`, `drift` omitted — the spec §3 preimage); CLI `graphhelm journey approve <id> [--project P]`.

- [ ] **Step 1: Failing tests** — approve a draft: file now has `status: approved`, `approved: {revision: <HEAD sha>, digest: ...}`, `drift: []`, is canonical, and `checkout.json` exists; editing one `expect` name afterwards → `flow.approval_stale` at `/approved/digest`; approve refuses a flow with findings (exit 2, nothing written); a test that computes the digest independently in the test (sha256 of the golden fixture with those three lines removed, then re-rendered) and asserts it equals the written `approved.digest`; approve in a non-git directory → exit 3 `GHCLI001_ARGUMENT_INVALID` naming the missing revision.
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement**; reuse `journey::head` for the revision. Write via temp file + rename.
- [ ] **Step 4: Docs** — JPD doc: a "Journey flows" section (what the file is, the three commands, that contracts are generated from flows and must not be hand-edited once a flow owns them, codes table pointer to the spec). Decision register: row D-057 "Journey flows compile to frozen contracts" with the one-paragraph rationale of spec §4.
- [ ] **Step 5: Run the whole proof set**: `cargo test -p graphhelm-cli --test journey_flow_cli --test journey_validate_cli --test journeys_surfaces`, `cargo clippy -p graphhelm-cli -- -D warnings`, `cargo fmt --check`. List each command and result in the PR body.
- [ ] **Step 6: Commit** `feat(jpd): journey approve and flow docs`, PR body `Closes` the phase-1 issue only, Keel card: promise "a journey-flow file validates with stable codes and compiles to a contract the existing readers accept", scope = the files above, proving command = the test set of Step 5.

---

## Later phases (outline; each gets its own plan)

- **Phase 2 — driver and deterministic replay.** `tools/journey-driver/driver.mjs` (protocol `graphhelm-journey-driver/1`, Playwright `getByRole(..., {exact: true})`, `ariaSnapshot`, host guard via `page.route`), shipped by the CLI to `.graphhelm/observers/`; `schemas/journey-replay-cache.schema.json`; `journey replay` recording `jpd.screen_captured` / `jpd.transition_walked` via the existing capture/walked code. Proof: fixture static app on 127.0.0.1, gateway stub that panics on any call, two replays green, `journeys` arrows fresh.
- **Phase 3 — explore.** Agent loop in the CLI over the gateway route; recorded model for tests; fingerprint/dedupe; secret redaction with CLI re-scan; deny list. Proof: golden flow byte-identical across two recorded runs; no secret bytes in the recorded transcript.
- **Phase 4 — drift and heal.** `drift.*` codes, `--heal` bounded to one edge, `status` back to draft. Proof: renamed-button fixture variant.
- **Phase 5 — Studio.** `GET /v1/journeys/flows` + CLI twin; graph view, drift colouring, Approve/Discard. Proof: vitest + CLI JSON test.
- **Phase 6 — scope mapping.** URL pattern → route files per framework table; else `unknown`. Proof: Next.js and Vue fixtures.
