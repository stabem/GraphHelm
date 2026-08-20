## Retraction: this PR's body asserts a gate defect that does not exist

The body of this PR says the gate result was *"content-verified, because the exit code carries no
information"*, citing #97, and describes an earlier run whose verdict was RED while the process
exited 0. **The claim is mine and it is wrong.** Correcting by comment rather than editing the body,
because a silent edit would hide an error that is more instructive than the thing it was attached
to.

**F's investigation (#100) invoked the gate directly: RED exits 1, GREEN exits 0, every time,**
across the reported pattern, the full structure, and both shells. The gate's exit code is correct
and always was.

What I actually observed:

| run | invocation | exit read | whose status |
|---|---|---|---|
| the RED run | `… -File ci/gate.ps1 2>&1 \| tail -60` | 0 | **`tail`'s** |
| the GREEN run | `… -File ci/gate.ps1 2>&1` | 0 | the gate's — and **correct** |

A bash pipeline reports the last command's status: `false | tail -1` returns 0, `false` returns 1.

**The two runs were two different instruments and I compared them as one.** The part of the claim
that sounded strongest — that the status was *constant* across RED and GREEN, therefore carrying no
information — was the weakest: one piped run and one unpiped run, presented as a controlled pair.

**The tell was in my own words.** I had already written, of the first run, *"my error, not the
script's"* — identifying the pipe and attributing the missing stage-by-stage log to it. I did not
carry the same attribution to the exit code sitting beside it, and a more confident framing then
hardened it into a filed issue. **A correct observation with the wrong conclusion attached is more
durable than a wrong observation**, because the evidence keeps checking out every time you look.

### What this does and does not change

**Does not change anything about the merged work.** The gate genuinely was GREEN — 23 stages, zero
failure signals across the 1088-line log, verified by reading the verdict line and the log body.
That verification stands on its own terms; it simply was not *necessary* for the reason the body
gives.

**Does not affect #98.** The allowlist finding — a new test file gets the workspace pass but never
the isolated pass, and nothing announces the omission — is untouched, independently verified, and
remains a real defect. `f7726b4` in this PR adds this suite by hand and documents the trap at the
site.

**Does affect one supporting argument.** The body's companion point — that the log carries no
per-test lines, so a wrong exit code would have no second source — loses its premise. The log
verbosity observation is still true (`grep -cE "^test [a-z_]+ \.\.\. ok"` over the green log returns
0) but it is no longer a defence against anything, and it should not be cited as one.

Retraction also applied to `.factory/c-agent-m10-defect-classes.md`, where the claim had propagated
into a defect class before it was caught.
