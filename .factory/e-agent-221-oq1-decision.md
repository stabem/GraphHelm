**Update to the blueprint above — OQ1 decided by the orchestrator (2026-08-24), not left to the implementer:**

A secret caught by the defense-in-depth scan (§2 step 3) refuses the **whole response**
(`OWNER_OUTPUT_SCHEMA_INVALID`), never a partial render with the offending slot swapped out.

Reasoning: a partial response that silently dropped a section lies about its own completeness —
the reader has no way to know half is missing, and "looks complete" is the worse failure, not the
safer one. The typed refusal names the **section** (`SlotId`) and the **reason code**, never the
caught content — the owner learns *where* without the system repeating *what*. Same shape as
#228's own fix: a response that can't say "I can't render this" in full lies by omission, exactly
the way a boolean that can't say "indeterminate" lies by collapsing into `absent`.

§2 step 3, §7 T9, and §9 OQ1 updated in place to record this as decided rather than open. The
other three §9 questions remain open for cross-review.