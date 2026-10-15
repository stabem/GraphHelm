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
| Authority | `serve/routes.rs`, `journey_flow_draft`, `journey_flow_review`, `approve_journey_flow` | Review and final approval need separate policy statements. |
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
removal revokes the grant. `approve` is not a grant purpose. A coordinator may request a grant
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

One Runtime owns the project registry. It holds an exclusive process lease against a second
writer. For each issue, rotate or revoke operation, it takes a write lock, validates current
state, appends one complete versioned record with length and checksum, flushes and durable-syncs
the file, then publishes the new immutable in-memory snapshot before acknowledging. Startup
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
the current digest and refuses a previous writer. Current
`POST /v1/journey-flows/{id}/approve` is owner-authenticated. The standing rule that only the
owner approves journeys remains in force. This RFC's `reviewer` purpose means a separate review
operation, never final approval. Whether independent review should count as approval remains an
unresolved authority change requiring an accepted decision-register entry and a governing-rule
update. D-058 concerns credential handoff and its shared-user limit, not approval authority.

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

## Follow-up implementation card (after security review)

1. `apps/cli/src/commands/serve/mod.rs` — replace startup-only raw credential loading with the
   trusted registry interface and reject invalid initial state.
2. `apps/cli/src/commands/serve/routes.rs` — authenticate provisioning requests and enforce exact
   scope and purpose before journey handlers.
3. `apps/cli/src/commands/tasks.rs` — remove owner-token-based lane self-provisioning or route it
   through the authorized broker contract.
4. `apps/cli/tests/journey_flow_surfaces.rs` — observe the HTTP scope, author/reviewer and
   no-write refusal cases below using isolated temporary stores.
5. `apps/cli/tests/api_http.rs` — add an isolated HTTP observer for provisioning, rotation and
   revocation without Runtime restart.

The OS isolation observer is a separate host-level deliverable and must not be claimed by these
HTTP tests. Security review may narrow or revise these paths before implementation; no code is
authorized by this proposed card alone.

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
