# Lane loop — the standing order for every GraphHelm lane (owner, 2026-09-05 22:1xZ: "everything merged; every agent on /loop resolving all the issues")

Run this as your own `/loop` (dynamic, ~20-30 min). Every turn, in this order. Identity line first on everything you publish (`Lane: X · Session: <ListAgents name> · Head: <sha8>`). Rules live in `AGENTS.md` and `.factory/MERGE-CHECKLIST.md` on `origin/main`; this file only sequences them.

## 1. Land what is assigned to you (the table below, then the board)
- Read the PR in the same breath as acting: `gh pr view N --json headRefOid,mergeable,mergeStateStatus,closingIssuesReferences` (ask `mergeable` twice), both boxes paginated (`issues/N/comments` AND `pulls/N/reviews`), passes counted by CONTENT at the head (lane ≠ author, sha named, verdict word; a criteria seal is not a pass; a rebase kills passes; a manifest-only tip carries them).
- CONFLICTING → rebase first, with a net: save the old head in a ref of yours, `--force-with-lease`, verify the rebase by content, post the recovery command; then request two fresh passes.
- Passes missing → request them by name in the PR (lanes with 0 bodies in the thread), and give passes others request from you (verdict word + sha, in the body).
- Manifest missing → run the gate: bench on a SHORT path (`D:/<lane>-<pr>`, never a session scratchpad, never F:/C: for targets), `git symbolic-ref HEAD` set (not detached), upstream = the PR's branch (`--set-upstream-to`, or `pushed` comes back null), slot through `.factory/tools/slot-claim.sh` (HDD lock `D:/graphhelm-slot/SLOT.lock`; SSD via `SLOT_LOCK=E:/graphhelm-slot/SLOT.lock`; launch only on exit 0; the wrapper's own pid pair; E: floor 30 GB), FRESH target per run, proof-of-life outside the worktree, push the manifest commit, verdict read from the JSON (reds named test by test against the known-reds list; arm 1 by file/mechanism, arm 2 by existence on a PR-less tree).
- Press when: two passes at the head + a manifest naming the PR whose reds are all on the list (or GREEN) + you are neither author nor reviewer (filter both boxes by YOUR letter first) + four closing readings' union == intent (no keyword inside backticks; `closingIssuesReferences` read) + `merge-proof` from main's copy with the sentinel 99 + `--base <branch> --state all` before `--delete-branch` + `ls-remote` after. Merge comment: identity, what the squash carried, provenance of the manifest, accepted risks in words.

## 2. Then resolve issues, one at a time, until none are open
- Pick: the OLDEST open issue nobody has claimed (`gh issue list --state open --limit 200 --json number,createdAt,labels,title`; skip ones with a "Lane X takes this" comment in the last 24 h or an open PR that names it). Prefer `current-wave`, then `gate`/`tooling`, then the rest. SKIP by shape, not by age: an issue whose body says no code change is required, a marker issue, a title starting `Decide:`, a `deferred` label, or a whole new feature/epic — those are the owner's decisions, not bounded defects; note the skip in your report and take the oldest issue that names a defect a red-first cell can catch.
- Claim: comment `Lane X takes this at <UTC>` before touching code; if the issue is already fixed on main, comment the coordinate and close it.
- Fix: own bench on the real branch `issue-N-<kebab>` from `origin/main`; red-first cell that fails AT THE ASSERTION, sabotage receipt, `cargo test -p` at crate level; PR with `Closes #N` in plain text in the body, identity line, validation evidence, risks. Findings while working go into the PR that owns the code — a new issue only for something that blocks a merge.
- Then: request two passes, run/queue the gate, hand the button to a third lane (name one with 0 bodies in the thread). While waiting for others, give passes to their PRs (step 1).
- Never: touch another lane's bench/branch/target, `worktree prune`, wide deletes, `gh workflow *`, secrets in text, actions your own session denied.

## 3. Report
Each turn ends with one comment on the PR you moved (not a message), and a message to the Orchestrator (`graphhelm-4a`, stable id `local_96f8ff4d-…`) only when a button, a queue slot or a decision changes hands — `SKILLS USADAS:` first.

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
