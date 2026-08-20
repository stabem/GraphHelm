# #120 archival — DELTA 1 (2026-08-20)

**Enumerated coverage, per the delta-2 lesson (a superseding document must account for what it
carries, or it silently unseals what it forgot):**

- **SUPERSEDES: exactly one item — the "MARKED, NOT RESOLVED: 11 logged runs per arm" paragraph in
  §3** of `m-agent-120-archival.md`.
- **CARRIES FORWARD UNCHANGED: everything else in that file** — §1 session 1, §2 session 2, the rest
  of §3, §4a, §4b, the commit × session bound in §4, the provenance table in §5, and the
  refuses-to-contain list in §6.
- **RETIRES: nothing.**

Base pin, untouched: `m-agent-120-archival.md`
`sha256 b5558ec74e71e4d0102a0566a5e937ee7ad52f3f550d54b34ad8ec04df80557d`, 9361 bytes.

---

## THE RESOLUTION — and it was neither of the two things a reader would guess

D read it from the file rather than from memory. **I verified every part myself** in
`.factory/d-agent-storm-rate-results.txt`:

| Item | Status |
|---|---|
| **`run=11`** | **AN EXTRA PAIR, one on each arm.** Verified at lines 42–43: `run=11 arm=pre verdict=PASS events_appended=45` / `run=11 arm=post verdict=PASS events_appended=59`. **Both PASS.** |
| **Positive control** | **A separate, PRE-only, pre-sequence run with NO `run=` line at all.** Verified: line 3 ends at `(not part of the sealed 20)` with nothing following. **It is not entry 11 and not entry anything.** Its job was to prove the instrument was actually compiled into the binary rather than silently absent. |
| **Sealed window** | **`run=1 … run=10`, 10 pairs, ABAB interleaved.** |
| **Verdict** | **PRE 0/10, POST 7/10.** Verified by counting FAIL over `run=1..10` only: PRE **0**, POST **7**. |

## WHY ELEVEN EXIST WHEN THE RULE SAID TEN

**Not a design of 11, and not a peek — a race between the stop and the loop.**

The loop was written `for i in $(seq 1 20)`; the original sealed design was **N=20 per arm.** L's
stopping rule (*look at 10 pairs, stop if unambiguous*) arrived **after the sequence was already
launched.** D adopted it **before opening any result**, then stopped the task at the 10-pair look
when it came back MOVED. **The 11th pair had already completed in the background between the look
and the kill.**

## IT WAS EXCLUDED BEFORE ITS VALUES MATTERED — which is the part that makes it clean

**L caught it first**, and D recorded it as *"ran, outside the sealed window."* The exclusion
predates any use of its numbers.

- **Folding an eleventh observation in after seeing the tally is exactly the peeking a
  pre-registration exists to prevent.**
- **Being strict cost nothing here:** both 11th runs PASSED, so including them gives **0/11 vs
  7/11 — the same MOVED.**
- **D's own line, and it is the durable half:** *"That it would not have changed the answer is
  exactly why it was cheap to be strict; the discipline earns its keep on the day it would."*

## THE CANONICAL FORM, for the evidence commit

```
sealed window : run=1..run=10, 10 pairs, ABAB interleaved
verdict       : PRE 0/10, POST 7/10   (rule: >=6 of 10 = MOVED)
run=11        : one PAIR (both arms PASS), completed after the stop was issued;
                RAN, OUTSIDE THE SEALED WINDOW, excluded from the verdict
positive ctrl : one PRE-arm run BEFORE the sequence, unnumbered, proving the
                instrument was in the binary; not part of the 20 and not a data point
```

## THE CAVEAT D ATTACHES, which bounds the pair more than the count does

**The storm's failure rate is a property of `commit × session`, not of a commit.** The same PRE
binary gave **0/10 in this session and 4/10 in a later one on the same machine.** **The 0-vs-7 split
survives because it was interleaved WITHIN ONE SESSION** — and **nothing in this file may be pooled
with any other run's numbers.** (This restates §4 of the base file; it is not new, and it is
repeated here because a delta read alone must not lose it.)

## WHY THE MARK WAS WORTH MAKING

The answer was **neither of the two possibilities a reader would have guessed** — not a control, not
a sloppy extra — but a **stop/loop race with a clean exclusion already recorded.** **An eleventh row
against a stated N=10 is exactly the kind of thing that gets silently rounded to 10**, and rounding
it would have erased both the race and the exclusion that makes the verdict trustworthy.
