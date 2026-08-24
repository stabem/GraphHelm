> **PROVENANCE: this document became a comment on the PR that closed #83.**
> Provenance is the strong relation and the one grep cannot see: the derived artefact carries the
> content and never the source's name, so only the author can record it. Established against
> `gh issue list --state all` and the PRs, not from memory.

## Review verdict: [APPROVED] — recorded on behalf of the reviewer (B), attributed not authored

Full delta reviewed whole and SHA-pinned, `0f4e7fe..f7726b4`, three commits, all five files read.
Final, no conditions outstanding. Posting it here so the approval lives on the PR rather than only
in the review channel; the wording of what was checked is the reviewer's, the summarising is mine.

**Verified against every standing item:**

- The fix is the approved shape: `prepare_drive` carries all four fallible steps and is called
  **before** `execute_prepared` in **both** routes; `drive()` is infallible by construction and says
  so; the `DriveSetup`/`PreparedPorts` split is clean; the lease autopsy sits in `prepare_drive`'s
  doc comment where the next reader meets the same scare.
- **The dropped sabotage row's structural argument is discharged by the diff itself:**
  `resume.rs` is **absent from the diff entirely**, so the triage loop is untouched by construction
  — the exact grain the argument needed. Row iv's protection is now argument **plus verified
  structure**, and is recorded as that rather than upgraded to "measured".
- The tests carry every ruled grain: the watermark-anchored absence with a landmark proving the read
  (with the last-event trap explained in place); `env_remove` in the serve helper, so a test
  asserting on sealer behaviour owns its environment; `start`'s **paired** control — empty to
  non-empty across an accepted retry, the solution for an absence with no landmark available; the
  same-key test asking what the different-key test cannot; and the retry probe in its own test so it
  executes even when the primary guard falls first.
- The gate change is one line plus the allowlist trap documented at the site, pointing at #98.
- The commit message carries the read-only hoist safety argument step by step, the shared-shape
  sentence for `start`, the NOT-atomic scoping of the per-node redispatch loop, and the (a)/(b)
  residue named as follow-up — with #96, #97 and #98 verified OPEN, so nothing named here is
  unfiled.
- The design document and the sealed predictions ride in the PR as files, so the provenance is
  in-repo rather than in a chat log.

**Ledger, final:** six rows confirmed with panic sites, one dropped with its reason named and its
structural claim verified, one seal honestly weakened by its own pre-registration, and expected
casualties named as casualties rather than counted as confirmations.

**Landing note.** This approval is pinned to the three SHAs as they stand. A **rebase** of the branch
rewrites them and voids it by its own terms. A squash-merge into `main` creates a new commit on
`main` without rewriting the branch, so the reviewed objects remain reachable — that reading is
flagged to the merger rather than assumed, since "preserving the three SHAs" and "squash" can be
read as being in tension.

Merging is not mine to do: it is irreversible and belongs to the session where the owner's authority
actually lives.
