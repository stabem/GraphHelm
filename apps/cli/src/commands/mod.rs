mod draft;
mod events;
mod hash;
mod lint;
mod replay;
mod schema;
mod simulate;
mod validate;

use std::sync::Arc;

use chrono::Utc;
use graphhelm_events::{EventRepositoryError, LocalEventRepository};
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{Actor, ActorType, Clock, IdGenerator};

use crate::args::{DraftCommand, EventsCommand, GraphCommand, SchemaCommand, TopLevel};
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
            SchemaCommand::View {
                catalog,
                schema: name,
            } => schema::view::run(&catalog, &name),
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
            EventsCommand::Backup { config, output } => {
                events::backup::run(events::backup::Request {
                    config: config.as_deref(),
                    output: &output,
                })
            }
            EventsCommand::Restore { config, archive } => {
                events::restore::run(events::restore::Request {
                    config: config.as_deref(),
                    archive: &archive,
                })
            }
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
