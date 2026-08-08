use std::path::Path;

use crate::commands::event_store;
use crate::output::Outcome;

pub fn run(events: &Path) -> Outcome {
    let store = event_store(events);
    let events = match store.read_all() {
        Ok(events) => events,
        Err(error) => return Outcome::internal("graph.replay", error.to_string()),
    };
    match graphhelm_events::replay(&events) {
        Ok(projection) => Outcome::success(
            "graph.replay",
            serde_json::to_value(projection).expect("projection is serializable"),
        ),
        Err(error) => Outcome::domain(
            "graph.replay",
            vec![graphhelm_protocols::Diagnostic::error(
                error.code(),
                error.to_string(),
                "/",
                events
                    .first()
                    .map_or("event-store", |event| event.stream_id.as_str()),
            )],
        ),
    }
}
