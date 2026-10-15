# RFC-1309: Journey Credential Provisioning

## Status

Proposed security design for issue #1309. This RFC does not change Runtime behavior or grant
review authority. Implementation requires independent security review and a follow-up issue.

## Promise and scope

Provision a journey author or reviewer credential to one authenticated lane without giving that
lane the owner token or another lane's secret. Bind every credential to one Runtime project,
journey flow, lane identity and purpose (`author` or `reviewer`). Keep journey approval owner-only
until the authority conflict below is resolved by an accepted decision and the standing owner-only
journey instruction is updated.

This design separates three claims:

1. **No secret handoff:** a lane receives only its own scoped token, never the owner token or
   another lane's token.
2. **Credential scope:** the Runtime refuses use outside the recorded project, flow, lane and
   purpose, regardless of caller-supplied actor headers.
3. **OS isolation:** an unrelated process cannot read another lane's token or the owner's token.

The first two are application guarantees. The third requires a separate process or OS security
boundary and is not true on the current shared OS user.

## Current baseline (documentary RED)

At the baseline commit `cf2d88d2433172ac438d6d060364ba20f8a7e7d6`:

- `apps/cli/src/commands/tasks.rs` mints lane credentials from the owner token. This does not
  let a lane request its own credential without access to owner authority.
- `apps/cli/src/commands/serve/mod.rs` loads the raw
  `GRAPHHELM_AGENT_CREDENTIALS` registry during startup. Rotation or revocation cannot take
  effect without restarting the Runtime.
- `docs/DECISION_REGISTER.md` D-058 says owner and agent token files share an OS user. It
  explicitly does not prevent a process under that user from reading the owner's token.

These are documentary observations, not an executed failing test. Existing journey-flow surface
tests cover author/reviewer distinction, previous-writer refusal and digest binding; they do not
prove secret isolation or live provisioning.

## Options

### Subject-separated derivation

Derive each lane token from a master secret and canonical scope tuple, for example
`project / flow / lane / purpose / key-version`. Store only the master secret in a trusted broker.
The Runtime must still authenticate the requester before deriving a token and must preserve
revocation. Rotating the master invalidates every derived token unless the Runtime supports
versioned keys and a controlled overlap. A derivation scheme alone does not authenticate a lane,
prevent a lane from requesting another subject, or provide same-user OS isolation.

### Hashed credential registry

Mint a cryptographically random token and store only its keyed digest in a registry with the same
project, flow, lane, purpose, status and version fields. On use, hash the presented token and
compare it in constant time. This supports per-token revocation and audit without retaining
plaintext credentials in registry storage. The issuer must still authenticate and authorize the
request. A plain unkeyed hash is insufficient protection for low-entropy tokens; tokens must be
high entropy and digest comparison must not expose timing information.

### Recommendation

Use random per-scope credentials with a hashed registry, minted by a trusted provisioning service.
It gives independent revocation and limits the blast radius of one leaked token. The broker returns
the plaintext once over an authenticated protected channel to the requesting lane. Neither registry
hashing nor transport protection changes the shared-user filesystem boundary.

## Documentary RED: unresolved protocol decisions

| Decision | Current source | Missing guarantee |
|---|---|---|
| Lane bootstrap | `apps/cli/src/commands/tasks.rs`, `credential` reads the owner token | No owner-free lane request protocol. |
| Grant and scope | `serve/mod.rs`, `load_agent_credentials` parses `credential=actor|project|execution` | No authenticated grant or flow/purpose binding. |
| Revocation | `serve/mod.rs`, `ServeState.agent_credentials` loads once | No restart-free update boundary. |
| Authority | `serve/routes.rs::journey_flow_review` calls `journey_flow::review_approve`, which calls `approve_as`; `journey_flow_surfaces.rs::a_reviewer_agent_approves_only_with_its_own_credential_and_never_its_own_draft` expects persisted status `approved` | Current behavior lets an agent reviewer perform and persist final approval, while `docs/guides/journeys.md` says approval belongs to the owner. The RFC's earlier claim that review is never final approval is false. |
| OS isolation | D-058 in `docs/DECISION_REGISTER.md` | No cross-lane unreadability guarantee. |

This is documentary RED, not an executed failing test. The protocol below is proposed behavior.

## Proposed trust and request flow

1. The coordinator creates an explicit grant for a known execution and lane, naming the exact
   project, flow and purpose. A lane cannot choose or widen these fields.
2. A trusted Runtime-side provisioning service authenticates the requesting lane through the
   existing execution identity and its authenticated session. It verifies the execution belongs
   to the named project and that the grant names that exact lane and scope. Caller-provided lane
   strings alone are not identity proof.
3. Only the trusted provisioning service may mint. The owner may create or revoke grants through
   the owner-authenticated control path. A coordinator may request provisioning only within an
   owner-approved grant; it cannot approve a journey or create broader grants.
4. The service creates a high-entropy random token, records its keyed digest and immutable scope,
   then returns the token once to the authenticated lane. It never returns the owner token and
   never returns another lane's credential. Secret values are excluded from events, logs,
   errors, prompts and task records.
5. Every use authenticates the token, then checks active status and exact project, flow, lane and
   purpose before the handler can write. Missing, malformed, expired, revoked, mismatched or
   ambiguous credentials fail closed without writes. Actor headers cannot override registry
   identity.

The provisioning service and registry must be part of the trusted Runtime boundary. A request
authenticated only by the shared agent session token is not sufficient to mint a scoped token:
that token identifies a capability class, not a specific lane. If the Runtime cannot establish
the lane from trusted execution state, provisioning refuses and reports a stable authorization
diagnostic without disclosing whether another lane or flow exists.

### Concrete bootstrap and grant

The owner-side coordinator uses today's owner-authenticated `tasks credential` command to mint
an execution-and-lane task bearer and delivers only that bearer to its assigned lane. The lane
never reads the owner token. The Runtime verifies the bearer against the execution path and lane
subject using the existing task-authentication rule, then checks the lane's assignment in trusted
execution state. The shared agent session token, a caller-supplied actor header, or a lane field
in the request cannot substitute. A stolen task bearer remains a bearer credential; the shared
OS-user limitation in D-058 remains.

Only an owner-authenticated control route creates or revokes a grant. Its record has `grantId`,
canonical `projectId`, `executionId`, `laneId`, `flowId`, `purpose` (`author` or `reviewer`),
`expiresAt`, `generation`, `state` and `issuer=owner`. The issuer verifies the execution belongs
to the Runtime project, the lane is assigned to it and the flow exists. Identifiers pass their
checked parsers and are compared byte for byte after canonicalization at creation. Assignment
removal revokes the grant under the shared transaction rule below. `approve` is not a grant
purpose. A coordinator may request a grant
from the owner but cannot issue one by itself.

The lane calls the broker with its task bearer and
`{grantId, projectId, executionId, flowId, purpose}`; there is no authoritative `laneId` request
field. The broker checks the authenticated lane and every field against one active, unexpired
grant. Wrong scope, missing assignment or ambiguous grants return one stable authorization
diagnostic before any write, without revealing whether another scope exists. On success, the
broker draws 32 random bytes from the OS CSPRNG, stores their keyed digest and returns
`{credentialId, token, expiresAt, generation}` once to that lane on the authenticated response.
There is no token recovery endpoint. If delivery is uncertain, the owner revokes that
`credentialId` and issues a new one. Plaintext tokens and digests are excluded from events,
logs, errors, prompts, crash output and task records; audit carries identifiers and outcome only.

### Registry and commit point

The Runtime broker holds a separate versioned HMAC key outside the registry. It computes a
domain-separated HMAC over all 32 token bytes and compares presented digests in constant time.
Each durable credential record stores `credentialId`, `grantId`, immutable canonical scope,
`generation`, `expiresAt`, `status` and digest, never plaintext. Missing keys, unknown versions,
collisions or corrupt records refuse all journey use and provisioning. Key rotation explicitly
reissues tokens; there is no fallback key. Registry and key backups require protection together.

One Runtime owns the project registry and trusted assignment state. It holds an exclusive process
lease against a second writer. Assignment changes, grant changes and credential issue, rotation
or revocation share one transaction lock and one durable journal. Provisioning holds that lock
from assignment and grant validation through credential commit. Assignment removal holds the
same lock and commits the removal and revocation of its grants and credentials as one journal
transaction; it cannot acknowledge removal while an earlier issue is in flight. The transaction
is one complete versioned record with length and checksum, flushed and durable-synced before
publishing the new immutable in-memory snapshot or acknowledging either operation. If removal
commits first, later provisioning refuses with the same stable authorization diagnostic and no
credential record. If issue commits first, removal revokes that credential before acknowledging.
Neither path may read assignment state from a separate unsynchronized cache or commit it to a
separate store outside this atomic boundary. Startup
replay refuses a corrupt or truncated record rather than guessing an earlier state. A crash
before sync is unacknowledged; a crash after sync is recovered on replay. A lost response after
commit requires revocation by `credentialId` before retry. A journey request holds the registry
read lock from authentication through its state mutation. Thus revocation cannot be acknowledged
while an earlier authorized write remains in flight; every request begun after acknowledgement
refuses the old token without a restart. Storage or lock failure refuses before writing.

The route checks active status, expiration, grant generation and exact project, flow and purpose
before invoking a handler. It takes lane and execution from the registry, never actor headers.
An author token may draft only its flow. A reviewer token may review only that flow's current
recorded digest and only if its lane has never written that flow. Neither can call owner approval.
An execution task bearer authenticates provisioning and task routes, but is rejected on journey
draft and review routes. Invalid scope, unknown token, ambiguity and concurrent update
uncertainty refuse without changing flow files or the Event Store.

## Lifecycle

- **Rotation:** the owner revokes the old generation and authorizes a replacement under the same
  grant. It has a new credential id, generation and digest. The first implementation permits no
  overlap; a coordinator cannot rotate by claiming authority.
- **Revocation:** owner can revoke a token or its grant. A revoked token is refused on the next
  request. Deleting a plaintext file is not considered revocation while the registry still accepts
  it.
- **Restart-free loading:** synced operations are published before acknowledgement. The read/write
  lock rule above prevents stale authorization after revocation.
- **Failure:** storage errors, authentication uncertainty, duplicate active grants, digest
  collisions, malformed registry state or unavailable update notifications cause refusal. No
  fallback to the owner token, shared agent token or an older credential is permitted.

## Journey author, reviewer and owner approval

Author and reviewer credentials are distinct credentials and distinct purposes. The Runtime
records the credential identity and scope with each draft and review. A reviewer must differ from
the author, and the review must bind to the exact recorded draft digest. A former author token,
execution token, owner token or token scoped to another flow cannot satisfy the reviewer check.

Current `POST /v1/journey-flows/{id}/draft` and `/review` accept a registered scoped agent
credential; it is not yet flow- or purpose-bound. The review handler records a blind review of
the current digest and refuses a previous writer. The current route in
`apps/cli/src/commands/serve/routes.rs` calls
`apps/cli/src/commands/journey_flow.rs::review_approve`, which constructs a reviewer approver and
calls `approve_as`. That path writes the approval record and approved status. The existing
`apps/cli/tests/journey_flow_surfaces.rs::a_reviewer_agent_approves_only_with_its_own_credential_and_never_its_own_draft` test verifies that the
result validates as approved. Therefore, the earlier sentence saying review is never final
approval described a proposal as current behavior and was wrong.

This conflicts with the standing owner-only journey rule and the policy intent of D-058. The
reviewer path is current implementation behavior, not an accepted authority decision. The
recommended target is owner-only final approval, with a distinct reviewer allowed to record a
review that does not approve the flow. Keeping agent final approval is the alternative, but would
need an explicit accepted decision and a change to the standing owner-only rule. A third option is
to defer reviewer credentials and reject reviewer approval until the authority decision is made;
that preserves the owner-only target but delays the proposed independent-review workflow. No
alternative is accepted by this RFC. Until an independent security reviewer assesses this exact
conflict and the owner accepts any required decision-register and governing-rule change, preserve
the observed behavior and report the policy mismatch as unresolved. Do not describe the existing
route as owner-only or silently treat its persisted approval as a mere review.

## Security review disposition

No independent security reviewer has yet recorded a disposition on this design head. The table
below makes the six required questions and their evidence explicit for that review; `PENDING`
means the question is not security-approved and cannot authorize implementation.

| Question | Evidence and current proposal | Independent disposition |
|---|---|---|
| Bootstrap identity: can execution identity be authenticated without a caller-supplied lane name? | The concrete bootstrap requires the owner-issued execution-and-lane task bearer and checks trusted assignment state; caller fields and the shared session token are insufficient. | PENDING; reviewer must verify the existing task-authentication consumer and assignment source can establish this identity. |
| Scope/purpose refusal: do wrong scope or purpose fail before writes? | The proposal checks project, execution, flow, lane and purpose before handler entry and specifies a stable refusal with unchanged state. | PENDING; route ordering and every write boundary need security review. |
| Revocation ordering: can a race accept a revoked token after acknowledgement? | The proposal serializes assignment, grant and credential changes and journey writes under one transaction boundary, with durable sync before acknowledgement. | PENDING; reviewer must validate the lock/journal boundary against actual storage and crash recovery. |
| Secret exposure: can token material reach logs, errors, events or task records? | The proposal returns plaintext once, stores a keyed digest, and excludes token and digest from recorded output. | PENDING; all output and failure paths require review. |
| Approval authority: does reviewer approval conflict with policy? | `review_approve -> approve_as` persists an agent approval; `a_distinct_reviewer_can_approve` expects approved status. The standing target is owner-only final approval. | PENDING; reviewer must explicitly disposition the conflict and compatibility path. |
| Host isolation: is there a real observer for cross-lane unreadability? | D-058 places owner and agent files under one OS user; this proposal does not establish unreadability. | BLOCKED; no real-host observer or separate enforced OS/process identity is delivered by this documentation slice. |

Only the independent security reviewer can fill the disposition column on the reviewed head.
Routine document approval is not security sign-off. Any unresolved blocker remains visible and
prevents the credential implementation from starting.

## OS and process isolation boundary

D-058 remains accurate: the current owner token and agent token belong to the same OS user. A
process running as that user may read files available to that user. File permissions, hashed
registry entries, separate bearer tokens, and redacted logs do not provide literal cross-lane
unreadability under this arrangement.

To claim OS isolation, place the provisioning broker and each lane in distinct OS identities or
equivalent enforced process sandboxes. Keep the owner token readable only by the broker/owner
identity; deliver each lane token through a scoped channel or file descriptor that other lane
identities cannot access. Verify effective permissions and attempt cross-lane reads on the actual
supported host. Until that real observer passes, report OS isolation as unimplemented and
unobserved. The narrower no-secret-handoff guarantee may be implemented earlier, but must not be
described as OS isolation.

## First implementation card (conditional on security review and accepted authority)

Do not start this card until an independent security reviewer has dispositioned all six questions
above, every blocker affecting the application protocol is resolved, and any owner-only approval
authority decision is accepted in the decision register with the required governing-rule update.
The host-isolation blocker may remain only if the implementation is explicitly limited to
no-secret-handoff and claims no OS isolation. This is a bounded first slice, not authorization
from this RFC:

- **Exact files:** `apps/cli/src/commands/serve/mod.rs`,
  `apps/cli/src/commands/serve/routes.rs`, `apps/cli/src/commands/tasks.rs`,
  `apps/cli/src/commands/journey_flow.rs`,
  `apps/cli/tests/journey_flow_surfaces.rs`.
- **Promise:** replace startup-only `ServeState.agent_credentials` use for journey routes with a
  trusted, owner-granted credential registry; authenticate the execution-and-lane task bearer,
  enforce project/flow/lane/purpose and revocation before writes, and keep final approval behavior
  aligned with the authority decision.
- **Storage/auth consumers:** `ServeState` and `load_agent_credentials` in `serve/mod.rs`, the
  journey draft/review handlers in `serve/routes.rs`, and owner-token task credential issuance in
  `tasks.rs`. The implementation must identify and reuse or add the durable transaction store
  named by security review; it may not claim atomicity by adding an in-memory cache alone.
- **Compatibility:** existing `GRAPHHELM_AGENT_CREDENTIALS` consumers and the persisted approved
  status produced by reviewer credentials need an explicit migration/refusal policy. Do not
  reinterpret existing approvals or erase their author/reviewer history. If the accepted decision
  requires changing reviewer approval, first add a failing isolated observer on the parent and
  preserve the existing digest and previous-writer coverage.
- **Proof:** isolated HTTP tests in `journey_flow_surfaces.rs` cover wrong scope/purpose, spoofed
  lane, execution bearer refusal on journey routes, former-writer review, exact digest binding,
  revocation without restart and unchanged state on refusal. Assignment-removal races and durable
  replay need separate observers if the selected store path reaches them. Real-host OS isolation
  remains a separate deliverable.

The independent reviewer may reject or narrow this card. No code is authorized by this proposal.

## Required implementation observers

The follow-up tests must use isolated Runtime instances and temporary stores, never the live
Runtime. They must show:

- an execution-scoped credential is rejected on journey author/reviewer routes;
- a wrong project, flow, lane or purpose is rejected before any write;
- the earlier author is rejected as reviewer;
- a distinct authorized reviewer is accepted only for the exact recorded draft digest;
- provisioning, rotation and revocation take effect without restarting the Runtime;
- rejected requests preserve the draft, review and event state.
- spoofed lane identity and a task bearer on journey routes are rejected;
- post-revocation use is refused before writes, including on the same Runtime process.
- concurrent assignment removal and provisioning serialize in either order: removal first refuses
  issuance, while issuance first is revoked before removal acknowledgement, including after replay.

These HTTP tests establish application authentication and behavior only. A separate real-host
observer must show one lane cannot read another lane's token or the owner's token under the
selected process boundary. Until that observer exists and passes, OS unreadability remains
unproven.

## Security review questions

- Can the execution identity be authenticated without trusting a lane name supplied by the caller?
- Does every route fail closed before writing when scope or purpose is wrong?
- Can registry update races allow a revoked token after revocation is acknowledged?
- Are token values absent from all logs, errors, events and task records?
- Does the proposed reviewer authority conflict with D-058 or the owner-only journey rule?
- Does the host-level isolation claim have a real observer on each supported platform?
