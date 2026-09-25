# GraphHelm JPD Plugin Implementation Plan

> Issue: #210
>
> Rule: execute this plan through GraphHelm public contracts where they exist. A missing contract is
> recorded as a typed gap, not bypassed through Runtime internals.

## Goal

Ship a validated built-in Journey-Proven Development data bundle, eight entry skills, typed persona
and contract resources, deterministic extension-package validation, and a contract-level GraphHelm
dogfood graph with deterministic simulation fixtures.

## Architecture

The existing `Extension` manifest remains the only plugin envelope. `graphhelm-jpd` declares all
resources under its open `spec.contracts` extension point. A new `graphhelm extension validate`
command validates the embedded extension schema, bounded package-relative resources, digests,
skill frontmatter, declared public surfaces, schemas, agents, and graph documents. It validates and
reports; it does not install or activate.

## Work packages

### 1. Decision and repository guidance

- Add D-041 and ADR-027.
- Replace universal TDD wording in `AGENTS.md` with JPD assurance selection.
- Link the new specification from `docs/INDEX.md`.

### 2. Generic extension validation

- Embed `extension.schema.json` in `graphhelm-schema`.
- Add bounded `load_extension` and public validators for extension and agent documents.
- Add `graphhelm extension validate <package>`.
- Validate safe relative paths, regular files, size bounds, SHA-256 digests, unique ids and paths,
  known contribution kinds, skill frontmatter, public-surface markers, inline schemas, agents, and
  graphs.
- Return stable `GHEX*` diagnostics and one JSON envelope.

### 3. Built-in JPD bundle

- Create `extensions/builtin/graphhelm-jpd/extension.json`.
- Add Claude and Codex host manifests that launch the canonical absolute `${GRAPHHELM_CLI}` path
  with a token file and no PATH/current-directory fallback.
- Add the eight entry skills.
- Add seven persona definitions, strict schemas for the journey and every declarative policy,
  evaluator, and observer contract, plus positive/negative fixtures.
- Record every resource digest in the extension manifest.

### 4. Dogfood journey

- Add a task-specific execution graph and deterministic fixture outcomes for validating this bundle.
- Run `graph validate`, `graph lint`, `graph hash`, `graph simulate`, and `graph replay`.
- Exercise the MCP surface against the Public Runtime API where the current local runtime supports
  it. Record that the graph simulation proves Graph DSL/package contracts, not execution of skill
  instruction files or external observers.

### 5. Conformance and review

- Add focused schema and CLI tests for valid, tampered, escaped, unsupported-surface, invalid-skill,
  `OBSERVER_MISSING`, and retry-lineage cases.
- Run skill and plugin validators supplied by the Codex skill tooling.
- Run targeted Rust tests, then the complete `./ci/gate.ps1`.
- Review the full diff for secret exposure, path traversal, symlink escape, ambiguous activation,
  direct Runtime imports, and false success claims.

## Explicit non-goals

- General extension installation or registry resolution.
- Executing untrusted plugin code in-process.
- Shipping a browser automation backend.
- Every Project Skill install, activation, or publication path, whether manual or automatic.
- Remote marketplace, signing service, billing, or telemetry.
