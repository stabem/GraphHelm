# Studio — functional and interaction specification

## 1. Objective

Studio is GraphHelm's local control plane. It brings together chat, the operational graph, running agents, documentation, files, artifacts, events, policies, and settings. The interface must let a non-specialist user understand the workflow, while offering enough depth for a Graph Engineer to edit contracts and policies.

The graph is not an illustrative animation. Each node represents a real unit of execution, and each edge represents dependency, data, evidence, condition, or control.

## 2. Main structure

Standard desktop layout:

```text
┌────────────────────────────────────────────────────────────────────────────────┐
│ Top bar: workspace / project / mode / execution / connection / search / actions │
├───────────────┬──────────────────────────────────────┬─────────────┬─────────────┤
│ Navigation    │ Graph canvas                          │ Agents      │ Docs/Files  │
│ Workspace     │                                       │ running     │ Artifacts   │
│ Projects      │                                       │             │ Images      │
│ Executions    │                                       │             │ Claims      │
│               ├──────────────────────────────────────┤             │             │
│               │ Chat and Command Composer             │             │             │
└───────────────┴──────────────────────────────────────┴─────────────┴─────────────┘
```

All panels are resizable. The user can collapse navigation, agents, or docs to expand the canvas. The layout is saved locally per workspace.


## 2.1 Conceptual references provided

- [Original Studio sketch](../assets/original-studio-sketch.png): projects on the left, central graph, chat at the bottom, agents and files/documents on the right.

Graph nodes need readable identities and properties; edges need clear names and relationships.

The references guide the functional organization. The final visual design must be refined, accessible, and responsive, without copying the limitations of the hand-drawn sketch.

### 2.2 Recommended widths

- left navigation: 240–320 px;
- agents panel: 260–360 px;
- docs/files panel: 320–480 px;
- bottom chat: 120–360 px in height;
- canvas occupies all remaining space.

These values are defaults, not fixed constraints.

## 3. Top bar

Components:

- Workspace selector;
- `Project / Subproject` breadcrumb;
- active execution selector;
- mode indicator: Autopilot, Supervised, Manual Graph;
- Runtime connection indicator;
- global status: running, paused, waiting capacity, blocked, completed;
- summarized cost/capacity;
- global search;
- `New work` button;
- `Graph Draft` button when a draft exists;
- export, replay, and settings menu.

### 3.1 Offline behavior

If Studio loses connection:

- it shows a persistent banner;
- it keeps reading the last local snapshot;
- it blocks mutations that require Runtime confirmation;
- it allows writing local command drafts;
- it reconnects automatically;
- after reconnecting, it compares versions before applying any draft.

## 4. Side navigation

Sections:

1. **Workspaces**
2. **Projects**
3. **Subprojects**
4. **Executions**
5. **Agents**
6. **Skills and capabilities**
7. **Documentation**
8. **Dreams**
9. **Models**
10. **Policies and security**
11. **Events and metrics**
12. **Settings**

Each project shows badges for active executions, blockers, stale docs, pending dreams, and connection status.

## 5. Graph canvas

### 5.1 Anatomy of a node

```text
┌─────────────────────────────────────┐
│ icon  Node name                status│
│ Agent • Model • isolation tier      │
├─────────────────────────────────────┤
│ summarized objective                │
│ progress / step / last action       │
├─────────────────────────────────────┤
│ context 12k | 3 tools | 04:21       │
│ evidence 4/6 | retries 0/2          │
└─────────────────────────────────────┘
```

Optional elements:

- `GATE` badge;
- `MANUAL OVERRIDE` badge;
- `DREAM GENERATED` badge;
- `PROPOSED` badge for ghost node;
- new output indicator;
- expanded context indicator;
- lock when the node is already materialized and valid;
- invalidated output warning.

### 5.2 Visual states

- draft: neutral dashed border;
- ghost/proposed: 50% opacity, dashed;
- queued: subtle indicator;
- running: pulse or reduced progress indicator, respecting reduced motion;
- waiting input: human icon;
- waiting capacity: clock/quota icon;
- paused: pause icon;
- blocked: warning with cause;
- succeeded: check mark;
- failed: error;
- waived: check mark with caveat;
- skipped: transparency;
- invalidated: hatching/strikethrough and badge.

Color is never the only means of indicating state.

### 5.3 Edges

Each edge can display:

- contract label;
- condition;
- type: data, control, evidence, event, failure, compensation;
- payload state;
- artifact count;
- schema mismatch;
- manual breakpoint.

On hover, it shows origin, destination, condition, payload, and last traversal.

### 5.4 Organization

- hierarchical, radial, swimlane, or free-form auto-layout;
- collapsible groups by branch, phase, domain, or subgraph;
- minimap;
- semantic zoom: at low zoom, show only names and status;
- pinning of important nodes;
- filters by status, agent, model, cost, risk, origin, and tag;
- side-by-side comparison of Graph Versions.

### 5.5 Interactions

- click selects a node;
- double click opens the full inspector;
- dragging changes only the visual position;
- dragging a port creates an edge in the Graph Draft;
- Delete creates a removal in the Graph Draft;
- Shift+click selects a subgraph;
- right click opens actions;
- Space+drag moves the canvas;
- Ctrl/Cmd+K opens the command palette;
- Ctrl/Cmd+Enter sends the chat message;
- Ctrl/Cmd+Shift+Enter sends it as a new execution.

## 6. Chat and Command Composer

### 6.1 Components

- multiline field;
- current target selector;
- attached context chips;
- attachments;
- optional `ask`, `instruct`, `new execution` modes;
- estimate of operational effect;
- send button;
- summarized history.

The user is not required to manually select an intent. The Command Router classifies it.

### 6.2 Classification result

After an operational message, show a strip:

```text
Interpreted as: mutation of the current graph
Target: exec-482
Confidence: high
Proposed action: remove Integration Tests and connect Implementation → Deploy
[Review draft] [Correct interpretation] [Cancel]
```

Advisory messages receive a response without mutation.

### 6.3 Mentions

Autocomplete for:

- `@project`
- `@execution`
- `@graph`
- `@node`
- `@agent`
- `@document`
- `@file`
- `@harness`

### 6.4 System questions

Genuinely necessary questions enter the graph as `human_decision` and appear in the chat. The user can respond there or in the node inspector.

## 7. Running agents panel

Compact list by status:

- name and role;
- current node;
- model/route;
- duration;
- capacity/quota;
- tokens when available;
- tool in use;
- isolation;
- last event;
- pause/stop/open button.

Actions:

- open agent;
- pause after the current call;
- stop immediately;
- switch model for the next attempt;
- view context;
- view tools;
- mute notifications;
- save definition after execution, via explicit action.

Disabling an agent pauses the branch at the next safe checkpoint. Replacements appear as proposals and never start automatically after manual intervention.

## 8. Docs, Files, and Artifacts panel

Tabs:

1. **Docs** — living documentation, freshness status, related claims.
2. **Files** — repositories, directories, attached and remote files.
3. **Artifacts** — patches, reports, images, datasets, builds, exports.
4. **Evidence** — tests, sources, logs, snapshots, diffs.
5. **Claims** — Knowledge Graph statements and relations.
6. **Pictures** — visual preview of images and screenshots.

Each item shows:

- origin;
- version;
- who produced it;
- consuming nodes;
- validity;
- hash;
- sensitivity classification;
- actions: open, pin to context, compare, export, mark obsolete.

## 9. Node Inspector

The inspector can open as a drawer or full screen.

### 9.1 Overview tab

- name, type, status;
- objective;
- rationale for existing in the graph;
- origin: harness, user, dream, mutation;
- dependencies and dependents;
- progress;
- current blocker.

### 9.2 Agent tab

- definition used;
- version;
- prompt/instructions;
- capabilities;
- prohibited actions;
- memory policy;
- agent history in the project;
- `Save as new agent` button.

Changes are overlays exclusive to that execution.

### 9.3 Model tab

- current route;
- candidates and scores;
- availability;
- quota/cost;
- executor independence;
- supported parameters;
- manual switch action.

### 9.4 Context tab

- token budget;
- Project Kernel;
- Task Capsule;
- Node Capsule;
- Evidence Bundle;
- Dependency Outputs;
- Agent Experience;
- excluded items;
- expansion requests;
- add/remove item button.

### 9.5 Skills and Tools tab

- loaded skills;
- tools and permissions;
- capability leases;
- calls made;
- allowed network and filesystem access;
- edit button.

### 9.6 Contracts tab

- input schema;
- output schema;
- completion contract;
- evidence requirements;
- live validation;
- example payload.

### 9.7 Runtime tab

- isolation tier;
- container/worktree/microVM;
- resources;
- timeout;
- retries;
- checkpoint;
- redacted logs;
- cleanup/quarantine.

### 9.8 Events tab

Filtered timeline for the node, including calls, outputs, signals, errors, retries, mutations, and waivers.

## 10. Graph Draft Review

When editing operationally, open a panel with:

- base version;
- visual diff;
- list of added/removed nodes;
- changed edges;
- branches to pause;
- invalidated outputs;
- ignored gates;
- unmet obligations;
- estimated cost/time;
- technical incompatibilities;
- non-blocking warnings.

Actions:

- apply;
- save draft;
- discard;
- ask the harness to repair;
- edit again;
- apply selection only.

Application is atomic. If the Runtime has changed version since the draft was created, Studio requires a visual rebase.

## 11. Ghost node review

The ghost node shows:

- proposed function;
- reason;
- evidence that triggered it;
- suggested model;
- cost/time;
- permissions;
- gates satisfied;
- dependencies.

Actions:

- approve;
- edit and approve;
- replace with a saved agent;
- reject;
- leave for later;
- save without executing;
- mark obligation as waived.

## 12. Workspace Home

Widgets:

- recent projects;
- active executions;
- blockers;
- model usage;
- subscription capacity;
- recent dreams;
- stale documents;
- best/worst performing agents;
- security risks;
- runtime health.

No widget depends on external telemetry.

## 13. Onboarding

Steps:

1. choose language and local name;
2. create or import a workspace;
3. connect to a VPS via SSH;
4. review installation plan;
5. install Runtime;
6. create vault;
7. connect at least one model route;
8. create/import project;
9. choose repository, files, or sources;
10. run diagnostics;
11. open first prompt.

Each step can be resumed. The user can use a local model without an external account.

## 14. Model Connections

Cards per route:

- provider;
- transport;
- auth type;
- connected profile;
- status;
- capabilities;
- observed quota;
- recent throttles;
- privacy note;
- test, reconnect, remove.

Subscription connections use official login. BYOK shows configured scope and cost. Secrets are never displayed after being stored.

## 15. Agent Registry

List with:

- name;
- purpose;
- active version;
- status;
- executions;
- success rate;
- false-positive rate;
- cost/duration;
- last validation;
- active memories;
- tags and scope.

Detail:

- definition;
- versions;
- performance by scenario;
- experiences;
- relationships with skills;
- graphs it has appeared in;
- merge/derive/archive;
- test-in-sandbox button.

## 16. Skills and Capabilities

Browser with filters by type, permission, runtime, publisher, trust level, and compatibility. Installation shows the manifest and permissions.

Views:

- installed;
- project-local;
- workspace-shared;
- community registry;
- quarantined;
- updates.

## 17. Docs and Knowledge

### 17.1 Documentation Browser

- docs tree;
- status: current, stale, conflicted, generated, manually edited;
- Markdown/diagram preview;
- claims and evidence sidecar;
- diff between versions;
- freshness score;
- pin as canonical.

### 17.2 Knowledge Graph Explorer

Visualization by entities and relations, with filters by confidence, status, temporality, project scope, and provenance.

The knowledge graph is separate from the execution graph, although they can reference each other.

## 18. Dreams Center

Shows:

- next trigger;
- budget;
- idle policy;
- recent cycles;
- proposed/applied changes;
- shadow tests;
- critic result;
- rollback;
- `dream_generated` tasks;
- estimated context savings.

The user can start a dream manually, pause the scheduler, or limit categories.

## 19. Policies and Security

Sections:

- global policies;
- project policies;
- hard constraints;
- dispensable gates;
- secrets;
- isolation defaults;
- network allowlists;
- production targets;
- waivers;
- plugin permissions;
- audit retention.

The interface distinguishes between:

- technical impossibility;
- hard policy defined by the owner;
- system recommendation;
- dispensed gate.

## 20. Events, Metrics, and Replay

### 20.1 Timeline

Filters by execution, graph version, node, agent, model, tool, severity, actor, and event type.

### 20.2 Replay

- play/pause;
- speed;
- scrubber;
- graph version switch;
- opening the payload of each event;
- before/after state comparison;
- hide sensitive content.

### 20.3 Metrics

- duration;
- tokens/cost;
- quota;
- context saved;
- retries;
- gates;
- errors;
- mutation count;
- agent performance;
- evidence coverage.

## 21. Critical flows

### 21.1 New prompt

1. User sends a message.
2. Studio shows classification in progress.
3. An initial graph draft appears.
4. In Autopilot, an approved lint publishes and executes it.
5. In Supervised/Manual, it waits for confirmation according to policy.
6. Agents appear in the panel.
7. Outputs appear in docs/artifacts.

### 21.2 Skip tests and go to deploy

1. User drags the `Implementation → Deploy` edge or writes a command.
2. The draft shows tests/review removed.
3. Studio lists risks and obligations.
4. User applies.
5. Runtime creates a waiver and a new Graph Version.
6. The branch proceeds without re-adding nodes.

### 21.3 Subscription limit

1. Node receives a quota response.
2. Checkpoint.
3. `waiting capacity` status.
4. Studio shows the route and options.
5. User waits or chooses another route.
6. Resumption preserves outputs.

### 21.4 Disable agent

1. User clicks stop/disable.
2. Node stops at the chosen checkpoint.
3. Harness calculates lost coverage.
4. Alternatives appear as ghost nodes.
5. Nothing starts without confirmation.

## 22. Accessibility

- all nodes accessible via an alternative list;
- keyboard navigation between nodes and edges;
- textual status labels;
- high-contrast mode;
- reduced motion;
- exportable linear description of the graph;
- configurable shortcuts;
- focus preserved after real-time updates.

## 23. Studio acceptance criteria

- the user can operate without opening a terminal after bootstrap;
- any operational action leaves an auditable trail;
- a graph draft never applies without confirmation when initiated by a manual action;
- a ghost node consumes no resources before approval;
- closing/reopening Studio preserves layout and reconnects the execution;
- each node allows opening context, agent, model, tools, contracts, and events;
- the user can go from implementation to deploy via explicit override;
- the canvas and the alternative list represent the same state;
- no credential appears in the UI, logs, or exports.
