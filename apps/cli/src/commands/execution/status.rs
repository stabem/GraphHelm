use std::path::Path;

use super::{Failure, finish, load_projection, render, repository_failure};
use crate::commands::event_store;
use crate::output::Outcome;

const COMMAND: &str = "execution.status";

/// `execution status`: replay only, no appends. The output is the operator's triage view — the
/// same `render` every mutating command replies with, including the untriaged-interruption list.
pub fn run(events: &Path, execution: Option<&str>) -> Outcome {
    finish(COMMAND, execute(events, execution), |value| value)
}

fn execute(events: &Path, execution: Option<&str>) -> Result<serde_json::Value, Failure> {
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let (_, _, projection) = load_projection(&store, execution)?;
    Ok(render(&projection))
}
