mod adoption;
pub(crate) mod architect;
mod development;
mod draft;
mod events;
mod execution;
mod extension;
mod gate;
mod gateway;
mod hash;
mod init;
mod journey;
mod journey_explore;
mod journey_flow;
mod journey_live;
mod journey_replay;
mod journey_validate;
mod journeys;
mod keel;
mod lint;
mod mcp;
mod observers;
mod quality;
pub(crate) mod remediation;
mod replay;
mod runtime_record;
mod schema;
mod secret_file;
mod serve;
mod simulate;
mod skills;
mod studio;
mod tool;
mod topology;
mod validate;
mod wake_wait;
mod workspace;
mod workspace_slot;

use std::sync::Arc;

use chrono::Utc;
use graphhelm_events::{EventRepositoryError, LocalEventRepository};
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{Actor, ActorType, Clock, IdGenerator};

use crate::args::{
    CredentialCommand, DevelopmentCommand, DraftCommand, EventsCommand, ExecutionCommand,
    ExtensionCommand, GateCommand, GatewayCommand, GraphCommand, KeelCommand, KeyringCommand,
    QualityCommand, RouteCommand, SchemaCommand, SynthesizeArgs, ToolCommand, TopLevel,
};
use crate::output::Outcome;

pub fn run(command: TopLevel) -> Outcome {
    match command {
        TopLevel::Graph(graph) => match graph.command {
            GraphCommand::Validate { file } => validate::run(&file),
            GraphCommand::Lint { file } => lint::run(&file),
            GraphCommand::Hash { file } => hash::run(&file),
            GraphCommand::Topology { file } => topology::run(&file),
            GraphCommand::Simulate {
                file,
                events,
                fixtures,
            } => simulate::run(&file, &events, fixtures.as_deref()),
            GraphCommand::Draft(args) => match args.command {
                DraftCommand::Apply {
                    base_file,
                    draft_file,
                    actor,
                    events,
                } => draft::run(&base_file, &draft_file, &actor, &events),
            },
            GraphCommand::Replay {
                events,
                workspace,
                project,
                execution,
                stream,
            } => replay::run(
                &events,
                workspace.as_deref(),
                project.as_deref(),
                execution.as_deref(),
                stream.as_deref(),
            ),
            GraphCommand::Synthesize(synthesize) => {
                let SynthesizeArgs {
                    goal,
                    out,
                    mode,
                    max_nodes,
                    allow_programs,
                    fixture,
                    manifest,
                    route,
                    broker,
                    keyring,
                    key_id,
                    judge_route,
                    judge_fixture,
                    drafts,
                    library,
                    critic,
                } = *synthesize;
                architect::run(&architect::SynthesizeArguments {
                    goal,
                    out,
                    mode,
                    max_nodes,
                    allow_programs,
                    fixture,
                    manifest,
                    route,
                    broker,
                    keyring,
                    key_id,
                    judge_route,
                    judge_fixture,
                    drafts,
                    library,
                    critic,
                })
            }
        },
        TopLevel::Schema(schema) => match schema.command {
            SchemaCommand::Catalog { catalog } => schema::catalog::run(&catalog),
            SchemaCommand::Check {
                baseline,
                candidate,
            } => schema::check::run(&baseline, &candidate),
            SchemaCommand::Migrate {
                catalog,
                migration,
                input,
                output,
            } => schema::migrate::run(&catalog, &migration, &input, &output),
            SchemaCommand::Conformance { catalog, fixtures } => {
                schema::conformance::run(&catalog, &fixtures)
            }
            SchemaCommand::Digest { file } => schema::digest::run(&file),
            SchemaCommand::View {
                catalog,
                schema: name,
            } => schema::view::run(&catalog, &name),
        },
        TopLevel::Extension(extension_args) => match extension_args.command {
            ExtensionCommand::Validate { package } => extension::run(&package),
            ExtensionCommand::Install { root, package } => extension::run_install(&root, &package),
            ExtensionCommand::Switch { root, digest } => extension::run_switch(&root, &digest),
            ExtensionCommand::Rollback { root } => extension::run_rollback(&root),
            ExtensionCommand::Uninstall { root, digest } => {
                extension::run_uninstall(&root, &digest)
            }
            ExtensionCommand::MintMcpToken {
                package,
                contribution,
                actor,
                ttl_seconds,
            } => extension::run_mint_mcp_token(&package, &contribution, &actor, ttl_seconds),
            ExtensionCommand::RevokeMcpToken { token_file } => {
                extension::run_revoke_mcp_token(&token_file)
            }
        },
        TopLevel::Development(development_args) => match development_args.command {
            DevelopmentCommand::ResolveContract => development::run_resolve_contract(),
            DevelopmentCommand::MemoryStatus => development::run_memory_status(),
            DevelopmentCommand::Present => development::run_present(),
            DevelopmentCommand::MemoryPropose {
                content,
                workspace_id,
                project_id,
            } => development::run_memory_propose(&content, &workspace_id, &project_id),
            DevelopmentCommand::DreamShadow {
                input,
                trigger,
                category,
                run_id,
                planner_id,
                critic_id,
                reject,
                events,
            } => development::run_dream_shadow(
                &input,
                &trigger,
                &category,
                &run_id,
                &planner_id,
                &critic_id,
                reject,
                events.as_deref(),
            ),
            DevelopmentCommand::CompileContext { budget, require } => {
                development::run_compile_context(budget, &require)
            }
            DevelopmentCommand::Accounting => development::run_accounting(),
        },
        TopLevel::Events(events) => match events.command {
            EventsCommand::Verify {
                repository,
                config,
                workspace,
                project,
                execution,
                stream,
                start,
                max_events,
            } => events::verify::run(events::verify::Request {
                repository: repository.as_deref(),
                config: config.as_deref(),
                workspace: workspace.as_deref(),
                project: project.as_deref(),
                execution: execution.as_deref(),
                stream: stream.as_deref(),
                start,
                max_events,
            }),
            EventsCommand::Rebuild {
                config,
                workspace,
                project,
                execution,
                stream,
                generation,
                page_size,
            } => events::rebuild::run(events::rebuild::Request {
                config: config.as_deref(),
                workspace: workspace.as_deref(),
                project: project.as_deref(),
                execution: execution.as_deref(),
                stream: stream.as_deref(),
                generation,
                page_size,
            }),
            EventsCommand::Backup {
                repository,
                config,
                output,
            } => events::backup::run(events::backup::Request {
                repository: repository.as_deref(),
                config: config.as_deref(),
                output: &output,
            }),
            EventsCommand::Restore {
                repository,
                config,
                archive,
            } => events::restore::run(events::restore::Request {
                repository: repository.as_deref(),
                config: config.as_deref(),
                archive: &archive,
            }),
        },
        TopLevel::Execution(execution) => match execution.command {
            ExecutionCommand::Start {
                file,
                events,
                fixtures,
                mode,
                execution,
                held,
                keyring,
                key_id,
            } => execution::start::run(
                &file,
                &events,
                fixtures.as_deref(),
                &mode,
                execution.as_deref(),
                held,
                execution::start::GenesisKeyring {
                    directory: keyring.as_deref(),
                    key_id: key_id.as_deref(),
                },
            ),
            ExecutionCommand::List {
                events,
                after,
                limit,
            } => execution::list::run(&events, after.as_deref(), limit),
            ExecutionCommand::Status {
                events,
                execution,
                html,
                file,
            } => execution::status::run(
                &events,
                execution.as_deref(),
                html.as_deref(),
                file.as_deref(),
            ),
            ExecutionCommand::Briefing {
                events,
                execution,
                keyring,
                key_id,
            } => execution::briefing::run(
                &events,
                execution.as_deref(),
                keyring
                    .zip(key_id)
                    .map(|(directory, key_id)| execution::signal::SignalKeyring {
                        directory,
                        key_id,
                    })
                    .as_ref(),
            ),
            ExecutionCommand::Signal {
                events,
                execution,
                signal,
                evidence_out,
                keyring,
                key_id,
                attach,
            } => execution::signal::run(
                &events,
                execution.as_deref(),
                &signal,
                &evidence_out,
                keyring.as_deref(),
                key_id.as_deref(),
                &attach,
            ),
            ExecutionCommand::Delivery {
                events,
                execution,
                node,
                delivery,
                project,
                keyring,
                key_id,
                actor_id,
            } => execution::delivery::run(
                &events,
                &execution,
                &node,
                &delivery,
                &project,
                &execution::signal::SignalKeyring {
                    directory: keyring,
                    key_id,
                },
                actor_id.as_deref(),
            ),
            ExecutionCommand::DocumentRead {
                events,
                execution,
                project,
                evidence_id,
                index,
                keyring,
                key_id,
            } => execution::documents::run_read(
                &events,
                &execution,
                &project,
                &evidence_id,
                index,
                &keyring,
                &key_id,
            ),
            ExecutionCommand::DocumentSave {
                events,
                execution,
                project,
                edit,
                keyring,
                key_id,
            } => execution::documents::run_save(
                &events, &execution, &project, &edit, &keyring, &key_id,
            ),
            ExecutionCommand::Approve {
                events,
                execution,
                node,
                keyring,
                key_id,
                actor_id,
                proposal_digest,
                draft_id,
            } => execution::approve::run(
                &events,
                execution.as_deref(),
                &node,
                keyring.as_deref(),
                key_id.as_deref(),
                actor_id.as_deref(),
                proposal_digest.as_deref(),
                draft_id.as_deref(),
            ),
            ExecutionCommand::Assign {
                events,
                execution,
                node,
                actor_id,
            } => execution::assign::run(&events, execution.as_deref(), &node, &actor_id),
            ExecutionCommand::AmendBudget {
                events,
                execution,
                node,
                seconds,
                at,
            } => execution::amend::run(&events, execution.as_deref(), &node, seconds, at),
            ExecutionCommand::Pause {
                file,
                events,
                execution,
            } => execution::pause::run(&events, execution.as_deref(), file.as_deref()),
            ExecutionCommand::Resume {
                file,
                events,
                fixtures,
                execution,
                keyring,
                key_id,
            } => execution::resume::run(
                file.as_deref(),
                &events,
                fixtures.as_deref(),
                execution.as_deref(),
                keyring.as_deref(),
                key_id.as_deref(),
            ),
            ExecutionCommand::Cancel { events, execution } => {
                execution::cancel::run(&events, execution.as_deref())
            }
            ExecutionCommand::Sweep {
                events,
                execution,
                as_of,
            } => execution::sweep::run(&events, execution.as_deref(), as_of.as_deref()),
            ExecutionCommand::Claim {
                file,
                events,
                execution,
                node,
                wait_seq,
                evidence,
                asserter,
                mode,
            } => execution::claim::run(&execution::claim::Arguments {
                file: &file,
                events: &events,
                execution: execution.as_deref(),
                node: &node,
                wait_seq,
                evidence: &evidence,
                asserter: asserter.as_deref(),
                mode: &mode,
            }),
            ExecutionCommand::Clear {
                file,
                events,
                fixtures,
                execution,
                claim_seq,
                manifest_hash,
                evidence,
                verifier,
            } => execution::clear::run(&execution::clear::Arguments {
                file: &file,
                events: &events,
                fixtures: fixtures.as_deref(),
                execution: execution.as_deref(),
                claim_seq,
                manifest_hash: manifest_hash.as_deref(),
                evidence: evidence.as_deref(),
                verifier: &verifier,
            }),
        },
        TopLevel::Tool(tool_args) => match tool_args.command {
            ToolCommand::Invoke {
                project,
                staging,
                protected,
                request,
                actor,
                capabilities,
                allow_programs,
                tests_runner,
                capture_out,
                keep_workspace,
            } => tool::invoke::run(&tool::invoke::InvokeArguments {
                project,
                staging,
                protected,
                request,
                actor,
                capabilities,
                allow_programs,
                tests_runner,
                capture_out,
                keep_workspace,
            }),
        },
        TopLevel::Gateway(gateway_args) => match gateway_args.command {
            GatewayCommand::Routes { manifest } => gateway::routes::run(&manifest),
            GatewayCommand::Probe {
                manifest,
                route,
                broker,
                keyring,
                key_id,
            } => gateway::probe::run(
                &manifest,
                &route,
                broker.as_deref(),
                keyring.as_deref(),
                key_id.as_deref(),
            ),
            GatewayCommand::Route(route_args) => match route_args.command {
                RouteCommand::Set {
                    manifest,
                    id,
                    provider,
                    base_url,
                    model,
                    credential_ref,
                    disabled,
                    replace,
                    tiers,
                    no_tiers,
                    context_window_tokens,
                } => gateway::route::set(
                    &manifest,
                    &gateway::route::RouteWrite {
                        id,
                        provider,
                        base_url,
                        model,
                        credential_ref,
                        profiles: None,
                        // `--no-tiers` clears; no tier flag at all leaves them unsaid, which a
                        // replace reads as "keep the replaced route's tiers".
                        tiers: if no_tiers {
                            Some(Vec::new())
                        } else if tiers.is_empty() {
                            None
                        } else {
                            Some(tiers)
                        },
                        // ADR-042: unsaid keeps a replaced route's window; `0` clears it.
                        context_window_tokens,
                        // The flag is `--disabled` and the field is `enabled`, so the DEFAULT is
                        // the safe one to type: a route written without saying anything about its
                        // state is on, which is what an operator adding a provider means.
                        enabled: !disabled,
                        replace,
                    },
                ),
            },
            GatewayCommand::Credential(credential_args) => match credential_args.command {
                CredentialCommand::Set {
                    broker,
                    keyring,
                    key_id,
                    reference,
                    provider,
                    usable_by,
                } => gateway::credential::set(
                    &broker, &keyring, &key_id, &reference, &provider, &usable_by,
                ),
                CredentialCommand::Remove {
                    broker,
                    keyring,
                    key_id,
                    reference,
                } => gateway::credential::remove(&broker, &keyring, &key_id, &reference),
            },
            GatewayCommand::Keyring(args) => match args.command {
                KeyringCommand::Init { keyring, key_id } => {
                    gateway::keyring::init(&keyring, &key_id)
                }
            },
            GatewayCommand::Setup(args) => gateway::setup::run(&args),
        },
        TopLevel::Serve(args) => serve::run(&args),
        TopLevel::Mcp(args) => mcp::run(&args),
        TopLevel::WakeWait(args) => wake_wait::run(&args),
        TopLevel::Init(args) => init::run(&args),
        TopLevel::Journeys(args) => {
            let project = args
                .project
                .clone()
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            journeys::run(
                &args.events,
                args.execution.as_deref(),
                &project,
                &args.keyring,
                &args.key_id,
            )
        }
        TopLevel::Journey(args) => journey::run(&args),
        TopLevel::Workspace(args) => workspace::run(&args),
        TopLevel::Update(args) => studio::update_cli(&args),
        TopLevel::Studio(args) => match args.command {
            crate::args::StudioCommand::Start(start) => studio::start(&start),
        },
        TopLevel::Skills(args) => match args.command {
            crate::args::SkillsCommand::Sync {
                host: crate::args::SkillsHost::Codex,
                home,
                dry_run,
            } => skills::sync(home.as_deref(), dry_run),
        },
        TopLevel::Setup(args) => adoption::run(&args),
        TopLevel::Backup(args) => adoption::backup(&args),
        TopLevel::Restore(args) => adoption::restore(&args),
        TopLevel::Keel(args) => match args.command {
            KeelCommand::Index { repo, out } => {
                keel::run(keel_contract_index::Operation::Scan { repo, out })
            }
            KeelCommand::Verify { repo, index } => {
                keel::run(keel_contract_index::Operation::Verify { repo, index })
            }
            KeelCommand::Query { repo, index, term } => {
                keel::run(keel_contract_index::Operation::Query { repo, index, term })
            }
            KeelCommand::Check {
                diff,
                card,
                repo,
                prove_new_tests,
                prove_target_dir,
                prove_timeout_secs,
                prove_command,
                events,
                execution,
                keyring,
                key_id,
            } => keel::check(
                &repo,
                &diff,
                card.as_deref(),
                prove_new_tests.then_some(keel::ProveArgs {
                    target_dir: prove_target_dir,
                    timeout_secs: prove_timeout_secs,
                    command: prove_command,
                }),
                events.map(|events| keel::JourneyRecords {
                    events,
                    execution: execution.unwrap_or_default(),
                    keyring: keyring.unwrap_or_default(),
                    key_id: key_id.unwrap_or_default(),
                }),
            ),
            KeelCommand::Plan {
                task,
                paths,
                promise,
                repo,
                judge_fixture,
                events,
                execution,
                keyring,
                key_id,
            } => keel::plan_with_fixture(
                &repo,
                &task,
                &paths,
                &promise,
                events.map(|events| keel::JourneyRecords {
                    events,
                    execution: execution.unwrap_or_default(),
                    keyring: keyring.unwrap_or_default(),
                    key_id: key_id.unwrap_or_default(),
                }),
                judge_fixture.as_deref(),
            ),
        },
        TopLevel::Quality(args) => match args.command {
            QualityCommand::Certify {
                events,
                execution,
                gate,
            } => quality::run(&events, execution.as_deref(), &gate),
        },
        TopLevel::Gate(args) => match args.command {
            GateCommand::ClassifyRed(args) => gate::classify_red::run(&args),
        },
    }
}

pub(super) struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc::now()
    }
}

pub(super) struct UuidIds;

impl IdGenerator for UuidIds {
    fn next_id(&self, prefix: &'static str) -> String {
        format!("{prefix}-{}", uuid::Uuid::new_v4())
    }
}

pub(super) fn event_store(
    path: &std::path::Path,
) -> Result<LocalEventRepository, EventRepositoryError> {
    match shared_prefix_cache(path) {
        Some(cache) => LocalEventRepository::open_with_prefix_cache(
            path,
            Arc::new(SystemClock),
            Arc::new(UuidIds),
            graphhelm_events::ReadBudget::unbounded(),
            &cache,
        ),
        None => LocalEventRepository::open(path, Arc::new(SystemClock), Arc::new(UuidIds)),
    }
}

/// How long `serve` reuses one journal verification before re-verifying from genesis.
const SHARED_PREFIX_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(30);

/// Set once by `serve`: a long-running process opens a store per request, and without a
/// shared verified prefix every request re-verifies the whole journal (a large journal then
/// misses the Studio's read deadline). One-shot commands leave it off, so their per-open
/// verification is unchanged.
static SHARED_PREFIX_CACHES: std::sync::OnceLock<
    std::sync::Mutex<std::collections::BTreeMap<std::path::PathBuf, graphhelm_events::PrefixCache>>,
> = std::sync::OnceLock::new();

pub(super) fn enable_shared_prefix_cache() {
    let _ = SHARED_PREFIX_CACHES.set(std::sync::Mutex::new(std::collections::BTreeMap::new()));
}

fn shared_prefix_cache(path: &std::path::Path) -> Option<graphhelm_events::PrefixCache> {
    let mut caches = SHARED_PREFIX_CACHES.get()?.lock().ok()?;
    Some(
        caches
            .entry(path.to_path_buf())
            .or_insert_with(|| graphhelm_events::PrefixCache::new(SHARED_PREFIX_MAX_AGE))
            .clone(),
    )
}

/// [`event_store`] for a read that declares a wall-clock budget (#750).
///
/// The budget carries the SAME kind of clock the handle does, so the deadline the open is
/// measured against and the timestamps the handle would write come from one seam.
pub(super) fn budgeted_event_store(
    path: &std::path::Path,
    budget: graphhelm_events::ReadBudget,
) -> Result<LocalEventRepository, EventRepositoryError> {
    match shared_prefix_cache(path) {
        Some(cache) => LocalEventRepository::open_with_prefix_cache(
            path,
            Arc::new(SystemClock),
            Arc::new(UuidIds),
            budget,
            &cache,
        ),
        None => LocalEventRepository::open_within(
            path,
            Arc::new(SystemClock),
            Arc::new(UuidIds),
            budget,
        ),
    }
}

/// The budget one status read declares.
///
/// Built once and shared by both halves of the read - the journal verification inside `open`
/// and the fold - because the deadline is what is being bounded, not each half separately: two
/// budgets started independently would let one read spend the whole budget twice.
pub(super) fn status_read_budget() -> graphhelm_events::ReadBudget {
    graphhelm_events::ReadBudget::starting_now(
        Arc::new(SystemClock),
        chrono::Duration::milliseconds(graphhelm_events::STATUS_READ_BUDGET_MILLIS),
    )
}

pub(super) fn publish_loaded(
    loaded: &graphhelm_schema::LoadedGraph,
    actor: Actor,
) -> Result<GraphVersion, String> {
    GraphVersion::publish(loaded.graph.clone(), None, actor, Utc::now())
        .map_err(|error| error.to_string())
}

pub(super) fn owner(id: &str) -> Actor {
    Actor::new(ActorType::Owner, id)
}

pub(super) fn repository_error(command: &'static str, error: &EventRepositoryError) -> Outcome {
    Outcome::application(
        command,
        graphhelm_protocols::Diagnostic::error(
            error.code(),
            error.to_string(),
            "/events",
            "event-repository",
        ),
    )
}
