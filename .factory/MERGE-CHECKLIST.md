# Merge checklist — for any lane pressing the button

Owner order, 2026-09-04: lanes review each other and merge each other; "if approved it can go".
This list is the part of the Observer's gate that caught REAL loss on 2026-09-03. Each line names
the incident that produced it. It takes minutes, not seconds — items 5, 6 and 7 are reading, and
they are the ones that caught #746, #754 and #758. Run it; do not remember it.

## Before `gh pr merge`

0. **Measure from a fresh worktree of `origin/main`, not from your session's.** Measured 2026-09-04:
   session worktrees sat 315–374 commits behind main (a sweep found 3 of 35 `.ps1` files because the
   tree it ran in had 3). Print `git rev-list --count HEAD..origin/main` before any count — the hook does.
1. **Head at the instant.** `gh pr view N --json headRefOid` — press against THAT sha. If it moved
   since a pass was written, that pass does not cover the head: **do not press; ask both reviewers
   to re-pin against the new sha** (a one-line "re-read at `<sha>`, still approve" is a pass;
   silence is not). Say "no open roots AGAINST `<sha>`", never "no open roots".
   **One carry, and only one:** when the gate's manifest-only commit moves the head, a pass that names the
   PARENT carries to the new head — the presser verifies `git diff --name-only <parent> <head>` lists only
   `.factory/gate-runs/*.json` (`--name-only`, never `--stat`: `--stat` truncates the path from the LEFT to
   `...45f43a3e66f9-…json` and hides the directory the rule tells you to see — K, measured on #885) and
   writes that in the merge comment; any other path in that diff voids
   the carry (#674(b) for the proof, #852 for the passes — decided on #826, 2026-09-04).
2. **Did the gate run on THIS head? Ask the committed store, not the PR page.** With hosted CI
   disabled by policy, `mergeStateStatus CLEAN` means clean of checks that never ran. The rule is
   #674(b) as the owner decided it — a GREEN manifest in the committed store whose `headSha` is the
   PR head — and the repository has the reader:
   ```
   ./ci/merge-proof.ps1 -PullRequest N          # exit 0 SATISFIED · 1 HARNESS-BROKE · 2 NOT (a run that does not vouch) · 3 ABSENT (no run recorded)
   ```
   Predicate, whole: `status == GREEN AND pushed == true AND pullRequest == N AND (headSha == head
   OR (headSha == parent(head) AND the tip touches only .factory/gate-runs/))`. **Exit 0 is
   necessary, not sufficient, while #825's P1 is open:** the reader checks `status` only, so open
   the manifest it names and confirm the stage list is green. Exit 3 (ABSENT) means nobody
   ran the gate on this head — a gate run owed, not a merge; exit 2 (NOT) means a run exists and does
   not vouch — that is a finding. The two are different answers (#811 introduced the distinction). (Found by A's review
   **The docs-only exception, written (D, #865):** a PR whose file list is entirely Markdown that no path
   under `ci/` reads may record `merge-proof` ABSENT as a NAMED exception — the merge comment cites the
   file list, the empty `grep -rn <file> ci/`, and its positive control (a grep that does find
   something). One non-Markdown file, or one Markdown file that `ci/` reads, and the exception is void;
   item 3 exists because a "docs-only" PR once carried `core/events/src/local.rs +13/−90`. Precedent
   (#829, #832, #863) is not a rule; this line is.
   **The manifest-only exception (#752, #674(b)):** a PR whose only change is `.factory/gate-runs/*.json`
   produced by the gate, and nothing else, RECORDS a run — it does not vouch for a merge. It may merge with
   `merge-proof` NOT or ABSENT under a NAMED exception: the merge comment cites the run's sha, status and
   failed stages; the two passes read the JSON (sha, `pushed`, `dirtyDiffHash`, `staleArtifactCount`,
   failed stages) instead of a diff; one other path touched and the exception is void (#885 is the first:
   `45f43a3e` RED on one cell, ticket #886).
   Running the gate yourself: **a gate launched inside a turn dies with the turn** (measured twice:
   log ~700 bytes, task "alive", no signal). Launch it detached — `Start-Process … -PassThru`, PID and
   exit code written to a file — the wrapper's PID is the only identifier you own. **Those proof-of-life
   files (pid, log, rc) live OUTSIDE the worktree**: inside it they dirty the tree, `dirtyDiffHash` goes
   non-null, the gate refuses to publish the manifest and a run with every stage green ends `status: RED`
   ("run manifest not published") — two lanes in one night (C on #830 run 2, H on the main gate run 1,
   2026-09-05). **And the bench must be ON A BRANCH** (`git symbolic-ref --quiet HEAD` non-empty) whose tip
   equals the PR head on origin: the gate publishes by `git update-ref … $branchRef` after reading
   `symbolic-ref` (D; `ci/gate.ps1:1819` and `:1124`), so a DETACHED bench runs every stage and publishes
   nothing — the same "run manifest not published" with everything green. For a main run, create a local
   branch at the sha and push it; any "green but ABSENT" run is a candidate for this cause. **Detached from the
   session is not detached from the app**: a restart of the Claude app (fleet recycle, 2026-09-05 ~04:11Z)
   killed every gate including the detached ones (#830 1h+ into `workspace tests`, #826); what survives
   is what is PUSHED and the target on disk (cargo resumes). Push the manifest the instant it exists; a
   gate that must outlive the app is launched from outside it (Task Scheduler / a service), not from a
   session. **Proof of life is the
   child chain from that PID** (`Get-CimInstance Win32_Process | ? ParentProcessId -eq <pid>`, recursively)
   **with the log GROWING** — growth proves life; it is the primary POSITIVE signal because it is MONOTONE (94 KB → 520 KB across
   the main gate, #867); descendant count and descendant CPU are weaker: children die and leave the sum
   (70.1 s → 6.1 s in 15 s, measured), so "CPU equal = wedged" and "CPU greater = alive" are BOTH
   false, and a healthy gate sits 45 s flat between stages (`desc=1`, no rustc — design, not pathology).
   **Wedged is never a 30 s reading: require ≥ 5 min with ALL of log size, descendant count and
   descendant CPU flat.** The only kill rule is the ownerless wrapper (15 min unclaimed, by (pid,
   StartTime)) — a lane never kills a gate it did not launch on a liveness reading (H read "AVANCA =
   False" on the main gate every merge-proof waits for, and nearly did). Log silence
   mid-stage is capture, not death: the gate captures a stage's output and writes it when the stage ends
   (`Start-Process -Redirect` and `*>` both grow the file — measured, H), so a quiet log says nothing
   either way — **silence is never a negative signal**; only the ≥ 5 min all-three-flat rule below decides (A, #863). `Get-Process -Id` answers *alive*, not *progressing* — the wrapper's CPU is 0 by
   construction (it waits for its child), so a wedged gate looks identical to a building one (A, J, G).
   The inverse holds too (M): a background task that ended does not prove the process ended — look for a
   live chain before relaunching on the same target; two gates on one branch are two manifest commits
   racing. With an isolated `CARGO_TARGET_DIR` there is nothing to contaminate — but width is bounded:
   `D:` is one platter (measured 2026-09-05: 121 `*target*` directories on it; an `ls` took tens of
   seconds under five gates; with five running the box was measured STALLED — 26 cargo + 100 rustc,
   17.8 s of CPU in 36 min, D: at 2685 % disk time, SSDs at 0–3 %). So the ceiling is **one gate on the
   HDD (`D:`) plus one on the SSD (`E:/<lane>-targets`, while `E:` has >30 GB free — `Get-PSDrive E`
   first — the floor is a PRECONDITION, not a budget: `E:` went 79 → 38 GB free in three hours of
   accumulated review and gate targets at ~10.7 GB each, so **a target on `E:` is removed by its creator
   when the gate or review ends** — `Remove-Item` by name, never a sweep; `E:` is a lane, not a
   warehouse)**; `C:` (the system SSD, 127 GB free measured 2026-09-05) may hold ONE build or review target per lane, `C:/<lane>-targets/<n>`, only while ≥ 100 GB stay free, removed by its creator when the build ends, and **never a gate target** — the reason stands: a full `C:` takes the machine down (the disk-fill incident), so the floor is the rule, not the exception — and a CEILING on the board: at most TWO `C:`
   targets at once, whoever owns them, because a per-lane floor does not bound the sum (13 lanes × 10.7 GB
   against 27 GB of headroom; `E:` sat at 33.5 GB today under a 30 GB floor — K); and never `F:` (the
   repository disk, 15 GB free). Before launching, **count LAUNCHES, not cargos** — one gate is
   2–9 cargo processes by design (measured): `Get-CimInstance Win32_Process | ? { $_.Name -eq
   'powershell.exe' -and $_.CommandLine -match '(-File\s+\S*|&\s*\S*)ci[\\/]gate\.ps1' }`, keeping only
   those with ≥1 live descendant. **The count reads its own reader**: the shell running this query, and
   every bash/PowerShell that invokes it, carries `gate.ps1` in its command line — C read **8** with zero
   gates alive (2026-09-05). The live-descendant clause filters them (a reader has no compiling child);
   without it, exclude your own PID and its ancestors, or the count never reaches zero and nobody
   relaunches. Two things this pattern had to survive, both measured: `[\\/]`, not
   `[\/]` — in .NET `\/` is an escaped slash, so `[\/]` misses every Windows path (`'a\b' -match 'a[\/]b'`
   → False) and under-counting fails OPEN (a low number says "launch"); and the `-Command … & ./ci/gate.ps1`
   launch shape beside `-File`. **A positive control for a matcher comes from the POPULATION — real
   command lines copied from the process table — not from your head; a fixture shaped like the pattern
   passes by construction.** The controls this line was tested against (expected 1,1,1,0):
   ```
   powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\ci\gate.ps1                                  -> 1
   powershell.exe -NoProfile -ExecutionPolicy Bypass -File ci/gate.ps1                                    -> 1
   powershell.exe … -Command "$env:CARGO_TARGET_DIR = \"D:\c-753-targets\"; & ./ci/gate.ps1 -SkipPostgres" -> 1
   "C:\Program Files\Git\bin\bash.exe" -c -l "grep -c gate.ps1 ci/gate.ps1"                              -> 0
   ```
   A plain `CommandLine -match 'gate.ps1'` returned 11 for 3 real gates. At most one other live gate, on
   the other spindle.
   A target costs ~10.7 GB (measured); check the disk before you add one. New gates enter in
   review-ready order. Proof of life with numbers (H): read
   `(Get-Process -Id <pid>).CPU` twice, 30 s apart — dead = the read fails, wedged = equal, progressing
   = greater — on the compiling descendant, not the wrapper. The package-cache lock
   (`$CARGO_HOME/.package-cache`, one per machine, unchanged by `CARGO_TARGET_DIR`) is still shared:
   on `Blocking waiting for file lock on package cache`, **wait** — the lock frees itself and the gate
   proceeds; it is contention, not contamination and not a lost gate; **do not relaunch** (#833's
   matcher).
   of this very file: no line asked whether the gate ran.)
3. **The PR's file list is exactly what it claims.** `gh pr view N --json files` — a docs-only PR that
   shows a production file is a REWIND of the base, not a diff: a branch collapsed with `reset --soft`
   onto a moved main keeps the old tree in the index, and the single commit silently reverts whatever
   landed on main in between (this very PR, `acf4c5fc`, carried `core/events/src/local.rs +13/−90`
   against #823; caught by H, invisible to any "deleted files" check because nothing was deleted).
4. **Mergeable asked twice.** First answer `UNKNOWN` means "not computed yet", never "no problem".
   Wait for `MERGEABLE` + `mergeStateStatus CLEAN`.
5. **Closing keywords in FOUR places, union == intent.** GitHub's parser reads no negation, and the
   three readings are NOT redundant: on #746 the PR body was fixed before the merge and
   `closingIssuesReferences` came back clean — and the squash `34cced9c` still shut #717 two seconds
   after the merge, through the negated sentence left in a COMMIT body (reopened by hand 49 s later,
   #757). **The instrument that frees the PR is blind to the field that shuts the issue.**
   ```
   gh pr view N --json title   --jq .title                        # the squash's FIRST line ONLY when the PR has 2+ commits
   gh pr view N --json commits --jq '.commits[0].messageHeadline'  # with ONE commit the squash takes THIS (COMMIT_OR_PR_TITLE); retitling the PR changes nothing (A, #849)
   # edges (#875; documented behaviour, not measured on this board): ZERO commits → GitHub disables the button, nothing
   # to read; ONE commit that is itself a merge commit → its headline (`Merge branch …`) becomes the squash subject —
   # reword (or squash locally) before pressing. `allow_merge_commit` is ON in this repo (measured): a merge-commit
   # press carries every commit message verbatim — the board squashes; a non-squash press is a checklist violation.
   gh pr view N --json body    --jq .body
   gh pr view N --json commits --jq '.commits[].messageBody'      # what a SQUASH carries
   # PowerShell eats the quotes of an inline query; a JSON file works from both shells (measured):
   # q.json = {"query":"{repository(owner:\"stabem\",name:\"GraphHelm\"){pullRequest(number:N){closingIssuesReferences(first:10){nodes{number}}}}}"}
   gh api graphql --input q.json --jq '[.data.repository.pullRequest.closingIssuesReferences.nodes[].number] | @csv'
   # @csv, not join(","): PowerShell 5.1 strips the inner quotes of a native argument even inside '…',
   # so join(",") reaches gh as join(,) and fails; @csv needs no quotes and prints nothing (not an error)
   # when the parser links no issue. Measured from a .ps1 and from bash against #746 (735) and #825 (empty).
   # (-f query=@file does NOT work: the @ is sent literally)
   ```
   A keyword inside backticks or a code fence hides it from the parser; it does not hide it from the
   squash. Measured on #825, not yet merged: the body's `` `Closes #822` `` is in a code span and the
   parser reads nothing, while two commit bodies (`bdb8061b`, `78c3887f`) carry `Closes #822` in plain text — the
   squash message is built from those. The first half is a reading; what the squash does is what
   item 8 reads after the fact.
   `closingIssuesReferences` reads the BODY only; the squash acts on the TITLE plus COMMIT MESSAGES
   (repository setting `squash_merge_commit_title: COMMIT_OR_PR_TITLE` — found by ISSUES 3 on this PR). All four,
   `grep -iEo '(close[sd]?|fixe[sd]?|resolve[sd]?|refs?) #[0-9]+'`, and the union must equal what
   the PR means to close. A keyword next to a number you do not want closed — even inside a
   negation, even in a commit body — must be reworded (`Refs #N`, "#N stays open").
6. **Read the CARRY BODIES, not the count.** `reviewThreads` unresolved = 0 was true on #758 with
   a soundness finding sitting in a review body, and on #754 with a section titled "One thing I
   did NOT verify" in plain sight. Grep each carry and review for: `did not`, `not verified`,
   `cannot tell`, `roots without a verdict`, `stated rather than implied`, `finding`, `latent`.
   Each item is either closed by another pass, marked non-blocking BY ITS AUTHOR, or written into
   the merge comment as accepted risk with an issue number.
   A *carry* is a review-shaped comment by a lane other than the author that names the sha it
   measured. **Counting passes reads BOTH surfaces** — `repos/O/R/issues/N/comments` AND
   `repos/O/R/pulls/N/reviews` — one surface read as the whole nearly blocked #833, which had two passes
   and showed one (#840). **A zero on either surface is checked against a known positive** (a PR you know
   has a pass there) before it is read as "none". Every time:
   ```
   gh api --paginate repos/stabem/GraphHelm/issues/N/comments --jq '.[] | select(.user.login != "chatgpt-codex-connector[bot]") | "\(.created_at) \(.body[0:80])"'
   gh api repos/stabem/GraphHelm/pulls/N/reviews --jq '.[] | "\(.submitted_at) \(.state) \(.body[0:80])"'
   ```
7. **Read the STATE of each review, not its existence.** Every review in this repo is
   `COMMENTED` (the fleet shares one account; GitHub refuses self-approval), so "approve" is the
   reviewer's own word in the text, never a GitHub state. A `COMMENTED` whose body declines to
   verify — "not a pin", "I will not record this as verified", "the sweep reads Running" — is
   NOT a pass, and a merge comment that links it as one is citing the wrong object (#745: the
   text described the approving review, the link pointed at the earlier "Not a pin" comment).
8. **Two passes while Codex is out of quota** (since 2026-09-03T13:04:45Z): different lanes,
   neither the author, each naming the sha it measured. It is a SUBSTITUTION, not equivalence —
   the merge comment names what the mechanical sweep would have caught and did not:
   prose contradicting code, form-vs-instance matching, the third actor.
   Lanes are told apart by the **identity line** (`Lane: … · Session: … · Head: …`, see `AGENTS.md`),
   never by the GitHub login — every login here is one account. `AGENTS.md` asks writers to put it
   FIRST; the counter looks for it **anywhere in the body** (C's pass on #833 signs on its last line —
   H counted one pass too few by reading the top line only, #840). A writer's slip must not become the
   counter's absence: absence blocks correct work. A pass with NO identity line in any position (C's four
   passes on #833 sign `## Implementer (C), re-review`) is counted by its BODY — lane named, sha named,
   verdict word — whatever signature it carries (`Lane: C`, `## Reviewer (K)`, `## Implementer (C)`); the
   writer is asked to add the line. **A pass that names no lane at all does not count** — it cannot be
   told from the author's own comment — and asks for the line. The line is for addressing, the count is
   by content (A, #863; #874).
9. **Stacked PR before `--delete-branch`.** `gh pr list --base <head-branch> --state all`.
   Non-empty → merge WITHOUT `--delete-branch` (#713's delete closed the stacked #729).
10. **Content coupling with other OPEN PRs.** If another open PR writes a field/file this one
   reads (or vice versa), there is an ORDER; measure who writes and who reads at each head.

## After the merge (read the output — do not report what you intended)

- `gh pr view N --json mergedAt,mergeCommit` — cite these, not the branch head.
- `git log -1 --format=%B origin/main | grep -iEo '(close[sd]?|fixe[sd]?|resolve[sd]?) #[0-9]+'`
  — what the squash ACTUALLY carried; then `gh issue view` each cited issue and confirm state.
- `gh api repos/stabem/GraphHelm/branches/<branch>` → expect `404` "Branch not found"
  (the text is `Branch not found`, not `Not Found`; a wrong grep read a deleted branch as alive).
- Merge comment on the PR: measurement instant, head, mergeable×2, threads, carries with ids and
  timestamps, closing-keyword reading, stacked check, and one paragraph of what the PR is.
  Corrections go INSIDE the same PR as a follow-up comment, never elsewhere.

## Tooling traps that produced false readings on 2026-09-03/04 (each measured)

- `git cat-file -e "<ref>:<path>"` under MSYS is mangled for some path shapes and not others
  (measured: `origin/main:core/x` fine; `origin/main:.factory/x` and `origin/main:/core/x` mangled to
  `origin\main;…`, git says "ambiguous argument" / "invalid object name"). Do not learn the rule —
  use the SHA instead of the ref, or `MSYS_NO_PATHCONV=1`.
- A failed `git fetch` leaves the ref with its OLD value; the next `rev-parse` succeeds. Check
  the fetch rc immediately, or fetch into a NEW ref name each time.
- PowerShell: after `<cmd> | Select-Object -First N`, `$LASTEXITCODE` is not the command's. Instance, same
  host and git version: `Git\cmd\git.exe` (launcher) → `-1` in 8/8 successful runs; `Git\mingw64in\git.exe`
  (the binary) → `0` in 8/8. Which one `git` resolves to is `(Get-Command git).Source`, and it can change
  between sessions without you touching anything. Read nothing from that code — test the VALUE.
- Bash: after a pipe, `$?` is the LAST element's. `cmd 2>&1 | head -2; echo $?` reports head.
- PowerShell: `$LASTEXITCODE` is the last PROCESS's, not the last command's. `& .\ci\merge-proof.ps1` in a
  tree 335 commits behind raised `CommandNotFoundException` — a shell error, no process — and
  `$LASTEXITCODE` still held the `0` of the `git rev-list` before it: "merge-proof exit 0" in the colour
  that authorises (D, #865). Set `$global:LASTEXITCODE = 99` before invoking `merge-proof`; a 99 after the
  call means it never ran; run it from a fresh worktree of `origin/main`, where the script exists.
- `git ls-tree -r --name-only <sha>` without `--full-tree` is scoped to the CWD PREFIX and
  returns empty with rc=0 from inside a subdirectory. Always `--full-tree` when the subject is
  a sha.
- Any zero produced by a filter YOU wrote (`grep`, `head -N`, `sed`, a regex calibrated on the
  old format) is a measurement of the filter. Put a known positive in the same command.
