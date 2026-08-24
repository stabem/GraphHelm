> **PROVENANCE: this draft became #130 — *The immediate-pause budget answers GHCLI016*.**
> Established by title match against `gh issue list --state all`, not from memory. Written here
> because the derived issue carries the content and never the source's name: provenance is the
> strong relation and the one grep cannot see, so only the author can record it.

The immediate-pause budget answers GHCLI016, which after #96 means "the drive committed and then failed"

Split out of #96, which separated that code's other two meanings on the start/resume path. Found
while doing that split; deliberately not folded in, because it belongs to a different command and a
different operator question.

## The site

`apps/cli/src/commands/serve/routes.rs:599` — the immediate-`pause` path waits for
`execution_paused` to appear and, when the budget elapses, answers:

```
GHCLI016_DRIVER_FAILURE
"the execution did not record execution_paused within the immediate-stop budget"
```

It wears that code because it was the nearest stable one when the path was written, not because a
driver failed. **Nothing about a drive happened here.**

## Why it matters more after #96 than before

Before #96, `GHCLI016` was a general "something on the serve side went wrong", so a pause timeout
sitting under it was untidy rather than wrong. #96 gave the code a **specific meaning** on the
start/resume path — *the decision committed and the work then failed; your hold is gone* — and left
this site untouched. So the code now says something concrete and, for this path, false.

Three distinct operator questions, one of which no longer belongs:

| path | the operator's actual question | code today |
|---|---|---|
| setup refusal | did anything commit? (no — hold intact) | `GHCLI019_DRIVER_SETUP` (#96) |
| mid-drive failure | did anything commit? (yes — hold gone) | `GHCLI016_DRIVER_FAILURE` |
| **immediate-pause budget** | **did my pause take effect?** | `GHCLI016` — wrong meaning |

The third has a genuinely different remedy: not "fix the environment and retry", not "the execution
is attended" — but **"your pause may or may not have landed; go and look."** The pause may still
record after the budget elapses, so the answer is inherently about uncertainty, which neither of the
other two carries.

## Suggested fix

Its own code, named for what happened — the budget for observing `execution_paused` elapsed —
carrying that the outcome is **unknown rather than failed**. That distinction is the whole point: a
failure tells you to act, an unknown tells you to look.

## Not fixed in #96 because

Bundling it would put two operator contracts in one diff, and the start/resume split is
revertable on its own only while it stays that. The site carries an at-the-site decision comment
naming this issue's reasoning, so the next reader finds a decision rather than an oversight.

## Related

- #96 — the start/resume split this was found by.
- The flattening defect class (M10 close doc): one value, several causes, different remedies.
