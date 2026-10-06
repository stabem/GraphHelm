# Studio: live team, needs-you beacon, handover, and proven journeys — design

Status: proposed (issue #301). Owner-approved direction from four mockups (2026-10-05).
Supersedes the layout sections of `docs/ux/STUDIO_SPEC.md` (§ top bar, canvas, bottom composer,
agents panel) and the Overview / Free canvas split in `apps/studio/src/components/board.tsx`.

## 1. Problem

The owner runs several agents (for example the `lojakit-levas-2-a-5` run: three Codex chats plus a
coordinator, about 1 700 recorded notes over hours) and opens the Studio to answer three
questions: **who is working, on what, and does anything need me?** Today the Studio cannot answer
them:

- The work canvas shows one `Start` node in `waiting input` for the whole run, because agents
  cannot add graph nodes; their work lives in signals the canvas does not draw.
- The Overview says "98 agents are recording work": every raw actor id (`codex`, `merge-812`,
  `orchestrator-f05895`) counts as an agent, and personas show thread UUIDs.
- What needs the owner is spread over a "Request status" box, a "JEV suggested next step" box,
  an attention block and the run panel, in transport vocabulary ("native outcome was not
  confirmed", "reconcile this request id").
- Returning after hours shows a wall of events with no summary.
- There is no way to see that the product works: Keel and JPD can certify a journey in records,
  but the Studio has no picture of it, and the Runtime cannot store or serve a screenshot.

## 2. Outcome and success criteria

Opening the Studio on a live run, the owner can, without reading an id:

1. See every real agent as a bot with a name, a colour, a one-line "doing now", and a state
   (working, waiting for you, quiet, done) — S1.
2. See which bots are talking to each other, live — S2.
3. Tell in one glance whether anything needs him; dark means he can close the laptop — S3.
4. Answer a question with one button when the question offers choices — S4.
5. After an absence, read a handover of what shipped, what needs him, what went quiet and what
   nobody touched, each line opening its records — S5.
6. Open a user journey and see the screenshots of its screens joined by arrows, with walked,
   never-walked and stale proof told apart — S6.
7. See before/after screenshots attached to the screen a PR changed — S7.

Each criterion is proven by the observer named in §10.

## 3. Layout

```
┌──────────────────────────────────────────────────────────────────────────────────┐
│ ml-saas · Leva 5 · loja kit                              ( ● 1 decision needs you ) │  top bar + beacon
├──────────┬───────────────────┬─────────────────────────────────┬─────────────────┤
│ Projects │ Chat              │ [ Team (live) ] [ Journey: … ]  │ Journeys        │
│          │ Everyone | A↔B …  │                                 │  ▮▮▮▯ 2 of 4    │
│ ml-saas  │ ┌ question card ┐ │   🤖Coordinator                 │ Before / after  │
│  Leva 5  │ │ [Wait] [Merge]│ │   ╱  ⋯ live line ⋯  ╲           │  [before][after]│
│  Leva 4  │ └───────────────┘ │ 🤖kit1    🤖kit2    🤖kit3       │ What just       │
│          │ messages…         │  │         │         │          │ happened        │
│          │ Jev suggests [Use]│ [task]   [task]   [task]        │  kit 3 … now    │
│          │ [ composer      ] │                                 │                 │
└──────────┴───────────────────┴─────────────────────────────────┴─────────────────┘
          "While you were away" card overlays the canvas on return.
```

Columns, left to right: Projects (the existing rail), Chat, Canvas (two tabs), Right panel.
Below 1 100 px the right panel folds under the canvas; below 768 px one column with tabs
Chat / Team / Journeys. The Overview / Free canvas toggle, the free-ink tools and the
`work-overview.tsx` scroll page are removed (§9).

## 4. Studio components

All derivations are pure functions over the event tail and opened envelopes, each with fixture
tests, in `apps/studio/src/runtime/` or `apps/studio/src/graph/`. No new polling: the existing
4 s incremental events poll feeds everything.

### 4.1 Team model (`team.ts`)

`teamModel(events, envelopes, aliases, now) -> { bots, otherRecorders }`.

- A **bot** is: a persona (`persona_created` / `native_persona_linked`), or an actor id with an
  owner alias (§5.2), or an actor id with a record in the last 2 hours. Everything else is folded
  into `otherRecorders`, shown as one line "N other recorders" that expands to a list.
- The shared `codex` actor is never mapped to a thread (NATIVE_CHAT_BRIDGE rule); it appears as
  "Codex (shared)" with the note that its records cannot be attributed to one chat.
- Each bot carries: display name (alias > persona title > actor id), stable colour (hash of the
  bot key), role (persona charter first line, else none), `doingNow` (first line of its newest
  opened `operator_note`, else "No note yet"), `lastRecordAt`, `state`, and `tasks`.
- **State** is derived from records only:
  - `waiting_for_you` — it has an open question to `studio-operator` (§4.3) or an assigned node
    in `waiting_input`;
  - `working` — a record in the last 15 minutes;
  - `quiet` — no record for 15 minutes or more; the label says "No new record for 50 min", never
    "stuck" (absence rule, `docs/ux/TRAJECTORY_VIEW.md`);
  - `done` — its last task completed and nothing newer.
- **Tasks** come from `agent_task_created` / `agent_task_completed` signals by that actor and from
  graph nodes assigned to it. Tasks are never inferred from note text. A bot without task records
  shows only `doingNow`.

### 4.2 Team canvas (`team-canvas.tsx`)

- Bots in a row (coordinator-like personas first), their tasks stacked below each bot. Layout is
  deterministic from the bot order; the owner can drag, and positions persist per run in
  `localStorage` (a per-viewer convenience).
- A line joins two bots when a signal from one names the other in `to` (the existing `talks`
  derivation, `App.tsx` "talks"). The line animates only while the pair exchanged a record in the
  last 60 seconds; otherwise it is static and thin. Motion means new records, never decoration.
- A bot `working` has a slow pulse ring; `waiting_for_you` has an amber outline; `quiet` is
  dimmed. `prefers-reduced-motion` turns all motion off.
- Clicking a bot opens its thread in the Chat column; clicking a task opens the existing node /
  delivery panel when it is a graph node, or the task's records otherwise.
- Reuses the pan/zoom sheet from `board.tsx`; drops ink, notes and the People / Conversations /
  Work regions.

### 4.3 Beacon and question cards (`needs-you.ts`, `beacon.tsx`)

`needsYou(status, events, envelopes, nativeLedger) -> { state, items }`:

- items: unanswered questions (an agent envelope with `to: "studio-operator"` and no owner
  signal whose `replyTo` names it — the rule `/reply-suggestions` already uses), nodes in
  `waiting_input`, pending drafts awaiting approval, and native chat requests the ledger cannot
  confirm.
- `state` is `lit(n)` when items exist, `dark` when the Runtime answered and nothing is open, and
  `unknown` when the connection is stale ("Can't tell: the Runtime is not answering"). Dark is a
  claim that nothing blocks, so `unknown` must never render as dark.

The beacon sits right in the top bar; clicking it scrolls the Chat column to the first card.
Each item becomes a **question card** at the top of the Chat column with one row of buttons:

- the question's `recommendations` (graph-signal envelope field) become choice buttons; choosing
  one sends an owner `operator_note` with `replyTo` and the choice text;
- **Answer** focuses the composer with `replyTo` set;
- **Refuse** sends an owner signal of kind `owner_refusal` with `replyTo` and an optional reason
  (§5.3);
- for a native request the card says in plain words "Your message to loja kit 2 was not
  confirmed. It may not have arrived." with **Check it** (the existing refresh) — the transport
  ids move to the card's details.

The existing gating in `main-chat.tsx` (pending requests block team sends; the composer is locked
until the ledger is read) is kept unchanged. The accessible names "Principal conversation",
"Next step" and "Refresh request status" are kept, or `docs/acceptance/studio-main-chat-journey-2026-10-03.json`
is updated in the same change.

### 4.4 Chat column (`chat-column.tsx`)

- Thread tabs: **Everyone** (records with no `to`), one tab per bot pair that exchanged records,
  one tab per bot for direct lines with the owner. Unread counts per tab since the owner last
  opened it.
- Messages show avatar, name, recipient, time, and text from opened envelopes; sealed envelopes
  not yet opened show "Opening…" and stay counted.
- Bot-to-bot tabs are labelled "recorded messages": they show what agents recorded to each other
  through the Runtime, not native chats talking directly (NATIVE_CHAT_BRIDGE rule).
- The composer sends to the selected tab's audience: Everyone → team send (with its existing
  gate); a bot tab → that bot; `@name` in Everyone targets one bot.
- Jev suggestions render as one dashed card above the composer with **Use** (fills the composer;
  never sends).

### 4.5 Handover (`handover.ts`, `handover-card.tsx`)

`buildHandover(events, envelopes, fromSeq, toSeq) -> { shipped, needsYou, quiet, untouched }`,
every line carrying the event sequences it came from.

- `fromSeq` is the last sequence the owner saw, stored per project and run in `localStorage`;
  it advances only when the owner clicks **Got it** or keeps the live view visible for 10
  seconds. Missing storage means no card, never a wrong card.
- The card shows when the gap is at least 15 minutes and 20 events.
- shipped: `agent_task_completed`, `node_outcome_recorded` with a completed outcome,
  `completion_cleared`, and `jpd.screen_captured` with `phase: "after"` (linking its before/after);
  needsYou: §4.3 items opened in the gap; quiet: bots with no record for 30 minutes or more
  inside the gap; untouched: tasks and graph nodes that existed at `fromSeq` and got no record in
  the gap.
- Clicking a line opens the records it cites.
- The 20-second replay from the mockup is out of scope for this design (§11).

### 4.6 Right panel

- **Journeys**: one row per journey contract in the project (§6), with a segmented bar (green
  proven, red stale, grey never walked) and "N of M steps proven". Clicking opens the Journey tab.
- **Before / after**: newest pairs first, each "PR #N · screen · bot".
- **What just happened**: the existing `recentActivity`, rewritten to "bot verb object · time".

### 4.7 Journey tab (`journey-canvas.tsx`)

Renders the folded journey map from §6.4: one card per step with the latest screenshot
thumbnail, title, bot, short revision and age; arrows between consecutive steps.

- solid arrow: a `jpd.transition_walked` for that pair exists at a revision where both endpoint
  captures are fresh;
- dashed grey arrow, label "never walked": no such record;
- a capture whose screen code changed after its revision is drawn greyed with a crack mark and
  "Code changed after this shot";
- freshness `unknown` (no git, dirty capture, missing scope) is drawn as unknown, never as fresh.

Clicking a card opens the full screenshot with its before/after if any, the records it came from,
and the step's promise text from the contract.

## 5. Runtime changes

### 5.1 Image evidence (ADR-043)

Today evidence accepts any media type at rest (16 MiB per item) but the only read route refuses
anything not JSON or text, and no route uploads a binary. Add:

- **Attach on signal.** `POST /v1/executions/{id}/signal` accepts `attachments: [{ mediaType,
  base64 }]`, at most 4 per signal and 8 MiB decoded each. `mediaType` must be `image/png`,
  `image/jpeg` or `image/webp`, and the decoded bytes must start with that format's magic bytes;
  anything else refuses the whole signal (no partial append). Each attachment is sealed as its
  own evidence item (Confidential, same cipher as envelopes) and listed in the event's
  `evidenceRefs` after the envelope. The envelope's `evidence` strings may cite an attachment by
  its evidence id.
- **Read an image.** `GET /v1/executions/{id}/evidence/{evidenceId}` serves the three image
  types as bytes with that `Content-Type`, `X-Content-Type-Options: nosniff`,
  `Content-Security-Policy: default-src 'none'` and `Cache-Control: private, no-store`. SVG and
  every other binary type stay refused. Owner credentials only; scoped agent credentials still
  cannot read evidence.
- **Parity (D-039).** MCP `signal` gains `attachments` with the same rules; the CLI gains
  `graphhelm signal … --attach <file>` (repeatable), which reads the file, checks the type, and
  sends base64.
- Erasure, legal hold and ciphertext deletion apply to attachments as to any evidence item.

### 5.2 Actor alias (owner record)

An owner signal of kind `actor_alias`, sealed, `to` = the actor id, description a versioned JSON
document `graphhelm-actor-alias-v1` with `displayName` and optional `personaThreadId`. The
newest alias per actor wins. Refused for the shared actor `codex` and for actor ids that are not
in the run's records. Studio offers "Name this bot" on a bot without a persona. This replaces the
need to guess that `codex-lojakit-3` is "loja kit 3".

### 5.3 Owner refusal

An owner signal of kind `owner_refusal`, sealed, `replyTo` = the question's signal id,
description `graphhelm-owner-refusal-v1` with an optional `reason`. It answers the question for
the §4.3 rule. Agents see it on their next events read like any reply; the `leave-records` skill
is updated to say a refusal is final for that question.

### 5.4 Journeys read route

`GET /v1/executions/{id}/journeys` returns the folded journey maps for the run's project (§6.4),
computed server-side because freshness needs git. Owner credentials; MCP and CLI parity as
`graphhelm journeys --execution <id>` and an MCP `journeys` tool.

## 6. Keel and JPD changes (ADR-044)

### 6.1 Journey contract: screens

`extensions/builtin/graphhelm-jpd/schemas/journey-contract.schema.json` gains an optional
`screen` on each step:

```json
"screen": {
  "screenId": "cart",
  "title": "Cart",
  "scopePaths": ["web/src/routes/cart/", "web/src/components/CartLine.tsx"]
}
```

`scopePaths` uses Keel's existing meaning (a file or directory, exact or prefix match, no globs;
`core/policy/src/keel.rs` `check_scope`). Additive and optional: existing contracts stay valid.
Journey contracts live in the project at `.graphhelm/journeys/<contractId>.json`; the Studio and
the route read them from there.

### 6.2 Capture records

Two signal kinds, recorded by any agent or observer through §5.1:

- `jpd.screen_captured` — description `graphhelm-screen-capture-v1`: `contractId`, `stepId`,
  `revision` (full git sha), `dirty` (bool), `viewport` (`{width,height}`), `observer` (actor id),
  optional `pr` (number) and `phase` (`before` | `after`); exactly one image attachment.
  A capture with `dirty: true` is shown but never counts as fresh proof.
- `jpd.transition_walked` — description `graphhelm-transition-walked-v1`: `contractId`,
  `fromStepId`, `toStepId`, `revision`, `observer`, and the two capture signal ids it walked
  between.

These are signals, like the existing `jpd.journey` / `jpd.obligation` / `keel.card` /
`keel.proof` chain (`docs/keel/RECORDS.md`), so no new event kind and no event-envelope schema
change. The two description documents get JSON schemas under
`extensions/builtin/graphhelm-jpd/schemas/`.

### 6.3 Producers

- `graphhelm journey capture --contract <id> --step <id> --image <file> [--pr N --phase before|after]`
  reads the revision and dirty bit from git and records `jpd.screen_captured`.
- `graphhelm journey walked --contract <id> --from <step> --to <step>` records
  `jpd.transition_walked` between the newest captures of the two steps.
- `tools/playwright-observer` gains `--journey <contractId>`: after each step it takes a
  screenshot and records the capture; after each consecutive pair, the transition.
- `docs/process/DELIVERY.md`: a PR whose Keel scope touches a screen's `scopePaths` records a
  `before` capture at the base and an `after` capture at the head for each touched screen, and
  links them in the PR body. Guidance, not a gate (gates are off since 2026-09-24).
- The Keel card schema gains an optional `journeys: [contractId]` so a card names the journeys
  that prove its promise; `keel check` reports (advisory) when the card's scope touches a
  screen whose journey has no fresh capture at the head.

### 6.4 Folding and freshness (`core/` crate, used by §5.4)

For each contract: steps in order; per step the newest non-dirty capture; per consecutive pair
the newest transition. A capture is:

- `fresh` when `git diff --name-only <revision>..HEAD -- <scopePaths>` in the project is empty;
- `stale` when it lists files (the response carries them, so the card can say which file);
- `unknown` when git is unavailable, the revision is not in the repository, the step has no
  `scopePaths`, or the capture is dirty.

An arrow is `walked` when its newest transition's two captures are both `fresh`; otherwise
`never_walked` (no transition) or `stale` (a transition exists but a capture is not fresh). The
route caches results per `(revision, HEAD)` pair.

## 7. Security

- Screenshots may show secrets or personal data: they are sealed Confidential at rest like every
  envelope, served only to owner credentials, never cached, and covered by erasure.
- Only raster image types are accepted and served; magic bytes are checked on write; SVG and HTML
  are refused, so an attachment cannot carry script. The Studio renders images from `blob:` URLs
  in `<img>` only.
- Size and count caps (§5.1) bound the cost of one signal; the existing batch cap (64 MiB) still
  applies.
- Aliases and refusals are owner-only records; an agent cannot rename another agent.
- Freshness runs `git` with a fixed argument list in the project directory the Runtime was
  started with; contract ids and step ids are validated as identifiers before use; no path from a
  record reaches the shell.

## 8. Docs to update

- `docs/ux/STUDIO_SPEC.md`: replace the layout sections with a pointer to this spec.
- `docs/ux/STUDIO_MVP.md`: remove "No mobile layout"; fix the stale §7 note on Studio tests.
- `docs/studio/NATIVE_CHAT_BRIDGE.md`: bot-to-bot tabs are recorded messages; question cards
  replace the request-status box.
- `docs/keel/RECORDS.md`, `docs/harness/JOURNEY_PROVEN_DEVELOPMENT.md`: the two capture records
  and the screen field.
- `docs/reference/REFERENCE_STACK_AND_ADRS.md`: ADR-043 (image evidence) and ADR-044 (journey
  screens and captures).

## 9. Removed or replaced in the Studio

| Today | Becomes |
|---|---|
| Overview page (`work-overview.tsx`) | Team tab + handover card |
| Free canvas People / Conversations / Work regions, ink, notes | Team tab (bots, lines, tasks) and Journey tab |
| Topstrip run name, verdict tag, chips | Top bar with mission title and beacon; chips move into a menu |
| Main chat "JEV suggested next step" box | Jev card above the composer |
| "Request status" box and attention block | Question cards in the Chat column |
| Raw ids on People cards | Details only |
| Resume clicking "Free canvas" by its text (`App.tsx` resume) | Resume selects the Team tab by state |

## 10. Phases and proof

Each phase is its own issue and PR, in this order. Each PR lists the tests it ran.

1. **Studio shell, Team, beacon, question cards, handover** — Studio only, existing data.
   Proof: unit tests for `teamModel`, `needsYou`, `buildHandover` over a fixture cut from the
   `lojakit-levas-2-a-5` journal shape (ids replaced); App tests for the layout; the main-chat
   acceptance journey still passes. Observer for S1–S5: open the live ml-saas run in the browser
   pane and record a screenshot of the Team tab, the beacon lit and dark, and the handover.
2. **Actor alias and owner refusal** — Runtime owner-only signal checks, Studio "Name this
   bot" and "Refuse". Proof: Rust tests that a non-owner alias and a `codex` alias are refused;
   Studio tests that an alias renames the bot and a refusal closes the question.
3. **Image evidence (ADR-043)** — attach, serve, MCP and CLI parity. Proof: Rust tests for each
   allowed type, wrong magic bytes, SVG refusal, size and count caps, no partial append,
   owner-only read, response headers, and the parity test.
4. **Journey screens, captures, folding, route (ADR-044)** — schema field, capture and walked
   CLI, folding with freshness against a temporary git repository (fresh, stale with the file
   named, unknown for each cause), route and MCP parity.
5. **Studio Journey tab and Before / after** — Proof: rendering tests over a folded fixture;
   observer for S6–S7: capture three screens of the Studio itself with the new CLI, open the
   journey in the browser pane, screenshot it.
6. **Producers and guidance** — Playwright observer `--journey`, DELIVERY.md guidance, optional
   `journeys` on the Keel card and the advisory `keel check` finding.

Phases 1 and 2 ship value without any Runtime change to evidence; phases 3–5 are needed for
journeys and before/after.

## 11. Out of scope

- The 20-second canvas replay, phone notifications, batching duplicate questions across bots,
  ghost-run comparisons, and self-arranging layouts. They can follow once phases 1–5 are used.
- Inferring tasks or states from note text with a model.
- Agents creating graph nodes directly; graph growth stays governed (AGENTS.md, Governor).
