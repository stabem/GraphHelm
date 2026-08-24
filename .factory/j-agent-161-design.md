# #161 design — clearance + the countersign identity registry (lane 2)

Author: J. Phase 1 (slot-free): design, test criteria, red fixtures. No code.
Sources read in the artifact, not in summary: issue #161 (sealed cells), B's
blueprint v3 §2c/2d/2e (`.factory/b-agent-159-blueprint.md`), #159 anchor,
#153 kill-bar item 3, #160 (lane 1, my dependency), and the existing keyring
(`adapters/sealed-key-provider/src/keyring.rs`, located via codebase-memory —
index root = main checkout; status read at query time, not cited by a stale sha).

Written to the MAIN checkout deliberately: this lane's predecessor evidence died
in an emptied worktree today. A worktree is a bench, not an archive.

---

## 1. The load-bearing property, and the trap inside it

Required: **"who could countersign at sequence N" answerable from the journal
alone** (kill-bar item 3 applied to identity). Registry at N =
spec-declared ∪ registered − revoked, evaluated AT N.

The fold gets this **for free, by the order of its own walk**: a left fold in
sequence order holds exactly the pre-N state when it reaches the event at N. No
history map, no per-sequence snapshots.

**That freeness is the trap.** The property is not a data structure, it is a
DISCIPLINE about WHEN validation happens, and three plausible implementations
break it while looking correct:

| Break | What it does | Consequence |
|---|---|---|
| Validate clearances in a second pass, after the fold | Compares against the FINAL registry | Revocation becomes RETROACTIVE: a clearance valid at N is refused because its signer was revoked at N+k |
| Same, other direction | Final registry contains an identity registered AFTER N | **A clearance signed by someone who could not sign at the time is ACCEPTED** |
| A surface answers "can X sign?" from the projection's CURRENT registry | Current is not at-N | Historical audit gets a confident wrong answer |

The second row is the security-relevant one, and it sharpens this lane's sealed
`UnknownIdentity` cell: the interesting red is **not** an identity absent
everywhere — any implementation catches that. It is an identity **present in the
final registry and absent at the claim's sequence**. A test that only tries a
never-registered identity passes against all three broken implementations above,
which is precisely the adjacency-shaped weakness: an expected value derivable
without doing the work.

This is the #88 named cause one layer up: a LAST-STATE view answering a
PER-SEQUENCE question. There the maps were per-session; here the registry is
per-execution. Same defect, same remedy — the answer comes from the ordered
walk, never from the end state.

**INVARIANT TO PIN (my wording; reviewer amends before code, as C amended the
#55 invariant in M09):**

> Neither revocation nor registration is retroactive. A clearance's validity is
> decided by the registry as of the clearance's own envelope sequence, and no
> event after it can change that verdict. Replaying the same journal always
> reaches the same verdict for the same clearance.

## 2. Fold shape

```
registry:   BTreeSet<Identity>              // accumulator == state at the cursor
clearances: BTreeMap<claim_seq, Outcome>
```

- `clearance_identity_registered` inserts; `clearance_identity_revoked` removes.
  The spec-declared set seeds the accumulator at genesis (governor path, D-039,
  one entry road).
- `completion_cleared`:
  - `claim_seq` must name a CLAIM. Not a claim ⇒ `ReplayError::Corrupt`
    (uninterpretable log — the blueprint's rule and the M09 refusal split).
  - `Countersign { identity, key_fingerprint }`: `registry.contains(identity)`
    **evaluated now, mid-walk**, and fingerprint equal to the registered
    fingerprint. Failure ⇒ recorded refusal `UnknownIdentity`; replay fine.
  - `MachineReplay { manifest_hash }`: equals the node's declared manifest hash
    ⇒ cleared; else `HashMismatch`, recorded, replay fine.
  - On success the state transition lane 1 already emits, so
    `ready_set(spec, states)` keeps its signature and BOTH drivers inherit with
    zero edits (sync CLI `execution/driver.rs`, async `core/runtime/src/driver.rs`,
    both through `dispatch_candidates`).
- `completion_rejected`: consumed; the node does NOT unlock; reason recorded.

**The fold never touches key material.** It compares a journaled fingerprint to
a journaled fingerprint. That is what keeps membership-at-N a pure function of
the journal.

## 3. MARKED decision resolved: countersign key custody

The blueprint marks custody as this lane's call, so I read the keyring before
deciding: `KeyringDocument { format_version, key_id, authentication_tag }` —
**one key id, authenticated by a tag**. It is sealing machinery for the events
key, not a multi-identity public-key store. Riding it as-is means bending a
single-key artifact into a keyset.

**Decision: split custody from the fold, and DECLARE the crypto gap instead of
faking it.**

1. Fold + registry (this lane): journal-only, no key material, fully guarded.
2. Cryptographic verification of an actual signature: command layer, and **not
   implemented in this lane**. It is a DECLARED gap, refused at the site that
   would perform it — the command layer must refuse to APPEND a countersign
   clearance it cannot verify, with a named code, until custody lands. A
   declared refusal at the executing site beats a field with no consumer.
3. Custody shape when it lands: per-identity public keys as verified children of
   the anchored keyring directory, reusing `open_and_verify` and
   `verify_child_identity` (symlink-never-followed, bounded reads) rather than
   widening `KeyringDocument` into a keyset. Named here so the next lane
   inherits a decision rather than a blank.

## 4. Test criteria

Each red observed BEFORE fix code, at its own assertion, panic site named.

- **R1 UnknownIdentity, the SHARP form** (§1 row 2): X is registered at sequence
  c; a clearance signed by X names a claim at sequence b, with b < c. The FINAL
  registry CONTAINS X. Must refuse `UnknownIdentity`. *This is the cell that
  discriminates a correct fold from all three broken implementations. The
  never-registered variant is kept as a cheap companion, never as the headline.*
- **R2 Late revocation is not retroactive**: register X @a, clearance by X @b,
  revoke X @c, with a < b < c. The clearance stays CLEARED on every replay and
  the downstream stays unlocked — the availability half of the same invariant.
- **R3 HashMismatch**: a `MachineReplay` bundle hash that does not match the
  node's declared manifest ⇒ refused `HashMismatch`, node stays parked, replay
  fine.
- **R4 Downstream unlocks ONLY after cleared**, at the `ready_set` grain, on
  BOTH drivers — the two-dispatch-drivers rule: a sabotage in one driver alone
  must not leave the suite green.
- **R5 Registry replay determinism**: a journal with interleaved
  register/revoke/clear reproduces identical membership-at-N and identical
  clearance verdicts across repeated folds.
- **R6 Corrupt vs refused split**: `completion_cleared` naming a `claim_seq`
  that is not a claim ⇒ `ReplayError::Corrupt`; a clearance refused for identity
  or hash reasons ⇒ recorded event, replay Ok. One fixture per side so the split
  cannot collapse into one.

## 5. Sabotage list

Each applied individually; report WHICH guards stay green BY NAME.

- **sA** validation moved to a second pass over the final registry ⇒ R1 red (it
  accepts the invalid clearance) AND R2 red (it refuses the valid one). Both
  directions from one edit — the cheapest proof that walk-order is load-bearing.
- **sB** fingerprint equality dropped, membership only ⇒ a clearance with the
  right identity and a wrong fingerprint must fail; R1's companion.
- **sC** the `Corrupt` arm softened into a recorded refusal ⇒ R6's Corrupt side
  red.
- **sD** the `ready_set` transition also emitted on `completion_rejected` ⇒ R4
  red on BOTH drivers. If it reds on only one, the other driver's guard is
  decoration, and that is a finding about the guard rather than about the fix.
- **sE** registry seeded from spec but `revoked` ignored ⇒ R2 stays GREEN (it
  asserts a clearance SURVIVES revocation) while a "revoked identity cannot sign
  a LATER claim" case reds. Named in advance as an expected-green so the pair is
  read as a pair: R2 alone does not measure revocation.

## 6. Open items for the reviewer, named rather than assumed

- The §1 invariant wording is mine and should be amended before code.
- Whether the command layer's pre-flight read may use head-state: I claim YES
  and that it is safe, because at append time head IS N. It deserves a second
  pair of eyes precisely because it is the one place where "current" and "at-N"
  legitimately coincide — and that coincidence is what makes the pattern easy to
  copy into a surface where they do NOT coincide.
- Dependency: R4 needs lane 1's transition landed. R1, R2, R5 and R6 are
  fold-and-journal only and can be written and RED before #160 merges.
