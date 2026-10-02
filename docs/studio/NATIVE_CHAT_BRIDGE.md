# Existing native chats in Studio

Studio can send a work request for a selected graph step to an existing native Codex chat. The native chat keeps its history, working directory, model, provider, sandbox and approval settings. Selecting a chat in another project is an explicit owner action; it does not move that project or widen the Runtime document root.

## Owner flow

1. Select a work step and open **Connect existing chat**.
2. Search the real native catalog by chat title, project path or thread identifier.
3. Select the intended chat. Check its displayed source project.
4. Enter a bounded work request and press **Send work**.
5. Read the request state and the actual chat reply in the inspector.

The native catalog is bounded. A stored chat is not proof that the Desktop is idle or that its agent is connected. Native resume/writer-lock refusals remain failures; the bridge never forks, restarts or replaces the selected thread to bypass them.

## Separate facts

- **Requested**: a sealed intent was durably recorded before the native effect.
- **Received**: the bridge observed the matching native turn start.
- **Chat reply**: the bridge observed the matching successful native turn completion and captured its bounded reply.
- **Blocked**: the native host required approval or refused the work.
- **Unobserved**: the native outcome or durable receipt was not confirmed.

A native reply is not accepted code delivery, a passing test, operational node assignment, graph completion, or JPD certification. The existing Governor and delivery contracts retain those responsibilities. Skill use and test results are not inferred from native text.

The selected source project remains visible. The association is a work request referencing a node; it does not create a `NodeAssigned` event. Source native settings are not overridden. Approval requests are never automatically accepted by this bridge.

## Runtime host configuration

The Runtime launcher supplies the trusted absolute `GRAPHHELM_CODEX_HOST_PROGRAM` executable and, when required, the existing `GRAPHHELM_CODEX_SQLITE_HOME` directory. They are administrator configuration, not browser input. Studio's normal owner flow requires no shell commands. A missing or failing host produces a diagnostic instead of an invented catalog or receipt.

The source profile is preserved; `CODEX_HOME` is not rewritten. The SQLite path is passed only as a transient native app-server option. The bridge does not repair, reset, delete or manually edit native thread databases. The current Windows observer uses the Desktop's actual native binary and existing native SQLite directory. This is host-specific evidence, not a claim about every supported OS or Desktop installation.

## Public Runtime boundary

`GET /v1/native-chats` discovers bounded existing native chat metadata. `GET /v1/executions/{id}/native-chats` reads durable work-request facts. Owner-authenticated `POST /v1/executions/{id}/native-chats` accepts the selected node, native thread, source directory, bounded request text and stable request identity.

The native adapter verifies the selected directory against native thread metadata before resuming it. It passes no working-directory, provider, model, approval or sandbox override. The adapter is separate from the existing one-shot `ModelCall` adapter; this does not change that milestone's call semantics.

Request and reply text stay in sealed evidence. Events carry opaque references and typed facts. Generic signals cannot author native observer receipts. The route validates owner authority, execution/node identity, source metadata and idempotency before native dispatch.

## Recovery

Retrying the same immutable request identity reads its recorded state; it never starts a second native turn. Divergent reuse is refused. A recorded intent without a confirmed outcome stays unobserved after a crash. The owner must inspect that request before choosing new work; the bridge does not guess whether an external effect happened.

The native subprocess and JSONL frames are bounded. Only the bridge's own process tree is cleaned up. Original chats and unrelated Desktop processes remain owned by their original host.

## Proof

The typed [journey contract](../acceptance/native-chat-journey-2026-10-02.json) defines catalog operation, actual receipt, visible reply, native history preservation and retry safety. Focused offline tests protect framing, identity, authority and UI failure/recovery behavior. Actual native and browser observations are required for the delivery claim; HTTP acceptance and mocked provider answers cannot replace them.

The [observation obligations](../acceptance/native-chat-obligations-2026-10-02.json) preserve the unresolved JPD certification boundary: the installed catalog is declarative and does not register a deterministic native-chat evidence matcher. Live host and browser results are ordinary evidence. They do not authorize `matched` JPD status or certification.
