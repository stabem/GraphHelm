mod draft;
mod hash;
mod lint;
mod replay;
mod schema;
mod simulate;
mod validate;

use std::sync::Arc;

use chrono::Utc;
use graphhelm_events::{EventStore, JsonlEventStore};
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{
    Actor, ActorType, Clock, EventKind, GraphImported, GraphVersionPublished, IdGenerator, NewEvent,
};

use crate::args::{DraftCommand, GraphCommand, SchemaCommand, TopLevel};
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
            GraphCommand::Replay { events } => replay::run(&events),
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

pub(super) fn event_store(path: &std::path::Path) -> JsonlEventStore {
    JsonlEventStore::new(path, Arc::new(SystemClock), Arc::new(UuidIds))
}

pub(super) fn publish_loaded(
    loaded: &graphhelm_schema::LoadedGraph,
    actor: Actor,
) -> Result<GraphVersion, String> {
    GraphVersion::publish(loaded.graph.clone(), None, actor, Utc::now())
        .map_err(|error| error.to_string())
}

pub(super) fn ensure_imported(
    store: &JsonlEventStore,
    version: &GraphVersion,
    source: &str,
) -> Result<(), String> {
    let stream_id = &version.graph().metadata.execution_id;
    if store
        .read_stream(stream_id)
        .map_err(|error| error.to_string())?
        .is_empty()
    {
        let events = [
            NewEvent {
                idempotency_key: format!("import:{}", version.content_hash()),
                kind: EventKind::GraphImported(GraphImported {
                    source: source.into(),
                }),
            },
            NewEvent {
                idempotency_key: format!("publish:{}", version.content_hash()),
                kind: EventKind::GraphVersionPublished(Box::new(GraphVersionPublished {
                    version: version.to_record(),
                })),
            },
        ];
        store
            .append_batch(stream_id, 1, &events)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub(super) fn owner(id: &str) -> Actor {
    Actor::new(ActorType::Owner, id)
}
