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
mod keel;
mod lint;
mod mcp;
mod quality;
pub(crate) mod remediation;
mod replay;
mod schema;
mod secret_file;
mod serve;
mod simulate;
mod tool;
mod topology;
mod validate;
mod wake_wait;

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
            DevelopmentCommand::MemoryPropose => development::run_memory_propose(),
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
            } => execution::start::run(
                &file,
                &events,
                fixtures.as_deref(),
                &mode,
                execution.as_deref(),
                held,
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
            ExecutionCommand::Briefing { events, execution } => {
                execution::briefing::run(&events, execution.as_deref())
            }
            ExecutionCommand::Signal {
                events,
                execution,
                signal,
                evidence_out,
                keyring,
                key_id,
            } => execution::signal::run(
                &events,
                execution.as_deref(),
                &signal,
                &evidence_out,
                keyring.as_deref(),
                key_id.as_deref(),
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
            } => execution::approve::run(&events, execution.as_deref(), &node),
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
            } => execution::resume::run(&file, &events, fixtures.as_deref(), execution.as_deref()),
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
                } => gateway::route::set(
                    &manifest,
                    &gateway::route::RouteWrite {
                        id,
                        provider,
                        base_url,
                        model,
                        credential_ref,
                        profiles: None,
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
    LocalEventRepository::open(path, Arc::new(SystemClock), Arc::new(UuidIds))
}

/// [`event_store`] for a read that declares a wall-clock budget (#750).
///
/// The budget carries the SAME kind of clock the handle does, so the deadline the open is
/// measured against and the timestamps the handle would write come from one seam.
pub(super) fn budgeted_event_store(
    path: &std::path::Path,
    budget: graphhelm_events::ReadBudget,
) -> Result<LocalEventRepository, EventRepositoryError> {
    LocalEventRepository::open_within(path, Arc::new(SystemClock), Arc::new(UuidIds), budget)
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
