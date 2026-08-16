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
    /// Milestone 05d Task 9: the real-executor wiring. Optional as a GROUP — see
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
    /// Admits a Graph Signal envelope, externalizes its evidence, and reports the governance
    /// verdict for the mode in force. Evidence externalizes to an operator-supplied file, not the
    /// encrypted Evidence store — the sealed-provider pipeline expects the Governor's own content
    /// slots, which a signal envelope does not have; operator-grade encrypted externalization of
    /// signal envelopes is Milestone 05 work.
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
