# #211 slice: the certification surface — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` or
> `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`)
> syntax for tracking.

**Goal:** Make `graphhelm quality certify` able to certify **more than one gate**, each still
carrying its own pathogen suite, and validate a JPD document against its declared schema before any
gate sees it.

**Architecture:** Two branches, in a forced order. The gate-machinery half adds a floor to `certify`
so an empty suite can no longer certify vacuously; the surface half derives the gate registry from a
single list and registers the retry-lineage gate as its second entry. The closed registry is
**kept** — it is a feature, not the limitation #211 appears to describe.

**Tech Stack:** Rust (`cargo +1.97.1`, `--locked`), `jsonschema =0.49.2` (**already a workspace
dependency** — no new dep), `tools/pathogens`, `apps/cli/src/commands/quality.rs`.

**Spec:** GitHub issue #211, deliverable *"Generic certification command / API / MCP contract"*.

---

## The reading of "generic" this plan is built on — say so before writing code

`apps/cli/src/commands/quality.rs:95` carries a decision made before me:

> *"The closed registry: adding a gate here means adding its thymus adapter — there is no generic
> `certify anything` door."*

**Read as a limitation, that comment invites exactly the wrong fix.** Its reason is load-bearing and
was verified rather than assumed:

```rust
// tools/pathogens/src/lib.rs — certify()
let fooled_by: Vec<String> = suite.iter().filter(|s| gate.evaluate(&s.evidence).passed)...
if fooled_by.is_empty() { Ok(Certification { ... specimens: suite.len() }) }
```

**An empty suite makes `fooled_by` empty, so `certify` returns `Ok` with `specimens: 0`.** A generic
"certify anything" door would let a gate be certified **with no pathogens at all** — a certification
that certifies nothing, which is the #294 class exactly.

**So `generic` in #211 means MORE ENTRIES IN THE CLOSED REGISTRY, each with its own suite — not an
open door.** If the issue's author meant an open door, this plan is wrong and should be corrected
before Task 3; the disagreement is named here rather than resolved silently.

## Three findings that shape the work, all measured

**1. The registry is spelled TWICE and the two can diverge.**

```rust
if gate != "gate-geometry" {                                       // the CHECK
    return refuse("... (the registry is closed: gate-geometry)",   // the MESSAGE
```

Two literals, one fact. They agree today. **The moment a second entry is added, whoever updates one
and not the other ships a refusal that lies about what is available** — and the message is the only
thing the operator sees. Fixed **before** the second entry, not after.

**2. `certify` has no floor, and the gap is already worked around twice.**

Two independent call sites hand-write the missing check:

| site | text |
|---|---|
| `tools/pathogens/tests/jpd_gates.rs:86` | `"HARNESS-BROKE: an empty suite certifies nothing and passes"` |
| `tools/pathogens/tests/retry_lineage_gates.rs:229` | `"an empty suite certifies vacuously"` |

**A defect worked around at N call sites instead of fixed at the choke point.** A third gate that
forgets the workaround certifies vacuously, and `specimens: 0` is recorded but nothing refuses.
**No test depends on the vacuous behaviour** — verified, including the one name that could have
inverted this: `empty_note_suite()` returns **one** specimen (a note whose text is empty), not an
empty suite. Same shape, different role.

**3. Schema validation needs no new dependency, and belongs at the extension tier.**

`jsonschema = "=0.49.2"` is already a workspace dependency (`core/protocols`, `core/schema`). But
`core/schema/src/registry.rs` embeds **11 CORE schemas** with `include_str!` at compile time —
enumerated, and **none is a JPD schema**. JPD schemas live in the extension package and load at
runtime through `core/schema/src/document.rs:60 load_extension`. The only `jpd` mentions inside
`core/schema` are **one test** and the `ARTIFACT_FLOW_FORMAT` constant.

**So JPD document validation lives at the extension/CLI tier.** Putting extension schemas into the
core registry would cross a tier boundary and force compile-time embedding to become runtime
loading — far larger than this slice, and not what #211 asks for.

## Why two branches, and why this order — forced, not chosen

`core/quality/src/lib.rs:461`:

```rust
const GATE_MACHINERY: [&str; 3] = ["core/quality/", "tools/pathogens/", "docs/gates/"];
```

`freeze_violation` returns a violation for **any** change touching gate machinery together with
**anything outside it**. So:

| branch | paths | contents |
|---|---|---|
| **A** (first) | `tools/pathogens/**` only | the `certify` floor; delete the two per-site workarounds |
| **B** (second) | `apps/cli/**` only | registry derived from one list; register the retry-lineage gate; schema validation |

**A must land before B.** B adds the registry's second entry, and the floor has to exist before a
second entry can be registered with an empty suite. B also *imports* `pathogens::retry_lineage`,
which is a dependency rather than a change — permitted, since `freeze_violation` reads changed paths.

**B additionally depends on #304 landing**, since `RetryLineageGate` does not exist on `main` yet.

## Global Constraints

- `cargo +1.97.1`, `--locked` everywhere.
- **ED-18:** `cargo check --workspace --all-targets` on the **merge result**, isolated
  `CARGO_TARGET_DIR`, base sha named, both window ends read from the clock.
- **`fmt -p <package>`, never `fmt --all`** — `--all` reformats `apps/cli/tests/development_plugin.rs`,
  which is fmt-dirty on `main` and owned by another lane. I did exactly that once this session and
  reverted it; scope the formatter instead of remembering not to.
- `.gitattributes` sets `*.rs text eol=lf`; a Windows merge shows phantom `M` with an **empty**
  `git diff`. Cure: `git checkout --`. Never commit another lane's renormalisation.
- Documentation in English.

---

### Task 1 (Branch A): `certify` refuses an empty suite

**Files:**
- Modify: `tools/pathogens/src/lib.rs`
- Test: `tools/pathogens/tests/thymus.rs`

**Interfaces:**
- Produces: `CertificationRefusal` gains the empty-suite case. `certify`'s signature is unchanged.

- [ ] **Step 1: Write the failing test**

```rust
/// An empty suite must REFUSE, not certify.
///
/// The production change that would make this fail: removing the emptiness check from `certify`.
/// Before it existed, `fooled_by` was empty for an empty suite, so the function returned
/// `Ok(Certification { specimens: 0 })` -- a gate certified against no pathogens at all.
///
/// `GeometrySpecimen` must be added to this file's existing `use pathogens::{...}` list; the id
/// below is `"reject-everything"` with NO `gate/` prefix -- read from `lib.rs`, not guessed. The
/// first draft of this plan invented the prefix, which is the #294 defect in miniature.
#[test]
fn an_empty_suite_refuses_instead_of_certifying_vacuously() {
    let empty: Vec<GeometrySpecimen> = Vec::new();
    let refusal = certify(reject_everything_gate().as_ref(), &empty)
        .expect_err("an empty suite must not certify");
    assert_eq!(refusal.gate_id, "reject-everything");
    assert!(
        refusal.fooled_by.is_empty(),
        "nothing fooled the gate; the refusal is about the SUITE, not the gate"
    );
}

/// And the floor must not fire on a real suite -- otherwise it refuses everything and the first
/// test above passes for the wrong reason.
#[test]
fn a_real_suite_still_certifies() {
    certify(reject_everything_gate().as_ref(), &suite())
        .expect("the bred suite must still certify a gate that rejects everything");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo +1.97.1 test -p pathogens --locked --test thymus`
Expected: `an_empty_suite_refuses_instead_of_certifying_vacuously` FAILS at `expect_err` —
`called Result::unwrap_err on an Ok value`. That is the assertion-level red: the current code
returns `Ok`. The second test passes already and is the control.

- [ ] **Step 3: Write the minimal implementation**

In `tools/pathogens/src/lib.rs`, at the top of `certify`:

```rust
    // A suite with no specimens cannot fool a gate, so `fooled_by` is empty and the function used
    // to return Ok -- certifying a gate against nothing at all. The refusal carries an empty
    // `fooled_by` because nothing fooled anything: the defect is in the SUITE.
    //
    // Enforced HERE rather than at each call site. Two call sites already hand-wrote this check
    // (`jpd_gates.rs`, `retry_lineage_gates.rs`), which is a defect worked around N times instead
    // of fixed once -- the third gate is the one that forgets.
    if suite.is_empty() {
        return Err(CertificationRefusal {
            gate_id: gate.id().to_owned(),
            fooled_by: Vec::new(),
        });
    }
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo +1.97.1 test -p pathogens --locked`
Expected: all pass, including the pre-existing thymus and jpd suites.

- [ ] **Step 5: Retire the two workarounds, now that the choke point holds**

Delete `assert!(!suite.is_empty(), ...)` from `tools/pathogens/tests/jpd_gates.rs:86` and
`tools/pathogens/tests/retry_lineage_gates.rs:229`, replacing each with a comment naming where the
guarantee now lives:

```rust
    // Emptiness is refused by `certify` itself since the floor landed; asserting it here again
    // would be a redundant blade that keeps passing while the real check is sabotaged.
```

**Do not skip this step.** A redundant check masks sabotage of the primary one: with both present,
breaking the floor in `certify` leaves these tests green.

- [ ] **Step 6: Sabotage, to prove the floor is what holds**

Commit first. Then delete the `if suite.is_empty()` block and run the suite.
**Expected: `an_empty_suite_refuses_instead_of_certifying_vacuously` FAILS and
`a_real_suite_still_certifies` PASSES.** If the second also fails, the floor was written to refuse
everything and the first test was passing for the wrong reason. Record the panic site, revert.

- [ ] **Step 7: Commit**

```bash
git add tools/pathogens/src/lib.rs tools/pathogens/tests/thymus.rs tools/pathogens/tests/jpd_gates.rs tools/pathogens/tests/retry_lineage_gates.rs
git commit -m "feat(211): certify refuses an empty suite, at the choke point instead of per call site"
```

---

### Task 2 (Branch B): one list, so the registry and its refusal message cannot disagree

**Files:**
- Modify: `apps/cli/src/commands/quality.rs`
- Test: `apps/cli/tests/gate_http.rs` (the existing `certify` harness lives there)

**Interfaces:**
- Produces: `const REGISTERED_GATES: [(&str, ...); N]`, and a refusal message built from it.

- [ ] **Step 1: Write the failing test**

**`apps/cli` has NO `[lib]` section** — it is binary-only, and `gate_http.rs` drives it as a
subprocess via `assert_cmd::cargo::cargo_bin!("graphhelm")`. **A test therefore cannot import
`REGISTERED_GATES`**, so the guard has to be written against observable behaviour. That is the
better test anyway: it checks what the operator sees rather than an internal constant.

```rust
/// Every gate that ACTUALLY certifies must be named in the refusal message.
///
/// The population is discovered by driving the binary, not read from a constant the test cannot
/// import. A candidate list that is too broad is harmless — an id that does not certify is simply
/// not required to appear — so this guard keeps working as the registry grows.
///
/// The production change that would make this fail: adding a registry entry while leaving the
/// message's own literal list alone, which is exactly what the two-literal shape invites.
#[test]
fn the_refusal_names_every_gate_that_actually_certifies() {
    let candidates = ["gate-geometry", "gate-retry-lineage"];
    let refusal = certify(&serve, "gate-does-not-exist");
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&refusal.stdout),
        String::from_utf8_lossy(&refusal.stderr)
    );

    let mut certifying = Vec::new();
    for id in candidates {
        if certify(&serve, id).status.success() {
            certifying.push(id);
            assert!(
                message.contains(id),
                "{id} certifies but the refusal message does not name it; the operator sees only \
                 this message. got: {message}"
            );
        }
    }
    // Without this, a binary where NOTHING certifies would satisfy the loop vacuously.
    assert!(
        !certifying.is_empty(),
        "HARNESS-BROKE: no candidate gate certified at all, so the loop above asserted nothing"
    );
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo +1.97.1 test -p graphhelm-cli --locked --test gate_http the_refusal_names_every_gate_that_actually_certifies`
Expected: with the message still hardcoded and only `gate-geometry` registered, this **passes** —
the single entry happens to be named. **That is a pass for the wrong reason, so prove it can fail:**
temporarily change the hardcoded message to omit `gate-geometry` and confirm the test goes red at
its assertion. Revert, then continue. The guard's real bite arrives in Task 3.

- [ ] **Step 3: Write the minimal implementation**

```rust
/// The closed registry, in ONE place.
///
/// Adding a gate here means adding its thymus adapter -- there is still no generic
/// "certify anything" door, and that is deliberate: `certify` over an empty suite would
/// otherwise stamp a gate that no pathogen ever attacked.
///
/// The refusal message is DERIVED from this list rather than spelling it again. Two literals of one
/// fact agree until the first person updates one of them.
const REGISTERED_GATES: [&str; 1] = ["gate-geometry"];
// Not `pub`: `apps/cli` is binary-only, so no test can import it. The guard in Task 2 tests
// the observable message instead, which is what the operator actually reads.

fn registry_refusal() -> String {
    format!(
        "no runnable gate by that id is registered (the registry is closed: {})",
        REGISTERED_GATES.join(", ")
    )
}
```

and replace the hardcoded check and message with `REGISTERED_GATES.contains(&gate)` and
`registry_refusal()`.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo +1.97.1 test -p graphhelm-cli --locked --test gate_http`

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/commands/quality.rs apps/cli/tests/gate_http.rs
git commit -m "refactor(211): derive the registry refusal from the registry, not a second literal"
```

---

### Task 3 (Branch B): register the retry-lineage gate as the second entry

**Depends on #304 having landed** — `pathogens::retry_lineage` does not exist on `main` before it.

**Files:**
- Modify: `apps/cli/src/commands/quality.rs`
- Test: `apps/cli/tests/gate_http.rs`

- [ ] **Step 1: Write the failing test**

```rust
/// The second registry entry certifies through the same command, with its own suite.
#[test]
fn the_retry_lineage_gate_certifies_through_the_same_command() {
    let output = certify(&serve, "gate-retry-lineage");
    assert!(output.status.success(), "gate-retry-lineage must certify");
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(body["gateId"], "gate-retry-lineage");
    assert!(
        body["specimens"].as_u64().is_some_and(|n| n > 0),
        "a registered gate must carry a non-empty suite; specimens was {:?}",
        body["specimens"]
    );
    // The two gates must NOT share a suite digest: a copied registry entry that forgot to swap the
    // suite would certify happily and stamp the wrong immunity.
    let geometry: serde_json::Value =
        serde_json::from_slice(&certify(&serve, "gate-geometry").stdout).expect("json");
    assert_ne!(
        body["suiteDigest"], geometry["suiteDigest"],
        "two gates certified against the same suite means one entry is wired to the other's suite"
    );
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo +1.97.1 test -p graphhelm-cli --locked --test gate_http the_retry_lineage_gate_certifies_through_the_same_command`
Expected: FAIL — the registry refuses `gate-retry-lineage`.

- [ ] **Step 3: Write the minimal implementation**

Extend `REGISTERED_GATES` to `["gate-geometry", "gate-retry-lineage"]` and dispatch:

```rust
    let certification = match gate {
        "gate-geometry" => pathogens::certify(&GeometryGate, &pathogens::suite()),
        "gate-retry-lineage" => pathogens::certify(
            &pathogens::retry_lineage::RetryLineageGate,
            &pathogens::retry_lineage::retry_lineage_suite(),
        ),
        // Unreachable: the registry check above already refused anything else. Kept anyway so a
        // new REGISTERED_GATES entry with no dispatch arm fails HERE rather than being refused by
        // a message that simultaneously claims it is registered -- the two-literal defect again,
        // one layer down.
        _ => return refuse(&registry_refusal(), "/gate"),
    };
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo +1.97.1 test -p graphhelm-cli --locked --test gate_http`

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/commands/quality.rs apps/cli/tests/gate_http.rs
git commit -m "feat(211): register the retry lineage gate as the registry's second entry"
```

---

## Death conditions

| claim | dies when |
|---|---|
| An empty suite certifies today | `certify` returns `Err` for `&[]` before Task 1 lands |
| No test depends on the vacuous behaviour | any test calls `certify` with a genuinely empty slice |
| `jsonschema` needs no new dependency | it leaves `Cargo.toml` |
| JPD schemas are absent from the core registry | a JPD `include_str!` appears in `core/schema/src/registry.rs` |
| Two branches are required | `GATE_MACHINERY` stops listing `tools/pathogens/` |
| "generic" means more entries, not an open door | the issue's author says otherwise — **ask before Task 3** |

## What this slice does NOT do — named

- **No MCP contract.** #211 names *"command / API / MCP"*; this plan builds the **command** only.
  The MCP surface (`apps/cli/src/commands/mcp/`) is a separate subsystem with its own review cost.
- **No schema validation yet.** Finding 3 establishes it is *feasible and where it belongs*; wiring
  it is the next slice, and it is the one that pays off #294's deferral. Stated so this plan's
  careful limits are not read as exhaustive.
- **Two validators still have no gate**: observation obligation (1 foreign positive, no negative)
  and journey contract (**zero** foreign material at any depth). Council result has a complete
  foreign document embedded at `/bindings/council/result`, positive only.
- **`certification_is_current` is still consumed by nobody.** A stale certification does not refuse
  execution. Unchanged by this slice, and still true.
