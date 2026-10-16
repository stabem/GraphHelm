# Archive names

Code comments and specifications cite design notes of the old process by the short names below.
Those notes (under `.factory/`, `.superpowers/` and `docs/superpowers/plans/`) were removed on
2026-09-24 together with the gate runner, the gate queue, `merge-proof` and the committed gate
receipts. Commit `c080739b` that last held them belongs to the private development archive
([source provenance](../open-source/SOURCE_PROVENANCE.md)); a public clone cannot read it. This
table keeps each name resolvable to the path it had.

| Name used in comments | Path at `c080739b` (private archive) |
|---|---|
| gateway-slice plan | `docs/superpowers/plans/2026-08-14-gateway-slice.md` |
| architect-judgments plan | `docs/superpowers/plans/2026-09-16-architect-judgments.md` |
| real-executor plan | `docs/superpowers/plans/2026-08-14-real-executor.md` |
| chat-surface plan | `docs/superpowers/plans/2026-08-14-chat-surface.md` |
| public-runtime-api plan | `docs/superpowers/plans/2026-08-13-public-runtime-api.md` |
| gate-under-five-minutes plan | `docs/superpowers/plans/2026-09-19-gate-under-five-minutes.md` |
| program plan index | `docs/superpowers/plans/2026-08-08-graphhelm-program-plan-index.md` |
| foundation-graph-kernel plan | `docs/superpowers/plans/2026-08-08-graphhelm-foundation-graph-kernel.md` |
| setup-backup-restore plan | `docs/superpowers/plans/2026-09-21-setup-backup-restore.md` |
| journey-proof-pilot plan | `docs/superpowers/plans/2026-09-21-journey-proof-pilot.md` |
| structured-agent-context plan | `docs/superpowers/plans/2026-09-21-structured-agent-context.md` |
| economic-route-selection plan | `docs/superpowers/plans/2026-09-21-economic-route-selection.md` |
| event-evidence-store SDD record | `.superpowers/sdd/2026-08-09-production-event-evidence-store/` |
| merge checklist | `.factory/MERGE-CHECKLIST.md` |
| lane loop | `.factory/lane-loop.md` |
| design note #107 | `.factory/b-agent-107-blueprint.md` |
| design note #119 | `.factory/c-agent-119-design.md` |
| design note #159 | `.factory/b-agent-159-blueprint.md` |
| design note #200 | `.factory/e-agent-200-design.md` |
| design note #211 | `.factory/h-agent-211-surface-blueprint.md` |
| design note #213 | `.factory/e-agent-213-blueprint.md` |
| design note #219 | `.factory/h-agent-219-blueprint.md` |
| design note #221 | `.factory/e-agent-221-refusal-code-gap.md` |
| scope amendment 5 (#222) | `.factory/b-agent-222-scope-amendment-5.md` |
| design note #285 | `.factory/e-agent-285-blueprint.md` |
| mode-semantics note (#89) | `.factory/d-agent-mode-semantics.md` |

Two cited notes were never committed to `main`, so no commit holds them: the owner-output
blueprint (`.factory/e-agent-221-blueprint.md`) and the #226 sabotage expectation record
(`.factory/n-agent-226-sabotage-expectations.md`). The findings drawn from them are in the
issues and pull requests those comments name.

The subsystem specifications that lived under `docs/superpowers/specs/` moved to
[`docs/specs/`](../specs/); the Keel specification moved to
[`docs/keel/KEEL_SPEC.md`](../keel/KEEL_SPEC.md). Historical records (ADRs, the decision register,
milestone and acceptance notes, the changelog) keep their original paths as written.
