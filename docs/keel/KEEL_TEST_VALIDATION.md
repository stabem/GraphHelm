# `graphhelm keel test validation`: journey execution evidence

```sh
graphhelm --json keel test validation --repo <project>
```

This command executes the paths declared in `.graphhelm/journeys/*.journey.yaml`, including draft
flows, and collects JavaScript function coverage from Chromium. It helps investigate code that
journeys do not exercise. It does **not** prove that code is dead and never deletes code.

## Execution boundary

The project must have Git source files, valid journey flows, the existing Playwright journey
driver, and a launcher declaring `isolated: true`. Each flow gets a fresh fixture and each path
gets a fresh browser. Paths in the same flow share fixture state. Flows run serially with the
existing preview deadline and owned-process cleanup. This executes the project's launcher and
journey actions: use a disposable test fixture, not production credentials or data.

Validation uses its own temporary result directories. It does not reuse the ordinary preview
cache, approve drafts, confirm held destructive actions, or record Runtime proof events. Standalone
compiled JSON contracts are inventoried as unsupported; they are not silently counted as executed.
This command runs journey paths, not every unit test whose name mentions JPD.

## Reading the report

- `used`: a collected generated JavaScript function had a positive execution count. Its source
  association matches the hash of a current repository file.
- `notObserved`: a loaded generated function had no observed calls in the complete collected
  sweep. This is a review candidate, not permission to remove the source declaration.
- `unknown`: execution or source attribution could not be established. Backend Rust, CSS, workers,
  missing scripts and unsupported source maps cannot be classified by browser coverage.
- `paths` and `failures`: execution and collection outcomes, including failed or held paths. An
  incomplete sweep cannot provide a global negative conclusion.
- `declaredScopes`: what the flow author associated with a screen. This is separate from measured
  execution and is never counted as coverage.
- `deadConfirmed`: always empty. Removal needs a separate caller and contract review.

Function names and offsets refer to **generated JavaScript**, not original TypeScript declarations
or lines. Source attribution uses content hashes: one generated script may associate with one
uniquely matching current source file. Raw JavaScript can match directly. Inline source maps can
supply a single original source hash; external, malformed, multiple-source and stale maps remain
unknown. Duplicate identical source files are ambiguous. Source changes during execution invalidate
current-source attribution.

Coverage collection is bounded by the artifact-wide function and range limits. The driver keeps
all valid ranges until that global bound is reached; it does not impose a smaller per-function
range cap. Truncation or navigation beyond the initial document load makes negative evidence
ineligible, while valid positive observations remain useful. Browser coverage does not measure
native/backend execution or guarantee every dynamic entry point was exercised.
Artifacts retain hashes and function ranges, not source text, map text or map paths. External maps
are never fetched for attribution. Capture completeness and source association are separate
decisions: a complete artifact can still have an ambiguous source association. If a source hash
appears in any multi-source script in any artifact or path, a zero-hit candidate with that hash is
reported as `unknown` with reason `ambiguous_source_mapping`; it is never reported as
`notObserved`. Positive observations remain in `used`, and the multi-source script itself remains
unknown when no unique association can be established.

## Deciding whether to remove code

First fix broken journeys or missing collection. For each remaining candidate, inspect production
callers, dynamic registrations, supported platforms, public APIs and persisted contracts. Add a
missing journey when the behavior should still exist; remove the code only when those checks show
it has no required consumer. Record the evidence in the PR's `Retirement:` section.

This command supplements the [Keel retirement rule](KEEL_SPEC.md); it does not change
[`keel check`](KEEL_CHECK.md) into a dead-code detector. A clean report proves only the declared
journeys and the collection limits reported for that run.

## Sabotage proof: `keel test sabotage`

```sh
graphhelm --json keel test sabotage --repo <project> --patch <break.diff>
```

Validation shows that journeys run; sabotage shows that they would notice a broken product. In a
temporary detached `git worktree` of HEAD (the working tree is never touched, and the worktree is
always removed), the command runs the same journey validation, applies the patch with `git apply`,
and runs it again. Green means every declared path completed; coverage completeness is not
required. The `verdict` is:

- `caught` (exit 0): the same declared paths ran after the patch and at least one executed and
  failed (`failed`, or `drift` when an expected screen was missing); `failing` lists them.
- `missed`: every path stayed green. The journeys do not observe the patched code.
- `no_journey_ran`: after the patch there was no well-formed validation report, or its paths were
  only `not_run` or `held`. Nothing observed the break.
- `journey_set_changed`: the patch changed which journey paths are declared.
- `patch_touches_observer`: the patch edits `.graphhelm/` (journeys or observers). Sabotage must
  break the product, not the test. Checked with `git apply --numstat` before anything runs.
- `baseline_not_green`: validation was not green before the patch.
- `patch_not_applicable`: `git apply` refused the patch at HEAD.

Every verdict except `caught` exits 2. The worktree gets copies of the ignored
`.graphhelm/observers/` files and a link to the project's `node_modules`, so the journeys can run.

Only committed HEAD is tested; uncommitted edits are not in the worktree.
