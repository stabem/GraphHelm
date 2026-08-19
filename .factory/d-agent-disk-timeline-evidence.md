# Disk timeline — preserved evidence (D Agent, 2026-08-19)

**Why this file exists:** the timestamps below were read from H's **session-scoped scratchpad**,
which H's own report marks as "copy out anything needed long-term". They are load-bearing for the
storm lane's confound analysis and would vanish with session cleanup. Preserved here so the claim
is not single-source-and-perishable. M verified the worktree half independently; the log half was
outside M's reach and is preserved here for anyone to re-check while the files last.

## What this bounds — and what it does not

**BOUND (not a measurement):** H's storm baseline was **not** run on a 0 GB disk. H completed a
full `cargo test --no-run` build plus 13 test runs, each creating tempdirs and event stores,
across 09:23–09:38 with **zero disk errors**. A 0 GB disk does not permit that — K's `os error
112` is exactly what 0 GB does to a build. So the disk-full event lies **between** the baseline
and the 14:46 relief: **the baseline predates the fill.**

**NOT ESTABLISHED:** free space at 09:23 was never recorded and is not recoverable. "Not zero"
spans 3 GB and 40 GB, and NTFS fsync latency degrades well before zero. The disk explanation for
the rate change is **weakened, not killed**.

## Timeline

| Event | Time (local, 2026-08-19) | Source |
|---|---|---|
| Storm baseline run 1 | 09:23:18 | `storm_run1.log` mtime |
| Storm baseline run 10 | 09:28:19 | `storm_run10.log` mtime |
| Full baseline suite ends | 09:38:37 | `suite_wake_run3.log` mtime |
| **Cache levers spent** (c-agent + k-agent `target/` deleted) | **14:46:48 / 14:46:49** | worktree dir mtimes; both now target-less |
| Re-baseline run 1 | 15:05:04 | `storm_rebase_run1.log` mtime |
| Re-baseline run 10 | 15:07:44 | `storm_rebase_run10.log` mtime |
| Free space at time of writing | 18.2 GB | `(Get-PSDrive F).Free` |

## Failing runs identified by log size

Passing storm runs write 172 bytes; failing runs write more (panic text). This independently
reproduces H's reported 4/10 without relying on the report:

| Run | mtime | bytes | verdict |
|---|---|---|---|
| 1 | 09:23:18 | 172 | pass |
| 2 | 09:25:07 | 172 | pass |
| **3** | **09:25:29** | **1516** | **fail** |
| 4 | 09:25:58 | 172 | pass |
| 5 | 09:26:22 | 172 | pass |
| **6** | **09:26:43** | **2221** | **fail** |
| **7** | **09:27:06** | **817** | **fail** |
| 8 | 09:27:30 | 172 | pass |
| 9 | 09:27:54 | 172 | pass |
| **10** | **09:28:19** | **817** | **fail** |

All ten re-baseline runs wrote 172 bytes — 0/10, consistent with H's report.

**No pattern signal here.** I first read failures at 3/6/7/10 as "spread rather than bunched",
weak evidence against a transient spike. M corrected it and the arithmetic is theirs: with 4
failures among 10 slots there are C(10,4)=210 arrangements, of which only C(7,4)=**35** have no
adjacent pair — so P(at least one adjacent pair) ≈ **83%**. Runs 6 and 7 *are* adjacent, so the
observed pattern is precisely what randomness produces; a fully non-adjacent spread would have
been the 1-in-6 surprise. **The pattern supports neither explanation.**

## Reproduce

```powershell
Get-ChildItem "C:\Users\gabri\AppData\Local\Temp\claude\F--github-GraphHelm--claude-worktrees-h-agent-cbfd67\07a04345-0dfa-4c42-80b0-650d859185c2\scratchpad" -Filter *.log |
  Sort-Object LastWriteTime | Select-Object Name,LastWriteTime,Length
Get-ChildItem F:\github\GraphHelm\.claude\worktrees -Directory |
  Select-Object Name,LastWriteTime,@{n='target';e={Test-Path (Join-Path $_.FullName 'target')}}
```

## The method note, which outlasts the finding

The recoverable evidence was **not** in message order, where we first looked for it. It was in
**mtimes on disk**. A lane that spent hours lamenting an unrecorded variable had an unexploited
instrument underneath it the whole time — the machine was recording, and nobody was reading it.
Worth trying before declaring any variable unrecoverable.
