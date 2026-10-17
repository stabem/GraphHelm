# RFC-1309: Journey Credential Provisioning

## Status

Proposed security design for issue #1309. This RFC does not change Runtime behavior or grant
review authority. Implementation requires independent security review and a follow-up issue.

**Delivered slice 1 (storage):** PR #1486, merged as `aa6d3d13`, implements the owner-only grant
create/revoke routes over the private journal in "Registry and commit point", with the three
conditions of the independent storage disposition on #1309 (gh-claude-14, 2026-10-16 11:49Z):
bounded frames and record count, refusal without the lifetime lock, and no isolation claim. Its HTTP
observer `owner_journey_grants_survive_a_restart_and_refuse_agents_and_a_torn_journal` covers
restart replay, idempotent receipt and conflict, agent and task-bearer refusal, and a torn tail.
It does **not** observe the deterministic fault points (failure before `sync_data`; durable commit
with lost acknowledgement). Those move to the credential-issue slice. No token is issued and no
journey route changed.

**Delivered slice 2 (credential issue):** PR #1490, merged as `7329d9b8`, issues one credential
once, as "Protected delivery channel" specifies, including both deterministic fault points.

**Delivered slice 3 (draft enforcement):** `POST /v1/journey-flows/{id}/draft` accepts a journey
credential (`Authorization: Bearer <token>` plus `X-GraphHelm-Journey-Credential: <credentialId>`).
The auth layer verifies it on every use against the replayed journal, before any write: the HMAC
digest in constant time; the credential unrevoked, unexpired, purpose `author`, for exactly that
flow; and its grant unrevoked and unexpired for the same lane and flow. The draft is then recorded
under that lane, and every refusal is one stable `403` that names no part of the scope. Scope
ruling (coordinator, 2026-10-16): `POST /v1/journey-flows/{id}/review` is approval and is NOT
reachable with a journey credential. It stays on the scoped-credential path until "Journey author,
reviewer and owner approval" is decided. The HMAC key file is now owner-only on Windows
(`protect_owner_only` at creation; a key provisioned before this is protected when next loaded).

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

At the baseline commit `b44477bb3d470c5211df36769f0cd3cef61d622e`:

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
| Bootstrap authorization | `serve/mod.rs::lane_credential_subject` verifies a bearer for the execution path and declared lane; `execution/assign.rs::execute` writes `NodeAssigned` only for a sealed ghost node and `core/events/src/projection.rs` replays it into `node_assignments` | Bearer possession, a task claim and a node assignment do not authorize an arbitrary journey flow. No flow grant exists today. |

This is documentary RED, not an executed failing test. The protocol below is proposed behavior.

### Storage and revocation RED baseline

The proposal puts `JourneyCredentialGrant` in the execution Event Store and keyed credential
digests in a separate registry, then assumes a shared mutex and journal make both durable writes
one transaction. The source does not provide that boundary. In `core/events/src/local.rs`,
`LocalEventRepository::append_atomic` takes that store's exclusive lock and `append_locked` commits its
own journal batch with `flush`, `sync_data` and a directory sync before publishing the active
marker. Its repository lock is acquired root-directory first, then `repository.lock`. Those locks
protect that Event Store only; they cannot atomically commit a grant event with an unrelated
registry, and a process-local mutex cannot serialize Runtime processes. This is documentary RED,
not an executed failing test.

## Proposed trust and request flow

1. The owner creates an explicit grant for a known execution and lane, naming the exact
   project, flow and purpose. A lane cannot choose or widen these fields.
2. A trusted Runtime-side provisioning service verifies the path-bound execution/lane task bearer.
   It verifies the execution belongs to the named project and that the owner grant names the
   authenticated lane and exact scope. Caller-provided lane strings alone are not identity proof.
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
never reads the owner token. The Runtime verifies the bearer against the path-bound execution and
declared lane by recomputing the derived credential in constant time, as `serve/mod.rs` does for
task routes. This proves possession for that `(execution, lane)`; it does not prove assignment or
journey entitlement. The broker also requires the owner-issued journey grant below. The shared
agent session token, an actor header alone, an agent-authored task claim, and a bearer for another
execution or lane cannot substitute. A stolen task bearer remains a bearer credential; the
shared OS-user limitation in D-058 remains.

The existing owner-only `POST /v1/executions/{executionId}/assign` writes `NodeAssigned` for one
approved ghost node and sealed proposal digest. Replay stores its actor by node in
`node_assignments`; it contains no journey flow or purpose. It cannot represent journey scope,
and a node assigned to the same lane is neither necessary nor sufficient for a journey grant.
Provisioning does not infer permission from that projection or from task-record acceptance. The
owner directly authorizes a lane for a journey through a **new**, proposed
`JourneyCredentialGrant` event; no trusted flow-grant store exists in the current Runtime.

The proposed owner-authenticated `POST /v1/executions/{executionId}/journey-credential-grants`
creates a grant; its owner-only revoke operation names the grant id. Its immutable event record
has `grantId`,
canonical `projectId`, `executionId`, `laneId`, `flowId`, `purpose` (`author` or `reviewer`),
`expiresAt`, `generation`, `state` and `issuer=owner`. The owner is the authorization source for
the named lane; a node assignment is not required. The writer verifies the execution belongs to
the Runtime project and the flow exists. Identifiers pass their
checked parsers and are compared byte for byte after canonicalization at creation. Grant
revocation revokes its credentials under the shared transaction rule below. Removing an unrelated
ghost-node assignment has no effect on a journey grant. `approve` is not a grant purpose. A
coordinator may request a grant from the owner but cannot issue one by itself. The grant writer
records the owner-authenticated grant in the private credential journal specified below. The
public execution Event Store does not receive a second grant copy. Replay rejects a duplicate id,
wrong project/execution, non-owner issuer or malformed scope and derives the active-grant view
from that journal alone. A missing, revoked, unrelated or ambiguous grant refuses issuance without
a credential or journey write.

The lane calls proposed `POST /v1/executions/{executionId}/journey-credentials` with its task bearer and
`{grantId, projectId, executionId, flowId, purpose}`; there is no authoritative `laneId` request
field. The broker checks the authenticated lane and every field against one active, unexpired
grant. Wrong scope, missing or revoked grant, or ambiguous grants return one stable authorization
diagnostic before any write, without revealing whether another scope exists. On success, the
broker draws 32 random bytes from the OS CSPRNG, stores their keyed digest and returns
`{credentialId, token, expiresAt, generation}` once to that lane on the authenticated response.
There is no token recovery endpoint. If delivery is uncertain, the owner revokes that
`credentialId` and issues a new one. Plaintext tokens and digests are excluded from events,
logs, errors, prompts, crash output and task records; audit carries identifiers and outcome only.

### Registry and commit point

#### Chosen authority and path

Use one private append-only journal as the sole durable authority for both grants and credentials.
For configured `serve --events <E>`, place its directory beside, not inside, the resolved `<E>`:
`<parent-of-E>/credential-broker/journey-credentials.v1.log`. Its sibling
`journey-credentials.hmac-key.v1` contains the versioned HMAC key and is readable only by the
Runtime identity. The parent directory and files must reject symlinks and non-regular files, use
owner-only permissions, and be created durably before the broker accepts requests. The one broker
process holds an exclusive OS lock on the exact file `journey-credentials.v1.lock` for its
lifetime; a process-local mutex alone is insufficient. Within that broker, a writer-preferring
read/write admission lock gives credential mutations priority over later journey requests. No
Runtime replica may serve this project from a private copy.

The journal is a canonical, versioned, length-delimited record stream with a per-record checksum
and monotonically increasing sequence. Its record variants are `GrantCreated`, `GrantRevoked`,
`CredentialIssued`, `CredentialRevoked`, and `CredentialReplaced` (old credential id and new
credential record in one frame). Every mutation carries an idempotency key and a canonical request
digest. Reuse with the same request returns the original operation receipt; reuse with a different
request fails with an idempotency conflict. A credential record contains its id, grant id, exact
scope, generation, expiry, active/revoked state, HMAC key version and keyed digest. It never
contains token plaintext. A grant record contains its owner issuer and canonical exact scope. The
only grant and credential projection is rebuilt by replaying this journal; it is not copied into
the execution Event Store, a cache file, or a second registry. Operational audit exports may
expose ids and outcomes but never the HMAC key or credential digests, and exports are not an
authorization input.

The key file is a versioned key bundle, provisioned before the broker starts. Version 1 is
immutable while the broker runs. Key rotation is offline: stop the broker, atomically add the new
key version to the bundle, restart, and let new credential records name that version. Keep older
versions while any active credential refers to them. Remove an old version only after replay
proves there are no active credentials using it; write and sync the replacement bundle before
restarting the broker. A crash while adding a version leaves the old key available and cannot make
an uncommitted credential record valid. An unknown or missing version fails closed. The first
implementation does not support online key replacement.

The execution Event Store is deliberately outside this transaction. `LocalEventRepository` protects
its own journal and staged artifacts; its root-directory and `repository.lock` locks, append
idempotency and active-marker protocol do not enlist a second store. Therefore the proposed grant
is not also emitted as a `JourneyCredentialGrant` execution event. This avoids split-brain grant
state rather than claiming a cross-store transaction that does not exist. It also means grant
audit is private to the credential journal until a separately reviewed projection/export is
designed; that limitation is explicit.

#### Lock order, commit and recovery

The broker takes locks in this order only: (1) its lifetime process lock on
`journey-credentials.v1.lock`, (2) the in-process writer-preferring credential admission lock,
then, for an authorized journey mutation only, (3) the existing journey-flow write lock, and (4)
if that operation writes events, the execution Event Store locks in their own order (root
directory, then `repository.lock`). Grant/credential operations never acquire journey-flow or
Event Store locks. No path may acquire the credential lock while already holding either of those
locks. Credential writes use the admission lock exclusively; credential reads hold it shared from
authentication through all durable journey effects, including the flow file, its owner sidecar,
and any Event Store append. A writer-preferring admission lock makes grant/credential revocation
wait for already-authorized writes while preventing later requests from overtaking it.

For an append, the broker validates the request against replayed state while holding the exclusive
admission lock, appends one complete bounded frame, flushes and durable-syncs the journal, syncs
the parent directory when creating it, then publishes a new immutable in-memory snapshot and
acknowledges.
The durable sync is the commit point. It mirrors the local Event Store's useful single-journal
ordering, but does not reuse or pretend to extend that store's lock. A failed write or sync returns
no success and publishes no new snapshot; its disk outcome is uncertain, so the broker stops
serving until restart replay determines whether a complete frame committed. Replay accepts only
complete, checksummed, correctly sequenced frames. A partial, malformed, corrupt or truncated tail makes the
broker unavailable for both provisioning and journey authorization; it is never truncated or
guessed past. A complete frame found after a crash is replayed as committed even if the caller
never received its acknowledgement. Retrying the same idempotency key discovers the committed
operation. For issuance whose response was lost, the retry returns the credential id and committed
status but never the token; the owner revokes that id and starts a new issue with a new key. A
reused key with changed content is always refused.

#### Issue/revoke ordering and request races

| Order or failure point | Durable result | Required response and recovery |
|---|---|---|
| Grant revoke gets the exclusive lock first | `GrantRevoked` commits; active credential checks require an active grant | Later issue refuses before writing a credential. Repeating the revoke key returns its committed receipt. |
| Issue gets the exclusive lock first | `CredentialIssued` commits under the active grant | Revoke waits, then commits. The credential is unusable after the revoke acknowledgement because each use checks both credential and grant state. |
| Revoke races an authorized journey write | The journey request holds the shared credential lock through flow-file and owner-sidecar changes, plus any Event Store append; revoke cannot commit first | Durable effects completed by that admitted request are ordered before revoke. After revoke acknowledgement, newly admitted requests refuse. If the request fails before mutation, no journey write occurs. |
| Failure before a complete durable frame | No acknowledged change; a torn tail may remain | Fail closed on restart if the tail is incomplete. Do not repair or append behind it automatically. |
| Complete frame durable, acknowledgement lost | The frame is committed and replayable | Same-key retry returns its receipt. Issue retry does not reveal plaintext; revoke the returned credential id and issue anew. |
| Corrupt/truncated credential record, or credential-store lock failure before route admission | State cannot be safely advanced or trusted | Refuse provisioning and journey writes before touching flow files or Event Store state; require explicit store recovery outside this protocol. |

Grant revocation need not rewrite every historical credential: every use checks that the
credential's grant is still active in this same replayed state. Rotation is one
`CredentialReplaced` frame, revoking the old id and activating the new one atomically; the new
plaintext is returned only after the frame is durable. The first implementation permits no
overlap. A journey write admitted before revocation may finish and commit before revocation can
acknowledge; a request admitted after acknowledgement must fail. Event Store append idempotency
handles a lost HTTP acknowledgement without changing that ordering. A sync error after a journey
handler has begun can leave its own durable outcome uncertain; it is not described as an
unchanged-state refusal. Authentication, scope and revoked-state refusals must occur before any
flow-file, owner-sidecar or Event Store mutation.

Independent security review must still accept this exact storage and race contract before
implementation. If it rejects a private authority journal or its audit boundary, revise this
choice before code; do not fall back to a mutex spanning two stores.

The route checks active status, expiration, grant generation and exact project, flow and purpose
before invoking a handler. It takes lane and execution from the registry, never actor headers.
An author token may draft only its flow. A reviewer token may review only that flow's current
recorded digest and only if its lane has never written that flow. Neither can call owner approval.
An execution task bearer authenticates provisioning and task routes, but is rejected on journey
draft and review routes. Invalid scope, unknown token, ambiguity and concurrent update
uncertainty refuse without changing flow files or the Event Store.

## Protected delivery channel

**Documentary RED (before this section):** the disposition table recorded "Secret exposure —
BLOCKED" with no traced list of writers that could carry a plaintext token. Nothing below is
implemented; it is the contract the credential-issue card must meet.

### Channel

The credential is delivered exactly once, in the body of the `200` response to the lane's own
`POST /v1/executions/{executionId}/journey-credentials`, on the loopback Runtime connection the lane
opened with its task bearer. There is no second channel: no file written by the Runtime, no event,
no task record, no MCP tool, no Studio view and no recovery endpoint. The response carries
`Cache-Control: no-store`, as other serve routes already do (`routes.rs:2870`, `:3015`, `:3030` at
`33109519`; corrected after the #1488 disposition, which found the earlier "no route sets it"
sentence false). The token is 32 CSPRNG bytes,
hex-encoded, held in `Zeroizing` from the draw until the response is serialized, following the
`tasks credential` precedent (`apps/cli/src/commands/tasks.rs:306`, `:309`).

### Every writer that could carry the plaintext

Source state at `33109519`.

| Writer | What it captures today | Required rule for the credential route |
|---|---|---|
| `--read-audit` recorder `record_read` (`serve/mod.rs:3393-3471`, wired at `:1088`) | Buffers the **whole response body** (`:3406`) and appends it in plaintext with method, path and query (`:3447-3453`). It runs outside auth. Only `/evidence` responses are redacted (`:3437-3445`). Headers never reach it (`:1086-1087`). | **The main leak path.** The recorder must replace the body of any `.../journey-credentials` response with `{"redacted":"journey credential","bytes":N}` before writing, using the same wholesale replacement as the `/evidence` branch for a non-JSON body. RED observer: with `--read-audit` on, issue one credential and assert the audit file never contains the token bytes. |
| Grant/credential journal receipts (`serve/journey_credentials.rs:88` stores `(digest, receipt)`; insert `:300`; replayed on the same key `:340`) | The full receipt `Value` is persisted in the journal frame and returned again for a repeated `Idempotency-Key`. | The `CredentialIssued` frame and its receipt hold `credentialId`, `grantId`, `generation`, `expiresAt` and the keyed digest only, **never the token**. The token is added to the HTTP body after the commit, outside the receipt. |
| Duplicate request under the same `Idempotency-Key` | Replays the stored receipt (`:340`). | It returns `409` with stable code path `/credentialId`, naming the already-issued `credentialId` and **no token**. The token is never sent twice. If the lane lost it, the owner revokes that credential and the lane requests a new one under a new key. |
| Panic hook and crash record (`serve/lifecycle.rs:219-237`, `finish` `:259-264`) | State, a sanitized `file:line` (`:226-229`), an opaque thread id, a timestamp and a fatal flag. **No panic message or payload.** | Safe as is. Keep the rule: the hook never records a payload. |
| Tracing / tower-http / `eprintln!` of bodies | None exists in serve's layers (`mod.rs:1014`, `:1080`, `:1084`, `:1088`). | Adding any body-logging layer later must exclude this route. A source guard is preferred over review memory. |
| CLI `--json` envelope | A client command prints whatever `data` it is handed (pattern: `tasks.rs:319-329`). | If a CLI client for this route is added, it writes the token to an owner-only file (`secret_file::write_derived`, `tasks.rs:318`) and prints only the path, as `tasks credential` does. |
| MCP tool replies (`commands/mcp/tools.rs`) and Studio WebMCP (`apps/studio/src/webmcp/adapter.ts:275` passes `JSON.stringify(result)` to the model) | Each tool's reply reaches a model transcript. | **No MCP or WebMCP tool may name this route.** A token in a transcript is a disclosed secret. The route joins `NOT_A_RUNTIME_VERB` in `apps/cli/tests/surface_completeness.rs` with that reason, following the grant routes added there by #1486. |
| `PUT /v1/gateway/credentials/{reference}` precedent (`serve/routes.rs:7943-7991`) | It keeps a secret out of path, query and response; the secret travels only in the request body (`:7945-7951`). | The credential route is the opposite direction (secret in the response), which is why the read-audit rule above is mandatory. |
| Error and refusal paths | Diagnostics carry a code, pointer and message. | No error is constructed after the token is drawn. Draw, commit and respond in that order, and refuse before the draw. A failure after the commit and before the response leaves an issued credential the lane never saw: the owner revokes it (below). |

### Lane-side storage

The lane writes the token to an owner-only file next to its task-bearer file, using
`secret_file::write_derived` (the `tasks credential` precedent), or keeps it in memory only. It
never writes the token to a task record, a PR, a comment, a log or an MCP argument. Under D-058 the
lane and the owner are the same OS user, so this is a no-secret-handoff guarantee only. It is
**not** OS isolation, and the "OS and process isolation boundary" section is unchanged.

### Uncertain delivery

| Case | Outcome |
|---|---|
| Refused before the draw (scope, grant, bearer) | No frame, no token, one stable diagnostic. |
| Crash before `sync_data` | No committed `CredentialIssued`; on replay the request never happened, so the lane retries with the same key. |
| Committed, response lost (reset, timeout, crash after `sync_data`) | The credential exists and the lane never saw it. A retry with the same key gets `409` naming the `credentialId` without a token. The owner revokes that `credentialId`; the lane asks again under a new key. |
| Same key, different body | `409` at `/idempotencyKey`, as for grants today. |

### Implementation card enabled by this section (conditional)

Conditional on an independent security disposition of this section. Approval authority and host
isolation stay BLOCKED and are untouched by this card.

- **Exact files:** `apps/cli/src/commands/serve/journey_credentials.rs`,
  `apps/cli/src/commands/serve/mod.rs` (route registration and the read-audit redaction),
  `apps/cli/tests/journey_flow_surfaces.rs`, `apps/cli/tests/surface_completeness.rs`.
- **Promise:** a lane holding its task bearer and an active exact-scope grant receives one
  credential, once. The token appears in no journal frame, receipt, audit line or replay. Journey
  routes do not yet accept the credential; that is the enforcement card.
- **Proof (RED first, on the parent):**
  1. With `--read-audit` on, the audit file contains no token bytes.
  2. The journal file contains no token bytes.
  3. A same-key retry gets `409` with no token.
  4. Wrong lane, execution, flow or purpose, and a revoked or expired grant, are refused with no
     frame written.
  5. Deterministic fault points: failure before `sync_data` leaves no credential; a commit followed
     by a dropped response is then revocable by the owner.

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

The independent review of PR #1344 at head `1b20121c` recorded six dispositions in
[#1344's security review](https://github.com/stabem/GraphHelm/pull/1344#issuecomment-6186277965).
The independent review of PR #1349 at head `bb94d8fe` accepted the narrowed bootstrap rule in
[#1349's security disposition](https://github.com/stabem/GraphHelm/pull/1349#issuecomment-6188269806):
the execution/lane bearer proves identity, while a separate owner-issued exact-scope grant proves
authorization. That decision is limited to bootstrap. It does not accept storage/revocation,
secret delivery, approval authority or host isolation. The storage contract in this revision needs
its own independent disposition on this exact head.

| Question | Evidence and current proposal | Independent disposition |
|---|---|---|
| Bootstrap identity and authorization | `serve/mod.rs` verifies the execution/lane bearer but grants task-route access only. `NodeAssigned` has no flow/purpose scope. Proposed private-journal `JourneyCredentialGrant` is owner-issued and exact-scope. | ACCEPTED for this narrowed bootstrap by #1349 at `bb94d8fe`; no Runtime behavior implemented. |
| Scope/purpose refusal | Exact checks and unchanged-state refusal are specified, but no Runtime route or observer exists. | DESIGN SPECIFIED on #1344 head; Runtime unobserved. |
| Revocation ordering | One private append-only authority journal with lock order, commit point, replay/idempotency and crash/race behavior. | ACCEPTED-WITH-CONDITIONS for a grant-only slice by the independent storage disposition on #1309 (gh-claude-14, `31dacf29`); implemented by #1486 (`aa6d3d13`). Fault-point observers remain open for the credential-issue slice. |
| Secret exposure | "Protected delivery channel" below: a one-shot loopback response, the full writer table with source citations, receipts that never hold plaintext, and no replay of a token. | PENDING independent disposition of the delivery contract on this head. Implementation stays blocked until then. |
| Approval authority | `review_approve -> approve_as` persists agent approval; `a_reviewer_agent_approves_only_with_its_own_credential_and_never_its_own_draft` expects approved status. | BLOCKED pending an accepted policy decision and compatibility plan on #1344 head. |
| Host isolation | D-058 places owner and agent files under one OS user; no real-host unreadability observer exists. | BLOCKED as a separate host obligation on #1344 head. |

Only an independent security reviewer can disposition the revised storage contract on this head.
Routine document approval is not security sign-off. The accepted #1349 bootstrap disposition is
carried forward with its original scope; delivery, authority/compatibility and host-isolation
blockers still prevent credential implementation.

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

## Conditional implementation dependency order

Dependency order: (1) the #1349 owner-grant bootstrap disposition is accepted and carried forward;
(2) obtain an independent security disposition of this private-journal transaction and separately
resolve protected delivery; (3) accept the approval-authority and persisted approval compatibility
decision; (4) only then begin the first bounded storage slice below, followed by grant issuance,
credential issue and journey-route enforcement slices with separate cards and isolated observers.
Host isolation remains separate. Documentation approval authorizes no code.

Do not start implementation until an independent security reviewer has dispositioned all six questions
above, every blocker affecting the application protocol is resolved, and any owner-only approval
authority decision is accepted in the decision register with the required governing-rule update.
The host-isolation blocker may remain only if the implementation is explicitly limited to
no-secret-handoff and claims no OS isolation. The following earlier five-file sketch is a later
integration target, not the first implementation slice or authorization from this RFC:

- **Exact files:** `apps/cli/src/commands/serve/mod.rs`,
  `apps/cli/src/commands/serve/routes.rs`, `apps/cli/src/commands/tasks.rs`,
  `apps/cli/src/commands/journey_flow.rs`,
  `apps/cli/tests/journey_flow_surfaces.rs`.
- **Promise:** replace startup-only `ServeState.agent_credentials` use for journey routes with a
  trusted, owner-granted credential registry; authenticate the execution-and-lane task bearer
  separately from checking the new owner grant,
  enforce project/flow/lane/purpose and revocation before writes, and keep final approval behavior
  aligned with the authority decision.
- **Storage/auth consumers:** `ServeState` and `load_agent_credentials` in `serve/mod.rs`, the
  journey draft/review handlers in `serve/routes.rs`, and owner-token task credential issuance in
  `tasks.rs`. A prior grant producer/replay slice must name its exact source files. The implementation must identify and reuse or add the durable transaction store
  named by security review; it may not claim atomicity by adding an in-memory cache alone.
- **Compatibility:** existing `GRAPHHELM_AGENT_CREDENTIALS` consumers and the persisted approved
  status produced by reviewer credentials need an explicit migration/refusal policy. Do not
  reinterpret existing approvals or erase their author/reviewer history. If the accepted decision
  requires changing reviewer approval, first add a failing isolated observer on the parent and
  preserve the existing digest and previous-writer coverage.
- **Proof:** isolated HTTP tests in `journey_flow_surfaces.rs` cover wrong scope/purpose, spoofed
  lane, execution bearer refusal on journey routes, former-writer review, exact digest binding,
  revocation without restart and unchanged state on refusal. Grant-revocation races and durable
  replay need separate observers if the selected store path reaches them. Real-host OS isolation
  remains a separate deliverable.

### First conditional implementation card (not authorized yet)

Proceed only after independent security disposition accepts the private-journal commit/recovery
contract, the protected delivery channel is decided, and any authority/compatibility decision
required by the selected purpose is accepted.

- **Exact files:** `apps/cli/src/commands/serve/journey_credentials.rs`,
  `apps/cli/src/commands/serve/mod.rs`, `apps/cli/src/commands/serve/routes.rs`,
  `apps/cli/tests/journey_flow_surfaces.rs`.
- **Promise:** Implement the private grant/credential journal and owner-only grant create/revoke
  routes. Prove exact-scope grant replay, durable idempotency and refusal behavior. This slice does
  not issue credential plaintext, change journey routes or change approval authority; those need
  later cards after delivery and authority decisions.
- **Proof:** Isolated temporary-store and HTTP tests with deterministic fault points cover grant
  create/revoke ordering, pre-sync failure, durable commit with lost acknowledgement, same-key
  replay, conflicting-key reuse, corrupt/truncated records and unchanged state on authorization
  refusal. No stress or live Runtime test. Journey-write/revocation races are observed when a later
  card integrates credential checks with the journey flow write lock.
- **Retirement/compatibility:** No caller is displaced in this storage-only slice. Do not read or
  migrate `GRAPHHELM_AGENT_CREDENTIALS`; route integration and its explicit compatibility policy
  are a later card.

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
- concurrent grant revocation and provisioning serialize in either order: revocation first refuses
  issuance, while issuance first is revoked before revocation acknowledgement, including after replay;
- a valid task bearer without a grant, one for another lane or execution, a wrong flow, an
  unrelated `NodeAssigned`, an agent-authored task claim and a revoked grant all refuse with
  unchanged persisted state.

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
