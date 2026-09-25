# Agents, skills, tools and plugins

## 1. Vision

GraphHelm treats agents as temporary work configurations, not permanent characters. The value lies in the combination of objective, capability, context, permissions, model, contract and evidence. Useful agents can be persisted in the project, but remain versioned and auditable.

## 2. Taxonomy

### 2.1 Capability

Atomic description of what can be done.

### 2.2 Tool

Executable mechanism that offers one or more capabilities.

### 2.3 Skill

Operational knowledge/instruction that guides how to use capabilities toward an objective.

### 2.4 Agent Definition

Persistent, reusable configuration.

### 2.5 Agent Runtime

Ephemeral instance bound to node, graph version and execution.

### 2.6 Agent Experience

Accumulated memories and metrics, subordinate to evidence and TTL.

### 2.7 Plugin

Installable package that adds capability, tool, skill, evaluator, policy, model adapter, retriever, visualizer or another extension point.

## 3. Agent lifecycle

```text
Need identified
→ Project Agent Registry search
→ match/reuse OR synthesize ephemeral
→ lint definition
→ bind model/context/tools
→ instantiate runtime
→ execute
→ evaluate
→ create memory candidates
→ optionally save/promote by explicit user action
→ active/suspended/deprecated/archived
```

## 4. Project Agent Registry

### 4.1 Scope

- subproject-local;
- project-shared;
- workspace-shared;
- imported read-only;
- community package.

Promotion between scopes is explicit.

### 4.2 Metadata

- purpose;
- versions;
- capabilities;
- contracts;
- default permissions;
- preferred model profiles;
- context strategy;
- memory policy;
- graph affinities;
- performance metrics;
- status;
- provenance;
- publisher/signature for external ones.

### 4.3 Matching

The Agent Matcher returns:

```yaml
agent_match:
  agent: project/payment-reviewer@4
  score: 0.91
  strengths:
    - objective_fit
    - strong_project_history
    - contract_compatible
  weaknesses:
    - model_route_currently_degraded
  required_overlays:
    - add_scope: refunds
  alternative: synthesize_new
```

### 4.4 No blind reuse

Reuse is prohibited when:

- contract is incompatible;
- permission is insufficient;
- contradicted/expired memory affects the objective;
- version is incompatible;
- status is suspended/deprecated without explicit pin;
- project scope does not allow it;
- required isolation is not supported.

## 5. Ephemeral agents

### 5.1 Generation

The Agent Synthesizer receives a subtask contract and capability catalog. It produces a complete definition, not just a system prompt.

### 5.2 Persistence

By default, an ephemeral definition stays in the Execution Manifest. It only enters the Project Agent Registry when:

- the user clicks `Save as agent`;
- the user saves the node as a template;
- an explicit API promotes it;
- a manifest import is confirmed.

Dreams can create and activate a new agent version only through the governed shadow validation workflow and in accordance with the project's policy. It never automatically turns an ad hoc node overlay into a stable definition just because the execution succeeded.

## 6. Execution overlays

An instance can change:

- objective;
- instructions;
- model route/profile;
- context policy;
- tools;
- permissions;
- completion contract;
- retry;
- isolation;
- memory writes.

These changes belong only to the node/execution. When it finishes, they do not change the Agent Definition.

## 7. Memory

### 7.1 Types

- `strategy_outcome`;
- `known_pitfall`;
- `project_pattern`;
- `evaluation_feedback`;
- `tool_limitation`;
- `context_hint`;
- `self_limitation`.

### 7.2 Status

- candidate;
- validated;
- contradicted;
- deprecated;
- expired.

### 7.3 Reading

Agent Experience enters the Context Capsule only when:

- scope matches;
- it has not expired;
- relevance is high;
- evidence is available;
- it does not silently conflict with the current claim;
- budget allows it.

### 7.4 Writing

The agent proposes a memory candidate. The Memory Validator and policy decide persistence.

## 8. Agent evaluation

Metrics per task class:

- completion success;
- evidence completeness;
- output schema compliance;
- reviewer findings;
- later regressions;
- false-positive/negative;
- token/cost;
- duration;
- context expansion;
- retries;
- user overrides;
- usefulness rating;
- calibration.

Scores must be segmented; a global average is misleading.

## 9. Status and maintenance

### Active

Available for matching.

### Suspended

Not selected automatically; can be pinned manually.

### Deprecated

Replaced, but reproducible.

### Archived

History/import only.

### Quarantined

Suspected security/integrity issue.

Dreams can change status, create versions, merge or archive agents after shadow validation, policy checks and with rollback available. Every change remains versioned and auditable.

## 10. Skills

### 10.1 Content

A skill can contain:

- purpose;
- applicability;
- method;
- checklists;
- examples;
- anti-patterns;
- required/recommended capabilities;
- context hints;
- completion requirements;
- evaluator recommendations;
- conformance tests.

### 10.2 Composition

The harness can load multiple skills. Conflicts are detected via declared constraints and semantic lint. A skill cannot change hard policy.

### 10.3 Context cost

Large skills are segmented. The agent receives only the relevant sections, with a ref to expand.

### 10.4 Quality

Skill score uses:

- task success uplift;
- error reduction;
- token overhead;
- generalization;
- conformance;
- freshness;
- reviewer agreement.

### 10.5 Externally maintained skills

A skill may be maintained outside this repository and loaded by the host. It is still a skill
under this section: advisory, unable to change hard policy, scored like any other. The first such
skill is TypeSafe's `typesafe-ai`, which teaches an agent to turn a prompt-and-parse step into a
typed judgment with a probability (route, rank, extract, verify, escalate). Its install commands
and the route it maps to are in `docs/reference/PROVIDER_AND_LICENSE_REFERENCES.md`; the model
family it targets is `docs/models/UNIVERSAL_MODEL_GATEWAY.md` §2.6.

### 10.6 Journey-Proven Development entry families

The built-in `graphhelm-jpd` extension exposes eight entry skills: journey contract, observation
compilation, plan council, defect bounty, skill synthesis, skill evaluation, retry provenance, and
journey verification. They are discoverable families, not a fixed sequence. A future activated
loader and Task Profiler must select only the smallest subset justified by the promise, risk, and
missing evidence.

Agent personas, schemas, policies, evaluators, observers, graphs, and fixtures remain separate
contribution kinds. They do not become extra skills merely to inflate a catalog. A contribution is
split when it has a distinct contract, effect, permission, lifecycle, or evidence responsibility.
The built-in package version 0.1.0 contains 53 such contributions while keeping only eight
user-facing selection points.

A generated skill begins as a task-local immutable Skill Capsule. Issue #210 validates the packaged
capsule schema and containing extension, not emitted capsule instances, and ships no install,
activation, or publication path. Instances remain advisory until a registered validator returns a
receipt. In the future registry lifecycle, repeated evidence may justify a promotion proposal, but
only the Graph Governor may publish the new Project Skill version.

## 11. Tools

### 11.1 Tool categories

- repository;
- filesystem;
- shell;
- tests/build;
- browser;
- web/search;
- database;
- cloud;
- design/media;
- communication;
- document;
- data analysis;
- deployment;
- security scanner;
- model runtime.

### 11.2 Tool Broker

Every call passes through:

1. schema validation;
2. identity check;
3. capability lease;
4. policy;
5. path/network/secret validation;
6. read cache;
7. sandbox routing;
8. execution;
9. redaction;
10. artifact persistence;
11. event emission.

The read cache (step 6) applies only to tools with a declared freshness class (§11.3): a hit
returns the already-persisted artifact reference and digest instead of executing. The key is
`tool id@version + canonical input hash + lease scope + source snapshot` — a declared subset of
the dependency-hash components (`SYSTEM_ARCHITECTURE.md` §7.2) — with no expiry inside a
snapshot: a snapshot-keyed hit is provably exact. `drifting` tools are not cached in v1. Every
decision is recorded as a `ReuseDecision` event, and an `EvidenceErasureCompleted` for the
underlying artifact is a mandatory invalidation input — a cache must never serve
cryptographically erased evidence.

### 11.3 Effects

A tool declares:

- read-only;
- reversible write;
- irreversible write;
- external side effect;
- production effect;
- secret use;
- network egress.

This influences gating and isolation.

For caching, a read-only tool additionally declares a freshness class:

- `immutable_by_input` — the output is a pure function of the input;
- `snapshot_closed` — exact within a source snapshot, invalid across snapshots;
- `drifting` — external state may change between identical calls.

A TTL is the wrong instrument for the first two (too short wastes re-execution, too long is
wrong across snapshots); the snapshot key carries the freshness, and `drifting` tools are
simply not cached until a calibrated refresh mechanism exists.

## 12. Plugins

### 12.1 Types

- `capability-provider`
- `tool`
- `skill-package`
- `agent-package`
- `model-adapter`
- `evaluator`
- `policy-pack`
- `retriever`
- `document-materializer`
- `sandbox-adapter`
- `trigger`
- `visualizer`
- `deployment-adapter`
- `dream-strategy`

### 12.2 Runtime models

- OCI container;
- WASI/WASM;
- local process with broker;
- remote HTTP/gRPC;
- MCP server;
- pure data package.

Plugins do not run in-process in the Runtime core by default.

### 12.3 Manifest

```yaml
apiVersion: p50.dev/v1
kind: Extension
metadata:
  id: community/playwright-tool
  version: 1.2.0
  publisher: did:key:...
spec:
  type: tool
  capabilities:
    - browser_navigation
    - screenshot_capture
    - dom_inspection
  permissions:
    network: required
    filesystem: optional
    secrets: none
  contracts:
    input: schema://BrowserCommand@1
    output: schema://BrowserArtifact@1
  runtime:
    kind: oci
    image: registry/...@sha256:...
    isolationMinimum: tier_2
  compatibility:
    framework: ">=0.1 <1.0"
    platforms:
      - linux_amd64
      - linux_arm64
  telemetry:
    external: false
```

### 12.4 Installation

Flow:

1. resolve package and signature;
2. show publisher/trust;
3. show permission diff;
4. check vulnerability/license;
5. download by hash;
6. run conformance sandbox;
7. enable in the chosen scope;
8. record event.

### 12.5 Update

An update resolves a new immutable version, validates its full inventory, compares permissions and
capabilities with the active version, and requires new approval for any increase. The future loader
must switch versions atomically, keep the previous good version active when replacement validation
fails, and reverse registrations when a version unloads. The local Extension host now supports
validated installation and active-version switching/rollback. That local pointer is not proof of
host loading or a complete dynamic registration loader; those require their own observed receipts.

### 12.6 One composition path

An installable bundle mounts ordinary `Extension` contributions. GraphHelm does not add a second
repository-plugin or skill-plugin wrapper with its own installation, cache, manifest, or version
rules. Package resolution owns source, immutable version, dependencies, and lock state; the
Extension manifest owns explicit composition and configuration.

Discovery does not activate a package. Activation remains explicit. Issue #210 delivered validation
and a data bundle. The current Extension host also supports validated local installation and
active-version switching; reviewed adoption configuration participates in backup and restore.
These file-state operations remain `installed_unverified` for actual host behavior. No trusted
host observer ships yet, and user-authored receipts retain `observer_missing`; see the
[adoption rehearsal](../acceptance/adoption-rehearsal.md).

A host-specific Skill or MCP file is a thin adapter. It may call only declared public CLI, MCP, or
HTTP contracts, and deleting it loses convenience only. It cannot import Runtime internals or
publish an operational graph mutation.

Never auto-expand permissions. If a new version requests additional access, confirmation is required.

## 13. Community Registry

The registry can be central or federated, but installation does not depend on a proprietary service. Public metadata:

- package/version/hash;
- source repository;
- license;
- publisher;
- signature;
- permissions;
- trust;
- compatibility;
- vulnerabilities;
- optional download count;
- conformance results;
- reproducible build status.

Runtime accepts custom registries and local packages.

## 14. Supply chain

- pin by digest;
- signatures;
- SBOM;
- provenance attestation;
- reproducible builds desirable;
- vulnerability scan;
- dependency policy;
- quarantine/revoke;
- no mutable `latest` in execution manifests.

## 15. Agent-to-agent communication

Agents do not talk over a global chat. Communication happens via:

- typed artifacts;
- node outputs;
- evidence refs;
- graph signals;
- human decisions;
- event triggers.

A coordinator agent can exist, but it also uses contracts and does not receive unrestricted authority.

## 16. Delegation

An agent can request a subtask by emitting the `delegation_requested` Graph Signal. The Graph Governor decides whether to create a node. An agent does not spawn a runtime arbitrarily.

## 17. Agent identity

Each Agent Runtime has:

- runtime ID;
- definition/version;
- node/execution;
- actor identity;
- model route;
- leases;
- sandbox;
- context capsule;
- timestamps.

The Tool Broker uses this identity for authorization and audit.

## 18. Secrets

Agents receive secret references, never a value in the prompt when avoidable. The tool/model runtime resolves it in the broker. Any output is scanned/redacted before being persisted.

## 19. Marketplace and future monetization

The ecosystem can allow free or paid packages, but:

- the format remains open;
- side-loading is permitted;
- the runtime does not require an official marketplace;
- a paid package cannot hide permissions;
- a license must be declared;
- the community edition remains able to run compatible extensions.

## 20. Acceptance criteria

- agent definition is not synonymous with a prompt;
- reuse searches the Project Agent Registry before synthesis;
- node overlay does not change a saved agent;
- memory has evidence/TTL/status;
- tool calls pass through the broker;
- an agent does not create an agent runtime directly;
- plugin permissions are visible;
- an update with permission expansion requires confirmation;
- packages are pinned by version/hash;
- communication uses artifacts, not global chat.
