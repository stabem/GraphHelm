mod development;
mod draft;
mod events;
mod execution;
mod extension;
mod gateway;
mod hash;
mod lint;
mod mcp;
mod quality;
pub(crate) mod remediation;
mod replay;
mod schema;
mod serve;
mod simulate;
mod tool;
mod validate;
mod wake_wait;

use std::sync::Arc;

use chrono::Utc;
use graphhelm_events::{EventRepositoryError, LocalEventRepository};
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{Actor, ActorType, Clock, IdGenerator};

use crate::args::{
    CredentialCommand, DevelopmentCommand, DraftCommand, EventsCommand, ExecutionCommand,
    ExtensionCommand, GatewayCommand, GraphCommand, QualityCommand, SchemaCommand, ToolCommand,
    TopLevel,
};
use crate::output::Outcome;

pub fn run(command: TopLevel) -> Outcome {
    match command {
        TopLevel::Graph(graph) => match graph.command {
            GraphCommand::Validate { file } => validate::run(&file),
            GraphCommand::Lint { file } => lint::run(&file),
            GraphCommand::Hash { file } => hash::run(&file),
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
            } => execution::start::run(
                &file,
                &events,
                fixtures.as_deref(),
                &mode,
                execution.as_deref(),
            ),
            ExecutionCommand::Status {
                events,
                execution,
                html,
            } => execution::status::run(&events, execution.as_deref(), html.as_deref()),
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
                &keyring,
                &key_id,
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
            ExecutionCommand::Pause { events, execution } => {
                execution::pause::run(&events, execution.as_deref())
            }
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
        },
        TopLevel::Serve(args) => serve::run(&args),
        TopLevel::Mcp(args) => mcp::run(&args),
        TopLevel::WakeWait(args) => wake_wait::run(&args),
        TopLevel::Quality(args) => match args.command {
            QualityCommand::Certify {
                events,
                execution,
                gate,
            } => quality::run(&events, execution.as_deref(), &gate),
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
