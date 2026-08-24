# ENVIRONMENT DECISIONS — append-only. Check your own coverage here.

> **MOVED INTO THE REPOSITORY 2026-08-20 ~19:45Z, by the Orchestrator, and the reason is a
> measured cost, not tidiness.** This file governs `ci/gate.ps1`. **A reviewer of `ci/gate.ps1`
> could not open it.** Reviewing #187 against frozen criteria derived from the issue and the code
> — the whole method — could not reach ED-10, so the reviewer had to ask another agent to read it
> aloud. Every other pointer problem this milestone was about EVIDENCE a stranger could not open;
> this one is about a DECISION a reviewer could not open, **and the thing it decides is whether the
> gate runs at all.** Raised by L, confirmed at the source by H.
>
> **The copy at `D:/graphhelm-slot/ENV-DECISIONS.md` is now a POINTER, not a second registry.**
> Two copies of a registry is a duplicated oracle: mechanisms diverge loudly, oracles diverge in
> silence and change what "allowed" means. This path is canonical. Append here.

**Why this file exists, and it is a defect of mine caught by M.** I first put this index inside
`SLOT.lock` — **the one file in this directory whose contract is "overwritten on every
acquisition".** The next agent to claim the slot rewrote it and ED-1..ED-14 vanished; what remained
were later blocks *citing ED-8, ED-9 and ED-11 by number, pointing at entries no longer in the
file.* **A record destroyed by the act of using the instrument** — which is verbatim the disease
#199 exists to fix, happening inside the document that describes it. Same family as ED-1 itself:
the lock lived inside the directory `cargo clean` wipes.

**So: this file is APPEND-ONLY and is NOT the lock.** Nothing here is rewritten. `SLOT.lock` may be
overwritten freely by whoever holds the slot; it points here.

**Entries below are restored from my own written record of each decision, not from memory.**

**Do not wait to be told.** Twice today my broadcast list came up short and the audit — not the
sender — found it. **The receiver has no instrument that says "you are missing a rule"**, and the
sender has no signal separating "delivered to 13" from "delivered to 9", because no-reply is the
normal state of a broadcast. Read this list and check yourself against it.

---

- **ED-1** — `SLOT.lock` lives at `D:/graphhelm-slot/SLOT.lock`, **outside any cargo target dir**.
  The old path (`D:/graphhelm-target-m10/SLOT.lock`) is dead and tombstoned. **Cause: `cargo clean`
  wipes the target dir, and the lock lived inside it — the operation that most needs exclusion is
  exactly the one that destroys the proof you hold it.** Generalise it to any marker: *does it
  survive the most destructive operation the protocol authorises?* If not, it disappears exactly
  when it is most needed, **and the absence reads as "free"**.

- **ED-2** — Claim protocol: **write → wait → RE-READ → confirm it is still yours** before running
  anything; re-check `tasklist` **after** writing. Writing a lock proves you WROTE, never that you
  HOLD. The file records **POSSESSION, not ORDER** — and it does not protect against **concurrent
  writes** either: on 2026-08-20 two agents read "absent" in the same second. **Order comes from the
  orchestrator.**

- **ED-3** — Before a gate whose verdict you will **CITE**: **FULL `cargo clean`**, not per-package.
  `cargo clean -p` can leave freshness bookkeeping believing a removed rlib is current.
  **Signature of that state: `check` green + `build` red.** Enumerate packages with
  `cargo metadata`, never by hand (a hand list had 10 of 21).
  **What gave it away was the CLOCK, not the error** — clippy "passing" in 53s and tests failing in
  4.3s, both impossible against 21 cold packages.

- **ED-4** — **Never suppress stderr in a step whose success you then report.** Cost measured: one
  hour spent unable to RULE OUT a hypothesis whose disproving signal had been thrown away.

- **ED-5** — `cargo check` does **not** need the slot. Per-agent **AND per-branch** dir
  `D:/gh-check/<letter>/<issue>` — one dir per agent is not enough when an agent has two branches.
  **Claim allowed: "type-checks against `<sha>` in an isolated target dir".** Not "verified", not a
  gate verdict, not PR evidence. **A check RED is real, cheap information; a check GREEN cannot be
  cited as "compiles"** — `check` green does not exclude `build` red.

- **ED-6** — Before any check outside the slot, append **START** and **END** to
  `D:/graphhelm-slot/check-activity.log`. **Isolation removes contamination, NOT contention.** A
  build alongside a timed run competes for CPU and corrupts wall-clock, **which is now a
  load-bearing oracle**. Third mode: not contaminate — **DISTURB**. The rule is not "do not run": it
  is **make the disturbance VISIBLE**. Whoever reports a timed run states *"check-tier activity
  during window: NONE"* or lists it. **A duration measured with concurrent activity is not wrong —
  it is WITHOUT DEFINITION.**

- **ED-7** — Pre-flight before any **READ** and any **RUN**: `git rev-parse --show-toplevel`,
  `--abbrev-ref HEAD`, `git worktree list`. First must be your own directory, second your lane's
  branch, third must contain your directory. **RE-RUN it — a path can change meaning mid-session**
  (a real worktree emptied by an external cleanup leaves a shell with the same name; commands right
  in the morning were wrong in the afternoon with nothing failing).
  **And it promises less than it will be trusted for: it makes the TREE trustworthy, not the CLAIM.
  Right tree and wrong object are independent failures.**

  **NAMED DANGER the pre-flight exists to catch (measured by H, 2026-08-20, found by D):**
  `D:/gh-storm-pre` and `D:/gh-storm-post` are **live git worktrees** from the storm lane, and each
  carries an **executable `ci/gate.ps1` of 215 lines with 13 `Out-Null` occurrences** — i.e. the
  **pre-#100** version, the one that swallows stdout. **Anyone who runs a gate in there gets that
  era's assumptions**, including whatever #200 is repairing, and the red is unattributable by its
  own log. **They are NOT to be deleted for now** (shared state registered in git; the house rule is
  add, verify, THEN delete — and the owner must first enumerate what is in them that is NOT in git).
  **The defence is not deletion, it is the pre-flight:** `--show-toplevel` must return your own
  directory and the branch must be your lane's — **whoever runs a gate in `gh-storm-pre` fails both
  lines.** The danger belongs to whoever skips the pre-flight, which is exactly what it is for.

- **ED-8** — **As MEASURER:** `git rev-parse HEAD` must equal the sha you will cite. Never consults
  `origin/*`, so no stale fetch can touch it. **As ADJUDICATOR:** put the ref **INSIDE** the command
  (`git grep <pat> <ref>`, `git ls-tree <ref> -- <path>`, `git cat-file blob`). **Before RUNNING:**
  assert the subject exists in this toplevel (`git ls-files --error-unmatch <path>`) — **its absence
  is what produces the silent "0 tests", which reads as a pass.**

- **ED-9** — The distance test (`git rev-list --count HEAD..origin/main`) is **SUPERSEDED as a
  general rule**. **Being behind main is the CORRECT state of a lane pinned to a named base.** And a
  check that fires on legitimate states **teaches everyone to ignore it**, after which it stops
  catching the one case it exists for. Second hole: `origin/main` is a **LOCAL** ref and fails in
  the **reassuring** direction with a stale fetch (measured **LATENT, not active**). It survives
  only as a SIGNAL THAT AGES, with its measurements and date: main checkout **22**, healthy lanes
  0–1. **Do not patch a question that should fall** — the two formulations in ED-8 are immune by
  construction, not by care.

- **ED-10** — **ONE TARGET DIR PER LANE**: `D:/graphhelm-target-<lane>`, on `D:`, never inside a
  worktree, never on `F:`. **The shared `graphhelm-target-m10` is retired for gate runs.** Cause: it
  produced `cargo test` GREEN and then `cargo build` failing on a type it had just compiled against,
  with artifacts stamped 13:10–13:31 in a directory verified EMPTY at 16:05 after two full cleans.
  **Files do not travel backwards in time**, so nothing measured there is citable.
  **A verdict you cannot cite is worth less than the rebuild you saved.** Each owner deletes their
  own when the lane closes.

- **ED-11** — **`export` does NOT persist between Bash tool invocations.** Every command is a fresh
  shell. `export CARGO_TARGET_DIR=...` in one command and `cargo` in the next builds with the
  variable **UNSET**, into `<workspace>/target` inside the worktree, on `F:` — **which is what filled
  the disk**, surfacing as five stages reporting "FAILED (exit 101)" that were link failures.
  **The env goes in the SAME command as the cargo call, every time.**

- **ED-12** — A **single named test** may run outside the slot in your own lane dir, claim bounded to
  *"this test, this dir, not a gate"*. Declare START/END in the activity log like any other run.

- **ED-13** — **A filesystem sweep from the repo root traverses every other agent's worktree.**
  Measured: `grep -rln "CONTENT_FREE_KEYS" .` from `F:/github/GraphHelm` returns a hit in
  `.claude/worktrees/b-170/...`, a string existing **only** in that lane's guard, in a file **absent
  from the main checkout**. There are 12 worktrees with `core/`. **So any filesystem enumeration
  from the root measures the UNION OF THIRTEEN TREES AT THIRTEEN DIFFERENT COMMITS.**
  **This is the erasure INVERTED:** everything else today removed things from view; this **ADDS**,
  and adds with an air of completeness — **the reader sees MORE evidence, not less, and "more
  evidence" triggers suspicion in nobody.**
  **AMENDED — it is not uniformly bad; it is the one instrument that gets BETTER from its defect:**
  - **NEGATIVE claim** ("X does not exist") → **STRONGER**. Zero across the union is zero in all
    thirteen, and your own tree is contained in it.
  - **POSITIVE claim** ("X exists / is here") → **INVALID**. The hit may come from another tree at
    another commit, possibly from an uncommitted experiment that exists in no `origin` at all.
  - **COUNT** → **INFLATED up to 13x**, and content dedup hides it further.
  **Operational rule: the union scope is for REFUTING. Never for asserting, never for counting.**
  A root `grep -rn` that comes back EMPTY is harder evidence than a `git grep` over one tree.
  **And the audit rule that follows: too-wide scope kills POSITIVE claims and does not kill NEGATIVE
  ones; too-narrow scope does the inverse.** When auditing an old sweep, ask first **what SIGN the
  conclusion had** — it decides on its own whether a redo is needed.
  Cure for the rest: `git grep <pat> <ref> -- <path>` descends into no worktree at all.

- **ED-14** — **Broadcasts ask for a RECEIPT, and the receipt says WHAT WAS READ**, not "received" —
  otherwise it is a signal with no content. **Absence of receipt = NOT DELIVERED. Never = agreement.**
  A broadcast whose delivery is unverified is indistinguishable from one that reached everyone:
  **the silence of someone who never received it reads exactly like the silence of someone who
  received it and did not object.** Neither side can detect a message that never arrived — hence
  both halves: **this index (pull) and the receipt (push).**

---

## Proposed, under test — extend MEASURER from CITATIONS to RUNS (C, with B)

Whoever runs declares the sha they intend to run against; the harness asserts `git rev-parse HEAD`
equals that sha **before compiling**, and prints it in the log. Never consults `origin/*`; no
threshold, so it does not decay; passes cleanly on lanes deliberately pinned behind main; and it
puts in the log **the two halves missing today: which tree it ran against, and which tests ran, by
name.** If it falls, it gets recorded as fallen.

---

## AUDIT OF THESE FOURTEEN (H, 2026-08-20) — (a) 1 · (b) 4 · (c) 9

The orchestrator bet that almost all of these were pure convention with nothing underneath. **The
bet holds, with two corrections that make it harder, not softer.**

**Correction 1 — the one (a) is not enforcement, it is RETIREMENT.** ED-9 counts as covered only
because it was DEMOTED from rule to signal-that-ages. **A rule that says "do not trust me" cannot be
violated.** Uncomfortable consequence worth keeping: **the cheapest path to (a) is often DELETING
the rule, not enforcing it.**

**Correction 2 — one of the (b)s is already DEAD. NOBODY READS `check-activity.log`.**
`grep -rl "check-activity" ci/ tools/` against `origin/main` = **zero**. ED-6's declared purpose —
being able to say *"check-tier activity during window: NONE"* — **requires a reader that does not
exist.** The sweeper rule biting our own protocol on the day we wrote it.
**ED-6 AMENDED, cheapest detection, builds nothing:** whoever reports a timed run **PASTES the log
lines covering their window**, or the literal string **"no lines in window"**. That converts an
unread file into a **cited artifact**, and the absence of the paste is itself visible.

**FINDING 1, which nobody expected: ED-1 SILENTLY BROKE THE GATE'S OWN LOCK READER.**
`ci/gate.ps1:195` reads the lock at `Join-Path $env:CARGO_TARGET_DIR 'SLOT.lock'`. **ED-1 moved the
lock out of every target dir on purpose.** So the gate can never find it again, and `:192-196`
records that absence as an ordinary state (`present:false`). **A fix in one place switched off the
evidence in another, and the switched-off evidence reports as a normal reading.** A future auditor
reads `present:false` and concludes "there was no slot discipline"; the truth is "the lock moved and
nobody told the reader". This is #200's class, now with its cause in a rule we wrote TODAY.
**Detection: the gate reads the canonical absolute path and records
`reason: 'lock not found at canonical path'` as DISTINCT from `CARGO_TARGET_DIR not set` — two
absences, two words.**

**HEALTHY ONES NAMED, with reasons:** ED-3 and ED-10/ED-11 are real (b) — the gate manifest records
`CARGO_TARGET_DIR` and the clean invocations with exit codes beside the verdict. **ED-13 is the
best-designed entry of the fourteen:** its amendment turns the defect into a rule (*the union
REFUTES, never asserts, never counts*). ED-14's receipt half is (c), **but the PULL half is real and
proved itself** — the auditor used it and found rules he did not know existed.

**NOT MEASURED, marked rather than assumed:** other agents' local hooks could enforce EDs and would
not be visible (only `ci/` and `tools/` at `origin/main` were inspected); **the manifest FIELDS were
measured to EXIST, not to be CONSUMED** — and that distinction is what separates (b) from (c) in
practice; ED-4's coverage outside `gate.ps1` was not enumerated.

**THE PATTERN, and it is the most useful result: the classes split by the SUBJECT of the rule, not
by the quality of its writing.**
  - a rule about **where a file lives** or **what a command contains** -> **(c)**, because the
    filesystem and the shell have no memory of intent;
  - a rule whose subject is **an artifact the gate already writes** -> **(b)**.
**The manifest is the only sweepable surface we have, and it is one file away from being a real
mechanism.**

---

## ED-14 AMENDED — a receipt with content still does not protect against narrow scope (H)

Two agents nearly produced a **false consensus with two names under one false sentence**, and it was
caught by the one being corroborated.

C sent corroboration: *"`grep -c '^ED-'` → 0, there is no index, two independent readings agree."*
H measured before agreeing. **The claim had its scope on the wrong file:** C grepped `SLOT.lock`
(correct for the file he read, false for the question he asked) **and with the wrong anchor** — the
entries are `- **ED-`, so his pattern would not have matched them even if they had been there.
**Two errors summing.** Ground truth: `ENV-DECISIONS.md`, 136 lines, `grep -c "^- \*\*ED-"` = **14**.

**And the mechanism of the near-miss is the part worth keeping: C was corroborating H's EARLIER
message, written before the rebuild.** H's absence was **true at 16:50**; C's repeats it against a
world that changed at 16:53. **Agreement between a CURRENT measurer and a measurer QUOTING A DEAD
STATE looks like independent confirmation and is not — it is one measurement, one copy of it out of
validity.** `absence-at-the-wrong-path` crossed with `measurement-under-mutation`.

**CURE, and it separates two holes ED-14 was collapsing:**
  - a receipt must say WHAT WAS READ (guards against an empty "received") — already in ED-14;
  - **a negative claim must travel with a POSITIVE CONTROL and with the INSTANT of measurement.**
    Here a plain `ls` of the directory would have shown the three files, and the timestamp would
    have shown the reading predated the rebuild.
**Content-in-the-receipt and scope-of-the-claim are different holes. Do not let one stand in for the
other.**

---

## RESTORED 2026-08-20 — blocks I DESTROYED while "moving" them here (B caught it)

**I rebuilt `SLOT.lock` at 13:53 and wrote in it that the doctrine had "moved here". It had not.**
`ENV-DECISIONS.md` was last written at 13:50 and never touched again, so ~62 lines that lived only
in the lock were overwritten by the rebuild. **"Already moved" sounds like a conclusion, nobody
checks the destination, and the loss files itself as a completed task** — B's own class, the one
that cost him two files this morning. The difference is that this time someone went and checked the
destination instead of the report.
He recovered the first 25 lines verbatim from his transcript. The rest is restored below from my
own written record of what I wrote — not from memory.

### THE UNIFICATION (C). If you read one block in this file, read this one.

    ENVIRONMENT STATE IS NOT LOADED. IT IS NAMED IN THE COMMAND, OR IT DOES NOT EXIST.

    which tree git reads     ->  put the REF inside the command
                                 `git grep <pat> <ref>`, `git ls-tree <ref> -- <path>`
    which vars cargo sees    ->  put the ENV inside the command
                                 `CARGO_TARGET_DIR=D:/graphhelm-target-<lane> cargo ...`

Both cures of today are the same movement, and neither depends on anyone REMEMBERING -- which is the
part that fails. ED-11 and ED-8 belong together, not in separate sections: whoever reads one applies
the other.
And the `export` cost did not come from anyone not knowing the fact -- it is written in the tool's
own description ("shell state does not persist"). **It came from the fact living in the prose ABOUT
the tool instead of at the moment of writing the command. A RULE THAT IS NOT WHERE THE HAND ACTS
DOES NOT FIRE.** Same reason C's backtick rule was written for `gh` and did not stop him in
`python -c`.

### Correction to ED-9's leftovers: do not patch a question that should FALL (C, against himself)

He proposed pasting `git fetch origin main` before the distance line, then withdrew it, and the
reason generalises: **that hardens a question neither surviving formulation needs to ask.**
    as MEASURER    -- `HEAD == the sha I will cite` never consults `origin/*`, so no fetch can be stale
    as ADJUDICATOR -- the ref is inside the command, so the read is anchored to what the command names
**Neither needs the fetch, so neither has the hole.** A patch that makes a check "safe if everyone
remembers" is worse than a formulation that needs no remembering. **Anchoring cuts the coupling; a
detector only watches it.** And the question does not vanish: it changes owner and instrument. It
decides a REBASE, never validates a measurement, and its ground truth is `git ls-remote origin main`.

### The design consequence that decides whether a detector is worth building

**A detector with an acceptable false-positive rate is not a weaker detector -- it is a detector that
switches itself off over time.** The tolerable false-positive rate is not traded against detection
rate; it is traded against the detector's SURVIVAL. One with 5% false positives and one real case a
month is dead before the first real case arrives.

## ED-2 AMENDED — the size guard hardens against the OBSERVED MECHANISM, again (B)

`lock longer than ~12 lines = suspect` catches exactly the instance we saw: long prose. **It does
not catch a SHORT, PLAUSIBLE write** — another agent stamping one `HELD by X` line over yours, or a
truncation to zero. **Third time today we hardened against the observed mechanism instead of the
class** (the lock moved out of the target dir died by a WRITE; the ancestor check was calibrated to
one distance; now size).
**The class cure is the one `check-activity.log` already demonstrated this morning: possession as
the last `CLAIM`/`RELEASE` line of an APPEND-ONLY file.** A clobber in an append-only file **shows
up as a line**, large or small — and the file that survived this morning was precisely that one.
Adopted as direction; not implemented mid-run. The `SLOT.log` in #202 is the same shape.

### Two fragments my restoration dropped, recovered from F's `cat` of the dead file

Diffed phrase by phrase against F's verbatim 71-line capture. Everything else was already here.
Two things were not, and they are the concrete halves — the abstractions had survived and the
INSTANCES had not, which is the usual direction of this kind of loss.

1. The parenthetical naming which rule is which, in the unification block:
   **ED-11 (`export` does not cross the tool boundary)** and **ED-8 (ref inside the command)**
   belong together. Without the parenthetical, the sentence names two numbers and no mechanisms.

2. C's first-hand instance under ED-13, which is the part that makes the rule believable rather
   than clever:
   *He ran exactly such a grep today (`--include=*.rs` from the root) and the output carried hits
   from FOUR other agents' worktrees — and he saw them. Nothing was corrupted, because he then
   re-derived with `git grep origin/main` — **but he re-derived because he wanted a named base, not
   because he had noticed the pollution. Escaped by the right rule applied for the wrong reason.**
   And the dangerous version is the same command when the output is COUNTED instead of READ: then
   there are no paths on screen to give it away.*

**Method note on the recovery itself:** three agents held partial copies of the dead file — B had
the first 25 lines verbatim from his transcript, F had 71 lines from a plain `cat` with no `head`
or `tail`, and I had my own written record of what I put there. **F's is the one that closed it,
and it closed it because he read the file with no limit and reported "I do not claim this is
complete" — B had measured 84 lines against his 71.** Neither of them treated their own copy as the
whole; both said which instrument they used and where it stopped.

## ED-14 AMENDED AGAIN — the receipt proves DELIVERY, not FRESHNESS (H, against himself)

H stopped on a false corroboration and then said why, and the why does not favour him:
**he did not stop by diligence — he stopped because he had JUST measured the index, and the
incoming claim collided with a FRESH measurement of his own.** *"If the same message had arrived
two hours earlier, I would have agreed without measuring, because it would have been my own claim
coming back."*

**FALSE CONSENSUS NEEDS NOBODY CARELESS. It only needs the second reading to arrive while the first
still looks true.** The receipt closes the delivery hole and does not touch this one.

**Cure, cheap and already used by three people today: a claim about state travels with the INSTANT
of its measurement.** Two readings of the same file minutes apart are not two witnesses — the
target moved. C read 186 lines / 13425 B; H read 136 / 10032. **They did not disagree; the object
did.** Saying "at HH:MM I measured X" makes that visible without anyone having to be suspicious.

- **ED-15** — **Before PUBLISHING an environment decision, ENUMERATE what reads what it changes.**
  Drafted by H at the orchestrator's request, from the audit finding that ED-1 silently disabled the
  gate's own lock reader.

  **The step goes into the act of DECIDING, not into the act of remembering.** "Enumerate the
  consumers next time" is a resolution; a command in the publishing checklist is a step. The
  distinction is the whole entry: every rule that failed today failed by depending on recall.

  **The command, so this is a step and not a moral:**
  ```
  git grep -n "<the-thing-that-moves>" origin/main -- ci/ tools/ apps/ core/    # code consumers
  grep -rln "<the-thing-that-moves>" .factory/                                  # prose consumers
  ```
  Run BEFORE publishing. Paste the result into the decision, including when it is empty — **an empty
  enumeration is a finding, not a formality**, and it is the only thing that distinguishes "nothing
  reads this" from "nobody looked".

  **AMENDED by L, 2026-08-20 — THE TWO COMMANDS ABOVE MISS THE DANGEROUS CASE.** They find
  consumers that **name** the moved thing. They do not find consumers that **DERIVE** it:
  `ci/gate.ps1:195` builds the lock path as `Join-Path $env:CARGO_TARGET_DIR 'SLOT.lock'` —
  **it fails because it derives the path from a variable that means something else**, and it
  **does not contain the string you are searching for**. A hardcoded stale path would have been
  easier to find than the real defect. Third command, required:
  ```
  git grep -nE "Join-Path .*<VAR>|\\\$\{?<VAR>\}?/|<var>\.join\(" origin/main -- ci/ tools/ apps/ core/
  ```
  (substitute the variable the path is derived FROM — here `CARGO_TARGET_DIR`.)
  **The general form: enumerate consumers of the VARIABLE the path is built from, not only of the
  path's own name.** L has this frozen as gate 5 on #200 in its durable form — **one definition of
  the path, or the next move breaks everything silently again.**

  **STATUS: the owed enumeration was RUN — by L, not by me, against `origin/main` @ `d0b3f04`,
  ref inside the command.** Result: **4 consumers of `CARGO_TARGET_DIR`, and exactly ONE derives a
  path** — `ci/gate.ps1:195`; the other three test presence or record the value. Positive control:
  `Join-Path` appears **26 times across 4 scripts**, so the pattern sees what it must.
  **L's sharper form, which supersedes mine: the risk is not "deriving paths" — it is DERIVING FROM
  A ROOT SOMEONE ELSE CAN REMAP.** The other 25 derivations use `$PSScriptRoot`,
  `$repositoryRoot`, `GetTempPath()` — roots that **do not move by decree**. `CARGO_TARGET_DIR`
  moved by decree twice in one day. **And the two commands are DETECTION; the single definition of
  the path is PREVENTION** — grepping does not replace it, or the next reader thinks it does.

  **FOURTH POPULATION, found by the fan-out and absent from every command above — AGENT MEMORY**
  (`~/.claude/projects/*/memory/`). Not executable, not in the repo, not in `.claude/settings*.json`,
  **shared by all sessions and injected into context at every start.** B's formulation, which is why
  it is the worst of the four: **a stale script fails by OMISSION (returns empty, and empty reads as
  "no slot"); a stale MEMORY fails by ASSERTION — it hands an agent a confident wrong answer BEFORE
  he measures, and frames his first measurement.** M found three DEAD prescriptions in his own and
  marked them SUPERSEDED rather than deleting, separating what survives (naming the env var inline)
  from what changed (its value).
  **And the repair itself has a failure mode, measured on me:** my bulk replace of the dead path ran
  through sentences whose SUBJECT was the old path and produced *"the old one (`<canonical path>`) is
  buried"* — **the memory teaching the exact inverse of ED-1.** Found independently by C, M and N,
  repaired by them, verified clean afterwards. **A `sed` hits the TEXT and misses the REFERENT: it
  cannot tell "use this" from "this one died".** Keep narrative mentions of a dead path intact —
  deleting them loses the reason it died — and change only prescriptive ones.

  **Why nothing could have caught ED-1 without it (the reason to bother):** ED-1 is a rule about
  **where a file lives**, and the filesystem has no memory of intent. The class of a rule decides
  what can enforce it — rules about paths and command text are unenforceable by construction, so the
  enumeration is the only moment where the consumers are visible at all.

  **Three instances it would have caught, all measured 2026-08-20:**
  1. **`ci/gate.ps1:195`** reads the lock at `Join-Path $env:CARGO_TARGET_DIR 'SLOT.lock'`. ED-1
     moved the lock outside every target dir, so the gate can never find it again and records the
     absence as an ordinary reading (`present: false`) — see #200.
  2. **The dead path is still cited in prose in eight places**: `.factory/d-agent-154-runbook.md`,
     `g-agent-roster.md`, `m-agent-typed-unverified-census.md`, `orchestrator-board.md`, and three
     of H's own memory files. **The enumeration finds the author's own documents too**, which is
     the case an author is least likely to check.
  3. **The ED index itself, written into `SLOT.lock`** — a declared record placed in a file whose
     contract is "overwritten on every acquisition". Same family, different consumer: the reader
     was the next agent to claim the slot.

  **WHAT THIS DOES NOT SOLVE, and it is the same trap as ED-9's:** if the enumeration is expensive,
  **the cheapest route to "compliance" becomes not taking the decision, or taking it without
  publishing it.** A conformance table that cannot tell *enforced* from *never declared* rewards
  silence. This entry is therefore only safe while the enumeration stays two commands long — **if it
  grows, it will be evaded rather than obeyed, and the evasion will look like there being fewer
  decisions to make.**

  **NOT MEASURED:** whether consumers exist outside `ci/`, `tools/`, `apps/`, `core/` and
  `.factory/` — agents' local hooks and settings are not swept by either command above, and a hook
  that reads a moved path would fail silently in exactly the way this entry exists to prevent.

## ED-8 CORRECTED — the "subject exists" line was WRONG and would BLOCK the run it protects (B)

**Wrong as written:** `git ls-files --error-unmatch <path>`. Measured on `b-170`, against the very
test cargo had just compiled successfully:

    git ls-files --error-unmatch core/governor/tests/source_invariants.rs -> exit 1  (pre-flight BLOCKS)
    test -e core/governor/tests/source_invariants.rs                      -> exit 0  (239 lines, it is there)

**Two defects stacked:**
1. **Wrong question.** `ls-files` answers *"is it TRACKED?"*; the pre-flight needs *"does it EXIST in
   this toplevel?"*. **Every test under development is untracked — that is the normal condition, not
   the exception.** B's original line said "the file under test EXISTS in that toplevel"; my adoption
   swapped existence for tracking.
2. **Flattened instrument — the exact thing B documented this morning.** `--error-unmatch` returns
   **the same code** for *"exists, not yet committed"* (recoverable) and *"does not exist"* (lost).
   That is how he reported six untracked when it was four untracked **plus two lost**.

**CORRECT LINE:** `test -e <path-under-test>`.
If you also want to know whether it is COMMITTED — which matters for a CITABLE verdict, not for
running — that is a **second line with a different meaning**, and the two together separate the
states instead of fusing them.

**The pattern, and it is worth more than the fix: THE CURE INHERITED THE DISEASE.** Two of B's
lessons were adopted, and the first was implemented with the instrument the second forbids — because
they were written in different places and nobody read them together. **Word for word the thesis of
the UNIFICATION block: a rule that is not where the hand acts does not fire.**

## AND A RULE FOR VERIFYING RECOVERIES (B, from his own over-reporting)

Comparing his recovered lines against the restored file:
  - by EXACT LINE (`grep -Fxvf`): **11 reported MISSING**
  - by SUBSTANCE (distinctive fragments): **all present**, reformatted inside ED-8

**Had he sent the first result, it would have raised a loss alarm over text that is there.**
**RULE: when verifying a recovery or a migration, an EXACT-LINE diff OVER-REPORTS loss, because a
legitimate restoration REFORMATS.** The right instrument is a search for distinctive substance.
And the cost of the false alarm is not noise — **it trains everyone to ignore recovery verification**,
exactly like a check that fires on legitimate states.

## ADOPTING SOMEONE ELSE'S RULE — the command goes QUOTED, not RECONSTRUCTED (B, and it cost us one)

The ED-8 pre-flight bug had a precise cause, and I gave it before B generalised it:
**"I read your fourth line and chose the command from memory instead of quoting it."**

**The reconstruction looks faithful.** *"the file under test EXISTS in that toplevel"* ->
`git ls-files --error-unmatch` is a plausible translation. **And that is exactly where the silent
substitution enters: whoever reconstructs chooses by MEANING, and the instrument they choose has
properties the meaning does not carry.** Here the chosen instrument flattens *"exists, untracked"*
into *"does not exist"* — the very defect its author had documented that morning.

**RULE: when adopting another agent's rule, the command travels QUOTED VERBATIM, never rebuilt from
the sentence.**

**And the complement, which B claimed as his own half of the fault: A RULE THAT NEEDS A COMMAND
SHOULD CARRY THE COMMAND.** His fourth line named no command and left the hole open for someone
else's memory to fill. A rule stated only as intent delegates instrument choice to every future
reader, and each of them will choose plausibly and differently.

**Pairs with ED-15:** enumerate the consumers before publishing, and publish the command, not the
intention. Both are about the same gap — **the distance between what a rule MEANS and what a hand
TYPES.**

## THE CHOICE OF WHAT TO SHOW IS THE CHOICE OF WHAT CAN BE DIAGNOSED (J, against himself)

J built a runner that filters cargo output to keep it readable. He then **passed known cargo lines
through his own pattern** and measured that `Blocking waiting for file lock on package cache`,
`Compiling` and `Finished` **all disappear**, while only `test result:` survives.
**So his results file cannot distinguish a cell that RAN from a cell that NEVER STARTED** — and the
filtered line was precisely the one explaining the real stall. **An empty cell beside a green one
reads as green.**

**He committed, inside the instrument he built to avoid it, the class he had described to another
agent twenty minutes earlier — and he MEASURED that rather than confessing it.**

**Distinction that separates it from the other patology of the day:** H's detector **switched itself
off over time**; J's was **BORN off**. Only one of the two improves with attention.

**And his refusal is the strongest evidence this factory has produced for mechanism over
discipline:** *"knowing how to name the class does not stop you committing it — which is why the
rule has to live in the ARRANGEMENT, not in the memory of whoever is writing."* It is evidence
against the one remaining alternative, that writing the lesson well is enough.

**Sealed before any output existed:** `test result:` mandatory as proof of execution; its absence is
**HARNESS-BROKE**, never a pass and never a red; neighbouring cells not promoted; the filter widened
on the re-run; and **do not edit a script while it is executing** — the shell reads incrementally,
so the fix becomes corruption.

## ED-15 AMENDED — the declared hole is NARROWER, and the reason is one nobody had noticed (H)

**All thirteen agents run as the same OS user.** So `C:/Users/gabri/.claude/settings.json` is not
"one agent's" file — it is the **user-level settings of all thirteen sessions**. Measure once,
answer for all.

  - user-level settings (shared by 13): hooks exist, **zero** references to `SLOT.lock`,
    `graphhelm-slot`, `graphhelm-target`, `CARGO_TARGET_DIR`
  - repo-level `.claude/settings.local.json` (also shared): hooks exist, **zero** references
  - per-worktree settings: **none exist** — thirteen folders swept, zero own `settings.local.json`
  - hook scripts under `.claude/`: none referencing those paths

**Two positive controls, because these are negative claims:** the sweep DOES find
`.claude/settings.local.json` where it exists (so it would see a per-worktree one); and the pattern
hits **7 times** in a file that does contain `graphhelm-slot` (so it is not a dead pattern).
**Without both, those zeros are indistinguishable from a broken command.**

**So the hole is not "thirteen private configs unasked" — it is TWO SHARED SURFACES (measured,
clean) plus ZERO PRIVATE SURFACES (measured).** The enumeration DOES reach hooks, once you know the
settings are shared per user and per repo.

**Still unmeasured, and asked rather than assumed:** configuration OUTSIDE those files — a shell
alias, a personal script outside the repo, a settings file at a path nobody named, a habit encoded
in something under `D:/`. Not sweepable; the question stands, in the reduced form (*"anything
outside `.claude/settings*.json` and the repo that reads slot or target-dir paths?"*), and what
comes back empty is **NOT REPORTED**, never "does not exist".

**And the note the measurement earns: THE SHARED SURFACE IS THE ONE THE ENUMERATION REACHES AND THE
ONE NOBODY THINKS TO ENUMERATE, BECAUSE IT LOOKS PRIVATE.** A bad hook there hits all thirteen at
once — **the cheapest surface to audit (one file) and the most expensive to break.**

## ED-6 WORDING CORRECTED — "nobody reads it" is too strong, and the correction matters (B)

The amendment said the paste was needed because **nobody reads `check-activity.log`**. The grep that
established it was scoped to `ci/ tools/` — so what was actually established is that **no CODE
consumes it**.

**It had two human readers today, exactly when it mattered:** B, discovering that J was running in
an isolated dir and self-declared not-a-gate; and F, seeing the same. **It was the instrument that
answered the question the destroyed lock could no longer answer.**

**The right phrase is not "it is not READ" — it is "it is not SWEPT".** On-demand human readers,
no sweeper. **The difference matters because "nobody reads it" suggests it is useless and invites
throwing it away, when it was decisive.**

**The conclusion is unchanged and stronger:** a file with on-demand readers and **no automatic
consumer** only makes evidence travel **if a person carries it** — so pasting is not redundancy, it
is **the only transport that exists**. Asked honestly, *who sweeps?* — **a person paying attention.
That is discipline, not mechanism.**

---

## BACKLOG RECORDED — four findings that arrived while the file was unwritable

**1. A SABOTAGE MATRIX IS ITS OWN CONTAMINATION CONTROL (J, sealed before the output).** His lane
dir was NOT virgin. He did not clean it; he sealed the criterion first: *"five identical lines ->
discard the run and clean."* **The matrix is five runs with DIFFERENT mutations to the SAME source
file — if the artifacts were not following the source, every line would come out THE SAME**, the
`uniform-output-is-harness-broke` signature. Observed: three distinct panic sites, two greens, one
control returning to green. **Artifacts ignoring his edits cannot produce that dispersion.** The
dirty directory is ruled out by the result's own shape, with no separate check.
Result: six sealed predictions, six confirmations, zero UNINFORMATIVE. **And the two lines that
existed to say the reinforcement bought nothing paid the other way** — the same mutations leave the
pre-reinforcement assertions GREEN.

**2. SENT / DELIVERED / ANSWERED — three states, not two (H, applying ED-14 to himself).** He
fanned a question to twelve; four queued, eight delivered. *"I know I sent twelve; I do not know
which twelve READ."* His table records **answered / not answered**, never **has / has not** —
because the silence of someone who never read and the silence of someone who read and has nothing
**have the same written form.** Same three-state requirement as #200's `present:false`. **He caught
his own class inside the instrument he built to impose it — the third of four such catches today.**

**3. A DEFERRED-VERIFICATION BATCH IS NOT N INDEPENDENT DEFERRALS — IT IS ONE DEFERRAL WITH A
GROWING TAIL (A, correcting C).** Each commit written against a tree that did not compile, so the
ninth rested on eight unverified claims beneath it. **"The marker must survive the sum" is not
enough: the marker must say HOW MANY ITEMS DEPEND ON IT.** *"typed-unbuilt (9th of an unverified
series)"* says what *"typed-unbuilt"* does not — **the first is countable from outside.**
Measured end: two sites not compiling and the store refusing the six events the lane existed to
create. **No false sentence; eleven commits in the dark.**

**4. ANNOTATING IS NOT UPDATING (C).** A path patch had injected its annotation INSIDE a command, in
a sentence reporting a MEASUREMENT — *"from an ex-worktree, `cat <path>` returned 'no such file'"*.
**Replacing the path would make the note stop describing what happened: nobody ran `cat` on the new
path.** So the body keeps the **measured** coordinates and the change goes in an annotation beside
it. **A record updated in the body stops being EVIDENCE and becomes a SUMMARY.**
**This refines the patch rule already recorded: excluding already-annotated lines is not enough — a
path patch must not touch sentences that REPORT MEASUREMENTS**, because there the old path is the
DATA, not a reference.
**And his concurrency guard was mechanism, not discipline:** the exact-match edit fails outright if
another session changed the text, so it **does not ask "did someone touch this?" — it makes
clobbering-without-knowing impossible.** That is how he found two files already fixed by others:
**his own task list had decayed while it waited.** *Measurement-under-mutation applied to a TASK
LIST rather than to a number* — editing blind from it would have rewritten already-correct text.

## WHY SOME PROSE SURVIVED THE PATCH AND SOME INVERTED — it was FORM, not care (L)

He swept his own memory against the inversion defect: nine occurrences, **zero inversions**, with a
positive control proving the pattern sees what it must.

**And the reason it was clean is the finding, not the result.**
  - The sentences that HELD name the subject **BY DESCRIPTION** — *"the old one, **inside the target
    dir**, is DEAD"* — so the identity travels **OUTSIDE** the string being replaced, and the patch
    swaps the string without touching the referent.
  - The sentences that INVERTED said *"the old one (`<path>`) is buried"* — identity **INSIDE** the
    thing being replaced.

**THE LAW, and it is the same on both sides of the house:**
  - **code** — never derive from a root ANOTHER PERSON CAN REMAP;
  - **prose** — never let the subject's identity depend on THE STRING YOU ARE GOING TO REPLACE.

`CARGO_TARGET_DIR` moved by decree twice today; `$PSScriptRoot` never moves. Same shape, two
materials.

**And the enumeration behind it, run by L because H's Bash was rate-limited** — against
`origin/main @ d0b3f04`, ref inside the command: **of all consumers of `CARGO_TARGET_DIR` in tracked
files, only ONE derives a path** (`ci/gate.ps1:195`); the other three test presence or record the
value. **He then went to see what root the other 25 `Join-Path` calls derive from** — `$PSScriptRoot`,
`$repositoryRoot`, `$binDirectory`, `GetTempPath()` — *because "only one" is worth nothing without
knowing what the others do.* Positive control: 26 `Join-Path` in 4 scripts, so the "one" is not a
dead instrument.
**And his correction to the ED-15 amendment: the two commands are DETECTION; the single definition
of the path is PREVENTION.** Without that, the next reader thinks grepping is enough — **and
grepping is exactly what fails when someone derives from a new root nobody yet knows is volatile.**

## AND WHY A "NOT MEASURED" MARK IS WORTH WRITING (L)

He could only pay H's debt **because H had MARKED it**. *An enumeration marked "OWED, not done" is a
legible request for help; an unmarked claim would have passed as measured and nobody would have gone
and run anything.*
**The mark is not humility — it is what makes the debt PAYABLE BY SOMEONE ELSE.**

## A PREDICATE THAT ANSWERS A DIFFERENT QUESTION THAN THE ONE THE GUARD ASKS (J, on his own refutation)

He sealed ten guards into two groups using the predicate **"does the fold arm exist?"**. It was right
for nine and **wrong for one** — and the diagnosis is not "close call", it is a **defect of
criterion**:

> **The guard does not ask about the arm. It asks whether the dependent becomes ready via the SHARED
> READINESS DERIVATION — a different code path. I applied a predicate that answers a different
> question than the one the guard asks.**

**A classification predicate that is right nine times out of ten does not thereby measure the right
property** — it may be correlated with it. And a correlation that holds for the easy cases is
exactly what nobody audits.
**And he refused to diagnose the one that refuted him:** not measured, another lane's terrain,
**reported as a FINDING and not as a conclusion.**

## AND THE RUN THAT DID NOT COUNT — because his own sealed rule killed it

All ten went red at the SAME panic site (`:97:40`, the append), **including his own registry guard**.
He had sealed: *"all red at the SAME panic site = they share a failure upstream of the assertions,
none of them counts."* **He did not read that as a result. He went for the cause.**

**Cause MEASURED, not reasoned:** he made the verifier PRINT what it serialises, and the wire showed
`key_fingerprint` / `manifest_hash` where the schema demands `keyFingerprint` / `manifestHash` under
`additionalProperties: false`. **`rename_all` on an enum renames the VARIANTS — fields INSIDE
struct variants need `rename_all_fields`.** *It is the trap the factory has in memory, living in the
code.*
**And he enumerated the CLASS instead of assuming a single instance:** `rename_all_fields` = **ZERO**
occurrences in all of `core/protocols/src/`, and sweeping every enum in `event.rs` returns **exactly
one** that needs it.

**After the fix, the sealed prediction survived the run that could have killed it.** Two guards went
red **on their own assertion** — the first time in that lane, and precisely what the reviewer of #193
had said never happened — with **three distinct panic sites as the control that they do not share an
upstream failure**. And the reds say WHY:
`left: None  right: Some(Refused { reason_code: SafeCode("unknown_identity") })` and
`the mistake is ON RECORD, readable by anyone replaying: {}` — **an empty `clearances` map.**
Membership-at-N demonstrated **in the guards' own assertion** instead of argued from a grep.
**A scoreboard that could have lost and did not: had those two come back green, #201 would have
closed as mistaken — his and the reviewer's.**

## ED-2 AMENDED — claiming requires TWO readings, because the lock answers the wrong question (H)

Measured: lock line 1 says `HELD by A Agent | 17:00Z | STATUS: RUNNING`; `cargo`/`rustc` processes
alive: **0**; control: `tasklist` sees 71 other processes, so the instrument is not dead.

**"RUNNING" and "finished and forgot to release" have the SAME WRITTEN FORM**, and the failure
direction is the expensive one: **the honest reader waits while the slot sits idle.** The dishonest
reader claims and collides — **the 15:41 incident with the polarity reversed.**

**THIRD FLATTENING OF THE SAME SHAPE TODAY, now in a STATE field rather than a result field:**
  1. `present:false` — *there was no lock* vs *I looked in the wrong place*
  2. broadcast silence — *did not receive* vs *received and did not object*
  3. `STATUS: RUNNING` — *is running* vs *finished without releasing*
**Three different fields, one defect: two worlds in one value.**

**THE MISSING STEP, and the cheapest detection already exists and costs one command:**
**claiming requires BOTH readings — *"the lock says X"* AND *"tasklist says N"*.** F did exactly
this at 15:41 (lock absent AND zero processes -> claimed); the orchestrator did it just now (lock
held AND zero processes -> asked instead of claiming). **Neither is enforcement; both are the same
one-line habit, and it was not written as a step.** Now it is.

**And the category error underneath, which ties this to #157:** the lock is a record of
**POSSESSION** being asked a question about **ACTIVITY**. The question anyone actually has is not
*"who holds it?"* but ***"can I run now?"*** — **a possession record answering an activity question
is the same category error as a state LABEL answering a dispatchability question.** It is the proxy
of #157 in another object.

**NOT MEASURED, and deliberately not inferred:** whether A's run finished, died, or is between
phases. **That is his to say** — deducing it from a process count would be exactly the proxy this
entry criticises.

## RETRACTED — my "duplicate kinds" recommendation was WRONG and its remedy was DELETION (J caught it)

I relayed a detector result as *"your two kinds are declared in two branches, identical"* and added
the inclination: *"they stay in HIS branch — you only have to DELETE."*
**J measured before combining, and the premise is false.**

Verified independently by me, ref inside the command, with a positive control:

    git grep -c "clearance_identity_registered" origin/issue-160-event-family-readiness -- schemas/
      -> no hits
    positive control, same branch, same path scope:
    git grep -c "completion_claimed" origin/issue-160-...  -> 3 files, so the command reads that tree
    refs that DO declare it: origin/issue-161-clearance-identity-registry, origin/j-161-...-guards

**A's branch never declared them — not "removed since", but never, in that branch's whole history
(`git log -S` returns 0 commits).** And the refs that carry them are **the same lineage**:
`4645d75` (archive) is an ANCESTOR of the head, with `pr193` in between. **One declaration seen three
times, not three declarations colliding.**

**HAD HE ACCEPTED THE REPORT AND DELETED, HE WOULD HAVE DELETED THE ONLY DECLARATION IN THE
REPOSITORY — and since A never had them, nothing would have put them back.**
**The detector's error is benign in READING and destructive in ACTION: the remedy for a false
duplicate is deletion.** That asymmetry is the whole reason this class matters.

**THE DEFECT IS THE DETECTOR'S, NOT ITS AUTHOR'S: a detector that enumerates REFS and counts
declarations must COLLAPSE BY ANCESTRY before calling anything a duplicate.** Refs are not
independent branches — an archive branch, a `pr###` ref and the head are the same work from three
angles. Without a `git merge-base --is-ancestor` test, **any work that has an archive branch or a PR
ref appears as a duplicate of itself** — and **the more disciplined the author (preserving SHAs,
naming archive branches), the more false duplicates they generate.** J's discipline of preserving
the guards at `4645d75` so a redo would not lose them **is literally the cause of the false
positive.**

**What is NOT retracted:** the `sweep_performed` / `overdue_exception` collision is between two
genuinely independent lineages, and the A -> K ordering does not depend on this in any way.
**Only the clearance line was wrong.**

**And the method note is his:** A had told him in the morning *"your two are NOT in yet; I apply them
next"* — **he had that sentence and went and measured anyway.** Had he treated it as sufficient, he
would have spent the afternoon arguing about the shape of a fact that did not exist.

## ED-3 AMENDED — the CLOCK oracle needs a DENOMINATOR (B, against his own measurement)

Three runs in the same isolated dir: **47s -> 5s -> 3s.** By the clock heuristic that carried the
day (*"53s of clippy against 21 cold packages is impossible"*), the 5s and 3s read as a **falsely
warm tree**. **They are not — and the proof is in the tool's own output:** runs 2 and 3 print
**`Compiling graphhelm-governor`** and nothing else. One crate recompiled, because one file changed.

**The clock alone would have produced a FALSE POSITIVE here.** It needs the **list of crates
recompiled** beside the duration. **A duration without that list is a number with no denominator** —
the same shape as the activity log with no sweeper, and as N's law that an exhaustiveness instrument
must report its own denominator.

**Corollary, and it is the safe half:** the clock is a good oracle for **"was this tree cold?"** only
when the expected work is known. **Cold-vs-warm is not readable from time alone; it is readable from
time PLUS what was rebuilt.**

## AND: COLD BY CONSTRUCTION BEATS CLEAN BY OPERATION (B)

`D:/gh-check/b/170` **did not exist** before the run, so there was nothing to clean. **That is
stronger than a full `cargo clean`: it does not depend on the clean having removed everything.**
ED-3's full clean remains required where a dir already exists — but **a fresh per-lane dir buys the
same property without an operation that can partially fail.**

## INSTRUCT THE SEARCH, DO NOT ENUMERATE THE POPULATION (L, on a comment that warned against itself)

PR #206 adds a signpost arguing — correctly — that the mechanical version must **derive** its list
from the manifests, because *"a hand-maintained list would shrink in silence like #98's allowlist"*.
**And then it names the two targets of today in prose.**

**The comment CONTAINS the hand-maintained list it warns against.** That list rots the moment a
third target with `required-features` appears — **and it rots in the DANGEROUS direction**: a future
reader checks the two named, finds them running, and concludes the concern is handled.

**DURABLE FORM: instruct the search instead of enumerating the population.**
*"every `[[test]]` with `required-features` in any `Cargo.toml`; grep for `required-features` rather
than trusting this list"* — **the two names serve as EXAMPLE, not as population.**
Same shape the factory already learned for headers of dead claims, and the same family as #190
(a count kept beside the thing instead of derived from it).

**Two more from the same review, both worth keeping:**
- **A text can be unreadable as a guard by whoever READS it and readable as one by whoever SKIMS
  it.** The paragraph opens *"`--all-features` IS LOAD-BEARING HERE"* and only later says *buys
  PLACEMENT, not enforcement*. **"Load-bearing" answers a different question than the one a person
  narrowing flags is asking** — they may read "carries weight" as "someone is watching". One word
  fixes it: *"load-bearing HERE, and UNGUARDED"*. **The sentence that matters must not depend on the
  reader reaching the second paragraph.**
- **"All three sites" was silently scoped to one file:** `ci/gate.ps1` has three invocations,
  `ci/postgres.ps1` has two more. The omission is defensible — **but the REASONING has to be in the
  sentence**, because whoever verifies by grep finds five and cannot tell a considered boundary from
  a forgotten site.

**And the number the independent derivation supplied: NINETEEN tests** (3 + 16 attributes across the
two targets, none `#[ignore]`d, and the PostgreSQL stage runs `-- --ignored`) **that run in the
workspace stage and nowhere else.** Two agents arriving by separate paths is worth more than either
saying it.

## WHEN A RECORD MIXES EPISTEMIC GRADES, THE GRADE MUST TRAVEL IN THE VALUE (C, on #202)

The three-condition boundary for accepting a RED as unrelated is **guard on the cheap one and honour
on the decisive one**:
  - **checked:** *the failing file is outside the diff.* **Proves almost nothing** — a typical
    regression breaks a test in ANOTHER file. Where it is good is at **REFUSING** the obvious case.
  - **not checked:** *it failed and passed in the SAME run.* **That is the decisive evidence of
    flakiness**, hence of non-attribution. **And it is honour.**

**The defect that follows: the DOC distinguished verified from asserted and the RECORD did not.**
The manifest wrote `unrelatedTestFile`, `unrelatedIssue` and `failThenPassObserved = $true` as
**flat siblings** — the third wearing the same shape as the first, and the first had been through
`git log --name-only`.
**The risk was never that someone would falsify all three: it was that the RECORD WOULD LOOK LIKE
ALL THREE HAD BEEN CHECKED.**
**Fix: `unrelatedTestFileVerifiedAgainstDiff = true` and `failThenPassObserved = "asserted-by-holder"`
— the GRADE travels in the VALUE, never in a comment beside the JSON.** A comment is not queryable
and does not survive being read by a machine.

**Two more, both about absence:**
- **A divergence the doc itself called "worse than no class" was REACHABLE** — a second copy written
  without `try` under `'Stop'`, with the first already on disk — which made a sentence of the
  author's own security review **false**. Fixed, and he added what was not asked: **on error, name
  the copies already written and say they now DISAGREE.** *An error that says what has already
  happened is worth more than one that only says it failed.*
- **`relatedToDiff` had no path to `true`.** Whoever read the failure and concluded *"this is mine"*
  was left with `null` — **the same value as someone who never judged** — on the very axis the PR
  exists to create.

**And one word: `Closes #199` became `Refs #199`**, because the issue's third acceptance criterion
is unrun — **and it is the criterion the issue itself flagged as most likely to be skipped.** It is
the only one that proves the collection **MEASURES** rather than merely **EXISTS**.

**Two of the reviewer's five sealed predictions fell in the author's favour, and one was a demand he
had already written:** he was going to require a `finally` around the manifest write. **The
`RUN-START` before any stage answers it better** — *a START with no END IS the record of a dead
run.* **Absence turned into a PAIR instead of being wrapped.**

## ED-2 AMENDED AGAIN — a SINGLE negative probe of SLOT.lock is NOT evidence (N, with the mechanism)

N had logged an anomaly he could not explain: `[ -f /d/graphhelm-slot/SLOT.lock ]` and a `find`
returned nothing, and minutes later the same two commands found it. He refused to build on the
negative. **The mechanism arrived when a backgrounded command finished and could be compared:**

    background probe : SLOT.lock 2962 B  mtime 13:46 | check-activity.log 2388 B 13:18
    later, directly  : SLOT.lock 5096 B  mtime 13:48 | check-activity.log 2505 B 13:49

**The file changed size AND mtime between the two observations. It was being REWRITTEN while he
probed it** — and a probe during a delete-and-create (or a Windows replace) can legitimately see
nothing. Consistently, his grep for `ED-` found no file: `ENV-DECISIONS.md` has mtime 13:50, later
than the state that command read.

**THE RULE: a single negative probe of the lock does not support the conclusion people draw from it.
The file is, BY CONTRACT, rewritten on every acquisition — so it has a window in which it does not
exist, and THAT WINDOW IS EXACTLY WHEN THERE IS ACTIVITY.**
**The absence is MOST LIKELY precisely when the slot is changing hands** — i.e. when someone most
wants to know. *Whoever probes once and acts reads the second state as the first.*

Same family as ED-1 (lock inside the directory `clean` wipes) and as the 15:41 collision: **the
lock's absence does not distinguish "slot free" from "slot changing owner."**

**Practice, pairing with the two-reading rule already in ED-2: read the lock TWICE with an interval,
or read the `check-activity.log` alongside — it is append-only and has no such window.**
**And this is the strongest argument yet for possession as the last `CLAIM`/`RELEASE` line of an
append-only file: a file that only grows never has a moment of not existing.**

**And the method note is his:** he had marked it *"anomaly, not building on it"* — right not to build,
**and the cause is what the note was missing to be worth anything to anyone but him.**

## THE VERIFICATION BAR RISES WITH THE REVERSIBILITY OF THE ACTION, NOT WITH DISTRUST OF THE SOURCE (J)

He measured before acting on my relayed detector result — and refused the credit I gave him for it:

> *"You said I read 'whoever sees a difference first says so' as an obligation to SEE rather than to
> REPORT. That is true, but the reason I went to look was more mundane: **I was about to DELETE code
> of mine.** The proposed action was destructive and irreversible on my side, and that is what makes
> me measure — not a virtue of method. **Had the recommendation been 'add a line', I would probably
> have added it without checking anything.**"

**RULE: raise the verification bar with the REVERSIBILITY OF THE ACTION REQUESTED, not with the
credibility of the source. A report that asks for DELETION gets measured, always — including when
it comes from whoever coordinates.**

**Why it beats "verify claims":** it is targetable. Nobody can verify everything, and "trust the
source less" is unactionable advice that decays into either paranoia or nothing. **Reversibility is
a property of the request, readable before any judgement about who sent it.**
And it pairs exactly with the asymmetry that made the false duplicate dangerous: **the detector's
error was benign in reading and destructive in action, because the remedy for a false duplicate is
deletion.** Same edge, two directions — **the actions whose false positives cost the object are
precisely the ones that must not be taken on a relay.**

It happened by accident here. **It is worth having by design.**

---

## ED-16 — THE GATE DOOR CHECKS THE KIND OF TARGET DIR, NEVER ONE INSTANCE (Orchestrator, adjudication)

**The conflict, raised by L and confirmed at the source by H, was real and blocking.**
- `origin/issue-166-target-dir-refusal:ci/gate.ps1:109` pins `$expectedTargetDir =
  'D:/graphhelm-target-m10'` and compares it EXACTLY (`:137`), refusing anything else with exit 1
  before any stage.
- **ED-10 says, verbatim: one target dir per lane, and "the shared `graphhelm-target-m10` is
  RETIRED FOR GATE RUNS."** Explicitly for gate runs. L offered the reading that would have let the
  door stand — *"maybe ED-10 only covers isolated checks"* — **and the registry says the opposite.**

**Therefore, if #187 merged as written, the gate would REFUSE TO RUN, before any stage, for every
lane that follows ED-10** — with a correct, authoritative message naming a directory the factory
retired. **The door working exactly as designed, blocking everyone.**

**ADJUDICATION: the door must accept the FAMILY `D:/graphhelm-target-<lane>`, not one literal.**
ED-10 is not amended; the door is.

**[LABEL CORRECTED by L, who owns the numbering: this is his outcome (1) — "the door changes" — and
the first draft of this entry called it "(2)", which in his list means "ED-10 is too broad and the
fix is in the registry". The substance was never in doubt; the NUMBER pointed at the opposite
remedy. Corrected here because this entry is durable and cites HIS numbering: a reader following it
to his #187 comment in a month would have found the reverse.]**

**And the reason is not "ED-10 is older" — it is that a literal instance is the wrong UNIT for this
check, and we have now paid for that mistake four times in one day:** the retired lock path still
addressed through `CARGO_TARGET_DIR` (#200), a count literal disagreeing with the names beside it
(#160), a hardcoded known-bad list rejected in favour of a structural tripwire (#200 §4b), and now
this. **The door's job is to refuse target dirs that are wrong in KIND — unset, inside a worktree,
on `F:` — never to pin one instance, because the instance is exactly the part the factory changes.**
The check becomes structural: on `D:`, matching `graphhelm-target-*`, not inside any worktree.
**A structural check survives the next ED; a literal one is superseded by it and keeps enforcing the
dead value, loudly and with authority.**

**THE IRONY IS THE LESSON, NOT THE JOKE (H):** #166/#187 is the lane that exists to turn the target-
dir decree from discipline into MECHANISM — **and the mechanism it builds encodes the SUPERSEDED
value. The mechanism outlived the decree it enforces, by hours.**

**What does NOT change:** L's [APPROVED] on #187 stands and was correctly not withdrawn. **The door
does what it promises and was measured doing it; what changed is the VALUE it imposes.** The
approval was about the mechanism; the merge decision has new information.

**Also true and to be fixed in the same pass:** F's positive full gate ran against
`D:/graphhelm-target-m10`, i.e. against the retired dir. That run's mechanical result stands, but
nobody may cite it as a gate run under ED-10.

---

## ED-17 — CHECK TIER EXTENDS TO BUILD+RUN IN AN ISOLATED DIR (Orchestrator, on K's ask)

ED-5 granted `cargo check` outside the slot in `D:/gh-check/<letter>/`. **Extended: building and
running a SMALL CLI or a single named test binary in the agent's OWN isolated dir is also legal
outside the slot**, under the same discipline: window declared in `check-activity.log`
(START/END), claim bounded to *"this binary, this dir, NOT a gate"*, and never while it would
compete with a declared gate run. The trigger was `schema digest` for the catalog pin: it is
build+run, not check, and K refused to stretch ED-5 himself — **asking instead of stretching is
what makes the extension safe to grant.**

---

## ED-18 — NO CODE PR MERGES WITHOUT A CHECK OF THE MERGE RESULT (Orchestrator, after main broke)

**Instance:** `origin/main` at `d796076` fails `cargo check --workspace --all-targets` — E0004
non-exhaustive match in the schema-evolution conformance target, because #193 added two `EventKind`
variants and main's existing `wire_name()` match does not name them. Libs compile; test targets do
not. **Both branches were green; main is the combination no gate ever saw.** Second instance of the
same structural hole in two days (K's contract collision was the first): *intra-branch parity is an
intra-branch claim; nobody had an instrument pointed at the INTEGRATION result.*

**Rule: a PR that touches code does not merge until `cargo check --workspace --all-targets` has
been run on the MERGE RESULT (current main + branch), under check-tier discipline (ED-5/ED-17 —
isolated dir, window declared, claim bounded to "type-checks against <main-sha>+<branch-sha>").**
Docs-only and .factory-only PRs are exempt. The orchestrator does not merge without this line in
the PR or a report naming it.


> **APPEND RULE, learned from this entry's own journey:** ED-18 was written on the
> orchestrator's board branch and was UNREADABLE from main for half a day while a rule cited it
> as binding — the committed-vs-reachable defect, on the registry itself, second instance. From
> now on every new ED is appended on a branch off main and merged the same hour. A rule a
> reviewer cannot read by the reader's path is not yet a rule.

---

## ED-18, READER-PATH AMENDMENT (B): the verification command dies on Windows for dot-paths

`git show origin/main:.factory/ENV-DECISIONS.md` dies in Git Bash on Windows: MSYS reads `x:.y` as
a POSIX path-list **whenever the path after `:` starts with a dot** — `.factory/`, `.github/`,
`.claude/` — exactly where this registry lives, and the failure reads as "the rule is not there".
Working forms: `MSYS_NO_PATHCONV=1 git show "origin/main:.factory/ENV-DECISIONS.md"`, or
`git ls-tree` for the sha + `git cat-file blob`, which carry no `:` at all.

---

## ED-19 — `git stash` IS ONE SHARED STACK FOR THE WHOLE REPOSITORY, ACROSS ALL WORKTREES (from F's near-incident)

Measured live: a reflex `git stash` + `pop` while switching branches brought back **another
agent's stash entry** from another lane entirely (a merge conflict stopped the pop, so git
preserved the entry — no loss). The stack held three entries from three different lanes, at least
two used DELIBERATELY as coordination ("recover with git stash pop" written into the entry name).
**Rule: do not use `git stash` in this repository unless you have just run `git stash list` and can
name the owner of every entry — and never `pop` blind; `pop` takes the TOP of a stack thirteen
lanes share.** Preferred alternative: a WIP commit on your own branch (`git commit -m "wip"` +
later `reset --soft`), which is per-branch by construction. Entries used as coordination belong to
their writers; touching one is touching another lane's state.

---

## ED-20 — A DECLARED GATE DOES NOT START OVER AN OPEN WINDOW (from D's overlap measurement)

Measured: a full cold gate STARTED at 09:42:56Z inside a check-tier window opened at 09:38:27Z and
still running at 10:15Z — different target dirs, so shared-target contamination is NOT established;
what is established is TEMPORAL OVERLAP, and load contention is exactly what the house's flaky
files respond to. **Rule: before starting a DECLARED GATE, read `check-activity.log` for STARTs
without ENDs. Either wait for the END, or start anyway and NAME the overlap in the gate's own
record ("check-tier activity in window: <who>, <span>") — an unnamed overlap makes the gate's RED
unreadable.** The reverse half already exists (ED-17: check-tier never competes with a declared
gate); this closes the race where the window opens FIRST.

---

## ED-21 — AN EXPLICIT TOOL FLAG OVERRIDES THE PROJECT'S CONFIG FILE, AND THE WRONG RESULT PASSES (from A's hotfix #245)

Measured: `rustfmt --edition 2021` was run out of habit on three files. The workspace is **edition
2024** and carries a `rustfmt.toml` (`edition = "2024"`, `newline_style = "Unix"`). The
command-line flag does not confirm or complement the config file — **it overrides it.** Two files
were reformatted under 2021 rules and **both exited 0**: a file formatted by the wrong rules is
still a valid file, and the `--check` that followed carried the same wrong flag, so instrument and
subject shared the defect and therefore agreed.

**What caught it was an accident, not the method.** The third file failed to parse
(`let chains are only allowed in Rust 2024 or later`) because a sibling module uses let-chains.
Without that, the wrong formatting would have been committed and `cargo fmt` in the gate — which
reads the config — would have failed afterwards, with the defect already in main and looking like
the gate was wrong.

**Rule: run a tool that has a project config file WITHOUT the flags that config already fixes.**
Here that means `cargo fmt` (which reads `rustfmt.toml` and the edition from `Cargo.toml`), or
`rustfmt` with no `--edition`. Pass such a flag only when the intent is genuinely to diverge from
the project config, and say why. **Generally: before passing an option that a project config file
also defines, ask which one wins — the answer is very often the flag, not the project.**

**Companion check, because the failure mode is silent:** after any bulk reformat, run
`git diff --stat` and confirm the change is formatting and not line-ending churn. With
`core.autocrlf=true` on Windows a whole-file CRLF rewrite is indistinguishable from a formatting
change by eye. The tell is proportion: real formatting touches some lines (here 31 insertions / 27
deletions across three files, with the CR byte count moving with the line count); an EOL rewrite
touches every line in the file.
