# #114 — deploy capability: design blueprint

D, 2026-08-20, read-only at main `21fd7dc`. **Nothing here is built and nothing should be until
ruled.** Every code claim was read at that commit.

## The headline: #114's framing understates what exists, and the gap is DECLARED rather than hidden

The issue says *"there is no deploy capability to force — the sovereignty machinery has real levers
and no deploy target."* Read against the tree, that is not quite the shape:

| Piece | Status at `21fd7dc` |
|---|---|
| `NodeType::Deploy` | **EXISTS** (`core/protocols/src/graph.rs:120`) |
| `NodeType::Rollback` | **EXISTS** (same enum) |
| A deploy node's required contract | **LINTED**: `GHG009_DEPLOY_TARGET_MISSING` requires a non-empty `targetRef` (`lint/deployment.rs`) |
| Irreversibility / compensation | **LINTED**: `effects.reversible` or `effects.compensationRequired` demands a `compensationNode` that resolves (`lint/deployment.rs`) |
| Timeout expectation | **LINTED (warning)**: `GHG101_DEFAULT_TIMEOUT` for a Deploy node with no `timeoutSeconds` |
| Policy denial for deploying | **EXISTS**: `deny: deploy` / `deny: production.deploy` → `GHG014_HARD_POLICY_DENIED` (`lint/mod.rs:110-114`) |
| Owner override with waived requirements + acknowledged risks | **EXISTS** in the governor/policy path (`evaluator.rs:73`, `complete_owner_override` at `:176`) |
| **Executing a Deploy node** | **REFUSED, BY NAME**: `work_kind` returns `Err(ExecutorRefusal::Unsupported)` (`core/runtime/src/classify.rs:42`) |

**So the deploy OBJECT is already specified and enforced. What is missing is EXECUTION — and the
system says so out loud.** `classify.rs`'s own doc: *"`ExecutorRefusal::Unsupported` for every type
this milestone does not execute."*

**That distinction matters more than it looks**, because it is the opposite of the two defects this
lane spent the night on. `userOverrideAllowed` (#93) was a field with no consumer that *looked* like
a capability. `Waived`/`Skipped` (#94) are legal in the transition table with no surface that
produces them and **no refusal anywhere** — silent. Deploy is neither: it is declared unsupported at
the one site that would run it. **The honest-gap pattern, done right.** The work is to give a named
refusal a body, not to invent a concept.

## Answering the brief's question: what is the smallest honest deploy object?

**None of the three options offered — it is already chosen, and the answer is "the one that
exists".**

- *A Tool-type node with a deploy contract?* **No**, and choosing it would be a regression.
  `NodeType::Tool` executes today (`work_kind` → `NodeWorkKind::Tool`), so modelling deploys as
  tools would make every deploy runnable **immediately and invisibly** — losing the `targetRef`
  requirement, the compensation lint, and the `deny: deploy` policy hook, all of which key on
  `NodeType::Deploy`. It would trade a declared gap for a silent capability.
- *A new node type?* **No.** Two already exist (`Deploy`, `Rollback`) with lints attached. A third
  would be the third `is_terminal` (#101) before it was written.
- *An external adapter?* **Yes — and only that.** The missing piece is a `NodeWorkKind::Deployment`
  arm plus an adapter behind it, in the same shape `Tool` already has (`adapters/tool-host`).

**Smallest honest object = the existing `NodeType::Deploy` + one new `NodeWorkKind` + one adapter
port.** No protocol change, no schema vocabulary, no new lint.

## How the sovereignty machinery attaches — and the one place it does NOT meet

RA steps 8-10 are: *remove the review, force the deploy to a test environment, the Draft shows the
risks.* Mapped onto what exists:

1. **Removing the review is a graph MUTATION.** Governed by `ExecutionMode` per D-022 (#79's
   ruling: mode governs mutation autonomy, nothing else). Under `Supervised` the proposal is held
   for the owner; under `Manual` it is rejected outright and the owner edits the graph themselves.
   **Exists.**
2. **The bypass is recorded as obligations + acknowledged risks.** `ManualOverride` carries
   `waived_requirements`, `acknowledged_risks`, `reason`, `actor`, and `complete_owner_override`
   (`evaluator.rs:176`) requires **all four** — owner actor, non-empty reason, non-empty waived
   requirements, non-empty acknowledged risks. **Exists, and it is strict.**
3. **The Draft showing risks** is that same structure surfaced. **Exists.**
4. **The deploy then runs.** **Does not exist.** `Unsupported`.

**THE STRUCTURAL FINDING, and it is the one I would put in front of whoever scopes this:** the
owner override and the deploy denial live in **different subsystems that do not connect**.

- `deny: deploy` produces `GHG014_HARD_POLICY_DENIED`, a **LINT ERROR**. Lint errors block the graph
  from loading at all — `start`/`resume` return `Outcome::domain` on any `report.errors` non-empty.
- `ManualOverride` operates on **policy OBLIGATIONS** in the governor's publication path. It waives
  *requirements*; it has no relationship to lint diagnostics.

**So an owner cannot override a `deny: deploy` policy. There is no path.** The lever exists, the
lock exists, and they are not the same mechanism. Anyone implementing steps 8-10 against a graph
that declares that policy will find the flow blocked at load time with no override to reach for —
and the failure will look like a lint bug rather than a missing connection.

*(Scope note: the denial only fires when the graph itself declares that policy. A graph without it
is not blocked, so this does not block every steps-8-10 rehearsal — only the ones that exercise the
denial, which is the interesting half.)*

## What steps 8-10 need that does not exist, in dependency order

1. **`NodeWorkKind::Deployment` + an adapter port.** The whole of the missing execution. Mirrors
   `Tool`'s existing shape.
2. **A decision about what a deploy to a *test environment* means.** `targetRef` is a free string
   today (`environment://staging` in the example graph). Nothing distinguishes test from
   production; the only distinction in the tree is the **policy key** (`deploy` vs
   `production.deploy`), which is authoring convention, not a checked property. **Steps 8-10 say
   "test environment" and the system has no concept of one.**
3. **The override↔denial connection, or an explicit decision that there is none.** Either
   `ManualOverride` gains a way to clear a hard policy deny, or `deny: deploy` is documented as
   absolute and steps 8-10 are rewritten to not require overriding it.
4. **Dispatch-time override (#93) and a reachable waive/skip surface (#94).** Both open. #93 records
   that no dispatch-time override exists at all; #94 that D-019's lever cannot be pulled. **Steps
   8-10's "forces deploy" is exactly the capability those two issues say is missing**, so #114 is
   downstream of both and should not be scoped ahead of them.

## What this blueprint does NOT establish

- **I did not read the RA document or PRD §4.6**, only #114's summary of them. If steps 8-10 say
  something more specific than "force a deploy to a test environment", the mapping above may be
  under- or over-scoped.
- **I did not check the HTTP/MCP surfaces** for deploy-adjacent routes; every claim is from
  `core/*` and `apps/cli/src`.
- **I did not price the adapter.** "Mirrors the tool-host shape" is a structural observation, not an
  estimate, and the tool host has isolation and credential machinery a deploy port may or may not
  need.
- **No frequency or demand evidence.** #114 is a marker issue recording a promise/reality delta;
  nothing here establishes that anyone has needed a deploy node yet.
