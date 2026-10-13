---
name: graphhelm-kickoff
description: Use when the owner wants to start a brand-new project from zero on GraphHelm and Keel, asks to be grilled about an idea, wants the user journeys and the full project doc written with them before any code, and then wants the build orchestrated and followed in the Studio.
---

# GraphHelm kickoff

Take a new project from an idea to a running, journey-first build. Three phases, in order:
**grill**, **document**, **build**. Never write product code before the owner accepts the doc.
For a project that already has code and no journeys, use `journey-map` instead.

## Phase 1: Grill

Interview the owner until the project is unambiguous. Rules:

- Ask **one question at a time**, in the owner's language, in small words. Offer 2-3 concrete
  options when you can, with your recommendation marked; accept a free answer.
- Push on vague answers ("users", "fast", "simple") until each is a person, a number or a screen.
- Do not ask what the repository or a sensible default can answer; state the default instead.
- Stop when you can write every section of the doc below without guessing. Then read back a
  five-line summary and ask: "Is this right?"

Cover, in this order:

1. **Problem and who has it.** One sentence each. Who pays, who uses, how they do it today.
2. **The promise.** The one thing a user can do that they cannot do now.
3. **Actors.** Every kind of user and outside system (payment, email, an API).
4. **Journeys.** For each actor, the 3-5 trips through the product they would notice if broken:
   where they start, each screen they see, what they click or type, where they end. Walk each one
   screen by screen with the owner. Ask for the exact button and heading text a user sees.
5. **Risks.** Money, personal data, sign-in, anything irreversible. These pick the Keel card row.
6. **Out of scope.** What the first version will not do.
7. **Stack and hosting.** Recommend one; accept the owner's.
8. **Done for v1.** Which journeys must pass for the first release.

## Phase 2: Document

Write the results into the project (create the repository folder if needed):

1. `docs/PROJECT.md` with sections: Problem, Promise, Actors, Journeys (one short paragraph and a
   screen list per journey), Risks, Out of scope, Stack, v1 done, Open questions.
2. `graphhelm init` in the project root, so the Runtime, MCP entry and Studio know the project.
3. One flow per journey in `.graphhelm/journeys/<id>.journey.yaml`, schema
   `graphhelm.journey-flow/1`, `status: draft`, `approved: null`. Follow the flow format, id rule
   and canonical form in the `journey-map` skill (GraphHelm JPD package), section (b). Screens
   that do not exist yet take `scope: unknown`; set real `scopePaths` once the code exists.
4. `graphhelm journey validate --all`, fix every finding, then
   `graphhelm journey compile --include-draft`.
5. Ask the owner to read `docs/PROJECT.md` and approve each flow in the Studio's Journey tab
   (`graphhelm studio start` opens it) or with `graphhelm journey approve <id>`. Never approve a
   flow yourself. Edits after approval follow `journey-map` (c2).

## Phase 3: Build

When the doc is accepted and the v1 flows are approved:

1. **Backlog.** Split the work into issues, smallest user-visible slice first, one journey (or one
   screen of it) per issue. Title and `Summary:` line follow the repository's naming rule if it
   has one (GraphHelm's `docs/process/DELIVERY.md` §1). Put the scaffold (repo, CI script,
   Playwright observer: `graphhelm setup --project <root> --home <home> --install-observer
   playwright`) in the first issue.
2. **Orchestrate.** Start one GraphHelm execution for the project (`mcp__graphhelm__start` or the
   CLI) so the Studio shows it. Run every issue through the five steps: `task-plan`, `implement`
   (under `keel`), `journey-prove`, `blind-review`, `merge`. Independent issues may run in
   parallel sessions; issues on the same screen run in order.
3. **Follow along.** The owner watches the Studio: Team tab for tasks and PRs, Journey tab for
   each screen's capture and freshness. A journey with no observer stays `OBSERVER_MISSING`;
   never report it as passing.
4. **Report.** After each merge, tell the owner in one line which journey moved and what is next.
   Ask only for what only the owner can give: a secret, an account, money, or a change to the
   promise. Everything else, decide and say which default you took.

On a Windows host, run commands hidden or in one terminal; never open a visible console window
per command.

## Done

v1 is done when every journey named in "v1 done" is approved and its latest replay at `main`
shows every step fresh. Until then, say what is missing.
