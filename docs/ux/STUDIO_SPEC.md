# Studio — functional and interaction specification

## 1. Objective

Studio is GraphHelm's local control plane for coding-agent work and its journey evidence.
The project screen presents Graph, Lanes and Proof through the public Runtime API.

The graph is not an illustrative animation. Each node represents a real unit of execution, and each edge represents dependency, data, evidence, condition, or control.

## Project navigation and views

The sidebar has one card per project, labelled with its name, and no execution rows or
execution-history controls. Opening a project shows its work in Graph, Lanes and Proof,
as required by [D-060](../DECISION_REGISTER.md).

- **Graph:** requests, their issues and pull requests, dependencies, and journey evidence in review.
- **Lanes:** agents, progress, questions, and the work that needs attention.
- **Proof:** journey previews and approved replays, checks, captured frames, comparisons, and run results.

The [Studio README](../../apps/studio/README.md) describes the current surface. The
[approved design files](../design/studio/README.md) define its visual design. A journey's
replay history in Proof is evidence for that journey, not a sidebar list of execution runs.

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
