# Architect judgments — the keyed Tier B recipe (#1109)

The Tier A cells of `docs/harness/GRAPH_ARCHITECT.md` §10.9 prove the road: recorded answers
compile to byte-stable documents, every refusal has its own kind, the thresholds have a cell on
each side. They prove nothing about whether the judge's answers track what a person would say.
That is Tier B (§6), and this page is the recipe for measuring it with real Jev. The table at the
end is the record a threshold change in `core/architect/src/judgment/policy.rs` MUST cite (spec
D6, decision D-054); a run is filed as `docs/acceptance/architect-judgments-<date>.md` carrying
this table filled, the head it ran against, and the binary's build line, the way
`m11-first-compile-2026-09-11.md` does.

## Prerequisites

1. **The judge route and its key, in one command** (#1139), in a project `graphhelm init`
   provisioned:

   ```sh
   graphhelm gateway setup --provider typesafe
   ```

   It writes the `judge` route — exactly as
   `core/gateway/tests/manifest_contract.rs::typesafe_is_a_legal_direct_api_provider` pins it —
   into `.graphhelm/manifest.json` beside whatever chat route the draft door uses (a second
   `setup --provider anthropic --model <name>` or a hand-written route), asks for the key once
   (hidden prompt, or piped on stdin), stores it in the Credential Broker under `secret_typesafe`,
   and probes the route. The manifest, the goal files and this record carry the reference
   `secret_typesafe`, never the key. A run whose transcript shows a key is not filed.
2. **A library directory** (`<dir>` below) with at least one template beside its
   `.template.json` sidecar (§10.7). The fixture library `core/architect/fixtures/library/` is
   test data; copy it or author templates for the goals — there is no default directory and no
   bundled catalog. A run without a library measures sites 3 and 2 only and says so.
3. **The program allowlist per goal.** Every goal that needs a program passes it explicitly
   (`--allow-program cargo`); the allowlist has no default (D-052), and a filled template naming
   a program outside it is refused, not measured.
4. **The goal set, pre-registered.** Write the ten goals into the table BEFORE the first call.
   Goals 1 and 2 are the two the suite already records (`core/architect/fixtures/first-compile/GOAL.txt`,
   the goal of `m11-first-compile-2026-09-11.md`, and `core/architect/fixtures/useful-change/GOAL.txt`);
   the remaining eight are the operator's, chosen to vary what the recorded ones never vary (a
   goal that needs `npm` or `make`, a goal a template covers exactly, a goal no template covers,
   a goal with two stated outcomes, a goal with one). A goal added after an answer was seen is a
   different run.

## The invocation, per goal

```sh
graphhelm graph synthesize \
  --goal "<goal text>" \
  --allow-program cargo \
  --manifest .graphhelm/manifest.json --route <draft route> \
  --judge-route judge \
  --broker .graphhelm/broker --keyring .graphhelm/keyring --key-id studio \
  --drafts 3 \
  --library <dir> \
  --out g<N>.json
```

One command per goal, `--out` a fresh path each time (an existing file is never overwritten).
Keep the full JSON the command prints: `judgments`, `ranking` and `reuse` are the measurement,
and the `usage` fields are the cost. Then, for every goal, `graphhelm graph lint g<N>.json`
must report zero `GHG102` (D-051) — a document that fails here is a defect, not a data point.

## What to read off each run

| field | where | what it says |
|---|---|---|
| road | `reuse.road` (`reuse`, `adapt`, `create`); absent when no library or no template | which of the three roads site 4 took; `reuse.template` names the template, `reuse.parameters` the values site 1 filled |
| chosen stance | `ranking.chosen` and `metadata.labels.stance` (`minimal`, `verified`, `explicit`) | which of the three drafts site 2 returned |
| `unresolved` counts | `judgments.unresolved.len()`, `ranking.unresolved`, `reuse.unresolved` | how often an answer fell under a threshold and nothing was done |
| a person's yes/no per `on_goal` | `judgments.nodes[*].onGoal` against the person's own answer to "does this node do work the goal needs?" | the semantic question the thresholds encode |

The person answers each `on_goal` question BEFORE reading the judge's number for that node
(write the yes/no column first, then fill the probability). A yes with `onGoal` at or above
`NOUL_YES_THRESHOLD`, or a no at or below `NOUL_NO_THRESHOLD`, is agreement; a value between the
two is unresolved and counts as neither; the rest is disagreement.

## The table to fill

One row per goal. `nodes` is the accepted document's node count; `agree / disagree / unresolved`
are the `on_goal` tallies over those nodes; `rounds` is `rounds` as printed.

| # | goal | allow-program | road | template | chosen stance | rounds | nodes | agree | disagree | unresolved (`judgments`) | `ranking.unresolved` | `reuse.unresolved` | lint `GHG102` |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | `check that the repository builds and summarize the result` | `cargo` | | | | | | | | | | | |
| 2 | `prove that src/lib.rs defines the FIXED constant by running the repository tests` | `cargo` | | | | | | | | | | | |
| 3 | | | | | | | | | | | | | |
| 4 | | | | | | | | | | | | | |
| 5 | | | | | | | | | | | | | |
| 6 | | | | | | | | | | | | | |
| 7 | | | | | | | | | | | | | |
| 8 | | | | | | | | | | | | | |
| 9 | | | | | | | | | | | | | |
| 10 | | | | | | | | | | | | | |

Beneath the table the run records: the head sha, the build line, the manifest with the key
reference (never the key), the library directory listing, the summed `usage` over the ten runs,
and the model string the replies carried (`jev-latest` resolves to a dated model on the
provider's side; the reply's `model` is the one to file).

## What a threshold change must show against this table

- Which constant moves, from which value to which, and which rows of the table it moves
  (the `unresolved` count that becomes an act, or the disagreement that becomes an abstention).
- The new value's two cells (`value - ε`, `value + ε`) in `core/architect/tests/judgment_nodes.rs::policy_edges_are_exact`.
- No row where the change turns a person's "no" into an acted "yes" (`GHA005` is repairable,
  so a false "no" costs a round; a false "yes" costs a node nobody wanted and is the direction
  the conservative start guards against).

A table with fewer than ten rows, or a row whose yes/no was written after the number was read,
does not support a change.
