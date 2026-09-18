# Architect fixtures

Recorded replies for the keyless model door (`RecordedDraftModel`, spec D7). Every file here
has one shape:

```json
{
  "rounds": ["<reply text for round 1>", "<reply text for round 2>", "..."],
  "replies": {"<sha256 of the full assembled prompt>": "<reply text>"}
}
```

- `rounds` is the AUTHORED half: the text a model is pretended to have answered on each round,
  written by hand for the test that owns the file. A single entry means "the same text on every
  round". The compiler never reads this key.
- `replies` is the DERIVED half, and the only key `RecordedDraftModel` reads: the same texts,
  filed under the sha256 of the exact prompt each round assembled. Round 2's prompt embeds round
  1's draft and the compiler's own diagnostics for it, so these keys cannot be computed without
  running the compiler.

## Recording

The template is versioned by its content hash and that hash is substituted into every prompt
(D6), so editing `core/architect/src/template.rs` moves every key at once: the golden suite
then fails with `FixtureMissing { prompt_sha256 }` naming the hash it needed. That is the named
event the design asks for, not drift. To re-record every file from its `rounds`:

```powershell
$env:ARCHITECT_RECORD = "1"
cargo +1.97.1 test -p graphhelm-architect --test golden --locked
Remove-Item Env:ARCHITECT_RECORD
cargo +1.97.1 test -p graphhelm-architect --test golden --locked
```

The recorder does what an operator does by hand: it asks the compiler with an empty recording,
reads the hash from the `FixtureMissing` refusal, files that round's text under it, and asks
again, at most `MAX_REPAIR_ROUNDS + 1` times. The second run (without the variable) proves the
committed files answer. `first-compile/expected.json` is rewritten by the same variable and
must be reviewed in the commit like any golden.

To record a NEW case: add a file with `rounds` and an empty `replies`, add its test, and run the
two commands above.

## Files

- `first-compile/GOAL.txt` is the goal of spec §4, read by the crate test and by the CLI test
  so one string exists. `first-compile/replies.json` is the golden reply for it, and
  `first-compile/expected.json` the byte-stable document it compiles to.
- `sabotage/*.json`: one case per refusal arm, and one per repairable diagnostic shape (a
  schema break, the three tool-call shapes the broker's parser refuses, a budget above the
  profile). Each is refused under its OWN diagnostic, and a refusal that lands in a neighbouring
  arm is a test failure.
- `judge/*.json`: the judge door (`RecordedJudgeModel`, spec D5) for `tests/judgment_nodes.rs`.
  A judge fixture has the same two halves as a draft fixture, keyed by the request digest:

  ```json
  {
    "rounds": [{"model": "jev-latest", "answers": {"<question id>": {"type": "noul", "noul": 0.1}}, "usage": {"input_tokens": 1, "output_tokens": 1}}],
    "answers": {"<sha256 of the canonical JudgeRequest JSON>": {"<the same judge reply>": "..."}}
  }
  ```

  `rounds` is the AUTHORED half (one judge reply per judge call, in order; the compiler never
  reads it); `answers` is the DERIVED half and the only key the door reads. The recorder in
  `tests/judgment_nodes.rs` files both a draft fixture's `rounds` and a judge fixture's `rounds`
  in one loop, reading the digest each `FixtureMissing` / `JudgeMissing` refusal names, under
  the same `ARCHITECT_RECORD=1` (run it with `--test judgment_nodes`). The draft fixture a judge
  case pairs with lives beside it when it differs from the golden; the golden
  `first-compile/replies.json` is rewritten only by `golden.rs`.
  - `nodes-off-goal.json` + `nodes-off-goal-replies.json`: round 1 judges `summarize` off goal
    (`noul` 0.10, a `GHA005_NODE_OFF_GOAL` fed back for repair); round 2 is the golden draft with
    that node's objective rewritten, judged on goal by every answer.
  - `nodes-kind-mismatch.json` + `nodes-kind-mismatch-replies.json`: round 1 judges the agent
    node `summarize` as `tool` at confidence 0.90, everything else on goal (a
    `GHA006_NODE_KIND_MISMATCH` fed back for repair); round 2 is the golden draft with that
    node's objective rewritten, judged `agent` by the same confidence.
  - `nodes-below-threshold.json` (paired with the golden draft): `kind:summarize` answers `tool`
    at confidence 0.60, under the acting threshold; the document is the golden and the node is
    reported `unresolved`.
    - **LOAD-BEARING ELSEWHERE.**
      `tests/library.rs::an_empty_library_with_a_judge_costs_no_judge_request_and_says_nothing`
      proves "no decision request" by this file holding NO decision-shaped answer (one digest,
      `kind:*`/`on_goal:*` keys only); adding one silently greens that cell.
  - `ranking-three-replies.json` (for `tests/judgment_ranking.rs`, recorded with
    `--test judgment_ranking`): one authored draft per stance of `Stance::ALL`, in order — the
    golden draft without `summarize` (`minimal`), the golden draft (`verified`), the golden draft
    with `summarize` split into two agent nodes (`explicit`). Each passes the whole deterministic
    chain, so the three `replies` keys are the three stance prompts and nothing is repaired.
  - `ranking-three.json`: four authored judge replies, in the order the compiler asks — the
    per-node judgments of each draft (every node on goal, every kind matching, so no draft is
    repaired) and then the ONE ranking reply, where candidate 2 scores `coverage` 2.0 at
    confidence 0.9 and the others lower, waste at most 0.1 everywhere; candidate 2 (`explicit`)
    is chosen.
  - `ranking-unresolved.json`: the same four replies except the top candidate's confidence is
    0.50, under the acting threshold; candidate 0 (`minimal`, today's road) is kept and the
    report says `unresolved`.
  - `reuse-*.json` (for `tests/library.rs`, recorded with `--test library`): the road decision
    over `fixtures/library/` (spec D8), then what the road asks next. `reuse-reuse.json`: road
    `reuse` (0.92), template `build-and-summarize` (0.90), then the fill (`program` `cargo`
    0.95, `audience` `maintainer` 0.88) — no draft is asked. `reuse-npm.json`: the same with
    `program` `npm`, which the catalog does not allow, so the filled template is
    `CapabilityMissing`. `reuse-adapt.json` + `reuse-adapt-replies.json`: road `adapt` (0.90);
    the draft prompt carries the template in a `<seed>` block, so the reply (the golden draft
    text) is filed under its own key; then the per-node judgments. `reuse-create.json`: road
    `create` (0.85), then the per-node judgments of the golden draft, drafted through
    `first-compile/replies.json` (never rewritten here). `reuse-unsure.json`: road `reuse` at
    0.55, under the acting threshold, so `create` is taken and the report says `unresolved`.
- `library/`: the fixture graph library of spec D8, two templates. Each is an authored YAML
  document (`<name>.yaml`, `metadata.labels.origin: library`, no `completion.customs` — the
  compiler stamps those) and a sidecar `<name>.template.json` declaring
  `{id, summary, parameters: {<name>: {question, options: {<value>: description}}}}`; a value
  substitutes `{{name}}` in the document's string leaves. `build-and-summarize` is the golden
  document with the shell program as `{{program}}` and the summary's audience as
  `{{audience}}`; `run-tests` is one `tests` call with `{{program}}` only.
