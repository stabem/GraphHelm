# Existing native chats in Studio

Studio can send a work request for a selected graph step to an existing native Codex chat. The native chat keeps its history, working directory, model, provider, sandbox and approval settings. Selecting a chat in another project is an explicit owner action; it does not move that project or widen the Runtime document root.

## Owner flow

1. Select a work step and open **Connect existing chat**.
2. Search the real native catalog by chat title, project path or thread identifier.
3. Select the intended chat. Check its displayed source project.
4. Enter a bounded work request and press **Send work**.
5. Read the request state and the actual chat reply in the inspector.

The catalog shows the most recently updated native chats first, with stable thread identity as
the tie breaker. Pagination preserves the selected recipient. Node inspectors show the newest
twenty history records first and keep older records behind **Older history**.

## Principal conversation

The principal conversation is visible on the left as soon as an activity opens. Its **Next step**
explains how to select the main native chat and enter an instruction. The selector defaults to
the first owner-declared native persona; the displayed recipient identity remains explicit.
**Send to main chat** addresses only that selected native thread. **Send to team** addresses the
other linked activity personas, with their names shown before the owner sends anything.

Each recipient gets a distinct immutable native request identity and its own activity charter.
The owner explicitly starts the batch; opening the panel, discovering chats or refreshing
requests sends nothing. A partial or unknown outcome is reconciled under its original identity.
The panel never repeats a batch automatically. A received reply is displayed as a reply, rather
than evidence that the requested code, review or deployment finished.

The composer stays locked until the request ledger has been read successfully. A failed ledger
read leaves it locked. Before dispatch, the browser saves only recovery identities in session
storage: execution, node, thread and request IDs. It does not save message text, charters, replies,
project paths or credentials. A reload or roster change preserves an uncertain request even if
its intent is absent from the ledger. Only an exact authoritative terminal receipt releases that
recovery identity; refreshing does not send the work again.

The [principal conversation journey](../acceptance/studio-main-chat-journey-2026-10-03.json)
defines rendering, target isolation, actual native replies and partial-failure recovery. Fan-out
is an explicit owner instruction to the selected team; it does not prove autonomous conversation
between native chats or assign Governor-owned graph steps.

Native Desktop updates can remove the version-specific executable configured by the Runtime
launcher. `GRAPHHELM_CODEX_HOST_PROGRAM` must identify the current trusted installed executable.
Validate its existence before launching the Runtime. A host validation failure before durable
intent means no native request was dispatched; retain the original UI request identity and
reconcile the ledger before sending new work. Do not rewrite native thread databases or change
the provider, approval or sandbox profile to bypass a missing host.

## Declaring a persona

After selecting a native chat, enter its **Role** and **Personality**, then press **Add persona
to activity**. This owner declaration links the existing chat identity to the selected activity
and step. It sends no native turn. The canvas lists explicitly declared personas; process
identities such as deployers remain in recorded technical history rather than becoming people.

Open the persona to see its name, source project and activity charter. Its **Work message**
composer sends only to the linked native thread. Each explicit order includes that activity's
charter within the existing 2,000-character request limit. The first charter wins; a later
membership record cannot silently replace that persona's role. Reloading replays the sealed
owner declarations. Switching activities clears the previous activity's projected membership.

Membership uses the public signal API with `native_persona_linked`, an owner source and a sealed
JSON description identified by `graphhelm-native-persona-v1`. The description carries
`executionId`, `nodeId`, `threadId`, `title`, `sourceDirectory` and `charter`; the envelope's `to`
must match the native UUID. Malformed, unopenable, agent-authored or cross-activity declarations
do not create personas. The declaration is separate from `persona_created`, so it cannot start
the legacy persona host. Native dispatch still verifies the live thread and project metadata.

Activity membership and native order receipts do not prove automatic conversation between
native chats. Existing addressed messages preserve their actual sender and delivery status;
Studio never invents native acknowledgements or maps a shared `codex` actor to a thread UUID.
The [persona journey](../acceptance/studio-personas-journey-2026-10-02.json) describes the required
browser observations. These remain ordinary evidence, without JPD certification.

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
