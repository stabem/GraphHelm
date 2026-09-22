# Lane loop — the standing order for every GraphHelm lane (owner, 2026-09-05 22:1xZ: "everything merged; every agent on /loop resolving all the issues")

Run this as your own `/loop` (dynamic, ~20-30 min). Every turn, in this order. Identity line first on everything you publish (`Lane: X · Session: <ListAgents name> · Head: <sha8>`). Rules live in `AGENTS.md` and `.factory/MERGE-CHECKLIST.md` on `origin/main`; this file only sequences them.

## 0. The order the lanes run in (owner's restructure, 2026-09-08)

**A plans -> B implements -> C runs the gate -> D reviews THE HEAD THAT CARRIES the manifest -> E
presses.** Not the head the manifest NAMES: on a normally published receipt the head IS the receipt
commit and its manifest names the head's PARENT, so "pin what the manifest names" would pin every
reviewer one commit behind the PR. A pass pins the head it READ; the manifest's `headSha` is the tree
the gate ran; THE TWO MANIFEST QUESTIONS (canonical block in `.factory/MERGE-CHECKLIST.md`) is what
links the two.
The gate runs BEFORE the passes, because `ci/gate.ps1` commits its manifest under `.factory/gate-runs/`
onto the branch it judged: running it MOVES the head, so a pass written before it names a sha the PR no
longer has and has to be re-pinned. Gating first is what stops the re-pin loop that left twelve PRs
"ready" on 2026-09-07 with no gate at the current head. Step 1 below used to read in the other order;
this section is what it defers to.

**After the passes there are no pushes.** A new finding on an otherwise clean PR is a DECLARED GAP in
the body of the PR that owns the code, carrying its measurement and saying in as many words that it is
NOT being filed anywhere. **No new issue and no follow-up PR** - the owner's order of 2026-09-05, given
twice that day ("why do the PRs and the issues keep growing", "the issues and PRs only grow, nobody is
merging"): writing a finding is cheap and landing it is expensive, and every new tracker competes for
the same gate slots and the same reviewers. The board that produced that order is the board today - 41
open PRs. If a finding genuinely needs its own tracker, say THAT in the comment and let the orchestrator
or the owner decide. **What still authorises moving the head is the three-condition rule below, and
nothing else** - this paragraph deliberately does not restate it. It listed two of the three until
Codex read the two lists against each other on this PR: a finding that met only the third was
authorised by one paragraph and forbidden by the other. Measured at `fe2d6aa4`, where the defect
lived: lines 21 and 55 of a 211-line file - **34 apart, one screen** - and a third partial copy of the
same list sat in `AGENTS.md`. Two reviewers and the author read past all three; the distance was never
what hid them. (The first telling of this said "130 lines", a number nobody had measured. In a file
whose subject is measuring instead of remembering, that is the worst place to put a remembered
number.) One
rule, one place; a second copy is a second thing to correct, and whichever copy is corrected last
starts lying. That is what makes a pass hold until the merge instead
of dying at the next push. **A docs-only PR is not exempt**: if its body or its diff changes after a
pass, that pass died with it and is requested again.

**Name a gap WITHOUT a closing keyword.** The squash reads the PR title and the commit messages, and the
parser does not read negation: `follow-up: closes #NNNN` beside a number that must stay open CLOSES it,
and a `closes` already pushed cannot be undone by editing the body (measured on #1016). Write `see #N`,
`follow-up in #N`, or the bare number; `closes`/`fixes`/`resolves` stays reserved for what the PR really
must close.

**Triage by the manifest commit, and by THE TWO MANIFEST QUESTIONS** - the canonical block at the top
of `.factory/MERGE-CHECKLIST.md`, which states both rules and the `--name-status` check. It is not
restated here, and it was not restated here five times over: each reformulation drew another review
round, and the rounds stopped only when the statements were replaced by pointers.

What belongs to THIS file is the measurement that makes triage necessary at all: a lane triaging from
`SLOT.log`, or from any run-end record keyed by the head sha, reads **zero** gates across the whole
board and concludes nothing is ready - measured by lane A on 2026-09-08 over 41 open PRs, where the
correct answer was four. The head is the RECEIPT and the run is below it.
**An automated reviewer's observation earns a PUSH under three conditions and no others:** (a) it
reddens a stage of the gate, (b) it is a false GREEN inside the PR's own mechanism, or (c) a path
exists in the tree TODAY that produces the wrong behaviour. None of the three: resolve the thread with
the measurement written into it and declare the gap in the body. A P2 suggestion with no live
reproduction does not hold the button, and a bot that answers each correction with two more edges has
an effective veto over the merge unless this rule is applied.

**Subagent lanes (orchestrator, 2026-09-11).** A lane may be an independent subagent with a fresh
context, spawned by an orchestrating session: the implementer subagents are the AUTHOR, two
reviewer subagents with no implementation context are the two PASSES, the registered runner is
the GATE, and the spawning session — which planned and wrote no code — PRESSES. The identity
line names the subagent and its spawning session so the comment is addressable, in the ONE form
`AGENTS.md` specifies - the parenthetical sits in the LANE field, because a census reads that field
and is told to treat `Session:` as an address rather than an identity:

`Lane: <letter> (subagent <name> of <session> [ref], <author|non-author> for this PR) · Session: <ListAgents name> [ref] · Head: <sha8>`

**One physical line, and kept as one** - this template is copied and it is grepped for, and a wrap
inserted by an editor is pasted into the middle of the string a census matches.
The spawning session may not review what its subagents wrote; whether it may PRESS is decided by
item 12 of `.factory/MERGE-CHECKLIST.md` under the subagent exception, and is not restated here.

**Two classes of finding, and they close differently.** A SPELLING finding closes **by form**: one
normalization in front of the predicates, and a new spelling is answered by citing the form rather than
by adding a case. A REACHABILITY finding does not - a predicate that names what an observer skips is a
deny-list, and a deny-list is closed only against the arms it enumerates. Measured on this workspace's
walk, which declines to read a file in **four** ways:

```
walk.rs:308-309   name in WORKSPACE_WALK_SKIPPED -> continue   named by the sweep's predicate
walk.rs:301-302   kind.is_symlink()              -> continue   not named
walk.rs:298-299   entry.file_type() -> Err       -> continue   not named; silent; loses ONE ENTRY
walk.rs:279-280   read_dir(start)   -> Err       -> return     not named; silent; loses THE SUBTREE
```

A reachability finding is therefore a legitimate sibling, never an invented edge, and it is retired by
MEASUREMENT rather than by citing a form. The form that would close such a class is to stop predicting
what the observer skips and ASK it - compare the declared thing against what the observer actually
returned.

**Criterion (c) is a question about the TREE, and only about the tree.** A gap whose subject is a
RUNTIME CONDITION - a permission, a checkout in flight, an I/O error - is not retired by counting
files: the count returns **nothing**, not zero, and writing that nothing down as a zero fabricates an
absence. Declare such a gap by naming **what would notice it**. If almost nothing would, THAT is the
finding and not a detail of how it was written down: an absence guard whose only alarm is a coarse
floor cannot fire on the failure it would itself suffer.

**Send another lane the TEST, not the verdict.** "This class is closed, cite the closure" is a
conclusion; "measure whether a path exists today" is a test the receiver can run and can fail. A lane
that applies a conclusion faithfully files a true observation under a false reason, which is worse than
not filing it - and an instruction that only works when its receiver distrusts it is the wrong
instruction to spread. This applies to lists as much as to rulings: a list that arrives in a message is
re-derived from the file before it is used (the four arms above were three until someone opened
`walk.rs` instead of trusting the message that carried them).

**Three shapes DELETE, and none of them looks like deleting. Sweep after each one.**

| shape | what it looks like | what it deletes |
|---|---|---|
| an EDIT | correcting | the wording it replaced |
| a CONSOLIDATION | tidying | N-1 of the N sites, and every pointer into them |
| a RETIREMENT | cleaning | everything that lived ONLY in the retired file |

After any of the three, sweep for **who named what is gone** - and OUTSIDE the file you edited, which
is where both of this PR's own instances escaped (a heading kept pointing at a removed table; a
consolidation left `AGENTS.md` delegating to rules it had deleted). Before RETIRING a file, sweep it
for what exists nowhere else and move that out FIRST: the board's tombstone would have taken the
`--body-file` rule and `git add` per file with it, both of which the presser uses on every merge.

**"Docs-only" is a DECISION we make, never a reading of the instrument.** Measured at `68519ce2` (C,
reproduced here on this PR's own four files):

```
ci/select-scope.ps1 -ChangedFiles '.factory/lane-loop.md'
{"escalated":true,"escalationRule":"unmapped-path","crates":[],"matrix":true,"rustInputsChanged":true}
```

The selector has NO documentation exemption: the only prefix it excuses is `.factory/gate-runs/`
(`ci/select-scope.ps1:232`), and `README.md` and `docs/milestones/name-the-state.md` escalate to FULL
the same way. So the two halves everyone pasted together come apart: *"no binary reads this file"* is
true and measurable - `git grep -n lane-loop <sha> -- ':!*.md'` returns **zero** - *"therefore the
gate ignores it"* is FALSE - the cheapest change asks for the most expensive run. Write the exemption as
what it is: nothing reads the file, the selector would escalate to FULL, and we choose not to spend a
slot on it. A body that states the second half as a consequence of the first is grounds for a BLOCK.

**The exclusion is not decoration.** The bare repository-wide grep does NOT return zero any more: this
PR added Markdown cross-references in `AGENTS.md`, `.factory/MERGE-CHECKLIST.md` and the tombstone, and
at `d862177e` the unfiltered grep returns seven. All seven are prose citations; `':!*.md'` is what
separates a consumer from a mention, and a lane that runs the unfiltered form cannot reproduce the
exemption this measurement justifies. The measurement was true when first written and MY OWN edits
falsified it - a claim about a repository decays fastest when its author keeps editing that repository.

## 1. Land what is assigned to you (NOT from a table in this file, and NOT from the board - it is a tombstone, see `.factory/orchestrator-board.md`, and assignments come from the execution record and the current orchestrator)
- Read the PR in the same breath as acting: `gh pr view N --json headRefOid,mergeable,mergeStateStatus,closingIssuesReferences` (ask `mergeable` twice), **THREE surfaces** paginated (`issues/N/comments`, `pulls/N/reviews`, and **`pulls/N/comments`** — the inline threads, where Codex's findings live; the reviews box shows only their headers, and "both boxes" is what let three P1s go unread by two reviewers on #996), passes counted by CONTENT at the head (lane ≠ author, sha named, verdict word; a criteria seal is not a pass; a rebase kills passes; gate manifests carry them under THE TWO MANIFEST QUESTIONS - the canonical block at the top of `.factory/MERGE-CHECKLIST.md`, not restated here. The "manifest-only TIP" wording this line used to carry is retired, for the reason in section 0).
- CONFLICTING → rebase first, with a net: save the old head in a ref of yours, `--force-with-lease`, verify the rebase by content, post the recovery command; then request two fresh passes.
- Passes missing → **first check the gate has run on THIS head** (section 0: the manifest commit moves it), then request them by name in the PR (lanes with 0 bodies in the thread), and give passes others request from you (verdict word + sha, in the body).
- Manifest missing → run the gate: bench on a SHORT path on the SSD (`E:/<lane>-<n>`, **~40 MB each — the cost is file operations, not bytes**: a checkout is ~1,550 files, and fourteen at once on one platter is a head-seek storm, not a volume problem; measured by L and re-measured here, 1,551 files / 40.2 MB. Remove it when the PR merges; never a session scratchpad, never F:/C: for targets). **`D:` is the HDD gate target's disk and the runner benches', not yours**: measured by L (relayed 2026-09-07T23:4xZ; the reading itself carried no clock), 14 concurrent `git` checkouts on `D:` put its disk time at 536% with two running 19 minutes, while `E:` sat at 5%. **Never start a second checkout while one of yours is running**, and never kill another lane's — a checkout killed mid-way leaves an `index.lock` and a tree that reports 1,500 files deleted (D, this PR's own bench). Checkouts already on `D:` finish where they are. Then, on the bench: `git symbolic-ref HEAD` set (not detached), upstream = the PR's branch (`--set-upstream-to`, or `pushed` comes back null), slot through `.factory/tools/slot-claim.sh` (HDD lock `D:/graphhelm-slot/SLOT.lock`; SSD via `SLOT_LOCK=E:/graphhelm-slot/SLOT.lock`; **and export `GRAPHHELM_SLOT_LOCK_PATH` to that SAME path before launching `ci/gate.ps1`** -- the claim is a subprocess and cannot export its choice to you, and the gate reads only that variable (`ci/slot-lock.ps1:345`), so claiming E: without forwarding it holds E: while the gate takes D:; the claim now REFUSES that split with `exit 6`; launch only on exit 0; the wrapper's own pid pair; E: floor 30 GB), FRESH target per run, proof-of-life outside the worktree, push the manifest commit, verdict read from the JSON (reds named test by test against the known-reds list; arm 1 by file/mechanism, arm 2 by existence on a PR-less tree).
- Scope, before you spend the slot: derive it (`ci/select-scope.ps1 -MergeBase (git merge-base origin/main HEAD) -Head HEAD`, last line only), write it OUTSIDE the bench, read what it decided, then `./ci/gate.ps1 -ScopeSelection <file>`. The runner does this for queued entries; a hand launch does not, and a run without it is a FULL run that must be reported as one. Recipe and reasons: `.factory/MERGE-CHECKLIST.md` item 2.
- Press when: two passes at the head + a manifest naming the PR whose reds are all on the list (or GREEN) + you are neither author nor reviewer (filter both boxes by YOUR letter first) — or, when EVERY non-author lane already holds a verdict on the PR, you are the reviewer whose pass at the head has the EARLIEST issue-comment timestamp (`AGENTS.md` exhaustion clause; `MERGE-CHECKLIST` item 12 is the census that proves the set empty, pasted into the merge comment) + four closing readings' union == intent (no keyword inside backticks; `closingIssuesReferences` read) + `merge-proof` from main's copy with the sentinel 99 + `--base <branch> --state all` before `--delete-branch` + `ls-remote` after. Merge comment: identity, what the squash carried, provenance of the manifest, accepted risks in words.

## 2. Then resolve issues, one at a time, until none are open
- Pick: **the N-th OLDEST eligible issue, N = your lane index** — letters by alphabet (A=1 … N=13, no I), `ISSUES n` = 13+n; **X and the queue lane do not claim**. If the N-th is already taken, **advance by the NUMBER OF CLAIMING LANES, not by one** — try N, then N+L, then N+2L, where L is how many lanes claim (letters plus `ISSUES n`; X and the queue lane do not). *Stepping to "the next eligible" walks straight into lane N+1's slot, and two lanes running the loop together both read before either writes — the collision the index removed, reintroduced by the fallback.* And **read the NEWEST comment immediately before claiming**: a claim two minutes old is invisible to a listing. *Why an index and not "the oldest": the rule is deterministic and every lane runs it at once, so "oldest" makes the whole fleet pick the same issue. Measured 2026-09-07 23:05–23:2xZ — #92 claimed by two lanes two minutes apart (one yielded), #93, #90 and #127 claimed within twenty minutes, two yields in all; each collision costs two turns, the claimer's and the yielder's.* **Sort ascending before indexing** — `gh issue list` returns newest-first (measured: #988, #981, #978, #975, #974), so indexing that list hands each lane the N-th NEWEST and inverts this rule:
  ```
  gh issue list --state open --search "sort:created-asc" --limit 200 --json number,createdAt,labels,title
  # ascending SERVER-SIDE: --limit takes the NEWEST n first, so sorting after it would
  # hide the genuinely oldest issues the moment more than 200 are open.
  ```
  Eligibility, unchanged (skip ones with a "Lane X takes this" comment in the last 24 h or an open PR that names it). Prefer `current-wave`, then `gate`/`tooling`, then the rest. SKIP by shape, not by age: an issue whose body says no code change is required, a marker issue, a title starting `Decide:`, a `deferred` label, or a whole new feature/epic — those are the owner's decisions, not bounded defects; note the skip in your report and take the oldest issue that names a defect a red-first cell can catch.
- Claim: comment `Lane X takes this at <UTC>` before touching code; if the issue is already fixed on main, comment the coordinate and close it.
- **A one-line change to a Markdown or `.factory/` doc needs NO working tree.** **Docs only — this is not a general file-editing recipe:** the `100644` below is the mode for a regular text file, and using it on anything else silently drops bits the tree already carries (`install/install.sh` is `100755` in this repository). For any other path, read the mode from `git ls-tree origin/main -- <path>` and pass THAT. A 1,550-file checkout to edit one Markdown line is what put 14 of them on one platter tonight; this PR's own bench cost 20 minutes and had to be repaired mid-way (H, #994):
  **PowerShell 5.1**, which is what the loop runs in on this machine. In Git Bash the same six
  steps work with `blob=$(...)` and a `GIT_INDEX_FILE=$t` prefix; `VAR=x cmd` is a parser error
  in PS, which is why the PS form is the one written out:
  ```powershell
  # 1. PIN the base once. origin/main moves under you in a 13-lane fleet: a tree read from one
  #    main with a parent from another produces a commit whose diff REVERSES everything fetched
  #    in between.
  git fetch origin main; if (-not $?) { throw "fetch failed" }
  $base = git rev-parse FETCH_HEAD; if (-not $?) { throw "rev-parse failed" }

  # 2. SEED the file from the pinned base, never from the checkout. hash-object hashes the WHOLE
  #    file, so a stale copy silently reverts every change that landed on main since - the same
  #    rewind item 3 of the MERGE-CHECKLIST exists to catch.
  cmd /c "git show $base`:<path> > edit.tmp"          # RAW bytes: a PS pipeline re-encodes,
  if (-not $?) { throw "show failed" }                #   and this file alone has 139 non-ASCII
                                                      #   bytes (19 em-dashes) to lose.
  if ((git hash-object edit.tmp) -ne (git rev-parse "$base`:<path>")) {
      throw "seed differs from base"                  # the recipe checks its OWN seed
  }
  #    ... make the one-line edit in edit.tmp ...

  $t = Join-Path $env:TEMP ("gh-idx-" + [guid]::NewGuid().Guid)   # unique, and it MUST be set
  $prev = $env:GIT_INDEX_FILE
  $blob = git --attr-source=$base hash-object -w --path <path> -- edit.tmp
  if (-not $?) { throw "hash-object failed" }
  # --attr-source: `--path` ALONE resolves clean/EOL attributes from the CURRENT working tree,
  # and this recipe exists FOR the stale checkout - so they must come from the pinned base.
  # A PATH, never a pipeline: Get-Content | git re-encodes and adds a terminator.

  $env:GIT_INDEX_FILE = $t
  git read-tree $base;                                    if (-not $?) { throw "read-tree failed" }
  git update-index --add --cacheinfo 100644,$blob,<path>; if (-not $?) { throw "update-index failed" }
  $tree = git write-tree;                                 if (-not $?) { throw "write-tree failed" }
  $env:GIT_INDEX_FILE = $prev          # RESTORE. Setting $null CLEARS the override, and an
                                       # unset GIT_INDEX_FILE means the REAL index - the
                                       # recipe would then stage over your own work.

  $commit = git commit-tree $tree -p $base -F msg.txt; if (-not $?) { throw "commit-tree failed" }
  # 3. A native failure does NOT stop a PS block. An empty $commit expands to
  #    `git push origin :refs/heads/<branch>` - which DELETES the branch (reproduced by review).
  if ($commit -notmatch '^[0-9a-f]{40}$') { throw "commit-tree gave no object id: [$commit]" }
  git push origin "$($commit):refs/heads/<branch>"
  ```
  **`--path=<path>` is not optional**: `hash-object --stdin` alone stores raw bytes and skips the
  destination's clean/EOL filters, so a CRLF source commits CR bytes into a path `.gitattributes`
  declares `text eol=lf` (`*.rs`, `*.toml`, `*.yml`, `*.yaml`, and named extension paths). Passing
  the destination makes the plumbing route store the same bytes `git add` would.
  And an orphan `index.lock` is removed **only after proving nobody holds it** — open it with `FileShare.None`; success means no process has it — and then only your own, by name. Reading the command lines of live `git` processes is weaker: on this machine 15 were running and none of them named my bench, which is evidence about what they said, not about the handle.
- Fix: own bench on the real branch `issue-N-<kebab>` from `origin/main`; apply AGENTS.md's Journey-Proven Development rules before adding or requesting a test: name the observable contract, plausible defect, and existing coverage gap, then choose the smallest adequate proof. Use red-first or bounded fault evidence only when it is the best observer; inspection or an existing validator is enough otherwise. Run the focused validation appropriate to the change; PR with `Closes #N` in plain text in the body, identity line, validation evidence, risks. Findings while working go into the PR that owns the code — **and no new issue or follow-up PR**, under section 0; the exception this line used to grant ("a new issue only for something that blocks a merge") is retired, because a lane judging its own finding blocking is how the tracker count grew. If it genuinely needs a tracker, say so in the comment and the orchestrator or the owner decides.
- Then, IN THIS ORDER (section 0): run/queue the gate first, then request two passes against the head that CARRIES the manifest, then hand the button to a third lane (name one with 0 bodies in the thread; when none exists, say so ON the PR and item 12 names the presser — the earliest comment-surface pass — instead of waiting). While waiting for others, give passes to their PRs (step 1).
- Never: touch another lane's bench/branch/target, `worktree prune`, wide deletes, `gh workflow *`, secrets in text, actions your own session denied. The one exception to all three of "another lane's target", `worktree prune` and "wide deletes" is the sweeper named in `AGENTS.md` "Disk hygiene" (`disk-sweep.ps1`, criteria fixed in code, never a session's judgement); it does not license a session to do any of them by hand.

## 3. Report
Each turn ends with one comment on the PR you moved (not a message) and, when a button, a queue slot or
a decision changes hands, **a record in the execution** - `type: "operator_note"` on THE RUN's signal
endpoint, which is what the owner reads in the Studio (`leave-records` skill; `ok: true` with
`decision: "rejected"` is the SUCCESS shape for a note, and `headSequence` moving is the proof it
landed). Then message whoever is the orchestrator **right now**, found in `ListAgents` at that moment.
`SKILLS USADAS:` first.

**Which execution, and how a restarted lane finds it.** A record needs an `executionId`, and a lane
that is not already attached to the fleet's run cannot publish or read the handover without one - which
would leave the authority declared here with no address at all. **Ask the orchestrator of the moment
(`ListAgents`), and only them.** The Runtime's listing does NOT answer this: a row carries
`executionId`, `mode`, `status`, `attention`, `startedAt`, `lastEventAt` and `headSequence`
(`apps/cli/src/commands/execution/list.rs:144-151`) and nothing that says which run the fleet is
working under - so "list the executions and take the fleet's" asks a restarted lane for exactly the
knowledge it does not have. Use the listing only to confirm that the id you were given is running.
Do NOT copy a run id out of an old message or out of this file: like a session name, it is good only
while that run is the live one. **Measured, 2026-09-08:** the store was repointed mid-day without a
notice and seven records - including the one for the day's only merge - landed in the run nobody was
reading, while the Studio showed another. A record written to the wrong run does not fail: it is
accepted, and reads as a lane that wrote nothing.

**Never a hard-coded session address.** This line used to name `graphhelm-b8` and a `local_…` id; both
died with the session that owned them, and a lane following the file would have sent the handover of a
button to an address nobody answers (Codex, on this PR). A session name is an address for as long as
that session lives and not one turn longer.

**Identity in the log is NOT free, and the file will not pretend otherwise.** The MCP server is started
with a fixed `--actor factory-agent` (`.mcp.json`), so **every record written through the `graphhelm`
MCP carries that one actor, whoever wrote it** - measured 2026-09-08, six records from four lanes all
attributed to `factory-agent`, a log that cannot tell one lane from another or the agent's work from
the owner's. There is no lane-level identity to "remember" to use there. What is actually available:

- **Through the MCP:** the actor is shared. The only distinguishing field is `signal.source.id`, which
  is written by the sender, so it separates lanes only while each uses a different one - two lanes
  pasting the same template collide silently, and the log shows one writer.
- **Through HTTP:** send your own `X-GraphHelm-Actor` (with `X-GraphHelm-Actor-Type: agent`) AND set
  `signal.source.id` to your lane. That is the only path where the attributed actor is yours.

Either way, **begin the record's `description` with your lane letter**: it is the one identifier that
survives a shared actor, and it is why the records read as four lanes today rather than as one.

## Assignment table - REMOVED, and not replaced by another table

A table of thirteen PRs stood here, written at 22:1xZ on 2026-09-07. Twelve of those PRs are closed
(eleven on 5-6 September) and the thirteenth is parked by the owner; none of the 41 PRs open on
2026-09-08 appeared in it, and every row assigned work to `M`, `G`, `K` or `ISSUES 2/3/4` - the same
departed fleet whose addresses put the tombstone on `.factory/orchestrator-board.md`. Step 1 above
named it as a source while also saying assignments come from the execution record and the current
orchestrator; two sources for one decision means the reader picks, and the stale one reads as
authoritative because it is specific.

**Assignments come from the execution record and from whoever `ListAgents` shows as the orchestrator
now.** Nothing is written here again: a table of who-does-what is a snapshot, and a snapshot in a file
lanes obey outlives the fleet it describes. (Found by lane C on this PR; it was the third round of the
same class, after the board's dead addresses and step 1's heading.)
