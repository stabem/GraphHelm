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
