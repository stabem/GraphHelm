# What does the current main branch actually support?

This reference audits repository documentation against `main` commit `1b9954f6c85b2c952936a3d3a6df80c5fd7c48e9` on 2026-08-31. It records every stale or missing statement found in the requested corpus and distinguishes current behavior from historical plans and evidence.

Content type: Reference.

## Audit plan and scope

The goal is to identify current-behavior drift, correct files under `docs/`, and leave an exact coordinate for findings outside the permitted edit scope.

The audited corpus contains:

- 171 audited entries under `docs/`, including 86 Markdown files and 153 text or structured-data files. Instrument: `git ls-tree -r --name-only 1b9954f6 -- docs | grep -Ev '(^|/)\.gitattributes$|\.jsonl$' | wc -l` (the raw tree has 198 entries; the filter excludes 10 nested `.gitattributes` files and 17 immutable `.jsonl` acceptance payloads).
- `README.md`
- `MASTER_PRD.md`
- `CHANGELOG.md`
- `DOCUMENTATION_MANIFEST.md`

The audit inventoried every entry, scanned the complete text corpus for the affected contracts, and read each candidate statement against current CLI help, source, tests, catalogs, and the local gate. Binary assets and immutable acceptance payloads were checked as inventory entries, not interpreted as current prose.

Dated acceptance runs, design specifications, implementation plans, and changelog entries describe the state at their stated time. They are not current-behavior claims unless they use an unqualified present-tense assertion. `MASTER_PRD.md` remains a target product contract, not a shipped-feature inventory. No false current-behavior statement was found there. `CHANGELOG.md` is historical and needed no correction.

## Verified current behavior

The following facts came from the checked-out `main`, not from a pending branch:

- `graphhelm schema catalog --catalog schemas/catalog.json` reports release `1.1.0` and 16 schemas. The immutable `1.0.0` baseline has 15 schemas. The conformance manifest has 52 cases.
- Commits that describe an additive `1.2.0` candidate are not ancestors of `main` at the audited SHA. Documentation must not claim `1.2.0` until that work merges.
- `events backup` and `events restore` accept `--repository` for the local filesystem store. Backup refuses non-regular blob entries, and restore requires an empty destination.
- Ordinary pause stops new dispatches in both drivers while in-flight work drains. Immediate pause interrupts in-flight work.
- The public memory proposal operation exists on CLI, HTTP, and Model Context Protocol (MCP) surfaces, but it uses fixed input and persists no memory record.
- Opted-in memory-admission refusals can be persisted as bounded `memory_admission_refused` events containing code, location, and byte count, never rejected content or its digest.
- A memory handoff into a scope without capture opt-in is refused as `handoff_target_not_opted_in`, distinct from refusal of the source project's own capture.
- `gateway probe` refuses a disabled route, and the pure eligibility helper filters disabled routes. Production `serve --route` resolves by route ID and does not enforce the route's `enabled` field.

## Findings at the audited base

Coordinates below refer to `1b9954f6` before this audit changed documentation.

| Coordinate | Finding | Current behavior | Disposition |
|---|---|---|---|
| `README.md:22` | Says nine JSON Schemas run today. | The current catalog has 16 schemas; the immutable baseline has 15. | Recorded only; the PR may not touch files outside `docs/`. |
| `README.md:39` | Says the Context Compiler is not built. | `core/runtime::context_compiler` is implemented and exposed through `development compile-context`; the current CLI slice still uses fixed content. Dreams remains unbuilt. | Recorded only; the PR may not touch files outside `docs/`. |
| `README.md:61` | Repeats the nine-schema claim. | The current catalog has 16 schemas at release `1.1.0`. | Recorded only; the PR may not touch files outside `docs/`. |
| `README.md:79-80` | Shows only PostgreSQL backup and restore, omitting the local store. | Both verbs accept `--repository`; `--config` remains the PostgreSQL path. | Recorded only; the PR may not touch files outside `docs/`. |
| `DOCUMENTATION_MANIFEST.md:3-54` | Lists 47 files, only 24 under `docs/`, and stale byte and word counts. | The audited `docs/` corpus has 171 entries under the instrument declared above; the raw tree has 198, and the root file counts also differ from the table. | Recorded only; the PR may not touch files outside `docs/`. |
| `docs/architecture/SYSTEM_ARCHITECTURE.md:168-170` | Says the closed event set has 20 members, production publishers do not exist, and scheduling, signals, pause/resume, and the operator CLI are missing. | `EventKind` has 44 variants, and production surfaces publish these event families. | Corrected in this PR. |
| `docs/milestones/protocols-and-schema-evolution.md:5` | Says there are nine checked-in schemas and Windows/Linux hosted CI. | The current catalog has 16 schemas; the authoritative gate is local and GitHub Actions is disabled. | Corrected in this PR. |
| `docs/milestones/protocols-and-schema-evolution.md:19` | Says current and baseline catalogs are both `1.0.0` with nine keys. | Current is `1.1.0` with 16 entries; baseline is `1.0.0` with 15 entries. | Corrected in this PR. |
| `docs/milestones/protocols-and-schema-evolution.md:94` | Says the conformance suite has 38 cases. | `conformance/manifest.json` has 52 cases. | Corrected in this PR. |
| `docs/milestones/protocols-and-schema-evolution.md:125` | Repeats nine schemas, 38 cases, and GitHub Actions enforcement. | The local gate checks 16-schema catalog integrity, 52 cases, and both PostgreSQL locales. | Corrected in this PR. |
| `docs/milestones/production-event-evidence-store.md:191-198` | Omits local backup/restore commands and says both operations are PostgreSQL-only. | Local backup and restore use `--repository`; only range verification and rebuild remain PostgreSQL-only. | Corrected in this PR. |
| `docs/operations/OBSERVABILITY_AND_RECOVERY.md:394-416` | Documents only PostgreSQL backup and restore. | Local repository backup and empty-target restore are shipped. Local `events verify --repository` recognizes the repository format but always reports `verified: false`; no local post-restore integrity observer is shipped. | Corrected in this PR. |
| `docs/milestones/graph-engine-governor.md:25` | Says three execution bounds are unused because 04c/04d do not exist. | Scheduling and in-flight governance consume all five declared bounds. | Corrected in this PR. |
| `docs/milestones/name-the-state.md:384-389` | Leaves issue #124 as an unresolved product question. | Ordinary pause now stops new dispatches while in-flight work drains; immediate pause interrupts. | Added a dated resolution without rewriting the historical finding. |
| `docs/harness/NATIVE_DEVELOPMENT_CONTRACTS.md:21-38` | Omits the shipped three-surface memory proposal and durable safe refusal-event slice. | CLI, HTTP, and MCP expose fixed-input proposal parity; opted-in refusal persistence is a separate library contract. | Corrected in this PR. |
| `docs/harness/NATIVE_DEVELOPMENT_CONTRACTS.md:101,528,642-644` | Four unique source citations still use memory-module line numbers from before `handoff_target_not_opted_in` landed. | The secret screen call, refusal construction, and detector now occupy lines 251, 256, and 296 of `core/governor/src/memory.rs`. | Corrected in this PR. |
| `docs/milestones/runtime.md:711-724` | Does not state the now-shipped ordinary-pause dispatch gate. | Both drivers stop new dispatches and preserve the immediate/ordinary distinction. | Corrected in this PR. |
| `docs/milestones/runtime.md:760-761` | Does not state the production gap in route `enabled` enforcement. | Probe and pure eligibility enforce `enabled`; `serve --route` currently does not. | Corrected in this PR. |
| `docs/INDEX.md:1-51` | Has no entry for a current-main behavior audit. | This audit is now the coordinate for implementation drift and root-file findings. | Corrected in this PR. |

## Remaining work outside this PR

The root-file findings remain intentionally open because this PR must touch only `docs/`. A later authorized change can update `README.md` and regenerate `DOCUMENTATION_MANIFEST.md`. The catalog must remain documented as `1.1.0` until a `1.2.0` commit becomes an ancestor of `main`.

At the 2026-08-31 audit of `1b9954f6`, the full local gate found that `extensions/builtin/graphhelm-development-contracts/fixtures/sabotage/s5a-secret-capture/markers.json:5` pointed to `core/governor/src/memory.rs:294` while the detector was at line 296; merge base `e7b98445` corrected that fixture before this branch was published, so this PR leaves no deferred citation work or red-gate condition from that finding.

The production gateway gap is documentation, not an invented workaround: operators must not assume `serve --route` enforces `enabled`. Closing that behavior requires a separate implementation issue and test.
