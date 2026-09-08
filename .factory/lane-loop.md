# Lane loop — the standing order for every GraphHelm lane (owner, 2026-09-05 22:1xZ: "everything merged; every agent on /loop resolving all the issues")

Run this as your own `/loop` (dynamic, ~20-30 min). Every turn, in this order. Identity line first on everything you publish (`Lane: X · Session: <ListAgents name> · Head: <sha8>`). Rules live in `AGENTS.md` and `.factory/MERGE-CHECKLIST.md` on `origin/main`; this file only sequences them.

## 1. Land what is assigned to you (the table below, then the board)
- Read the PR in the same breath as acting: `gh pr view N --json headRefOid,mergeable,mergeStateStatus,closingIssuesReferences` (ask `mergeable` twice), **THREE surfaces** paginated (`issues/N/comments`, `pulls/N/reviews`, and **`pulls/N/comments`** — the inline threads, where Codex's findings live; the reviews box shows only their headers, and "both boxes" is what let three P1s go unread by two reviewers on #996), passes counted by CONTENT at the head (lane ≠ author, sha named, verdict word; a criteria seal is not a pass; a rebase kills passes; a manifest-only tip carries them).
- CONFLICTING → rebase first, with a net: save the old head in a ref of yours, `--force-with-lease`, verify the rebase by content, post the recovery command; then request two fresh passes.
- Passes missing → request them by name in the PR (lanes with 0 bodies in the thread), and give passes others request from you (verdict word + sha, in the body).
- Manifest missing → run the gate: bench on a SHORT path on the SSD (`E:/<lane>-<n>`, **~40 MB each — the cost is file operations, not bytes**: a checkout is ~1,550 files, and fourteen at once on one platter is a head-seek storm, not a volume problem; measured by L and re-measured here, 1,551 files / 40.2 MB. Remove it when the PR merges; never a session scratchpad, never F:/C: for targets). **`D:` is the HDD gate target's disk and the runner benches', not yours**: measured by L (relayed 2026-09-07T23:4xZ; the reading itself carried no clock), 14 concurrent `git` checkouts on `D:` put its disk time at 536% with two running 19 minutes, while `E:` sat at 5%. **Never start a second checkout while one of yours is running**, and never kill another lane's — a checkout killed mid-way leaves an `index.lock` and a tree that reports 1,500 files deleted (D, this PR's own bench). Checkouts already on `D:` finish where they are. Then, on the bench: `git symbolic-ref HEAD` set (not detached), upstream = the PR's branch (`--set-upstream-to`, or `pushed` comes back null), slot through `.factory/tools/slot-claim.sh` (HDD lock `D:/graphhelm-slot/SLOT.lock`; SSD via `SLOT_LOCK=E:/graphhelm-slot/SLOT.lock`; launch only on exit 0; the wrapper's own pid pair; E: floor 30 GB), FRESH target per run, proof-of-life outside the worktree, push the manifest commit, verdict read from the JSON (reds named test by test against the known-reds list; arm 1 by file/mechanism, arm 2 by existence on a PR-less tree).
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
- Fix: own bench on the real branch `issue-N-<kebab>` from `origin/main`; red-first cell that fails AT THE ASSERTION, sabotage receipt, `cargo test -p` at crate level; PR with `Closes #N` in plain text in the body, identity line, validation evidence, risks. Findings while working go into the PR that owns the code — a new issue only for something that blocks a merge.
- Then: request two passes, run/queue the gate, hand the button to a third lane (name one with 0 bodies in the thread; when none exists, say so ON the PR and item 12 names the presser — the earliest comment-surface pass — instead of waiting). While waiting for others, give passes to their PRs (step 1).
- Never: touch another lane's bench/branch/target, `worktree prune`, wide deletes, `gh workflow *`, secrets in text, actions your own session denied.

## 3. Report
Each turn ends with one comment on the PR you moved (not a message), and a message to the Orchestrator (`graphhelm-b8`, stable id `local_96f8ff4d-…`) only when a button, a queue slot or a decision changes hands — `SKILLS USADAS:` first.

## Assignment table at 22:1xZ (thirteen open PRs)
| PR | state | who does what |
|---|---|---|
| #910 | BLOCK (wiring) by ISSUES 2 | ISSUES 3 fixes → ISSUES 2 + ISSUES 4 re-pin → gate (ISSUES 3) → M presses |
| #908 | fixed after BLOCK, gate running unclaimed | author claims/redoes properly → ISSUES 3 + ISSUES 4 pass → ISSUES 2 presses |
| #871 | 2 passes, manifest RED on main's reds; `process.rs` rewritten by #879 | M rebases (net), two fresh passes (G, K), gate, K presses |
| #864 | rebase proven clean in scratch (`6c13a2d8`) | ISSUES 4 pushes it → ISSUES 3 + ISSUES 2 pass → gate → G presses |
| #858 | CONFLICTING, 0 passes | ISSUES 2 rebases (net) → ISSUES 3 + ISSUES 4 pass → gate → K presses |
| #854 | 0 passes, manifest present | ISSUES 3 + ISSUES 4 pass → read the manifest → K presses |
| #850 | 0 passes (tooling/inventory) | K + G pass → docs-only exception by control or gate → D/H presses |
| #849 | 0 passes, manifest present | G + K pass → M presses |
| #833 | 1 pass (J, sessionless) | G adopts the gate; K second pass → M presses |
| #830 | 0 passes, manifest RED on #880 (C, sessionless) | G + K pass → M presses |
| #826 | 3 passes, manifest RED on entry 1, arm 2 by existence | K presses (0 bodies) — accepted-risk wording |
| #825 | CONFLICTING, superseded by #856 | author narrows to `$unmeasured` DATA after #910, or closes with the coordinate |
| #595 | PARKED by the owner (2026-09-04T11:37Z: "nobody merges this until #786 lands"; #786 OPEN at 23:4xZ). Nine non-`.md` files, 2,627 lines of shell/Python/systemd units that run as root during recovery (`deploy/*-vps.sh`, `seal-vps-file.py`) — the docs-only exception is VOID here (M, pull/595#issuecomment-5555523436) | no press until #786 lands; then K + H re-pin at the tip + a FULL gate → G presses |
