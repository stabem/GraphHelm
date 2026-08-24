> **PROVENANCE: this document became the correction posted on #130.**
> Provenance is the strong relation and the one grep cannot see: the derived artefact carries the
> content and never the source's name, so only the author can record it. Established against
> `gh issue list --state all` and the PRs, not from memory.

## Correction: this issue's lead argument is overstated — the code is not saying something false

I wrote, above, that after #96 `GHCLI016` "now says something concrete and, for this path, false."
**That is wrong, and it was caught by a reviewer reading my own doc comment rather than my summary
of it.**

#96's constant carries an explicitly **path-scoped** meaning:

```rust
/// #96: this code now means ONE thing on the start/resume path — the decision committed and the
/// work then failed.
```

*On the start/resume path.* So nothing #96 asserts is contradicted by the immediate-pause site
continuing to use the same code. The site is a **pre-existing approximation, now documented as a
decision** — untidy and named, not falsified. My framing quietly widened my own scoping clause and
then convicted the code of a claim it never made.

**What survives, and it is why the issue stands:**

- The immediate-pause budget answers a **different operator question** — *did my pause take effect?*
  — from either class #96 split.
- Its remedy is genuinely third: **an UNKNOWN, not a failure.** The pause may still record after the
  budget elapses. A failure tells you to act; an unknown tells you to look. Neither of the other two
  carries that.
- One code covering a question with a different remedy is the flattening class regardless of whether
  any doc comment is contradicted.

**What does not survive:** any reading of this issue as urgent because #96 *made something wrong*.
#96 made this site's approximation more visible, not more incorrect. The severity is "a distinct
operator question deserves its own code", not "a fix introduced a false statement".

Recorded as a comment rather than an edit to the body, so the overstatement and its correction both
stay in the record — the same disposal used for the retracted gate claim in PR #99.
