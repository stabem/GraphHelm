//! `execution list` / `GET /v1/executions`: the operator's index of execution streams.
//!
//! WHY THIS EXISTS AS A PUBLIC CONTRACT. Before it, the only way to learn which executions a
//! store holds was `GET /monitor`, an HTML page for a human browser. A programmatic client had
//! to scrape anchors out of that page - a presentation surface pressed into service as an API,
//! which breaks the moment the page's markup changes and which D-040 never promised. This is the
//! smallest correct JSON answer to the same question, on the same authenticated Public Runtime
//! API every other verb uses.
//!
//! ONE TRUTH, NOT A SECOND PROJECTION. Each row is a KEY SUBSET of the exact `render` value
//! `execution status` and `GET /v1/executions/{id}` reply with - same replay, same attention
//! judgement, same instants. A row therefore cannot disagree with the status read that follows
//! it; it can only carry fewer fields. `summary_of` names that subset in one place, and
//! `every_index_row_field_equals_the_status_reply_for_the_same_execution` in
//! `apps/cli/tests/api_http.rs` holds it to that claim by comparing the two surfaces field by
//! field on a live store - it fails on a row whose `attention` is decided anywhere but here.
//!
//! THE CURSOR IS THE EXECUTION ID, EXCLUSIVE. Streams are sorted by `executionId` ascending
//! before slicing, so the order is a property of the data rather than of the store's internal
//! key layout, and `after` names the last id the caller already has. Exclusive, exactly like the
//! events tail's own `after`, so a caller that pages with the last id it saw never re-reads a row
//! and never skips one.

use std::path::Path;

use super::{Failure, argument, finish, render, repository_failure};
use crate::commands::event_store;
use crate::output::Outcome;

const COMMAND: &str = "execution.list";

/// The page size when the caller names none.
pub(crate) const DEFAULT_LIMIT: usize = 20;
/// The largest page a caller may ask for. A larger request is REFUSED rather than silently
/// clamped: a caller that asked for 500 and received 100 cannot tell a clamp from a short store,
/// which is the same "absence reads as an answer" trap the events tail's own bound documents.
pub(crate) const MAX_LIMIT: usize = 100;
/// The longest `after` cursor accepted. An execution id is an `OpaqueId`; anything longer than
/// this was never a stream id, and bounding it keeps an unbounded caller string out of the
/// comparison loop.
pub(crate) const MAX_CURSOR_LEN: usize = 128;

/// Replays every listed stream and returns one summary row per stream, sliced by the cursor.
///
/// Replay per row is deliberate and is what makes `attention` real here. The alternative - a row
/// of bare ids - would force a client to issue one status request per id to answer the only
/// question an index is ever asked ("which of these needs me?"), turning the page into an N+1
/// storm against the same store this function opens once.
pub(crate) fn execute(
    events: &Path,
    after: Option<&str>,
    limit: usize,
) -> Result<serde_json::Value, Failure> {
    if limit == 0 || limit > MAX_LIMIT {
        return Err(argument("limit must be between 1 and 100", "/limit"));
    }
    if let Some(cursor) = after
        && cursor.len() > MAX_CURSOR_LEN
    {
        return Err(argument("after is not a valid execution id", "/after"));
    }

    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let mut streams = store
        .list_streams()
        .map_err(|error| repository_failure(&error))?;
    // ONLY the rows the rest of the API can answer for (#560). `list_streams` enumerates every
    // stream in the repository, including any the generic `events` commands wrote under another
    // workspace or project, while every follow-up verb addresses a stream through
    // `addressable_scope`. Offering the others is worse than omitting them: the caller reads a
    // row as an execution that exists, asks for its status, and is told there is nothing there.
    // Two streams sharing an id across scopes are also indistinguishable in a listing, so the
    // one this filter keeps is the one the id actually resolves to.
    //
    // The filter runs BEFORE the sort and the slice, so `hasMore` and `nextCursor` describe the
    // page the caller can actually walk rather than counting rows that were never offerable.
    streams.retain(|stream| {
        super::addressable_scope(&stream.stream_id).is_ok_and(|scope| scope == stream.scope)
    });
    // Sort by the id the cursor names, not by the store's own key order: the ordering a paging
    // contract promises must be a property of the values the caller can see.
    streams.sort_by(|left, right| left.stream_id.cmp(&right.stream_id));

    let remaining: Vec<_> = streams
        .into_iter()
        .filter(|stream| after.is_none_or(|cursor| stream.stream_id.as_str() > cursor))
        .collect();
    let has_more = remaining.len() > limit;

    let mut rows = Vec::new();
    for stream in remaining.into_iter().take(limit) {
        let history = store
            .read_replay_stream(&stream.scope, &stream.stream_id)
            .map_err(|error| repository_failure(&error))?;
        let head = super::at_sequence(&history);
        // A stream whose fold refuses is REPORTED, not skipped and not fatal: one corrupt stream
        // must not make the whole index unreadable, and a silently dropped row would read as
        // "that execution does not exist". `attention` is `unknown` there because that is the
        // honest verdict when the projection could not be built at all.
        let row = match graphhelm_events::replay(&stream.scope, &stream.stream_id, &history) {
            Ok(projection) => {
                // Budgets derived, not passed (#176).
                let inputs = graphhelm_execution::AttentionInputs::for_surface(
                    &projection,
                    super::node_silence_seconds(&history, chrono::Utc::now()),
                    head,
                );
                let full = render(
                    &projection,
                    &inputs,
                    &super::Liveness::measured(&history),
                    // #134: `list` holds no graph, so no dispatch gate is published here.
                    None,
                );
                let objective = projection
                    .declared_form
                    .as_ref()
                    .and_then(|form| form.objective.as_deref());
                summary_of(&stream.stream_id, &full, head, objective)
            }
            Err(_) => unreadable_row(&stream.stream_id, head),
        };
        rows.push(row);
    }

    // The cursor to pass back as `after`, present only when there IS another page. A cursor
    // published on the last page reads as "there is more" to every client that tests for its
    // presence, which is why `hasMore` and `nextCursor` move together here.
    let next_cursor = if has_more {
        rows.last()
            .and_then(|row| row.get("executionId"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    } else {
        None
    };

    Ok(serde_json::json!({
        "executions": rows,
        "hasMore": has_more,
        "nextCursor": next_cursor,
    }))
}

/// The fixed subset of `render`'s reply that one index row carries.
///
/// Declared as a list rather than open-coded field by field so the subset claim above is
/// checkable in one place. `executionId` falls back to the STREAM id when the projection carries
/// none - an index whose rows cannot be addressed is not an index.
///
/// `objective` (#1083 F7) is the ONE key that is not taken from the status reply: it is the
/// declared form's objective, the same value `execution briefing` publishes, so the Studio rail
/// can name every run by what it was started for without one briefing read per row. Its content
/// is the operator's own words, re-bounded here by `MAX_DECLARED_OBJECTIVE_CHARS`; `null` when the
/// stream declared none (or was recorded before the field existed).
fn summary_of(
    stream_id: &str,
    full: &serde_json::Value,
    head_sequence: Option<u64>,
    objective: Option<&str>,
) -> serde_json::Value {
    const CARRIED: [&str; 7] = [
        "executionId",
        "mode",
        "status",
        "attention",
        "startedAt",
        "lastEventAt",
        // #1064: the executor declared at start rides the row, so `execution list`, the API's
        // index and the Studio rail can mark a demonstration run without opening it.
        "executor",
    ];
    let mut row = serde_json::Map::new();
    for key in CARRIED {
        if let Some(value) = full.get(key) {
            row.insert(key.to_owned(), value.clone());
        }
    }
    if !row
        .get("executionId")
        .is_some_and(serde_json::Value::is_string)
    {
        row.insert("executionId".to_owned(), serde_json::json!(stream_id));
    }
    row.insert(
        "headSequence".to_owned(),
        serde_json::json!(head_sequence.unwrap_or(0)),
    );
    row.insert(
        "objective".to_owned(),
        serde_json::json!(objective.and_then(graphhelm_protocols::bound_declared_text)),
    );
    serde_json::Value::Object(row)
}

/// The row for a stream whose replay refused. Every key `summary_of` publishes is present, so a
/// client never has to branch on shape - only on the value of `status`.
fn unreadable_row(stream_id: &str, head_sequence: Option<u64>) -> serde_json::Value {
    serde_json::json!({
        "executionId": stream_id,
        "mode": serde_json::Value::Null,
        "status": "unreadable",
        "attention": "unknown",
        "startedAt": serde_json::Value::Null,
        "lastEventAt": serde_json::Value::Null,
        "executor": serde_json::Value::Null,
        "headSequence": head_sequence.unwrap_or(0),
        "objective": serde_json::Value::Null,
    })
}

pub fn run(events: &Path, after: Option<&str>, limit: Option<usize>) -> Outcome {
    finish(
        COMMAND,
        execute(events, after, limit.unwrap_or(DEFAULT_LIMIT)),
        |value| value,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status_like(execution: &str, attention: &str) -> serde_json::Value {
        serde_json::json!({
            "executionId": execution,
            "mode": "supervised",
            "status": "running",
            "attention": attention,
            "attentionReasons": [{"kind": "blocked", "node": "implementation"}],
            "nodeStateCounts": {"blocked": 1},
            "nodeStates": {"implementation": "blocked"},
            "customs": {"quarantinedNodes": [], "nodes": {}, "clearances": {}},
            "startedAt": "2026-08-27T00:00:00+00:00",
            "lastEventAt": "2026-08-27T00:01:00+00:00",
            "executor": "fixture",
        })
    }

    /// The subset claim, checked on the value shape rather than on the prose: every key the row
    /// carries is a key the status reply carries, with the identical value.
    #[test]
    fn every_carried_key_holds_the_status_reply_value() {
        let full = status_like("exec-a", "needs_you");
        let row = summary_of("exec-a", &full, Some(9), None);
        for (key, value) in row.as_object().expect("the row is an object") {
            if key == "headSequence" || key == "objective" {
                continue;
            }
            assert_eq!(
                Some(value),
                full.get(key),
                "{key} diverges from the status reply"
            );
        }
        assert_eq!(row["headSequence"], serde_json::json!(9));
        // #1064: the executor is one of the carried keys, so the index can mark a demonstration.
        assert_eq!(row["executor"], serde_json::json!("fixture"));
    }

    /// The row must never carry the heavy detail fields: a client that reads `nodeStateCounts`
    /// off an index row would be reading a field the contract does not promise, and the day the
    /// index stops replaying it would silently vanish.
    #[test]
    fn the_row_omits_the_detail_fields_status_owns() {
        let row = summary_of("exec-a", &status_like("exec-a", "can_sleep"), Some(3), None);
        for absent in [
            "attentionReasons",
            "nodeStateCounts",
            // #133: the per-node map is heavier than the counts and just as much the status
            // reply's to own; the fixture above carries it so this cell is not vacuous.
            "nodeStates",
            // #163: the scan history is the heaviest field of all and just as much the status
            // reply's to own; carried by the fixture above for the same reason as `nodeStates`.
            "customs",
            "untriagedInterruptions",
        ] {
            assert!(
                row.get(absent).is_none(),
                "{absent} must stay on the status reply, not on an index row"
            );
        }
    }

    /// A projection with no execution id still produces an ADDRESSABLE row.
    #[test]
    fn a_projection_without_an_execution_id_falls_back_to_the_stream_id() {
        let mut full = status_like("exec-a", "unknown");
        full["executionId"] = serde_json::Value::Null;
        let row = summary_of("stream-fallback", &full, None, None);
        assert_eq!(row["executionId"], serde_json::json!("stream-fallback"));
        assert_eq!(row["headSequence"], serde_json::json!(0));
    }

    /// The unreadable row is shape-compatible with a readable one: same keys, so a client
    /// branches on `status`, never on which keys exist.
    #[test]
    fn an_unreadable_row_carries_the_same_keys_as_a_readable_one() {
        let readable = summary_of(
            "exec-a",
            &status_like("exec-a", "can_sleep"),
            Some(4),
            Some("Ship it"),
        );
        let unreadable = unreadable_row("exec-a", Some(4));
        let readable_keys: Vec<_> = readable
            .as_object()
            .expect("object")
            .keys()
            .cloned()
            .collect();
        let unreadable_keys: Vec<_> = unreadable
            .as_object()
            .expect("object")
            .keys()
            .cloned()
            .collect();
        assert_eq!(readable_keys, unreadable_keys);
        assert_eq!(unreadable["attention"], serde_json::json!("unknown"));
    }

    /// #1083 F7: the row carries the declared objective, bounded, and `null` when there is none -
    /// so the rail names every run from the index alone.
    #[test]
    fn the_row_carries_the_declared_objective_bounded() {
        let full = status_like("exec-a", "can_sleep");
        let named = summary_of("exec-a", &full, Some(3), Some("  Locate related tests  "));
        assert_eq!(
            named["objective"],
            serde_json::json!("Locate related tests")
        );

        let long = "é".repeat(graphhelm_protocols::MAX_DECLARED_OBJECTIVE_CHARS + 7);
        let bounded = summary_of("exec-a", &full, Some(3), Some(&long));
        assert_eq!(
            bounded["objective"]
                .as_str()
                .map(|text| text.chars().count()),
            Some(graphhelm_protocols::MAX_DECLARED_OBJECTIVE_CHARS)
        );

        // Built, not spelled: a whitespace-run literal trips `source_invariants.rs`'s
        // operator-string guard, which reads every literal in this crate.
        let blank = " ".repeat(3);
        for none in [None, Some(blank.as_str())] {
            assert_eq!(
                summary_of("exec-a", &full, Some(3), none)["objective"],
                serde_json::Value::Null
            );
        }
    }

    /// A stream written under a DIFFERENT workspace is not offered as a row (#560).
    ///
    /// Both streams are real and both are in the store; the difference is that only one of them
    /// can be addressed by `status`, `events`, or any other verb, because those reconstruct the
    /// scope through `addressable_scope`. The foreign row is the defect: an index that offers it
    /// tells the caller an execution exists and then hands back nothing when asked about it.
    ///
    /// **The addressable stream is the control on this cell.** Without it, a filter that dropped
    /// EVERY row -- a mistyped constant, a comparison that can never hold -- would satisfy an
    /// assertion about the foreign id's absence and look like the fix.
    #[test]
    fn a_stream_outside_the_addressable_scope_is_not_offered_as_a_row() {
        use graphhelm_events::PreparedAppend;
        use graphhelm_protocols::{
            ActorId, EventKind, ExecutionId, ExecutionMode, ExecutionStarted, NewEvent, OpaqueId,
            PersistedActor, PersistedActorType, ProjectId, RepositoryScope, Sensitivity, WireHash,
            WorkspaceId,
        };

        fn addressable_scope_or_panic(execution: &str) -> RepositoryScope {
            match super::super::addressable_scope(execution) {
                Ok(scope) => scope,
                Err(_) => panic!("{execution} is not addressable, so this cell has no subject"),
            }
        }

        fn started(directory: &Path, scope: RepositoryScope, stream: &str) {
            let store = crate::commands::event_store(directory).expect("the store opens");
            let execution_id = OpaqueId::parse(stream).expect("a wire-safe id");
            let event = NewEvent::new(
                OpaqueId::parse(format!("started-{stream}")).expect("a wire-safe key"),
                PersistedActor::new(
                    PersistedActorType::System,
                    ActorId::parse("system-test").expect("a wire-safe actor"),
                ),
                Sensitivity::Internal,
                EventKind::ExecutionStarted(ExecutionStarted {
                    execution_id: execution_id.clone(),
                    graph_version: 1,
                    graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64)))
                        .expect("a wire hash"),
                    mode: ExecutionMode::Autopilot,
                }),
                vec![],
                vec![],
            );
            let next = store
                .next_sequence(&scope, execution_id.as_str())
                .expect("a fresh stream starts at 1");
            let request =
                PreparedAppend::new(scope, execution_id, next, vec![event], vec![], vec![])
                    .expect("the append is well-formed");
            store.append_atomic(&request).expect("the append lands");
        }

        let directory = tempfile::tempdir().expect("a temp directory");
        started(
            directory.path(),
            addressable_scope_or_panic("exec-addressable"),
            "exec-addressable",
        );
        started(
            directory.path(),
            RepositoryScope::new(
                WorkspaceId::parse("workspace-elsewhere").expect("a wire-safe workspace"),
                ProjectId::parse("project-elsewhere").expect("a wire-safe project"),
                Some(ExecutionId::parse("exec-foreign").expect("a wire-safe execution")),
            ),
            "exec-foreign",
        );

        let page = match execute(directory.path(), None, DEFAULT_LIMIT) {
            Ok(page) => page,
            // `Failure` carries no `Debug`, so the refusal is named rather than unwrapped: a
            // panic reading "called unwrap on an Err" would not say WHICH refusal this was.
            Err(_) => panic!("the index refused a store it wrote itself"),
        };
        let ids: Vec<&str> = page["executions"]
            .as_array()
            .expect("the page carries rows")
            .iter()
            .filter_map(|row| row["executionId"].as_str())
            .collect();
        assert_eq!(
            ids,
            vec!["exec-addressable"],
            "the index must offer exactly the rows the rest of the API can address"
        );
        assert_eq!(page["hasMore"], serde_json::json!(false));
        assert_eq!(page["nextCursor"], serde_json::Value::Null);
    }
}
