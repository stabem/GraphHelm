//! The row-to-artifact mapping the generator rides (#225 wiring): mechanical, column-by-NAME,
//! and refusing where a guess would fabricate. The provider's wire shape is the measured one
//! (codebase-memory-mcp 0.10.8, `format:"json"`): `cols` names beside per-row value lists.

use graphhelm_development_benchmark::{BenchmarkRefusal, artifact_from_search_rows};
use graphhelm_protocols::DeclaredLimits;

fn limits() -> DeclaredLimits {
    DeclaredLimits {
        max_results: 5,
        max_pages: 8,
        max_bytes: 1_000_000,
        max_tokens: 250_000,
    }
}

fn cols() -> Vec<String> {
    ["qn", "label", "file", "lines", "rank"]
        .iter()
        .map(|name| (*name).to_owned())
        .collect()
}

fn row(qn: &str, file: &str, lines: &str) -> Vec<serde_json::Value> {
    vec![
        serde_json::json!(qn),
        serde_json::json!("Function"),
        serde_json::json!(file),
        serde_json::json!(lines),
        serde_json::json!(-20.0),
    ]
}

/// The happy mapping: file and lines columns by name, order preserved, wire fields carried.
#[test]
fn rows_map_to_hits_by_named_columns() {
    let rows = vec![
        row("p.a.open", "core/events/src/local.rs", "3546-3552"),
        row("p.b.code", "core/events/src/store.rs", "60-80"),
    ];

    let artifact = artifact_from_search_rows(
        "case-a",
        "which function decides?",
        "sha256-g1",
        &cols(),
        &rows,
        &limits(),
    )
    .expect("a well-formed page maps");

    assert_eq!(artifact.case_id, "case-a");
    assert_eq!(artifact.query, "which function decides?");
    assert_eq!(artifact.repo_snapshot, "sha256-g1");
    assert_eq!(artifact.index_generation, "sha256-g1");
    assert_eq!(artifact.hits.len(), 2);
    assert_eq!(artifact.hits[0].path, "core/events/src/local.rs");
    assert_eq!(artifact.hits[0].lines.as_deref(), Some("3546-3552"));
    assert_eq!(artifact.hits[1].path, "core/events/src/store.rs");
    assert_eq!(artifact.hits[1].lines.as_deref(), Some("60-80"));
    assert_eq!(
        artifact.coverage, "partial",
        "best-effort coverage, whatever has_more said -- see the dedicated cell for why"
    );
    assert_eq!(artifact.max_results, 5);
}

/// Two functions from one file at the same range collapse; a different range stays -- the pair
/// is the identity, not the path.
#[test]
fn duplicate_path_and_range_pairs_collapse_to_first_appearance() {
    let rows = vec![
        row("p.a.one", "core/x.rs", "1-10"),
        row("p.a.two", "core/x.rs", "1-10"),
        row("p.a.three", "core/x.rs", "20-30"),
    ];

    let artifact =
        artifact_from_search_rows("case-b", "q", "sha256-g1", &cols(), &rows, &limits()).unwrap();

    assert_eq!(artifact.hits.len(), 2);
    assert_eq!(artifact.hits[0].lines.as_deref(), Some("1-10"));
    assert_eq!(artifact.hits[1].lines.as_deref(), Some("20-30"));
}

/// Rows beyond max_results are never consulted -- the bound is the artifact's own declaration.
#[test]
fn rows_beyond_the_declared_bound_are_not_consulted() {
    let rows: Vec<Vec<serde_json::Value>> = (0..8)
        .map(|index| row(&format!("p.f{index}"), &format!("src/f{index}.rs"), "1-2"))
        .collect();

    let artifact =
        artifact_from_search_rows("case-c", "q", "sha256-g1", &cols(), &rows, &limits()).unwrap();

    assert_eq!(artifact.hits.len(), 5);
}

/// A page without a `file` column refuses BY NAME: a positional or last-string fallback would
/// fabricate paths the moment the provider reorders its columns.
#[test]
fn a_page_without_a_file_column_refuses_instead_of_guessing() {
    let cols: Vec<String> = ["qn", "label", "rank"]
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    let rows = vec![vec![
        serde_json::json!("p.a.open"),
        serde_json::json!("Function"),
        serde_json::json!(-20.0),
    ]];

    let refusal = artifact_from_search_rows("case-d", "q", "sha256-g1", &cols, &rows, &limits())
        .expect_err("a page with no file column was mapped anyway");

    match refusal {
        BenchmarkRefusal::Unreadable { detail } => {
            assert!(
                detail.contains("file"),
                "the refusal names the missing column, got: {detail}"
            );
        }
        other => panic!("readability did not decide this: {other:?}"),
    }
}

/// A missing lines cell is a WHOLE-FILE hit (`lines: None`), not a refusal -- the artifact
/// schema keeps that case on purpose for hits that genuinely are whole-file.
#[test]
fn a_null_lines_cell_is_a_whole_file_hit() {
    let mut no_lines = row("p.a.open", "core/x.rs", "unused");
    no_lines[3] = serde_json::Value::Null;

    let artifact =
        artifact_from_search_rows("case-e", "q", "sha256-g1", &cols(), &[no_lines], &limits())
            .unwrap();

    assert_eq!(artifact.hits.len(), 1);
    assert_eq!(artifact.hits[0].lines, None);
}

/// Codex P2 (post-rebase), and it caught the generator contradicting the adapter I wrote myself.
///
/// `has_more: false` was frozen as `complete` -- but the production adapter states the opposite
/// at `provider.rs:156-160` and always reports `Partial`, because `has_more == false` means the
/// PAGE ended, never that the SEARCH was exhaustive. A best-effort provider cannot support a
/// completeness claim, and the generator was manufacturing one and freezing it.
///
/// The consequence is not cosmetic: a zero-result query frozen as `complete` reaches
/// `compile_plan` as VERIFIED ABSENCE -- a fabricated "this does not exist in the repository",
/// which is the single claim this whole mechanism exists to refuse. And it would be frozen,
/// so every future run would measure against it.
///
/// So coverage is `partial`, unconditionally — and `has_more` no longer reaches this function at
/// all, because a parameter that decides nothing is theatre. It stays required at the GENERATOR
/// (`has_more_of`, cell below) for a different reason, stated there: a provider that will not
/// describe its own page is a provider whose answer should not be frozen.
///
/// One cell, not a pair: the pair this replaced fed `true` and `false` to a parameter that no
/// longer exists, so both halves would now be the same call — a comparison of a thing with
/// itself, green forever and about nothing.
#[test]
fn best_effort_coverage_is_partial_unconditionally() {
    let artifact = artifact_from_search_rows(
        "case-f",
        "q",
        "sha256-g1",
        &cols(),
        &[row("p.a.open", "core/x.rs", "1-10")],
        &limits(),
    )
    .unwrap();

    assert_eq!(
        artifact.coverage, "partial",
        "a page that ended is not a search that was exhaustive: the adapter reports Partial on \
         every call and the frozen artifact must not claim more than the provider did"
    );
}

/// Codex P1-2, and the reason it outranks an ordinary default: these artifacts are FROZEN under
/// a digest and become the corpus every future run measures against. A page whose `has_more` the
/// provider did not state, silently read as `false`, becomes `coverage: "complete"` — and that
/// overclaim is then PERMANENT. Absence read as a negative at the exact instant the value stops
/// being revisable.
///
/// So a missing or non-boolean `has_more` REFUSES. "The provider did not say" and "the provider
/// said no" are different facts, and only one of them may freeze.
#[test]
fn a_page_whose_has_more_is_absent_or_malformed_refuses() {
    for hostile in [
        serde_json::json!({"total": 2, "cols": ["file"], "rows": []}),
        serde_json::json!({"total": 2, "cols": ["file"], "rows": [], "has_more": "false"}),
        serde_json::json!({"total": 2, "cols": ["file"], "rows": [], "has_more": null}),
        serde_json::json!({"total": 2, "cols": ["file"], "rows": [], "has_more": 0}),
    ] {
        let refusal = graphhelm_development_benchmark::has_more_of(&hostile, "case-x")
            .expect_err("a page that did not state has_more was read as complete");
        match refusal {
            BenchmarkRefusal::Unreadable { detail } => assert!(
                detail.contains("has_more"),
                "the refusal names the field that was not stated, got: {detail}"
            ),
            other => panic!("readability did not decide this: {other:?}"),
        }
    }
}

/// POSITIVE CONTROL: both stated values are read, and neither is invented.
#[test]
fn a_stated_has_more_is_read_as_stated() {
    let more = serde_json::json!({"has_more": true});
    let done = serde_json::json!({"has_more": false});

    assert!(graphhelm_development_benchmark::has_more_of(&more, "case-x").unwrap());
    assert!(!graphhelm_development_benchmark::has_more_of(&done, "case-x").unwrap());
}

/// The doc promises columns are read by NAME "never by position -- ... a fallback that grabs the
/// last string cell fabricates paths the moment the provider REORDERS its columns", and every
/// other cell here feeds the canonical order, so the promise was never exercised: the fixture and
/// the claim shared a shape. This one reorders.
#[test]
fn a_reordered_column_page_maps_to_the_same_hits() {
    let canonical = artifact_from_search_rows(
        "case-r",
        "q",
        "sha256-g1",
        &cols(),
        &[row("p.a.open", "core/x.rs", "1-10")],
        &limits(),
    )
    .unwrap();

    // Same page, columns shuffled: file and lines move away from indices 2 and 3 entirely.
    let shuffled_cols: Vec<String> = ["rank", "lines", "qn", "file", "label"]
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    let shuffled_row = vec![
        serde_json::json!(-20.0),
        serde_json::json!("1-10"),
        serde_json::json!("p.a.open"),
        serde_json::json!("core/x.rs"),
        serde_json::json!("Function"),
    ];

    let reordered = artifact_from_search_rows(
        "case-r",
        "q",
        "sha256-g1",
        &shuffled_cols,
        &[shuffled_row],
        &limits(),
    )
    .unwrap();

    assert_eq!(
        reordered.hits, canonical.hits,
        "reading by name means a reordered page yields identical hits; a positional read would \
         have produced `-20` or `Function` as a path here"
    );
}

/// Codex P2 pair: silent `filter_map` drops in the generator's page decoding.
///
/// A non-string entry in `cols` was removed while the ROW arrays kept their length, shifting the
/// computed `file` index onto a different cell -- so a label or a rank could freeze as a
/// repository path. A non-array entry in `rows` was dropped outright, silently shrinking the hit
/// set that recall is measured from.
///
/// Both are the same defect: a malformed page being repaired into a plausible one. The page is
/// decoded by shape now, and anything that does not fit refuses.
#[test]
fn a_page_with_malformed_columns_or_rows_refuses_instead_of_dropping() {
    use graphhelm_development_benchmark::page_of;

    let hostile = [
        serde_json::json!({"cols": ["qn", 7, "file"], "rows": [], "has_more": false}),
        serde_json::json!({"cols": ["file"], "rows": [["a.rs"], "not-an-array"], "has_more": false}),
        serde_json::json!({"cols": "file", "rows": [], "has_more": false}),
        serde_json::json!({"rows": [], "has_more": false}),
        serde_json::json!({"cols": ["file"], "has_more": false}),
        // Duplicate names make the mapping ambiguous, and `position` would resolve it by silently
        // preferring the first -- freezing whichever string came first as a repository path.
        serde_json::json!({"total": 0, "cols": ["file", "file"], "rows": [], "has_more": false}),
        serde_json::json!({"total": 0, "cols": ["qn", "lines", "file", "lines"], "rows": [], "has_more": false}),
    ];

    for page in hostile {
        let refusal = page_of(&page, "case-x")
            .expect_err("a malformed provider page was repaired into a plausible one");
        assert!(
            matches!(refusal, BenchmarkRefusal::Unreadable { .. }),
            "shape must decide this: {refusal:?}"
        );
    }
}

/// POSITIVE CONTROL: a well-formed page decodes, and the column order is preserved exactly as the
/// provider sent it (the index computation depends on it).
#[test]
fn a_well_formed_page_decodes_with_its_columns_in_order() {
    use graphhelm_development_benchmark::page_of;

    let page = serde_json::json!({
        "total": 1,
        "cols": ["qn", "label", "file", "lines", "rank"],
        "rows": [["p.a", "Function", "core/x.rs", "1-10", -20.0]],
        "has_more": false,
    });

    let (cols, rows) = page_of(&page, "case-x").expect("a well-formed page decodes");

    assert_eq!(cols, vec!["qn", "label", "file", "lines", "rank"]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].len(), 5);
}
