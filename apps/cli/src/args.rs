use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "graphhelm",
    version,
    about = "GraphHelm Foundation Graph Kernel"
)]
pub struct Cli {
    #[arg(long, global = true)]
    pub pretty: bool,
    #[command(subcommand)]
    pub command: TopLevel,
}

#[derive(Debug, Subcommand)]
pub enum TopLevel {
    Graph(GraphArgs),
    Schema(SchemaArgs),
    /// Extension package operations. Validation is offline and never installs or activates.
    Extension(ExtensionArgs),
    /// Development-contract Runtime services (#223), exposed identically here, over MCP, and
    /// over HTTP. This surface is under construction: the existence-parity guard in
    /// `apps/cli/tests/development_surface_parity.rs` is what keeps the three adapters honest
    /// about which operation families actually exist as it grows.
    Development(DevelopmentArgs),
    Events(EventsArgs),
    Execution(ExecutionArgs),
    Gateway(GatewayArgs),
    /// Invokes one brokered tool call against a project (Tier 0 reads in place, Tier 1 in an
    /// ephemeral worktree), producing a digest-only record and operator-side captured streams.
    Tool(ToolArgs),
    Serve(ServeArgs),
    /// Serves the chat surface: a stateless MCP server over stdio whose tools map 1:1 onto
    /// Public Runtime API requests. Speaks newline-delimited JSON-RPC 2.0 until stdin closes.
    Mcp(McpArgs),
    /// The wake doorbell's sidecar: creates the rendezvous for an opaque id, blocks
    /// for free, exits 0 on ring / 3 on timeout / 2 on unusable arguments. Content never
    /// crosses: the woken host learns only THAT it should re-read its log.
    WakeWait(WakeWaitArgs),
    /// Quality-gate operations: the thymus ritual and the certification stamp.
    Quality(QualityArgs),
}

#[derive(Debug, Args)]
pub struct ExtensionArgs {
    #[command(subcommand)]
    pub command: ExtensionCommand,
}

#[derive(Debug, Subcommand)]
pub enum ExtensionCommand {
    /// Validates extension.json and every declared contribution in a local package directory.
    Validate { package: PathBuf },
    /// #213: mints one per-contribution MCP capability token, allowed_tools frozen from the
    /// named contribution's own declared `surfaces` at this instant. Prints the token as JSON
    /// to stdout (the caller redirects to a file); this command never writes one itself.
    MintMcpToken {
        #[arg(long)]
        package: PathBuf,
        /// The contribution id, exactly as declared in `extension.json`'s `contributions[]`.
        #[arg(long)]
        contribution: String,
        #[arg(long)]
        actor: String,
        #[arg(long = "ttl-seconds")]
        ttl_seconds: u64,
    },
    /// #213: flips `revoked` on a presented token file's JSON and prints the result to stdout
    /// -- every other bound field is untouched. The caller decides where the revoked bytes
    /// land (overwrite in place, or a new file); this command never writes one itself.
    RevokeMcpToken {
        #[arg(long = "token-file")]
        token_file: PathBuf,
    },
}

#[derive(Debug, Args)]
pub struct DevelopmentArgs {
    #[command(subcommand)]
    pub command: DevelopmentCommand,
}

#[derive(Debug, Subcommand)]
pub enum DevelopmentCommand {
    /// Resolve layered code rules into one contract (#218's resolver, exposed here).
    ResolveContract,
    /// Report the governed memory states, transitions, and the moves policy allows (#220).
    MemoryStatus,
    /// Render an owner-facing presentation from a task result (#219's renderer, exposed here).
    Present,
}

#[derive(Debug, Args)]
pub struct McpArgs {
    /// The Public Runtime API's base URL. Loopback-only, fail-closed: userinfo is stripped
    /// before host inspection (the post-#36 rule), so `[::1]@evil.com` shapes never pass.
    #[arg(long)]
    pub url: String,
    /// File whose first line is the bearer token. Alternative: `GRAPHHELM_API_TOKEN`. The
    /// flag wins; both absent is a refusal naming the two options. The token value itself
    /// NEVER travels via argv.
    #[arg(long = "token-file")]
    pub token_file: Option<PathBuf>,
    /// The actor every mutation is attributed to (the serve layer's actor id rules).
    #[arg(long)]
    pub actor: String,
    /// `agent` (the chat is an agent) or `owner` for an owner-driven chat.
    #[arg(long = "actor-type", default_value = "agent")]
    pub actor_type: String,
    /// File holding one per-contribution MCP capability token (#213), JSON, matching
    /// `graphhelm_tool_broker::mcp_capability::McpCapabilityToken`'s wire shape. Opt-in by
    /// PRESENCE, not by `--actor-type`: `--actor-type agent` is the default for ordinary chat
    /// sessions with no contribution to scope, so requiring this flag there would refuse the
    /// common case, not the threat. The party that decides whether an extension contribution's
    /// session gets this flag is whatever harness launches `graphhelm mcp` on the
    /// contribution's behalf -- never the contribution's own declared config, since that would
    /// let a hostile contribution simply omit the flag and keep today's unrestricted access.
    /// When present, requires `--package` (the digest every presented tool call is checked
    /// fresh against); when absent, every tool is reachable exactly as before #213.
    #[arg(long = "capability-token-file")]
    pub capability_token_file: Option<PathBuf>,
    /// The extension package this session's capability token was minted against (required
    /// alongside `--capability-token-file`). Its digest is recomputed fresh on every tool call,
    /// never cached for the session's lifetime -- a package edited mid-session immediately
    /// stales any token minted before the edit (#213 blueprint T2).
    #[arg(long = "package")]
    pub package: Option<PathBuf>,
    /// Append-only JSONL audit trail (required alongside `--capability-token-file`): one
    /// redacted `McpCapabilityAuditRecord` per tool call, allowed or refused. Never call
    /// arguments, never the token's own bytes. Read with `jq` or any line-oriented JSON tool; a
    /// dedicated reader/summarizer command is a named follow-up, not built by #213.
    #[arg(long = "capability-audit-log")]
    pub capability_audit_log: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct QualityArgs {
    #[command(subcommand)]
    pub command: QualityCommand,
}

#[derive(Debug, Subcommand)]
pub enum QualityCommand {
    /// Runs a registered gate against the pathogen suite; on FULL rejection, stamps
    /// GateCertified onto the stream (what the certified-or-not-at-all precondition reads).
    Certify {
        #[arg(long)]
        events: PathBuf,
        #[arg(long)]
        execution: Option<String>,
        /// The registered gate id. The registry is closed; an unrecognised id is refused and
        /// the refusal names what IS registered.
        ///
        /// Deliberately does NOT enumerate the gates. clap renders this from a compile-time
        /// literal, so it cannot be derived from the registry and could only be kept in sync by
        /// memory -- in a different file from the check that decides membership, and read by the
        /// operator BEFORE anything runs. The refusal message is the single place that enumerates.
        #[arg(long)]
        gate: String,
    },
}

#[derive(Debug, Args)]
pub struct WakeWaitArgs {
    /// The events directory holding the lease this session armed.
    #[arg(long)]
    pub events: PathBuf,
    /// The execution whose stream carries the lease. Optional when the directory holds one.
    #[arg(long)]
    pub execution: Option<String>,
    /// The session whose OWN lease this waits on. The rendezvous and the deadline both come
    /// from that lease — this surface used to take a rendezvous id and a timeout from the
    /// caller, which made two numbers answer "how long before I give up" with nothing tying
    /// them together.
    #[arg(long = "session-id")]
    pub session_id: String,
}

#[derive(Debug, Args)]
pub struct ServeArgs {
    /// The events directory the server reads and writes through the shared command layer; the
    /// same directory a co-located `graphhelm execution`/`events` invocation would use. Created
    /// if it does not already exist.
    #[arg(long)]
    pub events: PathBuf,
    /// The address to bind, e.g. `127.0.0.1:8080` or `127.0.0.1:0` for an OS-assigned port.
    /// Refused unless it names a loopback address — the Public Runtime API is never exposed
    /// beyond localhost.
    #[arg(long)]
    pub bind: String,
    /// The real-executor wiring. Optional as a GROUP — see
    /// `commands::serve::mod`'s startup validation for the exact all-or-none rule this and its
    /// siblings below must satisfy. Absent entirely, `serve` stays the fixture-only 05a server.
    #[arg(long)]
    pub manifest: Option<PathBuf>,
    #[arg(long)]
    pub broker: Option<PathBuf>,
    #[arg(long)]
    pub keyring: Option<PathBuf>,
    #[arg(long = "key-id")]
    pub key_id: Option<String>,
    #[arg(long)]
    pub route: Option<String>,
    #[arg(long)]
    pub staging: Option<PathBuf>,
    /// The deployer's own default workspace root for `start`/`resume`, used whenever a request
    /// omits `"project"` — never required, never part of the real-executor group above. Absent,
    /// the prior behavior is unchanged: the server's own process working directory. Exists
    /// because neither `start` nor `resume`'s MCP tool schema exposes a `project` field (issue
    /// #82) — an operator confined to the MCP surface has no channel to avoid the default
    /// colliding with `--staging` when the server happens to run from a `--staging` ancestor, so
    /// the deployer must be able to fix the default once, the same way `--staging` itself is
    /// fixed once, rather than every caller needing infrastructure knowledge it was never given.
    /// Meaningful only alongside the real-executor group above (`drive` only ever consults it
    /// there); given without them, it is accepted but silently unused.
    #[arg(long)]
    pub project: Option<PathBuf>,
    /// Append every request and the exact bytes served to this file, as JSON lines.
    ///
    /// Off unless asked for: the recorded bodies carry the operator's own execution data, so
    /// recording is a deliberate act rather than a default. The file is written OUTSIDE the
    /// event store on purpose - a surface that wrote into the execution stream in order to
    /// observe it would advance `headSequence` without advancing `lastEventAt`, and head
    /// movement would stop implying progress.
    #[arg(long = "read-audit")]
    pub read_audit: Option<PathBuf>,
    #[arg(long = "tests-runner", default_value = "cargo")]
    pub tests_runner: String,
    /// Repeatable; defaults to `git`+`cargo` when empty (the same default the plan gives
    /// `graphhelm tool invoke`'s equivalent flag).
    #[arg(long = "allow-program")]
    pub allow_program: Vec<String>,
    /// Repeatable; directories joined ahead of the child's inherited PATH.
    #[arg(long = "path-prepend")]
    pub path_prepend: Vec<PathBuf>,
}

#[derive(Debug, Args)]
pub struct ExecutionArgs {
    #[command(subcommand)]
    pub command: ExecutionCommand,
}

#[derive(Debug, Subcommand)]
pub enum ExecutionCommand {
    /// Publishes a graph, starts an execution and drives it to quiescence.
    Start {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        events: PathBuf,
        #[arg(long)]
        fixtures: Option<PathBuf>,
        /// `autopilot`, `supervised`, or `manual` — D-022's three levels of GRAPH-MUTATION
        /// autonomy, and nothing else. `autopilot` lets the Governor accept its own proposed
        /// mutations; `supervised` holds a proposal until the owner approves it — something is
        /// WAITING for you; `manual` rejects every proposal outright — nothing is queued,
        /// nothing waits, the owner changes the graph by other means. It does NOT hold dispatch:
        /// a `Ready` node runs identically in every mode (`core/runtime/src/driver.rs` never
        /// reads this value) — to hold dispatch, use `execution pause`, not a stricter mode.
        /// (#89: this flag previously had no help text at all, and D-022's own "Manual Graph"
        /// wording — the qualifier that carries this scope — is absent from every other
        /// operator-visible surface too.)
        #[arg(long)]
        mode: String,
        #[arg(long)]
        execution: Option<String>,
    },
    /// Replays a stream and reports the execution's current state — the operator's triage view.
    Status {
        #[arg(long)]
        events: PathBuf,
        #[arg(long)]
        execution: Option<String>,
        /// Also writes the monitor page as a frozen, self-contained HTML snapshot (no
        /// refresh tag) to this path — the incident artifact and the live page are the same
        /// renderer. The JSON envelope still prints to stdout.
        #[arg(long)]
        html: Option<PathBuf>,
    },
    /// Admits a Graph Signal envelope and reports the governance verdict for the mode in
    /// force.
    ///
    /// Its evidence is written to a file YOU name (`--evidence-out`), not to the encrypted
    /// evidence store: a signal envelope has none of the content slots that pipeline seals.
    /// Encrypted externalization for signals is still unbuilt.
    //
    // NOTE, deliberately a code comment and NOT a doc comment: everything above this line is
    // printed by `--help`, and this paragraph is for whoever edits the file.
    //
    // The previous help text called that gap "Milestone 05 work" while other commands in this
    // same binary announced themselves as 05g and M06 — both shipped. A newcomer probe read
    // the contradiction and stopped trusting the help output entirely, which was the correct
    // response: internal milestone numbers date instantly and say nothing to a reader outside
    // the project.
    //
    // Writing this as `///` is how the first version of this fix leaked the project's own
    // history back into the surface it was cleaning, one paragraph after removing it.
    Signal {
        #[arg(long)]
        events: PathBuf,
        #[arg(long)]
        execution: Option<String>,
        #[arg(long)]
        signal: PathBuf,
        #[arg(long = "evidence-out")]
        evidence_out: PathBuf,
        /// Sealed keyring directory; the envelope now also seals into the Evidence store
        /// (Milestone 05d Task 6) — the command refuses to run without a keyring rather than
        /// silently skipping the seal. The 32-byte key arrives via `GRAPHHELM_EVENTS_KEY`.
        #[arg(long)]
        keyring: PathBuf,
        #[arg(long = "key-id")]
        key_id: String,
    },
    /// The owner approves a `Ghost` or `Blocked` node, readying it. Does not auto-drive: nothing
    /// auto-starts out of a manual intervention (D-020); run `resume` to continue.
    Approve {
        #[arg(long)]
        events: PathBuf,
        #[arg(long)]
        execution: Option<String>,
        #[arg(long)]
        node: String,
    },
    /// Declares how long one node may stay silent before it needs you, valid from now
    /// forward. Use it when `execution status` answers `unknown` for a node: the answer names
    /// this command as its remedy.
    ///
    /// Earlier versions of this text explained the finding that produced the command instead
    /// of what the command does. A newcomer probe read it and concluded the project had an
    /// audience of two people, which was fair.
    ///
    /// Answers with the RECOMPUTED verdict, never a bare ok -- a write that says "done" forces
    /// a second read, and between them two surfaces can disagree about whether the operator
    /// may sleep.
    AmendBudget {
        #[arg(long)]
        events: PathBuf,
        #[arg(long)]
        execution: Option<String>,
        #[arg(long)]
        node: String,
        /// The bound the OPERATOR decides. Nothing suggests a value: a default here would
        /// turn "absent means unknown" into "absent means 300s" through the back door.
        #[arg(long)]
        seconds: u64,
        /// The frontier this was computed against, from the verdict that asked for it. An
        /// amendment the store cannot place is refused, never guessed.
        #[arg(long)]
        at: u64,
    },
    /// Holds every dispatchable node (`Ready`/`Queued`), refusing unless the aggregate status is
    /// unset or `Running`.
    Pause {
        #[arg(long)]
        events: PathBuf,
        #[arg(long)]
        execution: Option<String>,
    },
    /// Recovers any crashed node, gates on the resume preconditions, then re-dispatches exactly
    /// the nodes the pause held and drives to quiescence again. `--file` names the graph the
    /// execution started with — the driver has no other source of the spec to drive against.
    Resume {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        events: PathBuf,
        #[arg(long)]
        fixtures: Option<PathBuf>,
        #[arg(long)]
        execution: Option<String>,
    },
    /// Cancels every non-terminal node and completes the execution as `Cancelled`. Refuses when
    /// the execution is already terminal.
    Cancel {
        #[arg(long)]
        events: PathBuf,
        #[arg(long)]
        execution: Option<String>,
    },
}

#[derive(Debug, Args)]
pub struct ToolArgs {
    #[command(subcommand)]
    pub command: ToolCommand,
}

#[derive(Debug, Subcommand)]
pub enum ToolCommand {
    /// One authorized tool call, end to end: authorize -> route by tier -> execute -> record.
    Invoke {
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        staging: PathBuf,
        /// Repeatable; directories the workspace must never overlap (keyring, broker, events).
        #[arg(long = "protected")]
        protected: Vec<PathBuf>,
        /// The ToolCall as JSON (checked deserialization: unknown fields refused).
        #[arg(long)]
        request: PathBuf,
        #[arg(long)]
        actor: String,
        /// Repeatable; builds the lease: repository.read, repository.write, shell.execute,
        /// tests.execute.
        #[arg(long = "capability")]
        capabilities: Vec<String>,
        /// Repeatable; the lease's shell program allowlist (bare names).
        #[arg(long = "allow-program")]
        allow_programs: Vec<String>,
        #[arg(long = "tests-runner", default_value = "cargo")]
        tests_runner: String,
        /// REQUIRED on every invoke (checked as GHCLI012, not by the parser, so the refusal
        /// speaks the envelope): captured stream bytes are written here as files.
        #[arg(long = "capture-out")]
        capture_out: Option<PathBuf>,
        /// Debug only: skip workspace cleanup and report what was kept.
        #[arg(long = "keep-workspace")]
        keep_workspace: bool,
    },
}

#[derive(Debug, Args)]
pub struct GatewayArgs {
    #[command(subcommand)]
    pub command: GatewayCommand,
}

#[derive(Debug, Subcommand)]
pub enum GatewayCommand {
    /// Validates a route manifest and reports every route it declares.
    Routes {
        #[arg(long)]
        manifest: PathBuf,
    },
    /// Quota-free health probe (§18) for one route: a `direct_api` route proves its credential
    /// leases from the broker; a `native_runtime` route proves its CLI spawns and exits cleanly on
    /// `--version`. Never places a real model call and never prints a credential value.
    Probe {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        route: String,
        #[arg(long)]
        broker: Option<PathBuf>,
        #[arg(long)]
        keyring: Option<PathBuf>,
        #[arg(long = "key-id")]
        key_id: Option<String>,
    },
    /// Manages BYOK credentials held in the broker.
    Credential(CredentialArgs),
}

#[derive(Debug, Args)]
pub struct CredentialArgs {
    #[command(subcommand)]
    pub command: CredentialCommand,
}

#[derive(Debug, Subcommand)]
pub enum CredentialCommand {
    /// Stores a credential. The value is read from stdin — one trimmed line — and is never
    /// accepted as an argument.
    Set {
        #[arg(long)]
        broker: PathBuf,
        #[arg(long)]
        keyring: PathBuf,
        #[arg(long = "key-id")]
        key_id: String,
        #[arg(long = "ref")]
        reference: String,
        #[arg(long)]
        provider: String,
        #[arg(long = "usable-by")]
        usable_by: String,
    },
    /// Revokes a credential. A revoked credential can never be leased again, including after the
    /// broker is reopened.
    Remove {
        #[arg(long)]
        broker: PathBuf,
        #[arg(long)]
        keyring: PathBuf,
        #[arg(long = "key-id")]
        key_id: String,
        #[arg(long = "ref")]
        reference: String,
    },
}

#[derive(Debug, Args)]
pub struct EventsArgs {
    #[command(subcommand)]
    pub command: EventsCommand,
}

#[derive(Debug, Subcommand)]
pub enum EventsCommand {
    /// Verifies a repository's format and, when a range is supplied, its hash chain.
    Verify {
        #[arg(long)]
        repository: Option<PathBuf>,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        execution: Option<String>,
        #[arg(long)]
        stream: Option<String>,
        #[arg(long)]
        start: Option<u64>,
        #[arg(long = "max-events")]
        max_events: Option<u32>,
    },
    /// Rebuilds a disposable projection generation from retained canonical history.
    Rebuild {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        execution: Option<String>,
        #[arg(long)]
        stream: Option<String>,
        #[arg(long)]
        generation: Option<u64>,
        #[arg(long = "page-size")]
        page_size: Option<u32>,
    },
    /// Writes a new encrypted backup archive. An existing target is never overwritten.
    Backup {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        output: PathBuf,
    },
    /// Restores an authenticated archive into the configured, already-empty target database.
    ///
    /// The destination is the database named by the configuration's `adminUrl`. The operator
    /// refuses to proceed unless that database is fresh, so there is no target flag to pass.
    Restore {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        archive: PathBuf,
    },
}

#[derive(Debug, Args)]
pub struct SchemaArgs {
    #[command(subcommand)]
    pub command: SchemaCommand,
}

#[derive(Debug, Subcommand)]
pub enum SchemaCommand {
    Catalog {
        #[arg(long)]
        catalog: PathBuf,
    },
    Check {
        #[arg(long)]
        baseline: PathBuf,
        #[arg(long)]
        candidate: PathBuf,
    },
    Migrate {
        #[arg(long)]
        catalog: PathBuf,
        #[arg(long)]
        migration: PathBuf,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    Conformance {
        #[arg(long)]
        catalog: PathBuf,
        #[arg(long)]
        fixtures: PathBuf,
    },
    Digest {
        #[arg(long)]
        file: PathBuf,
    },
    View {
        #[arg(long)]
        catalog: PathBuf,
        #[arg(long)]
        schema: String,
    },
}

#[derive(Debug, Args)]
pub struct GraphArgs {
    #[command(subcommand)]
    pub command: GraphCommand,
}

#[derive(Debug, Subcommand)]
pub enum GraphCommand {
    Validate {
        file: PathBuf,
    },
    Lint {
        file: PathBuf,
    },
    Hash {
        file: PathBuf,
    },
    Simulate {
        file: PathBuf,
        #[arg(long)]
        events: PathBuf,
        #[arg(long)]
        fixtures: Option<PathBuf>,
    },
    Draft(DraftArgs),
    Replay {
        #[arg(long)]
        events: PathBuf,
        #[arg(long)]
        workspace: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        execution: Option<String>,
        #[arg(long)]
        stream: Option<String>,
    },
}

#[derive(Debug, Args)]
pub struct DraftArgs {
    #[command(subcommand)]
    pub command: DraftCommand,
}

#[derive(Debug, Subcommand)]
pub enum DraftCommand {
    Apply {
        base_file: PathBuf,
        draft_file: PathBuf,
        #[arg(long)]
        actor: String,
        #[arg(long)]
        events: PathBuf,
    },
}
