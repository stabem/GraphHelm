# #87 commit 1 — reviewer's criteria, FROZEN BEFORE READING THE PACKAGE

Written by D (arithmetic reviewer) before opening `a-agent-m10-d1-design.md` or re-opening
`a-agent-issue87-core.patch`. Frozen-then-diff: the deltas between this and what the package
actually says are the value of the review.

## Contamination disclosure — answering the orchestrator's item (4) honestly

**Partly contaminated, and the honest split matters:**

- **The ARITHMETIC premises are MY OWN.** The ~94%-structural figure and the 2.4–2.56 opens/request
  range come from the storm lane I ran earlier in this session. I am not an independent party to
  those numbers; I am their author. That makes me the right reviewer for whether they are being
  USED correctly and the wrong reviewer for whether they are CORRECT. I will re-derive every figure
  I cite from `.factory/d-agent-storm-study.md` and the run spec rather than from memory of my own
  work — the same rule I have applied to everyone else's claims tonight.
- **The DESIGN DOC is unread.** Criteria below are uncontaminated with respect to it.
- **The PATCH is partly contaminated.** I opened `a-agent-issue87-core.patch` earlier in this
  session; it was too large to hold and its contents are no longer in my context. So I have *seen*
  it without *retaining* it. I cannot claim a clean freeze on the patch, and I am not going to
  pretend one. What follows is what I would demand knowing only the issue title, the stated commit
  boundary, and my own storm figures.

## What I expect commit 1 must contain, before reading

1. A cache keyed on something that makes STALENESS IMPOSSIBLE, not merely unlikely. A verified
   prefix is only sound if the verification covers every byte the reader will trust.
2. Verification of the SUFFIX ONLY on subsequent operations, with the prefix's validity carried by
   a hash/sequence the store already must record — not by a side structure the cache maintains.
3. NO CHANGE to how many times the store is opened per request. That is CONV-1 and it is the whole
   reason this is commit 1 rather than commit 2.

## The arithmetic I will check, and the trap I am watching for

**THE TRAP I MOST EXPECT, because I wrote the warning it would violate.** My own run spec records,
verbatim, that the storm figures are **disk-conditional**: "Comparing an 'after' number taken at a
different disk state against these reproduces exactly the confound this lane spent a day failing to
untangle." If the design uses the 2.4–2.56 range or the 94% figure as a BASELINE for a
before/after improvement claim, that is the confound, and my authorship of the numbers is exactly
why I must say so rather than be flattered by their reuse.

**The premise chain must survive being separated.** "94% of an open is structural O(history) work"
and "2.4–2.56 opens per request" are two different measurements with two different bases. A claim
of the form "so commit 1 removes X% of request cost" multiplies them and inherits both bases. I
will check whether the design multiplies them and whether it names the base it is multiplying
against.

**The per-handle claim — 1 full load + N suffix verifications vs a full load per op — is a claim
about CODE PATHS, not about arithmetic.** It follows only if every operation on a handle actually
reaches the cached prefix, with no path that quietly re-loads. I will look for the path that does
not, because a single re-loading path makes the claim false while leaving every test green.

## SC6 — the sabotage-the-cache guard

Must FAIL when the cache is made to serve a stale or wrong prefix, and must fail **in its own
assertion**, named by panic site. A guard that fails because the sabotage broke an append or a
fixture proves the sabotage was destructive, not that the guard sees staleness.

I will also ask what the guard does when the cache is simply DISABLED rather than corrupted: a
guard that passes with the cache absent is testing the store, not the cache.

## SC7 — budget path-independence

"Incremental accounting == from-zero on EVERY counter, exact equality." Two things I will check:

1. **EVERY counter, enumerated.** A property asserted over a struct's fields is only as strong as
   the enumeration; if it compares a hand-listed subset, a new counter added later is unprotected
   and nothing fails. I want the comparison to be over the whole value (derived equality), not a
   field list — or a compile-time reason a field cannot be missed.
2. **Exact equality, not tolerance.** Stated as the flattening-proof form, and I will verify no
   epsilon, no rounding, no "close enough" anywhere in the path.

## CONV-1 — opens per request unchanged

The claim is byte-equivalent per-operation lock semantics on the fresh-open path. I will check the
lock acquisition/release sites specifically, and whether any lock is now held ACROSS a cache
lookup — which would change contention characteristics without changing the open COUNT, and would
satisfy the letter of CONV-1 while breaking its intent. That is the finding I would most expect to
be missed by everyone including the author.

## What would make me reject commit 1 outright

- Any before/after number compared against a differently-conditioned disk state.
- A cache whose soundness rests on a side structure rather than on what the store must record.
- SC7 comparing a hand-enumerated subset of counters.
- A lock newly held across a cache lookup.
