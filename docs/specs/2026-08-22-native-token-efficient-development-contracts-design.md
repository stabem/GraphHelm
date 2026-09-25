# Native Token-Efficient Development Contracts

**Status:** Independently reviewed; ready for implementation planning
**Date:** 2026-08-22  
**Original tracking issue:** #216 in the private development archive.  
**Scope:** Design only; no runtime, schema baseline, extension activation, or persistence change

## 1. Purpose

GraphHelm needs one native, interconnected system for:

- resolving how code must be written for a particular task;
- retrieving the smallest sufficient code and project context;
- learning reusable project knowledge without uncontrolled capture or publication;
- returning a concise, decision-ready final answer to the owner; and
- proving that token savings did not remove required evidence or reduce result quality.

The system adapts useful principles from Caveman, codebase-memory-mcp, and ai-memory without
installing, copying, forking, or adopting those ecosystems. It uses GraphHelm's existing Extension,
Context Capsule, policy, evaluator, Governor, Event Store, Tool Broker, and public-surface contracts.

This design extends Journey-Proven Development (JPD). It does not replace it and does not introduce
a second plugin format or a fixed development pipeline.

## 2. Authority and existing decisions

This design is subordinate to the Decision Register and preserves these decisions:

- D-009: memory is limited, auditable, evidence-bound, confidence-bearing, valid, and expiring;
- D-017: the Context Compiler creates layered, node-specific capsules with justified expansion;
- D-027: context and policies inherit through Workspace, Project, and Subproject scopes;
- D-035/D-036: free-form durable content is externalized as encrypted Evidence, authorized erasure
  does not falsify journal history, and persisted topology contains only safe typed references; and
- D-039: host wrappers use the public MCP, CLI, or HTTP contracts and create no second operational
  path.

It also depends on the proposed D-041 in PR #215: JPD begins with the complete user journey and
selects the smallest adequate proof methods; no testing ritual is universal. D-041 is not yet on
`main`, so this specification treats PR #215 as an explicit dependency rather than silently citing
the proposed decision as current authority.

The current repository contribution rule requiring focused RED -> GREEN -> REFACTOR remains in
force for GraphHelm implementation work. This product design does not silently amend that repository
rule. It defines the runtime's journey-led selection of proof methods.

## 3. Rejected adoption and retained lessons

### 3.1 Caveman

Retained:

- lead with the result;
- remove filler and repeated context;
- make the next owner action explicit;
- use short, predictable sections; and
- emit no options without a decision and exactly two options with one recommendation when a
  decision is required.

Rejected or corrected:

- compressed prose is not used for machine contracts, agent-to-agent artifacts, code, commits,
  pull requests, or normative documentation;
- compression must not omit destructive, security, financial, production, privacy, or irreversible
  consequences; and
- an unsafe or misleading short answer is refused rather than emitted.

### 3.2 codebase-memory-mcp

Retained:

- graph-first structural discovery;
- exact symbol, call-path, architecture, and impact queries;
- explicit index generation and path-coverage checks;
- bounded Scout, Verify, and Auditor evidence tiers;
- source fallback for partial, skipped, excluded, or stale coverage; and
- full-session token measurement rather than answer-only measurement.

Rejected or corrected:

- a graph result is candidate evidence, never source authority;
- zero resolved edges do not prove absence;
- a live repository identity must not be confused with the indexed snapshot identity;
- stale coordinates must never slice current source using old ranges;
- structural extraction gaps must be visible rather than indistinguishable from genuine zeros; and
- indexing cost, cache writes, memory use, zero-result queries, pagination, and fallback reads count
  toward the retrieval cost.

The relevant upstream failure classes include silently missing TypeScript call edges
([#1682](https://github.com/DeusData/codebase-memory-mcp/issues/1682)), stale source coordinates
([#1750](https://github.com/DeusData/codebase-memory-mcp/issues/1750)), and a live Git head presented
beside a stale graph generation ([#1559](https://github.com/DeusData/codebase-memory-mcp/issues/1559)).

### 3.3 ai-memory

Retained:

- project-scoped handoff;
- durable decisions, procedures, gotchas, and evidence-linked learning;
- deterministic retrieval without requiring an LLM;
- TTL, supersession, contradiction, and provenance;
- background consolidation separated from request latency; and
- typed proposals before durable writes.

Rejected or corrected:

- capture is opt-in per project and uses an allowlist of typed event fields;
- raw chat, prompts, broad tool output, secrets, and unrelated-project material are never default
  memory inputs;
- canonical authority participates in retrieval and outranks lexical similarity;
- later corrections are retained beside earlier claims and cannot be silently pruned;
- one memory provider cannot ingest its own rendered output or another provider's memory files as
  new knowledge without explicit, provenance-aware import; and
- an LLM may propose consolidation but cannot publish memory or decide authority.

The relevant upstream failure classes include noisy session pages outranking canonical decisions
([#262](https://github.com/akitaonrails/ai-memory/issues/262)), later corrections being pruned from
long sessions ([#126](https://github.com/akitaonrails/ai-memory/issues/126)), one memory system
recapturing another's files ([#194](https://github.com/akitaonrails/ai-memory/issues/194)), and
machine-global prompt capture without project opt-in
([#446](https://github.com/akitaonrails/ai-memory/issues/446)).

## 4. Architecture

The system adds three discoverable entry skills and several atomic Extension contributions. Skills
guide orchestration; deterministic contracts enforce behavior.

```text
User intent
  -> Journey Contract
  -> Resolved Code Contract
  -> Retrieval Plan
  -> Context Capsule
  -> selected implementation harness
  -> Journey Verification Result
  -> [when Project memory capture is enabled: Memory Candidate
      -> optional governed memory proposal -> versioned Memory Record]
  -> Owner Final Response
```

This is a dependency flow, not a mandatory sequence. The Task Profiler selects the smallest subset
needed by the task, risk, installed capabilities, and evidence gaps.

### 4.1 New entry skills

#### `code-contract`

Reads applicable rule sources and requests deterministic resolution into a
`ResolvedCodeContract`. It does not create rules, waive obligations, write code, or decide
compliance.

#### `context-retrieval`

Requests a bounded `RetrievalPlan`, uses installed retrieval capabilities, and asks the Context
Compiler to materialize a `ContextCapsule`. It does not treat provider results as authority, write
memory, or mutate code.

#### `memory-curator`

Normalizes a `MemoryCandidate`, requests validation, finds duplicate or contradictory knowledge,
and may emit a governed publication proposal. It cannot publish, supersede, expire, merge, or delete
memory.

### 4.2 Policy and evaluator contributions

The first native bundle defines contracts for:

- code-rule resolution;
- retrieval admission and expansion;
- structural absence proof;
- context utilization;
- memory candidate validation;
- memory authority and supersession;
- owner final-response formatting; and
- token-efficiency evaluation.

These remain separate contributions where their contracts, effects, permissions, lifecycle, or
evidence differ. They are not extra user-facing skills merely to inflate the catalog.

### 4.3 Runtime authority and Extension contributions

The deterministic Runtime owns `CodeContractResolver`, `ContextCompiler`, `MemoryAdmissionPolicy`,
`MemoryTransitionPolicy`, and `OwnerOutputValidator`. These services validate schemas, resolve
authority, enforce scope and freshness, preserve required evidence, decide typed refusals, validate
the final rendered bytes, and select a safe built-in rendering fallback. They are not provider ports
and cannot be replaced by an Extension.

External capability contracts remain brand-neutral and map only to existing Extension types:

| Capability contract | Existing Extension type | Permitted contribution | Forbidden authority |
|---|---|---|---|
| `StructuralCodeIndex` | `retriever`, optionally exposed through `tool` or `capability-provider` | Symbol, edge, architecture, impact, generation, and coverage evidence | Declaring source truth, completeness, or permission |
| `DurableMemoryStore` | `capability-provider` mediated by a Tool Broker tool | Scoped reads, contradiction lookup, status reads, and execution of a Governor-authorized storage command | Staging raw candidates, admitting content, or publishing a transition |
| `SourceReader` | `tool` or `capability-provider` | Exact snapshot-bound file, range, symbol, and literal reads | Bypassing leases, scope, snapshot checks, or the Tool Broker |
| retrieval-specific compressor | `capability-provider` | A bounded representation with provenance and declared losses | Removing required evidence or deciding sufficiency |
| owner presentation stylist | `capability-provider` | Selecting allowed layout/style tokens and immutable slot ordering in a closed `OwnerPresentationPlan` | Producing prose, changing slot values, inferring status, suppressing refusal, or certifying final bytes |

The Runtime constructs a closed `OwnerPresentation` AST from the typed result. Required slots carry
immutable typed values; no Extension may paraphrase or replace them. An optional stylist may return
only a closed plan containing allowed layout tokens, style tokens, and slot IDs. The Runtime
validates that every required slot appears exactly once, applies the plan, and exclusively
serializes final bytes. A missing, failed, or non-conforming stylist causes the Runtime to use its
deterministic safe built-in plan. Studio and host skills call only public Runtime API, CLI, or MCP
contracts.
The three entry skills are `skill-package` contributions, deterministic policy contributions are
`policy-pack`, and benchmark or journey evaluators are `evaluator`; this design adds no Extension
kind or manifest format.

### 4.4 New artifact envelope and existing contract bindings

Every artifact introduced by this design uses a checked-in JSON Schema and this closed common
envelope:

- `apiVersion`, initially `p50.dev/development/v1alpha1`;
- a closed `kind`;
- `metadata.id`, `metadata.artifactVersion`, and D-027 `workspaceId`/`projectId` scope, with optional
  `subprojectId` and `executionId` where the owning schema permits them;
- producer Extension identity and version, or the closed Runtime producer identifier;
- repository, graph, context, evidence, and clock snapshot/watermark bindings required by the kind;
- `spec`, validated before typed deserialization; and
- a canonical digest over every semantic envelope and `spec` field.

The initial new kinds are `CodeRule`, `ResolvedCodeContract`, `RetrievalPlan`, `MemoryCandidate`,
`MemoryRecord`, `AdvisoryDecisionResult`, `OwnerTaskResult`, `OwnerPresentationPlan`, and
`OwnerPresentation`. Each kind has its own schema version and compatibility table. Unknown major
versions fail closed;
schema-permitted unknown fields in a compatible minor version are preserved but never interpreted as
authority. Canonical serialization, digest input, stable ID grammar, maximum sizes, and migration
rules are normative schema concerns, not provider behavior.

Existing closed artifacts are not wrapped or mutated in place. `JourneyContract` and
`JourneyVerificationResult` use the versioned schemas delivered by the accepted JPD dependency;
`ContextCapsule` uses the existing checked-in Context Capsule schema. A new-envelope artifact
references one of them through a closed `ArtifactBinding` containing artifact ID, schema `$id`,
document/schema version, canonical digest, scope, producer, and required snapshot bindings. An
incompatible successor requires a new schema version plus an explicit compatibility/migration rule;
accepting PR #215 never silently rewrites its closed wire shapes.

## 5. Layered code rules

### 5.1 Rule sources

Rules resolve across these scopes:

```text
machine-global Owner configuration
  -> Workspace -> Project -> Subproject -> selector conjunction -> Task
```

Owner configuration is local control-plane configuration outside `RepositoryScope`; it does not add
an `ownerId` scope or alter D-027. At compilation time it contributes versioned owner defaults and
hard limits to the target Workspace/Project contract. Durable task artifacts remain scoped by the
existing `workspaceId` and `projectId` contract.

A selector is a conjunction over the closed dimensions language, module, path, symbol kind, change
class, risk class, and artifact kind. Selector A dominates selector B only when every predicate in B
is present with the same value in A and A adds at least one predicate. Selectors with different or
conflicting dimensions are incomparable; language never silently outranks module, for example.

### 5.2 `CodeRule`

Every rule contains at least:

- stable rule ID and version;
- source scope and selector;
- enforcement class;
- normative requirement;
- rationale and bounded edge cases;
- required evidence kinds;
- compatibility or conflict keys;
- valid-from and optional valid-until;
- provenance and semantic digest; and
- secret/PII scan status.

Enforcement classes are closed:

- `structural`: impossible to waive, because compliance is required for a valid artifact or safe
  state;
- `quality`: may be waived only by a complete owner override preserving actor, reason,
  acknowledged risks, affected contract and Graph Versions, and accurate result status; and
- `preference`: resolved by specificity and an explicit owner task decision.

### 5.3 `OwnerCodePolicy` and scoped policies

`OwnerCodePolicy` contains machine-global defaults and hard limits. Workspace, Project, Subproject,
and selector-scoped policies reference or add versioned rules. Every conflict key declares one
closed compatibility operator: `minimum`, `maximum`, `set_subset`, `set_superset`,
`boolean_required`, or `exact`. A descendant strengthens a structural or quality requirement only
when the operator proves that its accepted-value set is a subset of the ancestor's accepted-value
set. An incomparable or unregistered operator cannot establish strengthening and fails closed.

Rules are atomic referenceable documents when they can stand independently. Narrative architecture
and runbooks remain narrative documents; they are not mechanically split only to feed retrieval.

### 5.4 Deterministic resolution

The resolver:

1. validates every source before interpretation;
2. filters by scope, selector, snapshot, validity, and permission;
3. accumulates all matching structural requirements;
4. applies complete recorded owner waivers only to waivable quality requirements;
5. compares same-key requirements with their registered compatibility operator and refuses any
   incomparable conflicting structural or unwaived quality pair;
6. resolves preferences by explicit owner task decision, then D-027 scope depth, then selector
   dominance, then declared stable priority;
7. refuses a remaining preference conflict instead of using load order, lexical order, or an
   arbitrary selector-dimension order; stable rule ID and version order only serialize results and
   never decide semantics;
8. records every included, shadowed, waived, expired, denied, and conflicting rule; and
9. emits one immutable `ResolvedCodeContract` with the common envelope and canonical digest.

The resolved contract contains only rules applicable to the task. It also includes required proof,
source references, exclusions, conflicts, token estimate, expiry, and the exact code and graph
snapshot bindings.

## 6. Retrieval and Context Capsules

### 6.1 `RetrievalPlan`

`RetrievalPlan` uses the common versioned envelope. Its `metadata.id`, schema/artifact version,
producer, complete snapshot bindings, and canonical digest identify the entire ordered plan. Every
step also has a stable step ID and a digest covering its query, limits, stop condition, and fallback;
a retry references the prior plan and records a typed delta rather than mutating it.

The planner uses this default ladder:

1. explicit IDs, references, files, and symbols;
2. contract-required rules and evidence;
3. structural code graph;
4. validated durable knowledge;
5. exact source snippets;
6. bounded literal or full-text search; and
7. an auditable context-expansion request.

The order is a cost-aware default, not permission to skip required evidence. The planner may move an
exact source read earlier when the graph is missing, stale, partial, or inappropriate for a literal
or non-code question.

Each retrieval step declares:

- question or obligation answered;
- provider capability and version;
- scope and snapshot;
- maximum results, pages, bytes, and tokens;
- minimum evidence tier;
- success, zero-result, partial, and stale handling;
- stop condition; and
- fallback recipe.

The checked-in schema closes the step kinds, outcomes, evidence tiers, units, and fallback kinds.
Two conforming implementations given the same resolved contract, capability receipts, snapshots,
clock, and task objective must emit byte-identical canonical plans or a typed refusal.

### 6.2 Evidence tiers

- `scout`: narrow positive discovery; provisional; no absence or exhaustive claims;
- `verify`: task-directed graph results, exact material source checks, relevant pagination, and
  path coverage; default for implementation; and
- `auditor`: bounded-scope current generation, complete relevant pagination, both relationship
  directions when material, broader source fallback, and every limitation disclosed.

The selected tier is bound to the task risk and claim type. A lower tier cannot be relabeled as a
higher one by an agent summary.

### 6.3 Negative structural claims

A structural absence claim requires all of:

- an exact indexed generation bound to repository, commit, tree/snapshot digest, configuration, and
  indexer version;
- the relevant graph query and complete pagination;
- coverage evidence for every cited path and the bounded negative scope;
- an unresolved-edge or equivalent extraction-gap signal;
- source fallback for every partial, skipped, excluded, stale, unknown, or unresolved range; and
- an exact or bounded textual/source confirmation suitable for the language and claim.

If the provider cannot distinguish a genuine zero from unresolved extraction, the result is
`NEGATIVE_CLAIM_UNVERIFIED`. The agent may report the positive graph findings but cannot claim
absence.

### 6.4 Freshness and source coordinates

Every graph result names the indexed snapshot separately from the live workspace snapshot. A live
HEAD value is never evidence of index freshness. If source metadata differs, stored coordinates
cannot slice the live file. The provider must reindex, serve snapshot-owned bytes, or return
`INDEX_STALE`.

### 6.5 Context representation

Large sources do not enter a capsule in full by default. Available representations are:

- exact excerpt with source identity and line/symbol reference;
- structural or hierarchical summary;
- typed table or JSON;
- diff;
- graph neighborhood;
- artifact pointer; and
- lazy retrieval handle.

Exploratory nodes may receive a compact handle index. Pulling a handle is an auditable expansion
under the node's leases and budget.

### 6.6 Ranking and authority

The existing Context Compiler ranking remains authoritative:

```text
relevance
x scope_permission
x freshness
x source_authority
x evidence_strength
x decision_impact
x contract_fit
x observed_utilization
- redundancy
- token_cost
- contamination_risk
```

Explicit contract references cannot be demoted by retrieval ranking. Relevant contradictions enter
together with status and provenance. The compiler never silently merges them.

### 6.7 Stop and expansion

Retrieval stops when every mandatory question and evidence obligation has adequate evidence, the
selected coverage tier is satisfied, and the expected decision impact of another query is below the
policy threshold. Remaining budget is not a reason to continue searching.

If required evidence does not fit, the compiler emits `CONTEXT_BUDGET_INSUFFICIENT` and an auditable
expansion request. It never silently removes required evidence to meet a token target.

### 6.8 Delta context and caching

Retries receive the prior stable references plus only changed rules, source, evidence, decisions,
and dependency outputs. Capsule serialization is canonical: stable Project Kernel and stable item
IDs first, volatile task material and deltas last. Compiler cache and provider prompt-cache hits are
measured separately.

## 7. Durable memory and handoff

### 7.1 Opt-in capture

Durable-memory observation is disabled unless the Project explicitly activates it. Activation uses
an allowlist of typed event fields. An unknown event kind or field is not captured by default.

Default prohibited inputs include:

- full chat or model reasoning;
- raw user prompts;
- unbounded tool input or output;
- secrets, credentials, cookies, tokens, and raw sensitive captures;
- another memory provider's rendered store;
- copied documentation or source code;
- opinions about a user;
- unrelated Workspace or Project content; and
- temporary outcomes with no expected reuse value.

Redaction is defense in depth, not admission. Content that should not be stored is dropped before
persistence rather than stored as a redacted memory candidate.

Admission occurs before candidate construction, provider calls, staging, logging, event emission,
or serialization. A deterministic pre-admission filter accepts only registered observation kinds
and allowlisted fields, applies bounds and secret/content classification, and returns either a safe
typed observation or `MEMORY_CONTENT_PROHIBITED`. Prohibited bytes are discarded from the memory
path and never reach `DurableMemoryStore`.

After admission, the Runtime sends the permitted bounded content to the Governor for immediate
externalization as provisional encrypted Evidence under D-035/D-036. The provisional Evidence has
a closed ephemeral retention class, custody metadata, admission-policy receipt, scope, content
digest, and expiry. Only its safe reference and receipt may enter a candidate, log, handoff, event,
or provider call. This gives asynchronous validation and restart recovery an inspectable,
authorized content path without placing plaintext in the candidate or Event Journal.

Rejection or abandoned expiry requests authorized Evidence erasure through the existing D-035
path; journal receipts and digests remain. Publication never changes the provisional Evidence
record or its authenticated retention metadata. The Governor opens it into a zeroizing buffer,
seals a new Evidence record with a new ID and the policy-selected retention class, verifies equal
workspace/project/execution scope, content digest, Evidence schema version, media type, sensitivity,
cipher algorithm/version, and `byteLength`, and atomically binds the published
`MemoryRecord` to the new reference. Only Evidence ID, retention class, `createdAt`, nonce/key
material, ciphertext, and their derived authenticated fields may change. After that commit, the
Governor requests authorized erasure of the provisional item. A missing or erased required slot
blocks validation or reuse without rewriting history.

### 7.2 `MemoryCandidate`

Agents and deterministic observers may propose a common-envelope candidate containing only:

- closed claim or procedure kind and typed content-slot digest;
- provisional Evidence reference and admission receipt;
- scope;
- evidence references;
- confidence and uncertainty;
- valid-from and proposed valid-until;
- source execution, actor, and Context Capsule item IDs;
- producer and proposed evaluator identities;
- dependency identities, versions, and semantic hashes;
- canonical authority class;
- expected reuse value;
- contradiction and supersession hints; and
- candidate semantic digest.

A candidate is advisory. It is not memory merely because it conforms to a schema.

### 7.3 `MemoryRecord` and compatible states

`MemoryRecord` is the versioned durable common-envelope artifact produced from a candidate. It
contains the candidate binding, current Evidence reference, semantic status, publication state,
authority, validity interval, dependency bindings, relationships, latest transition receipt, and
predecessor record version/digest. Candidates are immutable inputs; each accepted transition emits
a successor `MemoryRecord` rather than mutating the candidate or an earlier record.

The schema permits only these cross-axis tuples:

| Semantic status | Allowed publication state | Meaning |
|---|---|---|
| `candidate` | `unpublished` | Admitted but not validated or proposed |
| `validated` | `unpublished`, `proposed`, `published`, `withdrawn` | Eligible according to the separate publication decision |
| `contradicted` | `withdrawn` | Historical only; never current retrieval |
| `deprecated` | `withdrawn` | Historical only; never current retrieval |
| `expired` | `withdrawn` | Historical only; never current retrieval |

`proposed` and `published` therefore structurally require `validated`. Any transition from
`validated` to `contradicted`, `deprecated`, or `expired` atomically sets publication state to
`withdrawn`. Withdrawal preserves the last published binding for audit but current retrieval cannot
serve it. No combination outside the table is schema-valid or replayable.

### 7.4 Validation and publication

The deterministic validator checks:

- evidence existence, custody, freshness, and scope;
- absence of prohibited content;
- authority and source type;
- producer/evaluator independence required by the claim kind;
- contradictions and later corrections;
- duplicate or provider-loop provenance;
- TTL adequacy;
- expected reuse value; and
- canonical digest bindings.

Claim-kind policy is closed and versioned. A canonical decision requires the accepted decision and
Graph Version reference. A code fact requires exact source snapshot and semantic dependency hashes,
plus structural evidence when the claim is structural. A procedure requires its Journey Contract
and successful verification evidence from the minimum distinct runs/environments declared by
policy. A gotcha requires a reproducible failure and discriminating successful correction. An
agent-authored statement cannot validate itself: when independent evaluation is required, producer
and evaluator identities and lineage must differ, and missing independence is a typed refusal.

The authority order is:

```text
structural policy
-> current canonical decision
-> current scoped rule
-> verified procedure or gotcha
-> validated semantic memory
-> episodic session evidence
-> raw observation
```

Lexical or semantic similarity cannot promote a lower-authority item above a canonical source on its
own. Historical and episodic items remain searchable when the task asks for history.

This design preserves the existing normative semantic status vocabulary. Semantic validity and
publication lifecycle are separate state axes:

```text
semantic status:
candidate -> validated | contradicted | deprecated | expired
validated -> contradicted | deprecated | expired
contradicted -> deprecated | expired

publication state:
unpublished -> proposed | withdrawn
proposed -> published | withdrawn
published -> withdrawn
```

Only the Graph Governor records operational transitions on either axis. Every transition records
actor, policy, deterministic clock/watermark, predecessor, evidence, reason code, and resulting
digest. A correction creates a new candidate, links `contradicts` and/or `supersedes`, and moves the
earlier record to the applicable existing `contradicted` or `deprecated` semantic status; it does
not invent a `superseded` status or rewrite historical evidence. Retrieval treats non-`validated`,
non-`published`, `withdrawn`, expired, or evidence-unavailable records as non-current by default
while preserving authorized audit access.

Freshness is evaluated at retrieval against the deterministic execution clock and every dependency
hash. A changed source, rule, decision, tool configuration, or Graph Version makes a dependent item
stale even before its TTL. The retrieval result emits `MEMORY_DEPENDENCY_STALE`; policy may propose
an existing `contradicted`, `deprecated`, or `expired` transition. D-035 retention, legal
hold, and authorized Evidence erasure use the existing evidence-erasure authority path; erasure is
not disguised as memory expiry or Event Journal deletion.

### 7.5 Dreams maintenance

Dreams may propose deduplication, conflict reports, TTL changes, consolidation, rule extraction,
retrieval-rank changes, or expiry. It runs outside request latency, records expected benefit and
risk, uses shadow validation, and cannot publish its own proposal or execute a transition.

### 7.6 Handoff

A handoff contains only current state, completed work, evidence, open decisions, blockers, and the
next bounded action. It is scoped to the receiving project and actor capability. It does not carry
the complete conversation or automatically become durable semantic memory.

## 8. Owner final-response policy

### 8.1 Scope

`OwnerOutputPolicy` is machine-global owner configuration loaded by the local control plane. It is
not a new persisted repository scope, a selectable skill, or repeated prompt text. The Runtime
binds its version and digest into the target Workspace/Project `OwnerTaskResult`. Internal agent
outputs remain typed artifacts.

The policy applies only to the final owner-facing projection. It does not rewrite documentation,
code, commit messages, pull-request bodies, schemas, diagnostics, or machine-readable output.

### 8.2 Typed input

Before rendering, the Runtime validates a common-envelope `OwnerTaskResult` containing:

- summary fact;
- actual result status;
- evidence strength and limitations;
- whether owner action is required;
- decision statement when required;
- zero options and no recommendation when no decision exists, or exactly two bounded options and
  exactly one recommendation when a real decision exists;
- recommendation rationale only when a real decision exists;
- consequences and rollback information;
- security, destructive, financial, production, privacy, or irreversibility flags; and
- redaction result.

An `AdvisoryDecisionResult` supplies the decision, two options, recommendation, rationale, and
consequences when a decision is required. A stylist does not infer success, create options, choose a
recommendation, or author prose. The Runtime rejects any conditional-field or cardinality mismatch
before constructing the presentation AST.

### 8.3 Rendering rules

Default order:

```text
Summary
Result
Your action
Options, only when a decision exists
Recommendation, only when options exist
```

Rules:

- lead with the result;
- use short sentences and omit tool narration and repeated context;
- state `Nothing now` when no owner action exists;
- when a decision exists, render exactly the two validated safe options and identify the one
  validated recommendation;
- if only one operational action is safe, the second option is a truthful defer, stop, rollback, or
  gather-more-evidence path with its consequence, never a fabricated unsafe alternative; and
- preserve commands, paths, identifiers, evidence limitations, and risk consequences exactly.

### 8.4 Safety expansion

Compression is relaxed when required for destructive, security, financial, production, privacy, or
irreversible decisions. The Runtime selects the safe expanded AST template; compression cannot
remove scope, target, cost, permanence, uncertainty, rollback, or required confirmation.

If the requested short form would overstate evidence or hide a required consequence, the Runtime
returns `OWNER_OUTPUT_UNSAFE_COMPRESSION` and uses the smallest safe built-in expanded form.

The Runtime maps `OwnerTaskResult` fields into a closed `OwnerPresentation` AST whose node kinds are
fixed: section, typed text slot, exact command/path/identifier, option, recommendation, consequence,
evidence limitation, and risk notice. Slot values are copied from the validated result without
Extension-authored prose. Optional style plans may reorder only policy-marked movable slots and
choose closed formatting tokens. The Runtime validates required-slot presence, uniqueness,
cardinality, order constraints, and exact values, then alone serializes the AST to final bytes.
Invalid plans are discarded, never repaired by an LLM.

## 9. Errors and typed refusals

The initial closed refusal vocabulary includes:

| Code | Meaning | Required response |
|---|---|---|
| `CODE_RULE_CONFLICT` | Equal-authority or incompatible applicable rules | Preserve both; request a governed resolution |
| `CODE_RULE_SOURCE_UNAVAILABLE` | Required rule source cannot be authenticated or read | Do not compile a partial contract as complete |
| `CODE_RULE_WAIVER_INVALID` | A waiver is incomplete, unauthorized, or structural | Refuse the waiver |
| `INDEX_STALE` | Indexed snapshot and requested source snapshot differ | Reindex, use snapshot-owned bytes, or fall back |
| `INDEX_COVERAGE_PARTIAL` | Relevant scope is partial, skipped, excluded, or unknown | Read the reported source ranges or narrow the claim |
| `NEGATIVE_CLAIM_UNVERIFIED` | Absence cannot be distinguished from extraction failure | Report unresolved, never absent |
| `CONTEXT_BUDGET_INSUFFICIENT` | Required evidence cannot fit the current capsule | Emit an expansion request |
| `MEMORY_CAPTURE_NOT_ENABLED` | Project did not opt in | Do not capture |
| `MEMORY_CONTENT_PROHIBITED` | Pre-admission found an unknown, non-allowlisted, secret, or prohibited field | Drop it before candidate construction or provider access |
| `MEMORY_SCOPE_MISMATCH` | Candidate or result crosses an unauthorized scope | Reject before persistence |
| `MEMORY_AUTHORITY_CONFLICT` | Candidate conflicts with equal or stronger current authority | Preserve conflict; require validation/resolution |
| `MEMORY_VALIDATOR_MISSING` | No registered deterministic validator exists | Keep candidate advisory |
| `MEMORY_VALIDATOR_NOT_INDEPENDENT` | The claim kind requires an evaluator independent from its producer | Refuse validation |
| `MEMORY_DEPENDENCY_STALE` | A bound source, decision, rule, tool, or graph digest changed | Exclude from current retrieval and propose revalidation |
| `MEMORY_PROVIDER_LOOP` | Memory output is being recaptured as new knowledge | Reject and retain provenance finding |
| `MEMORY_STATE_INVALID` | Semantic/publication tuple or transition is outside the closed matrix | Refuse the transition and leave the predecessor current |
| `MEMORY_EVIDENCE_RESEAL_FAILED` | Publication Evidence could not be re-sealed and rebound with identical content/scope | Publish neither record nor reference; retain provisional custody |
| `OWNER_OUTPUT_SCHEMA_INVALID` | Typed result, style plan, presentation AST, or serialized bytes violate the output contract | Reject the style plan and use the safe built-in plan/serializer |
| `OWNER_OUTPUT_UNSAFE_COMPRESSION` | Short rendering would hide material meaning | Render the smallest safe expanded response |

Unknown refusal codes fail closed at deterministic boundaries. Skills cannot translate a refusal
into success prose.

## 10. Token accounting and evaluation

### 10.1 Cost boundary

Token accounting includes the complete task-context acquisition and owner-output flow:

- orientation and project selection;
- rule resolution;
- graph and memory queries;
- zero-result calls;
- pagination;
- source fallback;
- expansion;
- retries and delta capsules;
- tool-result summaries;
- compiled model input;
- model output; and
- final formatting.

Index construction is reported separately as amortized and cold-start cost. CPU, memory, disk
writes, latency, and cache hit rate remain visible; tokens are not the sole resource metric.

### 10.2 Utilization

Every Context Capsule item has a stable ID. Structured node output cites the IDs actually relied on.
GraphHelm computes per-item and per-recipe utilization from those citations, not from model
self-estimates or private reasoning.

Utilization may demote low-value optional context with decay and a floor. It cannot overrule scope,
permission, an explicit rule reference, or contract-required evidence.

### 10.3 Paired benchmark

Each evaluation case runs against the same repository snapshot, task objective, permissions,
model route, and acceptance contract:

- `traditional`: ordinary file/search exploration without compiled graph or durable-memory context;
- `compiled`: code contract, retrieval plan, Context Capsule, and delta/caching behavior from this
  design.

Answers are evaluated against deterministic journey obligations and, for subjective dimensions, a
blind fixed rubric. A zero-result answer is a failed retrieval result, not an omitted benchmark row.

The committed benchmark manifest versions the corpus, required-evidence oracle, critical-answer
rubric, blind-quality rubric, baseline recipe, tool/query/page budgets, cache state, index mode and
generation policy, model route and settings, repetition count, case order, clock, seeds, and token
counter implementation. Traditional and compiled arms start from equivalent clean state and differ
only by the retrieval/compilation treatment under evaluation. Runs that violate the manifest are
invalid rather than comparable. Required-evidence recall is computed from stable oracle IDs, and
critical-answer downgrade is decided by closed per-case clauses before any run is observed.

Initial acceptance thresholds across the committed evaluation corpus are:

- median compiled input-context tokens are at most 40% of the traditional baseline;
- required-evidence recall is 100%;
- deterministic acceptance outcomes are identical or stronger;
- no critical-answer downgrade occurs;
- mean blind quality on a 0..1 rubric is not more than 0.02 below the traditional baseline;
- every negative structural claim satisfies the absence-proof contract; and
- median full-session tokens, including every item in the cost boundary, are no greater than 100%
  of the traditional baseline; and
- answer-only token ratio, cold-start cost, and amortized index cost are reported, with full-session
  ratio as the headline.

An execution does not pass because it is cheap. Any missing required evidence, false completion,
scope leak, secret capture, or critical quality regression fails the gate regardless of savings.

## 11. Verification strategy

### 11.1 Contract tests

- structural rules accumulate and cannot be waived;
- quality waivers require the complete owner record;
- scope depth and selector dominance resolve compatible preferences deterministically;
- incomparable selector conflicts and incompatible mandatory rules refuse independent of input
  order;
- each closed compatibility operator accepts strengthening and rejects weakening fixtures;
- expired or wrong-scope rules never enter the resolved contract; and
- the same inputs produce byte-identical canonical contracts;
- existing JPD and Context Capsule fixtures remain valid byte-for-byte and bind through
  `ArtifactBinding`; and
- incompatible successor artifact schemas require an explicit version and migration fixture.

### 11.2 Retrieval tests

- a positive graph result reaches exact source;
- a silent missing edge cannot become an absence claim;
- partial parse ranges force source fallback;
- stale coordinates never slice live bytes;
- live HEAD and indexed generation remain distinct;
- pagination and result limits are complete or explicitly truncated;
- large files enter as handles or bounded representations; and
- required evidence over budget produces an expansion request, not omission.

### 11.3 Memory tests

- canonical decisions outrank lexically stronger session pages;
- later corrections remain visible and supersede earlier claims only after validation;
- cross-project and cross-actor scope bleed fails before retrieval or persistence;
- raw prompt, secret, and broad tool-output capture fails before candidate construction, provider
  access, logging, event emission, or persistence;
- admitted free-form content is immediately provisional encrypted Evidence and survives an
  authorized restart/handoff without plaintext leakage;
- rejected or abandoned candidates request authorized provisional-Evidence erasure;
- publication re-seals into a different Evidence ID and retention-bound AAD, atomically binds the
  published record, and never mutates provisional Evidence metadata;
- recaptured memory-provider output is rejected;
- an agent cannot be the sole validator of its own claim when independence is required;
- a changed dependency hash makes a published item stale before TTL expiry;
- semantic status remains exactly `candidate|validated|contradicted|deprecated|expired`, independent
  from `unpublished|proposed|published|withdrawn` publication state;
- every allowed `MemoryRecord` state tuple validates, every other tuple fails, and leaving
  `validated` atomically produces `withdrawn`;
- transition-pair fixtures cover `unpublished -> proposed|withdrawn`,
  `proposed -> published|withdrawn`, and `published -> withdrawn`;
- expired memory is excluded by default but remains auditable;
- authorized Evidence erasure preserves journal truth and blocks reuse requiring the erased slot;
- contradictions are presented rather than silently merged; and
- Governor publication is the only operational memory mutation path.

### 11.4 Output tests

- a completed no-action result says no action and emits no options;
- a real decision emits exactly two options and one recommendation;
- an only-safe-action case renders a truthful stop/defer alternative;
- a failed or unresolved result cannot be phrased as success;
- destructive and security consequences survive compression;
- a malicious or broken style plan is rejected and the built-in safe plan is used;
- Extension output contains no owner-facing prose and cannot replace immutable AST slot values;
- a no-decision result contains neither recommendation nor recommendation rationale;
- internal JSON remains unchanged by owner formatting; and
- secrets never reach the rendered response.

### 11.5 Journey and sabotage cases

Every development harness consumes an existing versioned Journey Contract or invokes
`journey-contract` before code/context compilation. The end-to-end journey begins with a real
request and always validates the Journey Contract, Resolved Code Contract, Retrieval Plan, Context
Capsule, implementation evidence, Journey Verification Result, and Owner Final Response.

The journey has two explicit memory branches:

- capture disabled: no observation or candidate reaches a provider or persistent boundary, and the
  attempt records `MEMORY_CAPTURE_NOT_ENABLED`; and
- capture enabled: the journey validates pre-admission, optional candidate construction, independent
  evaluation when required, Governor proposal/publication, and retrieval-time freshness.

Required sabotage cases include:

- delete or misresolve an inbound call edge;
- shift a source symbol after indexing;
- claim the live Git head as indexed freshness;
- hide a relevant file behind partial or excluded coverage;
- inject a noisy episodic memory with stronger lexical overlap;
- place a later correction beyond the ordinary observation-selection cap;
- attempt cross-project memory retrieval;
- feed rendered memory back into capture;
- inject a secret into a rule, memory candidate, tool output, and final response;
- attempt to stage prohibited memory before admission;
- let a producer validate its own independence-required claim;
- erase required Evidence and then attempt memory reuse;
- return a style plan that omits, duplicates, or replaces a refusal or material-consequence slot;
- remove an option consequence during compression;
- omit a required Context Capsule citation; and
- optimize tokens by deleting a required evidence item; and
- shift work into extra queries, retries, or outputs while reducing only compiled context.

Every sabotage must make a named test fail before the behavior is accepted.

## 12. Integration with JPD

JPD remains journey-first. The new skills add code and context obligations without turning the eight
existing entry families into an eleven-stage pipeline.

Examples:

```text
Small deterministic change:
existing Journey Contract or journey-contract -> code-contract -> context-retrieval
-> implementation -> journey-verifier

Risky user-facing change:
journey-contract -> observation-compiler -> code-contract -> context-retrieval
-> plan-council -> implementation -> defect-bounty -> journey-verifier
-> memory-curator -> owner final response
```

`skill-synthesizer` may compose the new atomic capabilities into a task-local Skill Capsule when an
actual orchestration gap exists. The capsule remains task-scoped and advisory and cannot
self-promote, install, activate, publish, or mutate an operational graph. It may enter the JPD
`skill-evaluator` path as an evaluated candidate; only a governed promotion proposal and explicit
Governor publication can make a versioned Project Skill operational.

## 13. Rollout decomposition

Implementation must be split into separately reviewable issues after the design is accepted:

1. normative schemas, policies, refusal codes, and deterministic rule resolution;
2. the three data-only entry skills and native Extension bundle wiring;
3. structural-code-index provider contract, snapshot/coverage rules, and source fallback;
4. opt-in durable-memory candidate, durable record/state matrix, Evidence re-sealing, authority,
   validation, handoff, and Governor proposal path;
5. owner final-response policy and deterministic formatter;
6. Context Compiler utilization, token accounting, delta, caching, and paired benchmark; and
7. JPD dogfood graph, sabotage corpus, and generic deterministic certification integration.

PR #215 and its proposed D-041 are rollout dependencies and must be accepted or this design must be
reconciled through the documentation authority process before implementation. Issue #211 remains
the generic JPD certification dependency, #212 the extension lifecycle dependency, and #213 the
per-contribution MCP authority dependency. Those issues are reused rather than duplicated where
their acceptance scope already covers the required work.

## 14. Security and privacy invariants

- Repository content, graph results, memories, agent reports, and formatter input are untrusted.
- An external adapter cannot grant permission, publish state, or define source authority.
- Only deterministic Runtime code enforces schemas, scopes, rule resolution, freshness, digests,
  admission, redaction, evidence sufficiency, output safety, budgets, transitions, and publication.
- Context Compiler, memory admission/transition policy, and final-output validation are Runtime
  services, never replaceable Extension authorities.
- Secrets never appear in rule documents, Context Capsules, memory, evidence payloads, fixtures,
  logs, manifests, or final responses.
- Capture is project opt-in and allowlist-based.
- Evidence identity and authenticated retention metadata are immutable; changing retention requires
  a newly sealed Evidence record and an audited old-reference erasure path.
- A graph or memory provider result is bound to provider identity, version, snapshot, configuration,
  and digest.
- Required evidence is never removed to save tokens.
- Structural impossibility is never waivable.
- Owner waivers preserve the accurate result label and full audit record.
- The Event Store remains append-only; projections and summaries never rewrite history.

## 15. Out of scope

- adopting, forking, or distributing Caveman, codebase-memory-mcp, or ai-memory;
- runtime implementation in this design commit;
- a general extension installer, activation, hot reload, or remote registry;
- provider-specific model adapters;
- a general browser or operating-system automation engine;
- automatic publication of rules, skills, memories, graphs, or owner waivers;
- hosted marketplace, billing, or telemetry export; and
- changing the repository's current contribution process without a separate accepted decision.

## 16. Design acceptance

The design is accepted when:

- the owner approves all six design sections;
- no placeholder, contradiction, or ambiguous authority boundary remains;
- every contribution uses the existing Extension model;
- every new normative artifact has an explicit checked-in, versioned schema and envelope requirement,
  while existing closed artifacts have explicit versioned `ArtifactBinding` compatibility rules;
- skills remain guidance rather than authority;
- token savings are paired with reproducible evidence, full-session cost, and quality gates;
- upstream lessons are adapted without importing their ecosystems; and
- the implementation is decomposed into issue-first, independently verifiable work.
