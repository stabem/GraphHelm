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
    /// Print the JSON envelope even at a terminal (#1172). Without it a terminal gets the
    /// rendered summary, and everything that is not a terminal gets the envelope either way.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: TopLevel,
}

#[derive(Debug, Subcommand)]
pub enum TopLevel {
    Graph(GraphArgs),
    Schema(SchemaArgs),
    /// Extension package operations. Validation stays offline; the lifecycle subcommands
    /// (install, switch, rollback, uninstall) act on an install root under an exclusive claim.
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
    /// Operations beside the CI gate's own run (`ci/gate.ps1`). Nothing here changes a verdict:
    /// the gate stays deterministic, and these commands only read what it left behind.
    Gate(GateArgs),
    /// First run in a project (#1062): provisions `<project>/.graphhelm/` — the events
    /// directory, the bearer token `serve` will read, the sealing key and keyring the Studio's
    /// message box needs — registers the MCP server with the chat harnesses it detects, ignores
    /// the directory in git, and prints the exact next commands. Idempotent: an existing token or
    /// key is kept, never rotated. Neither secret is ever printed.
    Init(InitArgs),
    /// Read-only inventory and preview for adopting GraphHelm methodology into supported hosts.
    Setup(AdoptionSetupArgs),
    /// Creates a local, verified checkpoint of supported host configuration files.
    Backup(AdoptionBackupArgs),
    /// Preview or apply an exact reviewed, offline configuration restore.
    Restore(AdoptionRestoreArgs),
    /// Build, verify, or query the source-bound Keel contract index.
    Keel(KeelArgs),
}

#[derive(Debug, Args)]
pub struct KeelArgs {
    #[command(subcommand)]
    pub command: KeelCommand,
}

#[derive(Debug, Subcommand)]
pub enum KeelCommand {
    /// Scan tracked repository files into an index outside the repository.
    Index {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Refuse stale or changed index data.
    Verify {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        index: PathBuf,
    },
    /// Query declarations and paths from a verified index.
    Query {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        index: PathBuf,
        #[arg(long)]
        term: String,
    },
}

#[derive(Debug, Args)]
pub struct AdoptionSetupArgs {
    #[arg(long)]
    pub project: PathBuf,
    #[arg(long)]
    pub home: PathBuf,
    #[arg(long, conflicts_with_all = ["apply", "recover", "verify"])]
    pub dry_run: bool,
    /// Owner decision for one unresolved item: `<item>=keep` or `<item>=replace:<file>` whose
    /// bytes are the reviewed replacement. Repeatable. With any `--resolve`, `--out` is required
    /// because the resulting plan carries bytes that never go to stdout.
    #[arg(long = "resolve", conflicts_with_all = ["apply", "recover", "verify", "plan"])]
    pub resolve: Vec<String>,
    /// Write the plan (preview, or the applyable plan built from `--resolve`) to this private,
    /// owner-only file. Its exact digest is what `--accept` takes.
    #[arg(long, conflicts_with_all = ["apply", "recover", "verify", "plan"])]
    pub out: Option<PathBuf>,
    /// Private recovery storage, outside the project and host roots.
    #[arg(long = "state-root")]
    pub state_root: Option<PathBuf>,
    /// Apply this private, reviewed plan after checking its exact digest.
    #[arg(long, requires_all = ["accept", "state_root"], conflicts_with_all = ["recover", "verify", "plan"])]
    pub apply: Option<PathBuf>,
    /// Preview this exact reviewed plan (redacted: digest, scopes, decisions, operation paths,
    /// digests and byte lengths; never the `after` text), or bind it to a verification attempt.
    #[arg(long, conflicts_with = "recover")]
    pub plan: Option<PathBuf>,
    /// Validate an activation receipt against --plan and current installed bytes.
    /// No trusted host observer is shipped; user-authored JSON remains observer_missing.
    #[arg(long, requires_all = ["plan", "state_root"], conflicts_with = "recover")]
    pub verify: Option<PathBuf>,
    #[arg(long, requires = "apply")]
    pub accept: Option<String>,
    /// Explicit local package trees; both must match the reviewed Extension pins.
    #[arg(long = "package", requires = "apply")]
    pub packages: Vec<PathBuf>,
    /// Reconcile an interrupted transaction using its private journal.
    #[arg(long, requires = "state_root", conflicts_with = "apply")]
    pub recover: Option<String>,
}

#[derive(Debug, Args)]
pub struct AdoptionBackupArgs {
    #[arg(long)]
    pub project: PathBuf,
    #[arg(long)]
    pub home: PathBuf,
    #[arg(long = "state-root")]
    pub state_root: PathBuf,
}

#[derive(Debug, Args)]
pub struct AdoptionRestoreArgs {
    #[arg(long = "state-root")]
    pub state_root: PathBuf,
    /// Original baseline, a verified manual checkpoint, or a checkpoint linked to an adoption transaction.
    #[arg(long, default_value = "original")]
    pub backup: String,
    #[arg(long, requires = "accept", conflicts_with = "recover")]
    pub apply: Option<PathBuf>,
    #[arg(long, requires = "apply")]
    pub accept: Option<String>,
    #[arg(long, conflicts_with = "apply")]
    pub recover: Option<String>,
}

#[derive(Debug, Args)]
pub struct GateArgs {
    #[command(subcommand)]
    pub command: GateCommand,
}

#[derive(Debug, Subcommand)]
pub enum GateCommand {
    /// SHADOW classification of a RED gate run by a typed judge (#1138, edge 1): a bounded
    /// excerpt of the log and the known-flake list are put to the judge as closed questions
    /// (`class` over `known_flake | environment_void | real_defect | harness_broke`, one
    /// `same_as:<issue>` per known flake) and the reading is printed and, with `--out`, written
    /// beside the manifest. The verdict is never touched, nothing is re-queued, and a
    /// classification never makes the command exit non-zero: only its own failures do (an
    /// unreadable log, a bad flakes file, a judge that cannot answer).
    ///
    /// The judge is the recorded door (`--judge-fixture`, keyless) or a `direct_api` `typesafe`
    /// route (`--manifest --judge-route --broker --keyring --key-id`); exactly one of the two.
    ClassifyRed(Box<ClassifyRedArgs>),
}

/// The flags of `gate classify-red`; see the variant's doc for the two judge doors.
#[derive(Debug, Args)]
pub struct ClassifyRedArgs {
    /// The gate log to classify, UTF-8 or UTF-16 (the runner's transcripts are UTF-16 with a
    /// BOM), at most 16 MiB.
    #[arg(long)]
    pub log: PathBuf,
    /// A JSON array of `{"issue": <n>, "test": "<name>", "summary": "<why it flakes>"}`.
    #[arg(long = "known-flakes")]
    pub known_flakes: PathBuf,
    /// The recorded judge door: a `{"answers": {"<request sha256>": <reply>}}` recording.
    #[arg(long = "judge-fixture", conflicts_with = "judge_route")]
    pub judge_fixture: Option<PathBuf>,
    #[arg(long, requires = "judge_route")]
    pub manifest: Option<PathBuf>,
    /// The judge door over a gateway route: a `direct_api` route whose provider is `typesafe`.
    #[arg(long = "judge-route", requires = "manifest")]
    pub judge_route: Option<String>,
    #[arg(long)]
    pub broker: Option<PathBuf>,
    #[arg(long)]
    pub keyring: Option<PathBuf>,
    #[arg(long = "key-id")]
    pub key_id: Option<String>,
    /// Where the classification is written, beside the manifest. Must end in `.json` and must
    /// not exist yet: a record is never overwritten.
    #[arg(long)]
    pub out: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// The project directory to provision. Defaults to the current directory; must exist.
    #[arg(long)]
    pub project: Option<PathBuf>,
    /// The loopback address the Runtime will bind; written into every harness registration and
    /// every printed command, so one number is used everywhere (`install.sh` uses 8080 on a VPS).
    #[arg(long, default_value = "127.0.0.1:8791")]
    pub bind: String,
    /// The sealing key's id inside the keyring.
    #[arg(long = "key-id", default_value = "studio")]
    pub key_id: String,
    /// Which chat harnesses to register, repeatable. Absent: every harness detected on this
    /// machine (`claude-code` when `.claude/` exists in the project or `~/.claude` exists;
    /// `codex` when `~/.codex` exists).
    #[arg(long, value_enum)]
    pub harness: Vec<Harness>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Harness {
    /// Claude Code: `<project>/.mcp.json`, merged into an existing one.
    ClaudeCode,
    /// Codex: a `[mcp_servers.graphhelm]` snippet at `<project>/.graphhelm/codex.config.toml`
    /// for the operator to append to `~/.codex/config.toml`; the home directory is never written.
    Codex,
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
    /// #212: stage, verify and adopt a package under the install root's versions/ tree. Does
    /// not switch; the adopted digest is printed for the switch that follows.
    Install {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        package: PathBuf,
    },
    /// #212: atomically make an adopted version the active one. The target is re-verified at
    /// the moment of use, and any refusal leaves the previous version active.
    Switch {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        digest: String,
    },
    /// #212: atomically return to the previous known-good version.
    Rollback {
        #[arg(long)]
        root: PathBuf,
    },
    /// #212: remove one adopted version that the active pointer no longer names.
    Uninstall {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        digest: String,
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
    /// Compile a context capsule (#222/#273's compiler, exposed here).
    CompileContext {
        /// Token budget the compiled capsule must fit within.
        ///
        /// Defaults to zero so the argument-free invocation keeps its existing meaning: with no
        /// required sections the required budget is zero, which fits any budget, so the
        /// degenerate capsule still compiles and the existence-parity guard still passes.
        #[arg(long, default_value_t = 0)]
        budget: usize,
        /// A required context section. Repeatable.
        ///
        /// Required context is never dropped to fit: if it does not fit, the command refuses.
        /// Trimming it would return a capsule, under budget, missing evidence the caller was
        /// required to see.
        #[arg(long = "require")]
        require: Vec<String>,
    },
    /// Propose content for governed memory and report the admission verdict (#220's admission,
    /// exposed here).
    MemoryPropose,
    /// Report a context-accounting receipt (#222/#273's accounting types, exposed here).
    Accounting,
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
    /// OPTIONAL because one `.mcp.json` is shared by every session in a repository, so a
    /// literal here makes every session the same actor (#1058). Falls back to
    /// `GRAPHHELM_ACTOR`; absent from both doors is a refusal, never a default.
    #[arg(long)]
    pub actor: Option<String>,
    /// `agent` (the chat is an agent) or `owner` for an owner-driven chat.
    #[arg(long = "actor-type", default_value = "agent")]
    pub actor_type: String,
    /// The model this session is running, verbatim and opaque (`claude-opus-5`, `gpt-6-astra`).
    /// ABSENT IS ABSENT: no default, and the Runtime never infers one from the route -- a
    /// `claude_subscription` route lets the CLI choose its own model, so a route-derived guess
    /// would be a label the run cannot support (#1054).
    #[arg(long)]
    pub model: Option<String>,
    /// `low`, `medium` or `high`. A CLOSED vocabulary, so a wrong value is refusable here
    /// rather than rendered as a badge nobody can interpret.
    #[arg(long)]
    pub effort: Option<String>,
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
    /// The MODEL half of the real-executor wiring: `--manifest`, `--broker` and `--route` are
    /// all-or-none (#1066). With them, cognitive nodes (agent, planner, classifier, evaluator)
    /// run on the named route. Without them, cognitive nodes are answered by node fixtures.
    /// Any real half also requires `--keyring`/`--key-id`. Absent entirely, together with the
    /// tool half below, `serve` stays the fixture-only 05a server.
    #[arg(long)]
    pub manifest: Option<PathBuf>,
    /// The credential broker directory the model half leases from. See `--manifest`.
    #[arg(long)]
    pub broker: Option<PathBuf>,
    /// The sealed keyring evidence is sealed under; with `--key-id` it is its own all-or-none
    /// pair, usable alone (sealing-only, for `signal`) and REQUIRED with either real half.
    #[arg(long)]
    pub keyring: Option<PathBuf>,
    #[arg(long = "key-id")]
    pub key_id: Option<String>,
    /// The manifest route cognitive nodes run on by default (a request may name another). See
    /// `--manifest`.
    #[arg(long)]
    pub route: Option<String>,
    /// The TOOL half of the real-executor wiring: `--staging` and `--allow-program` are
    /// all-or-none (#1066), and need NO model credential. With them, tool nodes run on the real
    /// tool host: one Tier 1 worktree per execution under this staging directory, kept across
    /// that execution's tool calls and removed when its drive ends; a `commit` node lands
    /// `refs/graphhelm/executions/<execution id>` in the project — a plain ref, never a branch,
    /// never the operator's checkout. Without them, tool nodes are answered by node fixtures.
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
    /// Meaningful only alongside the tool half (`--staging`/`--allow-program`; `drive` only ever
    /// consults it there); given without it, it is accepted but silently unused.
    #[arg(long)]
    pub project: Option<PathBuf>,
    /// Run the customs sweep automatically every N seconds, journalling each one as
    /// `SweepCaller::Tick`.
    ///
    /// OFF UNLESS ASKED FOR, and that default is a safety property rather than caution. A tick is
    /// a BACKGROUND WRITER: it appends to the same streams operators are mutating, so with it on
    /// by default every existing caller that compares two head sequences across a request pair
    /// would start seeing a third append it never made. `--sweep-interval` makes the writer a
    /// deployment decision, and makes the tick testable without arming it for everyone.
    #[arg(long = "sweep-interval")]
    pub sweep_interval: Option<u64>,
    /// Append every request and the exact bytes served to this file, as JSON lines.
    ///
    /// Off unless asked for: the recorded bodies carry the operator's own execution data, so
    /// recording is a deliberate act rather than a default. The file is written OUTSIDE the
    /// event store on purpose - a surface that wrote into the execution stream in order to
    /// observe it would advance `headSequence` without advancing `lastEventAt`, and head
    /// movement would stop implying progress.
    #[arg(long = "read-audit")]
    pub read_audit: Option<PathBuf>,
    /// The bare program name a `tests` tool node runs, resolved on the child's PATH (see
    /// `--path-prepend`). Host configuration, never caller input.
    #[arg(long = "tests-runner", default_value = "cargo")]
    pub tests_runner: String,
    /// Repeatable; the other half of the `--staging` pair (#583, #1066). There is no default: the
    /// set of programs an execution may spawn is declared per run, so the journal records a choice
    /// rather than an inheritance. The tests runner is host configuration (`--tests-runner`) and
    /// is not listed here.
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
        /// Publish the graph and record `execution_started` WITHOUT entering the drive loop, so
        /// every node stays `Draft` until an explicit `execution resume` (#90).
        ///
        /// `resume` WORKS on the result, which is the half that makes this sentence a promise
        /// rather than a description: the hold is recorded as `ExecutionPaused`, because
        /// `resume_preconditions` gates on `simulation_status == Paused` and would otherwise
        /// answer `NotPaused` to every held execution.
        ///
        /// WHERE `Draft` LIVES, since the reply does not show it: `node_states` holds only nodes
        /// that have ALREADY changed state, so a node still to be dispatched is ABSENT, and the
        /// driver reads that absence as `Draft` (`.get(node).copied().unwrap_or(NodeState::Draft)`
        /// -- core/execution/src/attention.rs). A held execution therefore reports
        /// `nodeStateCounts.draft` as 0 with both nodes staged. That is the projection's
        /// documented encoding for every execution before its first dispatch, not something this
        /// flag introduces.
        ///
        /// This is the axis `mode` is NOT: `mode` governs graph-MUTATION autonomy and the driver
        /// never reads it, so before this flag the only way to stage work without running it was
        /// to start (which dispatches) and then pause -- a reaction racing the driver rather than
        /// a precondition. Holding dispatch and holding mutation stay orthogonal, which is the
        /// reasoning #79 was closed on.
        #[arg(long)]
        held: bool,
    },
    /// Lists the execution streams the event store holds: one summary row per stream, ordered
    /// by execution id, sliced by `--after` (exclusive) and `--limit`.
    ///
    /// The same answer `GET /v1/executions` replies with. It exists so a client never has to
    /// scrape the human monitor page to discover which executions are present.
    List {
        #[arg(long)]
        events: PathBuf,
        /// Exclusive cursor: the last execution id the caller already read.
        #[arg(long)]
        after: Option<String>,
        /// Page size, 1..=100. Defaults to 20. A larger value is refused, never clamped.
        #[arg(long)]
        limit: Option<usize>,
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
        /// #134: the execution's ACTIVE graph -- the latest version published on its stream, or
        /// the graph it started from when nothing was published (the CLI's own streams). With it,
        /// the reply carries `dispatch` (`ready` to dispatch, `gated` behind unmet predecessors,
        /// `gatedNodes`) derived from the same predicate the driver uses; without it `dispatch` is
        /// null and `dispatchUnavailable` says why. Any other graph, the start graph included once
        /// a newer one was published, is refused.
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Replays a stream and reports the resume briefing: what the execution is for, every
    /// decision with its actor in order, the work done, what is pending, and the next step.
    /// Derived from the store alone; the first thing to read when picking up a run another
    /// session or harness drove.
    Briefing {
        #[arg(long)]
        events: PathBuf,
        #[arg(long)]
        execution: Option<String>,
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
        /// REQUIRED, with `--key-id`. Sealed keyring directory; the 32-byte key arrives out of
        /// band via `GRAPHHELM_EVENTS_KEY`.
        ///
        /// Have no keyring? Create an empty directory, then run
        /// `gateway keyring init --keyring <dir> --key-id <id>` with `GRAPHHELM_EVENTS_KEY` set —
        /// that command does not create the directory. Then pass both here.
        // ^ THE LINES ABOVE ARE `--help` TEXT. clap turns a doc comment on a field into what the
        // operator reads, so everything a maintainer needs and an operator does not belongs below
        // this line, as an ordinary comment (#1002, Codex P2: the rationale had become the help,
        // and `signal --help` answered an operator with "#135 was filed as …").
        //
        // WHY `Option` FOR A REQUIRED PAIR: so THIS crate produces the refusal instead of clap.
        // clap can say which arguments are missing and cannot say what to do about it, and that
        // sentence is the whole point of #135. The behaviour is unchanged — `sealing()` rejects
        // `(None, None)` exactly as the parser used to.
        //
        // THE COST, stated because it is real: the usage line now shows these inside `[OPTIONS]`
        // rather than as required, so clap's own metadata is looser than the contract. The help
        // text carries the requirement instead. `required = true` would restore the metadata and
        // take the message back to clap's, which is the thing being repaired.
        //
        // #135 was filed as "a keyless store cannot record a signal at all". Measured, that is not
        // a capability gap: `gateway keyring init` mints exactly the keyring this command opens —
        // same provider, same environment variable, same 32 bytes — and the signal then records,
        // sealed, on a store created without one. The operator was blocked by not knowing that
        // command. An earlier version made the pair genuinely optional behind `--unsealed`; it
        // reversed the rule in the real-executor plan
        // ("refusing to run without a keyring rather than silently skipping the seal") to solve a
        // problem that already had a compliant answer, and was withdrawn.
        #[arg(long)]
        keyring: Option<PathBuf>,
        /// REQUIRED, with `--keyring`. The key id inside that keyring.
        #[arg(long = "key-id")]
        key_id: Option<String>,
    },
    /// Record what a node delivered and why, with project-relative file and journey references.
    /// Claims are agent-reported; this command does not verify filesystem changes.
    Delivery {
        #[arg(long)]
        events: PathBuf,
        #[arg(long)]
        execution: String,
        #[arg(long)]
        node: String,
        #[arg(long)]
        delivery: PathBuf,
        /// Main project filesystem directory; execution stream scope remains fixed.
        #[arg(long = "project-directory")]
        project: PathBuf,
        #[arg(long)]
        keyring: PathBuf,
        #[arg(long = "key-id")]
        key_id: String,
        /// Record the calling agent's identity. Omit for an owner-entered report.
        #[arg(long = "actor-id")]
        actor_id: Option<String>,
    },
    /// Read a delivery's referenced text document from the explicitly selected main project.
    DocumentRead {
        #[arg(long)]
        events: PathBuf,
        #[arg(long)]
        execution: String,
        /// Main project filesystem directory; execution stream scope remains fixed.
        #[arg(long = "project-directory")]
        project: PathBuf,
        #[arg(long = "evidence-id")]
        evidence_id: String,
        #[arg(long)]
        index: usize,
        #[arg(long)]
        keyring: PathBuf,
        #[arg(long = "key-id")]
        key_id: String,
    },
    /// Save an owner edit to the main project and record the change notice.
    DocumentSave {
        #[arg(long)]
        events: PathBuf,
        #[arg(long)]
        execution: String,
        /// Main project filesystem directory; execution stream scope remains fixed.
        #[arg(long = "project-directory")]
        project: PathBuf,
        #[arg(long)]
        edit: PathBuf,
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
    /// Holds every edge-eligible node, refusing unless the aggregate status is unset or
    /// `Running`.
    ///
    /// `--file` supplies the graph this execution started from. With it, the hold is the
    /// driver's edge-eligible set, upstream of capacity planning. Without it, pause falls back
    /// to bare state and reports `heldNodesGated: false` rather than making an edge claim. The
    /// flag is optional because the graph is not stored on the CLI's own execution streams.
    Pause {
        #[arg(long)]
        file: Option<PathBuf>,
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
    /// Evaluates the stream's customs stages and journals the result: one `sweep_performed`, plus
    /// one `overdue_exception` for every episode found lapsed, appended together.
    ///
    /// A sweep that finds nothing STILL writes its record. Without it, "no exceptions" and "no
    /// sweep ever ran" are the same absence in the log.
    ///
    /// `--as-of` asks about a past instant and defaults to now. THE FUTURE IS REFUSED: a
    /// future-dated answer is indistinguishable from a real one in the journal, while permanently
    /// spending the episodes it touches -- the honest sweep arriving later would find nothing left
    /// to raise.
    Sweep {
        #[arg(long)]
        events: PathBuf,
        #[arg(long)]
        execution: Option<String>,
        #[arg(long = "as-of")]
        as_of: Option<String>,
    },
    /// Claim that a `waiting_input` node's external work is done, presenting evidence. Testimony
    /// only: nothing is released until `execution clear` countersigns. A claim the pipeline
    /// cannot accept is journaled as `completion_refused` with its registry code.
    Claim {
        /// The graph this execution started from — checked against the recorded hash, and the
        /// source of the node's declared `completion.customs.proofKinds`.
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        events: PathBuf,
        #[arg(long)]
        execution: Option<String>,
        #[arg(long)]
        node: String,
        /// The envelope sequence of the exact open wait this answers. Absent: the node's open wait.
        #[arg(long = "wait-seq")]
        wait_seq: Option<u64>,
        /// A JSON array of `{"kind","contentHash","size"}` — the wire shape of `ClaimEvidence`.
        #[arg(long)]
        evidence: PathBuf,
        /// Who asserts the completion. Absent: the owner actor this command runs as.
        #[arg(long)]
        asserter: Option<String>,
        /// `operator_attested` (default) or `machine_verified`.
        #[arg(long, default_value = "operator_attested")]
        mode: String,
    },
    /// Countersign a claim by machine replay: present the digest of the evidence bundle you hold.
    /// A matching digest clears the node and drives its dependents; a mismatch is journaled as a
    /// rejection and drives nothing.
    Clear {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        events: PathBuf,
        #[arg(long)]
        fixtures: Option<PathBuf>,
        #[arg(long)]
        execution: Option<String>,
        #[arg(long = "claim-seq")]
        claim_seq: u64,
        /// `sha256:<64 hex>` — the bundle digest computed elsewhere. Exactly one of this and
        /// `--evidence`.
        #[arg(long = "manifest-hash", conflicts_with = "evidence")]
        manifest_hash: Option<String>,
        /// The bundle itself (same JSON shape as `claim --evidence`); its digest is computed here.
        #[arg(long, conflicts_with = "manifest_hash")]
        evidence: Option<PathBuf>,
        /// Only `machine_replay` exists today; `countersign` is refused and names the decision that
        /// will supply it.
        #[arg(long, default_value = "machine_replay")]
        verifier: String,
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
    /// Writes one `direct_api` route into a manifest (#1171): the edit `setup` cannot do, and the
    /// CLI half of `PUT /v1/gateway/routes`. Never touches a credential value, and never rewrites
    /// a `native_runtime` route.
    Route(RouteArgs),
    /// Manages BYOK credentials held in the broker.
    Credential(CredentialArgs),
    /// Manages the sealing keyring itself, independently of any credential.
    Keyring(KeyringArgs),
    /// One command that wires a model provider into a project `init` provisioned (#1139): adds
    /// the provider's `direct_api` route to `<project>/.graphhelm/manifest.json`, reads the API
    /// key ONCE (hidden prompt on a terminal, one line on a pipe; never an argument), stores it in
    /// the Credential Broker under the keyring `init` made, probes the route, and prints the next
    /// commands. The key never appears in output, logs, or any file outside the sealed broker.
    Setup(SetupArgs),
}

/// The providers `gateway setup` knows how to wire; each pins the defaults `--route-id`,
/// `--model` and `--base-url` may override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SetupProvider {
    /// TypeSafe's System One judge (`https://api.typesafe.ai`, model `jev-latest`, route `judge`).
    Typesafe,
    /// Anthropic's Messages API (`https://api.anthropic.com`, route `anthropic`; `--model` required).
    Anthropic,
    /// OpenAI's API (`https://api.openai.com`, route `openai`; `--model` required).
    Openai,
}

#[derive(Debug, Args)]
pub struct SetupArgs {
    #[arg(long, value_enum)]
    pub provider: SetupProvider,
    /// The project `init` provisioned. Defaults to the current directory; must exist.
    #[arg(long)]
    pub project: Option<PathBuf>,
    /// The route id written into the manifest; the provider's default when absent.
    #[arg(long = "route-id")]
    pub route_id: Option<String>,
    /// The model the route names. Defaulted for `typesafe` only; the other providers publish no
    /// single model this command could pin, so it must be given.
    #[arg(long)]
    pub model: Option<String>,
    /// The route's `baseUrl` (`https://…`, or `http://` to loopback); the provider's when absent.
    #[arg(long = "base-url")]
    pub base_url: Option<String>,
    /// Replace an existing route with the same id. Without it a second run is refused and the
    /// manifest is left untouched.
    #[arg(long)]
    pub replace: bool,
    /// The sealing key's id inside the keyring, as given to `init`.
    #[arg(long = "key-id", default_value = "studio")]
    pub key_id: String,
}

#[derive(Debug, Args)]
pub struct RouteArgs {
    #[command(subcommand)]
    pub command: RouteCommand,
}

#[derive(Debug, Subcommand)]
pub enum RouteCommand {
    /// Adds or replaces one `direct_api` route in the manifest. The whole document is validated
    /// before anything is written, and the write is a temporary file renamed into place, because
    /// `serve` re-reads the manifest on every request that resolves a route.
    Set {
        #[arg(long)]
        manifest: PathBuf,
        /// The route id callers will name (`--route`, `"route"` in a request body). Unique within
        /// the manifest: an id already present is refused unless `--replace` is given.
        #[arg(long)]
        id: String,
        /// The WIRE FORMAT the adapter selects on, not the vendor: `anthropic`, `openai` or
        /// `typesafe` today. A DeepSeek endpoint is an `openai` route with its own base URL.
        #[arg(long)]
        provider: String,
        /// `https://…`, or `http://` to a loopback address. No trailing slash.
        #[arg(long = "base-url")]
        base_url: String,
        #[arg(long)]
        model: String,
        /// The broker reference holding this route's key. Absent: `secret_<id>`, which cannot
        /// collide the way a provider-keyed name does when two vendors share one wire format.
        #[arg(long = "credential-ref")]
        credential_ref: Option<String>,
        /// Write the route disabled. A disabled route is listed and refused at dispatch, which is
        /// how an operator parks a provider without deleting what it took to configure.
        #[arg(long)]
        disabled: bool,
        /// Replace an existing route with this id. Without it, an existing id is refused and the
        /// manifest is left byte-identical.
        #[arg(long)]
        replace: bool,
    },
}

#[derive(Debug, Args)]
pub struct KeyringArgs {
    #[command(subcommand)]
    pub command: KeyringCommand,
}

#[derive(Debug, Subcommand)]
pub enum KeyringCommand {
    /// Creates the sealing key a Runtime needs to seal evidence, and nothing else.
    ///
    /// This key encrypts evidence held on this machine. It is not a model credential, is never
    /// sent anywhere, and grants access to nothing outside this keyring.
    ///
    /// It exists because until now the ONLY way to put a key in a keyring was
    /// `gateway credential set`, which stores a BYOK model credential and reads a secret from
    /// stdin. A Runtime that seals a message needs the key and does not need a credential, so an
    /// operator who wanted only the message path had to invent an API key to get past the setup.
    Init {
        #[arg(long)]
        keyring: PathBuf,
        #[arg(long = "key-id")]
        key_id: String,
    },
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
    /// Writes a new backup archive. An existing target is never overwritten.
    ///
    /// `--repository` backs up a local filesystem store; `--config` backs up the configured
    /// Postgres database. They are mutually exclusive.
    Backup {
        #[arg(long)]
        repository: Option<PathBuf>,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        output: PathBuf,
    },
    /// Restores an archive into an empty target.
    ///
    /// With `--config`, the destination is the database named by the configuration's `adminUrl`,
    /// which is why no target flag is taken in that mode: the operator refuses to proceed unless
    /// that database is fresh. With `--repository`, the destination IS that path — a local store
    /// has no configuration naming it, so the flag that selects the mode also names the target.
    Restore {
        #[arg(long)]
        repository: Option<PathBuf>,
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
    /// Reports a graph file's shape - entrypoints, nodes, edges - plus the semantic hash that
    /// says WHICH graph it is.
    ///
    /// The hash is the load-bearing field: an execution's log records the graph's hash and never
    /// its topology, so a caller that wants to draw a run's graph compares this hash against the
    /// `graphHash` in that run's `execution_started` event before believing the edges belong to
    /// it. Nodes carry identity only - no objectives, agents or completion controls.
    Topology {
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
    /// Compiles a goal into a graph document through a model (#107): one draft, a bounded
    /// repair loop, the same validation chain an authored graph takes, and `completion.customs`
    /// stamped on every node that can park. Writes the document to `--out` and never publishes
    /// or starts it: the file enters the system through `execution start --file` like any other.
    ///
    /// The model is either the recorded door (`--fixture`, keyless) or a gateway route
    /// (`--manifest --route`, with `--broker --keyring --key-id` for a `direct_api` route);
    /// exactly one of the two.
    /// Boxed: the flag set is the widest of the family (#1123 added four), and clap implements
    /// `Args` for `Box<T>`, so the enum stays the size of its other variants.
    Synthesize(Box<SynthesizeArgs>),
}

/// The flags of `graph synthesize`; see the variant's doc for the two model doors.
#[derive(Debug, Args)]
pub struct SynthesizeArgs {
    #[arg(long)]
    pub goal: String,
    /// Where the document is written. Must end in `.json` and must not exist yet.
    #[arg(long)]
    pub out: PathBuf,
    /// `autopilot`, `supervised` or `manual`; the profile's default (`supervised`) when
    /// absent.
    #[arg(long)]
    pub mode: Option<String>,
    #[arg(long = "max-nodes")]
    pub max_nodes: Option<usize>,
    /// Repeatable; the programs a synthesized shell call may name. There is no default: a
    /// draft naming any other program is refused, and the architect never widens the list.
    #[arg(long = "allow-program")]
    pub allow_programs: Vec<String>,
    /// A `{"replies": {"<prompt sha256>": "<text>"}}` recording (`core/architect/fixtures`).
    #[arg(long)]
    pub fixture: Option<PathBuf>,
    #[arg(long)]
    pub manifest: Option<PathBuf>,
    #[arg(long)]
    pub route: Option<String>,
    #[arg(long)]
    pub broker: Option<PathBuf>,
    #[arg(long)]
    pub keyring: Option<PathBuf>,
    #[arg(long = "key-id")]
    pub key_id: Option<String>,
    /// The judge door over a gateway route: a `direct_api` route whose provider is
    /// `typesafe`, named in `--manifest` and leased through `--broker --keyring --key-id`
    /// like `--route`. Exclusive with `--judge-fixture`; no judge means today's road.
    #[arg(
        long = "judge-route",
        requires = "manifest",
        conflicts_with = "judge_fixture"
    )]
    pub judge_route: Option<String>,
    /// The recorded judge door: a `{"answers": {"<request sha256>": <reply>}}` recording
    /// (`core/architect/fixtures/judge`).
    #[arg(long = "judge-fixture")]
    pub judge_fixture: Option<PathBuf>,
    /// How many drafts to ask for and rank, 1..=3; more than one needs a judge.
    #[arg(long)]
    pub drafts: Option<u8>,
    /// A directory of graph templates (`<id>.yaml` beside `<id>.template.json`) the judge
    /// may choose to reuse or adapt; read only when a judge is named.
    #[arg(long)]
    pub library: Option<PathBuf>,
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
