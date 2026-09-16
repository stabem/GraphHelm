# Agent presence: model and effort beside the name — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An agent that attaches over MCP declares which model it is running and at what reasoning effort, the Runtime records that declaration as its own event, and the board renders `gpt-6-astra · low` beside the agent's name without inventing anything.

**Architecture:** The declaration follows the path `--actor` already takes — CLI flag → `ApiClient` → request header → `serve` → recorded event — and stops short of the actor identity itself. Identity is stable; a model is a property of a SESSION, so it becomes a new event kind (`agent_presence_declared`) rather than a field on `PersistedActor`. That choice is what keeps the change additive: `$defs.actor` in `event-envelope.schema.json` is `additionalProperties: false` and `PersistedActor` carries `deny_unknown_fields`, while `$defs.eventKind` is a `oneOf` — and `core/schema-evolution`'s analyser classifies a DISJOINT `oneOf` addition as `composition widened` (Compatible), where any edit to an `allOf` branch is Breaking/Major.

**Tech Stack:** Rust (clap, serde), JSON Schema 2020-12, React + TypeScript (Studio), Vitest.

**Spec:** https://github.com/stabem/GraphHelm/issues/1054

## Global Constraints

- **Absent is absent.** No default model, no `"unknown"` written as a value, no inference from the route. A session that declares nothing produces no presence event and renders no badge.
- **`schemas/releases/1.0.0/**` is never edited.** It is the frozen previous release; divergence is declared, not hidden.
- **Every schema edit carries three companions in the SAME commit:** the canonical digest and `documentVersion` in `schemas/catalog.json` (digest from `graphhelm schema digest --file <f>`, verified first against an UNCHANGED schema whose catalog entry must match), a `schemas/CHANGELOG.md` entry, and the schema's name added to the declared divergence ledger in `core/schema-evolution/tests/baseline_origin.rs:421`.
- **Never edit an existing `allOf` branch.** `core/schema-evolution/src/compatibility.rs:900-940` compares branch SETS: editing one leaves baseline and candidate incomparable (`composition ambiguous`, Breaking/Major) and adding one narrows it. Only a disjoint `oneOf` addition is compatible.
- **Effort vocabulary is closed:** `low | medium | high`. A closed enum is refusable; a free string is not.
- **Model is an opaque string**, never an enum — the set changes faster than this repository ships.

---

### Task 1: The two flags exist, and a malformed one is refused before a protocol byte

**Files:**
- Modify: `apps/cli/src/args.rs:158-163` (beside `actor` / `actor_type`)
- Modify: `apps/cli/src/commands/mcp/mod.rs:62-70` (beside the existing `--actor` refusals)
- Test: `apps/cli/tests/mcp_cli.rs`

**Interfaces:**
- Produces: `McpArgs.model: Option<String>`, `McpArgs.effort: Option<String>`; both `None` when the flag is absent.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn an_effort_outside_the_vocabulary_is_refused_before_a_protocol_byte() {
    let out = mcp_cmd(&["--url", "http://127.0.0.1:1", "--actor", "a", "--effort", "medium-ish"]);
    assert!(!out.ok, "a free-text effort must be refused");
    assert_eq!(out.diagnostics[0].path, "/effort");
}

#[test]
fn model_and_effort_are_optional_and_absent_is_not_an_error() {
    let out = mcp_cmd(&["--url", "http://127.0.0.1:1", "--actor", "a"]);
    assert!(out.reached_connect_stage, "absent flags must not refuse the session");
}

#[test]
fn effort_without_model_is_refused_because_an_effort_alone_describes_nothing() {
    let out = mcp_cmd(&["--url", "http://127.0.0.1:1", "--actor", "a", "--effort", "low"]);
    assert!(!out.ok);
    assert_eq!(out.diagnostics[0].path, "/model");
}
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test -p graphhelm-cli --test mcp_cli -- effort model`
Expected: FAIL — `--effort` is not a recognised argument.

- [ ] **Step 3: Add the flags**

```rust
/// The model this session is running, verbatim and opaque (`claude-opus-5`, `gpt-6-astra`).
/// ABSENT IS ABSENT: no default, and the Runtime never infers one from the route -- a
/// `claude_subscription` route lets the CLI choose its own model, so a route-derived guess
/// would be a label the run cannot support (#1054).
#[arg(long)]
pub model: Option<String>,
/// `low`, `medium` or `high`. A CLOSED vocabulary, so a wrong value is refusable here
/// rather than rendered as a badge nobody can interpret.
#[arg(long)]
pub effort: Option<String>,
```

- [ ] **Step 4: Add the refusals beside the existing `--actor` ones**

```rust
if let Some(effort) = args.effort.as_deref()
    && !matches!(effort, "low" | "medium" | "high")
{
    return Err(refuse("--effort must be \"low\", \"medium\" or \"high\"", "/effort"));
}
if args.effort.is_some() && args.model.is_none() {
    return Err(refuse("--effort requires --model: an effort with no model names nothing", "/model"));
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p graphhelm-cli --test mcp_cli -- effort model`
Expected: PASS, 3 of 3.

- [ ] **Step 6: Commit**

```bash
git add apps/cli/src/args.rs apps/cli/src/commands/mcp/mod.rs apps/cli/tests/mcp_cli.rs
git commit -m "feat(1054): an MCP session may declare its model and effort, and a malformed one is refused"
```

---

### Task 2: The declaration crosses the wire on its own headers

**Files:**
- Modify: `apps/cli/src/commands/mcp/client.rs:88-104` (fields), `:128-129` (headers)
- Modify: `apps/cli/src/commands/mcp/mod.rs:86-90` (the `ApiClient::new` call)
- Test: `apps/cli/src/commands/mcp/client.rs` (unit tests beside `is_loopback_url`'s)

**Interfaces:**
- Consumes: `McpArgs.model`, `McpArgs.effort` from Task 1.
- Produces: `ApiClient::new(url, token, actor, actor_type, model: Option<String>, effort: Option<String>)`; headers `X-GraphHelm-Actor-Model` and `X-GraphHelm-Actor-Effort`, **emitted only when `Some`**.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn an_undeclared_model_emits_no_header_at_all() {
    let client = ApiClient::new(url(), token(), "a".into(), "agent".into(), None, None);
    let names: Vec<_> = client.mutation_headers().into_iter().map(|(k, _)| k).collect();
    assert!(
        !names.iter().any(|k| k.starts_with("X-GraphHelm-Actor-Model")),
        "absent must send NO header, never an empty one: an empty string is a value"
    );
}

#[test]
fn a_declared_model_and_effort_ride_their_own_headers() {
    let client = ApiClient::new(
        url(), token(), "a".into(), "agent".into(),
        Some("claude-opus-5".into()), Some("medium".into()),
    );
    let headers = client.mutation_headers();
    assert!(headers.contains(&("X-GraphHelm-Actor-Model".to_owned(), "claude-opus-5".to_owned())));
    assert!(headers.contains(&("X-GraphHelm-Actor-Effort".to_owned(), "medium".to_owned())));
}
```

- [ ] **Step 2: Run and watch it fail**

Run: `cargo test -p graphhelm-cli --lib -- mcp::client`
Expected: FAIL — `ApiClient::new` takes four arguments.

- [ ] **Step 3: Carry the two values and emit the headers conditionally**

```rust
if let Some(model) = self.model.as_ref() {
    headers.push(("X-GraphHelm-Actor-Model".to_owned(), model.clone()));
}
if let Some(effort) = self.effort.as_ref() {
    headers.push(("X-GraphHelm-Actor-Effort".to_owned(), effort.clone()));
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p graphhelm-cli --lib -- mcp::client`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/commands/mcp/client.rs apps/cli/src/commands/mcp/mod.rs
git commit -m "feat(1054): the model declaration travels on its own headers, and absent sends none"
```

---

### Task 3: The event kind exists in the schema, additively

**Files:**
- Modify: `schemas/event-envelope.schema.json` (`$defs.eventKind`'s `oneOf`, plus a new `$defs.agentPresenceDeclared`)
- Modify: `schemas/catalog.json` (`schemas["event-envelope"].sha256` and `.documentVersion`)
- Modify: `schemas/CHANGELOG.md`
- Modify: `core/schema-evolution/tests/baseline_origin.rs:421` (the divergence ledger)
- Test: `core/schema-evolution/tests/baseline_origin.rs`, `apps/cli/tests/schema_cli.rs`

**Interfaces:**
- Produces: event kind `agent_presence_declared` carrying `data: { actorId, actorType, model, effort? }`.

- [ ] **Step 1: Write the failing evolution test**

```rust
#[test]
fn the_new_presence_kind_is_a_disjoint_addition_and_therefore_compatible() {
    let report = compare_catalogs(&baseline(), &candidate());
    let envelope: Vec<_> = report
        .changes
        .iter()
        .filter(|c| c.schema == "event-envelope")
        .collect();
    assert!(!envelope.is_empty(), "CONTROL: the change must be visible at all");
    assert!(
        envelope.iter().all(|c| c.class == CompatibilityClass::Compatible),
        "a disjoint oneOf addition must not be Breaking: {envelope:#?}"
    );
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p graphhelm-schema-evolution --test baseline_origin`
Expected: FAIL — the CONTROL assertion fires, because no `event-envelope` change exists yet.

- [ ] **Step 3: Add the variant and its payload**

Append to `$defs.eventKind`'s `oneOf` (never edit a sibling branch):

```json
{
  "type": "object",
  "required": ["type", "data"],
  "properties": {
    "type": { "const": "agent_presence_declared" },
    "data": { "$ref": "#/$defs/agentPresenceDeclared" }
  },
  "additionalProperties": false
}
```

and add the payload beside the other `$defs`:

```json
"agentPresenceDeclared": {
  "type": "object",
  "required": ["actorId", "actorType", "model"],
  "properties": {
    "actorId": { "$ref": "#/$defs/actorId" },
    "actorType": { "enum": ["owner", "human", "agent", "system"] },
    "model": { "type": "string", "minLength": 1 },
    "effort": { "enum": ["low", "medium", "high"] }
  },
  "additionalProperties": false
}
```

- [ ] **Step 4: Recompute the catalog entry and declare the divergence**

```bash
graphhelm schema digest --file schemas/agent.schema.json
graphhelm schema digest --file schemas/event-envelope.schema.json
```

The first is a CONTROL: its answer must equal `schemas.agent.sha256` in the catalog, which proves the tool computes the same canonical form the catalog stores. Then put the second digest and `"documentVersion": "1.1.0"` into `schemas/catalog.json`, add `"event-envelope"` to the ledger vector in `baseline_origin.rs:421` (and change its message from `three` to `four`), and write the `schemas/CHANGELOG.md` entry.

- [ ] **Step 5: Run both suites**

Run: `cargo test -p graphhelm-schema-evolution && cargo test -p graphhelm-cli --test schema_cli`
Expected: PASS. A `GHC002_HASH_MISMATCH` on `/schemas/event-envelope/sha256` means the digest was taken before the final edit — recompute it, never hand-edit a digest.

- [ ] **Step 6: Commit**

```bash
git add schemas/event-envelope.schema.json schemas/catalog.json schemas/CHANGELOG.md core/schema-evolution/tests/baseline_origin.rs
git commit -m "feat(1054): agent_presence_declared joins eventKind as a disjoint addition"
```

---

### Task 4: The Runtime records the declaration, once, and records nothing when absent

**Files:**
- Modify: `apps/cli/src/commands/serve/mod.rs:892-915` (beside the actor header reads)
- Modify: `core/events/src/projection.rs` (project the new kind onto the execution's actor roster)
- Test: `apps/cli/tests/api_http.rs`

**Interfaces:**
- Consumes: the headers from Task 2 and the event kind from Task 3.
- Produces: one `agent_presence_declared` whenever the declared `(actorId, model, effort)` triple
  DIFFERS FROM THE NEWEST declaration that actor already has in the stream.

  That is the rule for a request that REACHES the decision. Three things return before it does,
  and none of them writes a declaration:

  - a header refusal in `parse_mutation_headers` (bad effort, blank/over-long/non-ASCII model, an
    effort without a model) — 400 before anything touches the store;
  - any non-`Absent` outcome of the idempotency pre-flight — a recognised retry
    (`KeyState::Complete`), a stuck partial, a divergent key reuse, or a failed classification —
    plus an `If-Match` mismatch, all of which return from `run_idempotent_mutation_inner` above
    the call to `plan_presence_declaration`;
  - every READ path, which never enters this code at all.

  An earlier version of this line ended "Nothing else suppresses it", which was the same overclaim
  class as the struck sentence in the note below.

  CORRECTED during task 4 (fix round 1). This line previously read "at most ONE ... per distinct
  `(actorId, model, effort)` triple per stream", which is FALSE about the shipped code and was
  copied into two doc comments before anyone noticed. The sequence that separates the two
  readings is the FLIP-FLOP: `medium -> high -> medium` appends THREE events, and the third
  carries a triple the stream already holds. Newest-wins is deliberate -- a roster answers "what
  is this agent running NOW", and suppressing the return to `medium` would leave it reporting
  `high` forever, a fact that stopped being true. Pinned by
  `a_flip_flop_appends_a_third_declaration_because_newest_is_what_a_roster_answers`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_declared_model_is_recorded_once_and_a_repeat_appends_nothing() {
    let before = head_sequence(&server, EXECUTION);
    declare(&server, "claude-opus-5", "medium");
    let after_first = head_sequence(&server, EXECUTION);
    declare(&server, "claude-opus-5", "medium");
    let after_second = head_sequence(&server, EXECUTION);
    assert!(after_first > before + 1, "the first declaration is recorded beside the signal");
    assert_eq!(
        after_second,
        after_first + 1,
        "the repeat appends only the signal itself, not a second presence event"
    );
}

#[test]
fn a_changed_effort_mid_session_is_recorded_as_a_new_declaration() {
    declare(&server, "claude-opus-5", "medium");
    let mid = head_sequence(&server, EXECUTION);
    declare(&server, "claude-opus-5", "high");
    assert!(
        head_sequence(&server, EXECUTION) > mid + 1,
        "effort is part of a declaration's identity, so a change is news"
    );
}

#[test]
fn an_undeclared_session_records_no_presence_event() {
    let before = head_sequence(&server, EXECUTION);
    signal_with_headers(&server, &[]);
    assert_eq!(
        head_sequence(&server, EXECUTION),
        before + 1,
        "absent must produce NO presence event -- only the signal"
    );
}

#[test]
fn an_effort_header_outside_the_vocabulary_is_a_400_and_records_nothing() {
    let before = head_sequence(&server, EXECUTION);
    let response = signal_with_headers(
        &server,
        &[("X-GraphHelm-Actor-Model", "m"), ("X-GraphHelm-Actor-Effort", "spicy")],
    );
    assert_eq!(response.status, 400);
    assert_eq!(
        head_sequence(&server, EXECUTION),
        before,
        "a refused request appends nothing, not even the signal it carried"
    );
}
```

- [ ] **Step 2: Run and watch them fail**

Run: `cargo test -p graphhelm-cli --test api_http -- presence`
Expected: FAIL — the headers are ignored, so no presence event is ever written.

- [ ] **Step 3: Read the headers beside the actor, and refuse the same way the actor does**

```rust
let declared_model = header_value(headers, "x-graphhelm-actor-model").map(str::to_owned);
let declared_effort = match header_value(headers, "x-graphhelm-actor-effort") {
    None => None,
    Some(value @ ("low" | "medium" | "high")) => Some(value.to_owned()),
    Some(_) => {
        return Err(mutation_bad_request(
            command,
            "X-GraphHelm-Actor-Effort must be \"low\", \"medium\" or \"high\"",
            "/actorEffort",
        ));
    }
};
if declared_effort.is_some() && declared_model.is_none() {
    return Err(mutation_bad_request(
        command,
        "X-GraphHelm-Actor-Effort requires X-GraphHelm-Actor-Model",
        "/actorModel",
    ));
}
```

Append the presence event immediately before the request's own event, and **only when the `(actorId, model, effort)` triple differs from the newest declaration this stream already holds for that actor**. That comparison is what makes the repeat free; without it every mutation would append a duplicate and the stream would grow with no news in it.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p graphhelm-cli --test api_http -- presence`
Expected: PASS, 4 of 4.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/commands/serve/mod.rs core/events/src/projection.rs apps/cli/tests/api_http.rs
git commit -m "feat(1054): the Runtime records a presence declaration once, and absent records nothing"
```

---

### Task 5: The board renders the badge, and renders absence as absence

**Files:**
- Modify: `apps/studio/src/components/board.tsx` (the agent row that today prints only the actor id)
- Modify: `apps/studio/src/runtime/session.ts` (carry the newest declaration per actor)
- Test: `apps/studio/src/components/board.test.tsx`

**Interfaces:**
- Consumes: `agent_presence_declared` events from Task 4.

- [ ] **Step 1: Write the failing tests**

```tsx
it("shows the model and effort beside the name once declared", () => {
  render(<Board model={withPresence("codex", "gpt-6-astra", "low")} />);
  expect(screen.getByText("gpt-6-astra · low")).toBeInTheDocument();
});

it("shows NOTHING beside a name that never declared", () => {
  render(<Board model={withoutPresence("codex")} />);
  expect(screen.queryByText(/unknown|default|n\/a/i)).not.toBeInTheDocument();
});

it("shows the model alone when effort was not declared", () => {
  render(<Board model={withPresence("codex", "gpt-6-astra", undefined)} />);
  expect(screen.getByText("gpt-6-astra")).toBeInTheDocument();
  expect(screen.queryByText("·")).not.toBeInTheDocument();
});

it("shows the NEWEST declaration when a session changed model mid-run", () => {
  render(<Board model={withPresenceHistory("codex", [["a", "low"], ["b", "high"]])} />);
  expect(screen.getByText("b · high")).toBeInTheDocument();
  expect(screen.queryByText("a · low")).not.toBeInTheDocument();
});
```

- [ ] **Step 2: Run and watch them fail**

Run: `npm --prefix apps/studio test -- board`
Expected: FAIL — the row renders only the actor id.

- [ ] **Step 3: Render it**

```tsx
{presence && (
  <span className="agent-badge" title="declared by the session, never inferred">
    {presence.effort ? `${presence.model} · ${presence.effort}` : presence.model}
  </span>
)}
```

- [ ] **Step 4: Run the tests**

Run: `npm --prefix apps/studio test -- board`
Expected: PASS, 4 of 4.

- [ ] **Step 5: Commit**

```bash
git add apps/studio/src/components/board.tsx apps/studio/src/runtime/session.ts apps/studio/src/components/board.test.tsx
git commit -m "feat(1054): the board shows the declared model and effort, and shows absence as absence"
```

---

## Not in this plan

**Token spend.** It arrives from `adapters/model-gateway/src/runtime.rs` as a property of a TURN, not of a session, and for Codex it does not exist at all until #1035 lands (`Usage::default()` — "absent, never invented"). A component that reads the badge and the spend from one source will be wrong about at least one of them. It needs its own plan, consuming this one's events.

**Route-derived badges.** `claude_subscription` declares no model — the CLI chooses one per session — so a route-derived label would be populated for Codex and blank for Claude, which reads as a UI bug rather than as the honest limit it is. If a route pins a model, the session it launches should DECLARE it through the flags Task 1 adds.
