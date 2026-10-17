# GraphHelm

**Run AI-agent workflows with a clear view of the work and control over what happens next.**

GraphHelm is a local-first runtime for AI-agent workflows. Define the work as a typed execution graph, follow its progress, and see when it needs your attention. Inspect the evidence, approve a blocked step, or pause and resume through the CLI, MCP, or local Studio. Claude Code and Codex plugins bring it into your coding sessions, with [Keel](https://github.com/stabem/GraphHelm/blob/main/KEEL.md) for how agents write code and user journeys that prove a change works.

[Try a local run](#try-a-local-run) · [Install in your project](https://github.com/stabem/GraphHelm/blob/main/INSTALL.md) · [Explore the Studio](https://github.com/stabem/GraphHelm/blob/main/apps/studio/README.md) · [Documentation](https://github.com/stabem/GraphHelm/blob/main/docs/INDEX.md)

**Experimental · Local-first · MIT licensed.** The CLI, Runtime, MCP server, and operator Studio work today. Expect a developer-oriented setup and evolving interfaces; there is no stable release yet.

## Why GraphHelm

Delegating a multi-step task raises practical questions: what is running, what is waiting, and what evidence supports the result?

- **Know when to step in.** An attention verdict names the blocked work or missing information. Uncertainty stays visible instead of becoming an all-clear.
- **Keep decisions explicit.** Typed graphs describe dependencies, completion requirements, and budgets. Supported controls let you pause, approve, resume, or cancel an execution.
- **Pick up with context.** Execution briefings and event history give a later session a recorded account of the work and pending decisions.
- **Prove it works.** A [user journey](https://github.com/stabem/GraphHelm/blob/main/docs/guides/journeys.md) replays the real flow in a browser and records what it saw. With no observer, the result stays `OBSERVER_MISSING` instead of passing.
- **Keep agent code in shape.** [Keel](https://github.com/stabem/GraphHelm/blob/main/KEEL.md) gives every agent the same steps (plan, execute, review, merge), and `graphhelm keel check` blocks the rules a machine can measure.
- **Inspect the result.** Append-only event history, evidence, and replay make a run reviewable beyond a chat transcript.

Use it for agent workflows with several steps, human review points, or work that continues across sessions. Start locally, learn the control model, then connect the model and tool routes your project needs.

<a id="try-it-locally"></a>
## Try a local run

This demo deliberately fails the implementation step so you can see how GraphHelm asks for intervention. **It uses fixture outcomes: it does not call a model, edit a repository, or deploy anything.**

Prerequisites: Git, the pinned **Rust 1.97.1** toolchain, and your platform's build dependencies. See [prerequisites](https://github.com/stabem/GraphHelm/blob/main/docs/install/GETTING_STARTED.md#prerequisites). Cargo may download dependencies during the first build.

Run in Bash on Linux or macOS:

```bash
git clone https://github.com/stabem/GraphHelm.git
cd GraphHelm

DEMO_DIR="$(mktemp -d)"
printf '%s\n' '{"nodeOutcomes":{"implementation":"failure"}}' > "$DEMO_DIR/fixtures.json"

cargo run --locked -p graphhelm-cli -- execution start \
  --file examples/graphs/manual-override-deploy.yaml \
  --events "$DEMO_DIR/events" \
  --fixtures "$DEMO_DIR/fixtures.json" \
  --mode supervised --execution demo --pretty

cargo run --locked -p graphhelm-cli -- execution status \
  --events "$DEMO_DIR/events" --execution demo
```

Look for `data.attention: "needs_you"` and a `blocked_node` reason naming `implementation`. The event store stays in the temporary directory printed by `echo "$DEMO_DIR"`.

| Attention | What it tells you |
| --- | --- |
| `needs_you` | A named condition requires human attention |
| `can_sleep` | Nothing needs attention, and in-flight work has been checked |
| `unknown` | There is not enough information to judge in-flight silence |
| `calmed_by_amendment` | An explicit silence-budget amendment accounts for the quiet |

For automation, read the JSON field: a successful `execution status` command can exit `0` while reporting `needs_you`. Treat unfamiliar attention values conservatively.

Continue with the [offline quickstart](https://github.com/stabem/GraphHelm/blob/main/QUICKSTART.md), or follow the [full local walkthrough](https://github.com/stabem/GraphHelm/blob/main/docs/install/GETTING_STARTED.md) for the Runtime and Studio, including Windows commands. Real model execution requires separately configured gateway routes and credentials; [provider-less mode](https://github.com/stabem/GraphHelm/blob/main/docs/product/PROVIDER_LESS_MODE.md) explains the boundary.

<a id="install-the-agent-plugin"></a>
## Use GraphHelm in your project

Follow [INSTALL.md](https://github.com/stabem/GraphHelm/blob/main/INSTALL.md) for the ordered, host-specific checklist. It covers:

1. Build and install the current CLI from source
2. Initialize the project for the host you choose
3. Start its authenticated local Runtime with the same project identity
4. Install the plugin and register MCP in that host
5. Open a fresh host session and verify an actual Runtime tool call

**Give an agent INSTALL.md when asking it to install GraphHelm.** Installing the plugin alone does not install the CLI, start the Runtime, or complete MCP registration.

| Integration | Available path |
| --- | --- |
| Claude Code | Guide, setup, and resume skills; project-aware MCP registration |
| Codex CLI | Guide, setup, and resume skills; hook compatibility companion and a reviewed MCP configuration snippet |
| Local Studio | Browser-based execution and evidence inspection, attention views, and supported control actions; Node 22+ required |

See the [plugin guide](https://github.com/stabem/GraphHelm/blob/main/plugins/graphhelm/README.md) for commands, host trust, and optional hooks. Configuration and local tests do not establish native host activation: check each selected host in a fresh session.

The [v0.1.1 CLI preview](https://github.com/stabem/GraphHelm/releases/tag/v0.1.1) provides Windows x86-64 and Ubuntu x86-64 binaries with the current project adoption flow. macOS and other platforms use the source path, with less platform validation. Studio also runs from source.

<a id="what-works-today"></a>
<a id="repository-map"></a>
## How it fits together

The Rust core owns typed contracts, graph validation, and execution rules. Adapters connect models, tools, and local or PostgreSQL event storage. CLI and MCP expose execution controls; Studio operates through the public Runtime API. Graph versions, transactional drafts, and event history keep changes inspectable.

Explore [`core/`](https://github.com/stabem/GraphHelm/tree/main/core), [`adapters/`](https://github.com/stabem/GraphHelm/tree/main/adapters), [`apps/`](https://github.com/stabem/GraphHelm/tree/main/apps), and the [example graphs](https://github.com/stabem/GraphHelm/tree/main/examples/graphs).

<a id="development-method"></a>
## Evidence before completion

GraphHelm includes two complementary development methods:

- [Journey-Proven Development](https://github.com/stabem/GraphHelm/blob/main/docs/harness/JOURNEY_PROVEN_DEVELOPMENT.md) defines what an observer must verify about the user's actual journey. Missing proof remains `OBSERVER_MISSING`.
  Start with the [user journeys guide](https://github.com/stabem/GraphHelm/blob/main/docs/guides/journeys.md): author a flow, validate, approve, replay, and read capture freshness.
- [Keel](https://github.com/stabem/GraphHelm/blob/main/KEEL.md) is the construction pattern every agent follows: one file of values, one file per step (plan, execute, review, merge), the cheapest observer that proves the promise, and a merge block for every rule a machine can measure. Six rules block today through `graphhelm keel check` (write scope, names, bare flags, panics, missing deadlines, swallowed errors); the others are checked by the reviewer until their check lands.

The specifications distinguish guidance from implemented checks. See the [delivery process](https://github.com/stabem/GraphHelm/blob/main/docs/process/DELIVERY.md).

## How work flows

Two processes, one inside the other. GraphHelm says how a change travels from an issue to `main`;
Keel says how the code inside that change is written and proved.

### GraphHelm delivery

```text
                 ┌──────────────────────────────────────┐
                 │  GitHub issue                        │
                 │  one label: current-wave | in-flight │
                 │  | tech-debt | product-vision        │
                 └──────────────────┬───────────────────┘
                                    │ branch issue-<N>-<desc>
                                    ▼
                 ┌──────────────────────────────────────┐
                 │  Keel card, sized to the change      │
                 │  (see Keel below)                    │
                 └──────────────────┬───────────────────┘
                                    ▼
                 ┌──────────────────────────────────────┐
                 │  Work: start from the promise,       │
                 │  add only what it needs, prove it    │
                 │  PR body lists every test run        │
                 └──────────────────┬───────────────────┘
                                    ▼
                 ┌──────────────────────────────────────┐
                 │  One review                          │
                 │  another session or a blind subagent │
                 │  reads earlier comments, runs the    │
                 │  reached tests + keel check          │
                 └───────┬──────────────────────┬───────┘
                         │                      │
                       BLOCK         APPROVE | APPROVE-WITH-RISK
                         │                      │
          fix (new head) │                      ▼
          or answer why  │     ┌──────────────────────────────────┐
          it does not    │     │  Merge by the approving reviewer │
          hold           │     │  head pinned to the reviewed sha │
                         │     │  squash, delete branch           │
                         ▼     └────────────────┬─────────────────┘
                 back to review                 ▼
                                    Delivered = merged into main
                                    and main green on the main proof
                                    (Keel checks block; no presser)
```

Every comment, review and merge starts with `Session: <name> · Head: <sha8>`. Long form:
[delivery process](https://github.com/stabem/GraphHelm/blob/main/docs/process/DELIVERY.md).

### Keel

```text
                    What does the change touch?
                                │
   ┌──────────────┬─────────────┼───────────────┬──────────────────────┐
   ▼              ▼             ▼               ▼                      ▼
 docs, config,  bounded       a screen       new public            persistence,
 one line,      code          (journey       surface               permissions,
 test only      change        scope)         (module, type,        security,
                                             fn, dep, route)       external effects
   │              │             │               │                      │
 summary        card: paths,  card +          full card,            full card
 only           promise,      journeys        surface named         + journey
                proof         replayed green                        + failure list
   └──────────────┴─────────────┴───────┬───────┴──────────────────────┘
                                        ▼
   PLAN    keel/KEEL_PLAN.md   promise, proof, search what exists,
                               cost (Big O), risks, measured gain
                                        ▼
   EXEC    keel/KEEL_EXEC.md   stay in the card, read the symbol not the file,
                               cheapest observer that proves the promise,
                               unit test only for a found bug, break it once,
                               whole reached plan, receipt in GraphHelm
                                        ▼
   REVIEW  keel/KEEL_REVIEW.md blind, reruns everything, lists every value
                               that does not hold plus the card's risk row,
                               measures any promised gain before and after
                                        ▼
   MERGE   keel/KEEL_MERGE.md  pinned head, rerun if main moved,
                               check after, clean up what you created
                                        ▼
           Values (KEEL.md): cohesion, no duplicate, no dead code,
           inward dependencies, searchable names, no hidden magic,
           reliability, idempotency, less complexity, Big O, quiet tools
           measured rule broken → merge blocked, exception written in code
           (6 rules measured today; the rest read by the reviewer)
```

Long form: [KEEL.md](https://github.com/stabem/GraphHelm/blob/main/KEEL.md), and the reasons in
[the Keel rationale](https://github.com/stabem/GraphHelm/blob/main/docs/keel/KEEL_SPEC.md).

## Control and trust

- Runtime endpoints bind to loopback; protected API operations use bearer-token authentication
- `graphhelm setup` previews supported host changes and requires an exact reviewed plan digest before applying them, with backups and a restore flow
- Model proposals do not grant permissions; deterministic validation governs supported graph changes
- Local-first orchestration does not mean every connected provider stays local. Review model and tool routes before sending project data
- The local event store is not encrypted at rest. Protect credentials and execution evidence accordingly

## Project status and direction

The local Studio is an operator MVP. A visual graph editor, embedded AI chat, the full Context Compiler, and Dreams Engine remain future work. The [roadmap](https://github.com/stabem/GraphHelm/blob/main/docs/product/ROADMAP_AND_ACCEPTANCE.md) describes targets, not a list of shipped features. There is no hosted CI service; validation is run locally.

<a id="contributing-and-trust"></a>
## Contribute

Try the documented journey and report where it breaks. Include your version, operating system, host, command, and observed result, with secrets removed.

[Contribution guide](https://github.com/stabem/GraphHelm/blob/main/CONTRIBUTING.md) · [Issues](https://github.com/stabem/GraphHelm/issues) · [Support](https://github.com/stabem/GraphHelm/blob/main/.github/SUPPORT.md) · [Code of conduct](https://github.com/stabem/GraphHelm/blob/main/.github/CODE_OF_CONDUCT.md)

Report vulnerabilities privately through [SECURITY.md](https://github.com/stabem/GraphHelm/blob/main/SECURITY.md). GraphHelm is [MIT licensed](https://github.com/stabem/GraphHelm/blob/main/LICENSE); bundled Studio fonts retain their [OFL notices](https://github.com/stabem/GraphHelm/blob/main/apps/studio/src/fonts/README.md).

<a id="make-claude-always-use-graphhelm--keel"></a>
<details>
<summary>Optional: GraphHelm + Keel instructions for Claude</summary>

1. **Claude Code:** follow [INSTALL.md](https://github.com/stabem/GraphHelm/blob/main/INSTALL.md) for MCP and optional instruction blocks
2. **claude.ai:** review and paste the preference below into Settings → Instructions for Claude. This instruction applies to that separate product; it does not connect it to your local Runtime

```text
Use GraphHelm and Keel for software work.
- If the GraphHelm MCP tools are available (named mcp__graphhelm__* or mcp__plugin_graphhelm_graphhelm__*, depending on the host), use them for state: briefing, status, events, evidence, resume. Otherwise use the graphhelm CLI with --json, and say which source you used.
- Keel: before any code work, read the project's KEEL.md and the keel/ file for your step (KEEL_PLAN.md, KEEL_EXEC.md, KEEL_REVIEW.md or KEEL_MERGE.md). They are the rules and win over these defaults. Without them: keep effort proportional, state the promise and the command that proves it, name the paths in scope, search before you add, add only what the promise needs.
- Prove the promise with the cheapest observer that proves it; every new test must catch a deliberate break.
- Journey-Proven Development: work is done only when an observer proves the user-visible promise. With no observer, say OBSERVER_MISSING; never claim success from a proxy.
- Report passed, failed, skipped and unobserved separately. A later green never erases an earlier red.
- Issue first, one PR per issue, one independent review before merge.
- A project's own KEEL.md, AGENTS.md or CLAUDE.md overrides these defaults.
```

</details>
