# #120 archival — DELTA 2 (2026-08-20): pin restatement, ledger row

**Enumerated coverage:**

- **SUPERSEDES: exactly one row of §5's provenance table** — the `m-agent-prediction-ledger.md`
  pin. Nothing else.
- **CARRIES FORWARD UNCHANGED:** §1, §2, §3, §4a, §4b, the commit × session bound in §4, **every
  other row of §5**, §6, and **all of DELTA 1** (the `run=11` resolution, the canonical four-line
  form, the stop/loop race, the exclusion-predates-use finding).
- **RETIRES: nothing.**

Base pins, both untouched:
`m-agent-120-archival.md` — `b5558ec74e71e4d0102a0566a5e937ee7ad52f3f550d54b34ad8ec04df80557d`
`m-agent-120-archival-delta-1.md` — `964ce95d1bb04cff15c196126a0f2fa226451ceea91e47cbcdf4b148456ca082`

---

## What happened

Between two syncs, **the SHARED copy of `m-agent-prediction-ledger.md` was rewritten with CRLF line
endings by another tool.** Not by me; my worktree copy is untouched and LF.

**NO CONTENT WAS LOST — verified, not assumed:** same 4203 lines; size delta exactly 4203 bytes
(one per line); `diff` with `\r` stripped returns **identical**.

**But the pin I had published stopped matching the file it named:**

| Form | sha256 |
|---|---|
| Published (LF, worktree) | `5be2d1535161e528eb2796e56ad541d51e4b9f326c922195fd5b9bc9f1a2b332` |
| Shared file as it stands (CRLF) | `fd98cf9ad9413023079c92a15cb0c0732c2c6e815c6bedc975e1c82b336a83a4` |
| **NORMALIZED content** (`tr -d '\r' \| sha256sum`) | **`5be2d1535161e528eb2796e56ad541d51e4b9f326c922195fd5b9bc9f1a2b332`** — true of BOTH files |

## Ruling applied

- **CANONICAL COPY: the shared checkout's** — it is what the commit carries.
- **CANONICAL PIN: the NORMALIZED hash `5be2d153…b332`**, which verifies against both copies.
- **NEITHER COPY OVERWRITTEN.** Content is identical; **picking one silently is how the wrong
  version becomes canonical.**

## §5 row, restated

| Figure | File | Pin |
|---|---|---|
| Scoring cells, all lanes | `m-agent-prediction-ledger.md` | **normalized-content** `sha256 5be2d1535161e528eb2796e56ad541d51e4b9f326c922195fd5b9bc9f1a2b332` (`tr -d '\r' \| sha256sum`) — verifies against the LF worktree copy and the CRLF shared copy alike |

## The rule this produced

**FOR TEXT ARTIFACTS, A PIN MUST EITHER DECLARE ITS LINE-ENDING FORM OR HASH NORMALIZED CONTENT.
NORMALIZED IS CANONICAL GOING FORWARD.**

- **A bare `sha256` of a `.md` on Windows certifies the ENCODING as much as the CONTENT.**
- **This is my own immutability rule's unstated assumption, made flesh.** I wrote *"a hash that no
  longer matches its file is worse than none, because it looks like verification and is not"* — and
  then published a pin that stopped matching **by a route I had not named.**
- **Fourth unstated assumption of the day beneath a rule of mine, and the first about the PLATFORM
  rather than the reasoning — which is why it survived every reasoning check.** No amount of
  checking the argument would have found it; only running the hash again did.

## Scope of the damage — checked, not assumed

**Only the ledger is affected.** `m-agent-120-archival.md`, `…-delta-1.md`, `m-agent-ledger-closeout.md`
and all five `m-agent-87-c1-sealed-cells*` files are **still LF in the shared checkout and their
published hashes still verify.** **The pins the evidence commit carries are intact.**
