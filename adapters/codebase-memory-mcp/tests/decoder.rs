use graphhelm_codebase_memory_mcp::{
    CoverageConfidence, DecodeError, DecodeLimits, IdentityEvidence, PagePosition,
    SearchPagination, TransportReceipt, decode_index_coverage, decode_search_graph,
};
use graphhelm_tool_broker::effect::IsolationTier;
use graphhelm_tool_broker::record::{ToolCallRecord, ToolDisposition};
use serde_json::{Value, json};

const SEARCH_FIXTURE: &[u8] = include_bytes!("fixtures/search_graph.json");
const GROUPED_SEARCH_FIXTURE: &[u8] = include_bytes!("fixtures/search_graph_grouped.json");
const COVERAGE_FIXTURE: &[u8] = include_bytes!("fixtures/check_index_coverage.json");

#[test]
fn decodes_real_search_graph_structured_content() {
    let page = decode_search_graph(SEARCH_FIXTURE, DecodeLimits::default()).unwrap();

    assert_eq!(page.total(), 43);
    assert_eq!(page.columns(), ["qn", "label", "file", "lines", "rank"]);
    assert_eq!(page.rows().len(), 1);
    assert_eq!(page.rows()[0][1], Value::String("Struct".to_owned()));
    assert!(page.has_more());
}

#[test]
fn decodes_real_grouped_search_graph_structured_content() {
    let page = decode_search_graph(GROUPED_SEARCH_FIXTURE, DecodeLimits::default()).unwrap();

    assert_eq!(page.total(), 1);
    assert!(page.rows().is_empty());
    assert_eq!(page.groups().len(), 1);
    assert_eq!(
        page.groups()[0].prefix(),
        "F-github-GraphHelm.core.tool-broker.src.record"
    );
    assert_eq!(page.groups()[0].file(), "core/tool-broker/src/record.rs");
    assert!(!page.has_more());
}

#[test]
fn decodes_real_coverage_shape_without_upgrading_best_effort() {
    let page = decode_index_coverage(COVERAGE_FIXTURE, DecodeLimits::default()).unwrap();
    let seal = page.seal().unwrap();

    assert_eq!(seal.project(), "F-github-GraphHelm");
    assert_eq!(seal.generation(), "2026-08-26T14:18:14Z");
    assert_eq!(seal.confidence(), CoverageConfidence::Unknown);
    assert_eq!(page.paths().len(), 1);
    assert_eq!(page.paths()[0].coverage.len(), 1);
    assert_eq!(page.paths()[0].coverage[0].ranges[0].start, 39);
    assert_eq!(page.scopes().len(), 1);
    assert!(!page.has_more());
}

#[test]
fn seal_rejects_a_whitespace_only_project() {
    let body = json!({
        "structuredContent": {
            "project": " \t ",
            "signal": "best_effort",
            "metadata": {"generation": "g1"},
            "paths": [],
            "scopes": []
        },
        "isError": false
    });
    let page = decode_index_coverage(&serde_json::to_vec(&body).unwrap(), DecodeLimits::default())
        .unwrap();

    assert_eq!(page.seal().unwrap_err().code(), "CBM_SEAL_UNAVAILABLE");
}

#[test]
fn seal_rejects_a_whitespace_only_generation() {
    let body = json!({
        "structuredContent": {
            "project": "p1",
            "signal": "best_effort",
            "metadata": {"generation": "\r\n"},
            "paths": [],
            "scopes": []
        },
        "isError": false
    });
    let page = decode_index_coverage(&serde_json::to_vec(&body).unwrap(), DecodeLimits::default())
        .unwrap();

    assert_eq!(page.seal().unwrap_err().code(), "CBM_SEAL_UNAVAILABLE");
}

#[test]
fn refuses_missing_malformed_and_error_structured_content() {
    let limits = DecodeLimits::default();
    let cases = [
        (json!({"isError": false}), "CBM_STRUCTURED_CONTENT_MISSING"),
        (
            json!({"structuredContent": "not-an-object", "isError": false}),
            "CBM_STRUCTURED_CONTENT_INVALID",
        ),
        (
            json!({"structuredContent": {"error": "boom"}, "isError": true}),
            "CBM_PROVIDER_ERROR",
        ),
    ];

    for (input, code) in cases {
        let bytes = serde_json::to_vec(&input).unwrap();
        let error = decode_search_graph(&bytes, limits).unwrap_err();
        assert_eq!(error.code(), code);
    }
}

#[test]
fn enforces_bytes_nesting_rows_and_columns_before_accepting_output() {
    let too_small = DecodeLimits {
        max_result_bytes: SEARCH_FIXTURE.len() - 1,
        ..DecodeLimits::default()
    };
    assert_eq!(
        decode_search_graph(SEARCH_FIXTURE, too_small)
            .unwrap_err()
            .code(),
        "CBM_RESULT_BYTES_EXCEEDED"
    );

    let nested = json!({
        "structuredContent": {"total": 0, "cols": [], "rows": [[[[[0]]]]], "has_more": false},
        "isError": false
    });
    let nested_limits = DecodeLimits {
        max_nesting_depth: 4,
        ..DecodeLimits::default()
    };
    assert_eq!(
        decode_search_graph(&serde_json::to_vec(&nested).unwrap(), nested_limits)
            .unwrap_err()
            .code(),
        "CBM_JSON_NESTING_EXCEEDED"
    );

    let two_rows = json!({
        "structuredContent": {"total": 2, "cols": ["name"], "rows": [["a"], ["b"]], "has_more": false},
        "isError": false
    });
    let row_limits = DecodeLimits {
        max_rows_per_page: 1,
        ..DecodeLimits::default()
    };
    assert_eq!(
        decode_search_graph(&serde_json::to_vec(&two_rows).unwrap(), row_limits)
            .unwrap_err()
            .code(),
        "CBM_PAGE_ROWS_EXCEEDED"
    );

    let two_columns = json!({
        "structuredContent": {"total": 1, "cols": ["name", "file"], "rows": [["a", "a.rs"]], "has_more": false},
        "isError": false
    });
    let column_limits = DecodeLimits {
        max_columns: 1,
        ..DecodeLimits::default()
    };
    assert_eq!(
        decode_search_graph(&serde_json::to_vec(&two_columns).unwrap(), column_limits)
            .unwrap_err()
            .code(),
        "CBM_PAGE_COLUMNS_EXCEEDED"
    );

    let compound_cell = json!({
        "structuredContent": {"total": 1, "cols": ["name"], "rows": [[{"nested":"value"}]], "has_more": false},
        "isError": false
    });
    assert_eq!(
        decode_search_graph(
            &serde_json::to_vec(&compound_cell).unwrap(),
            DecodeLimits::default(),
        )
        .unwrap_err()
        .code(),
        "CBM_STRUCTURED_CONTENT_INVALID"
    );

    let blank_group_identity = json!({
        "structuredContent": {"total": 1, "cols": ["name"], "groups": [{"qn_prefix":" ", "file":"", "rows":[["x"]]}], "has_more": false},
        "isError": false
    });
    assert_eq!(
        decode_search_graph(
            &serde_json::to_vec(&blank_group_identity).unwrap(),
            DecodeLimits::default(),
        )
        .unwrap_err()
        .code(),
        "CBM_STRUCTURED_CONTENT_INVALID"
    );

    let missing_group_identity = json!({
        "structuredContent": {"total": 1, "cols": ["name"], "groups": [{"rows":[["x"]]}], "has_more": false},
        "isError": false
    });
    assert_eq!(
        decode_search_graph(
            &serde_json::to_vec(&missing_group_identity).unwrap(),
            DecodeLimits::default(),
        )
        .unwrap_err()
        .code(),
        "CBM_STRUCTURED_CONTENT_INVALID"
    );

    let empty_group = json!({
        "structuredContent": {"total": 0, "cols": ["name"], "groups": [{"qn_prefix":"p", "file":"x.rs", "rows":[]}], "has_more": false},
        "isError": false
    });
    assert_eq!(
        decode_search_graph(
            &serde_json::to_vec(&empty_group).unwrap(),
            DecodeLimits::default(),
        )
        .unwrap_err()
        .code(),
        "CBM_STRUCTURED_CONTENT_INVALID"
    );
}

#[test]
fn grouped_search_requires_an_exact_returned_count() {
    let cases = [
        json!({
            "structuredContent": {"total": 1, "cols": ["name"], "groups": [{"qn_prefix":"p", "file":"x.rs", "rows":[["x"]]}], "has_more": false},
            "isError": false
        }),
        json!({
            "structuredContent": {"total": 2, "count": 2, "cols": ["name"], "groups": [{"qn_prefix":"p", "file":"x.rs", "rows":[["x"]]}], "has_more": true},
            "isError": false
        }),
        json!({
            "structuredContent": {"total": 0, "count": 1, "cols": ["name"], "groups": [{"qn_prefix":"p", "file":"x.rs", "rows":[["x"]]}], "has_more": false},
            "isError": false
        }),
    ];

    for body in cases {
        assert_eq!(
            decode_search_graph(&serde_json::to_vec(&body).unwrap(), DecodeLimits::default(),)
                .unwrap_err()
                .code(),
            "CBM_STRUCTURED_CONTENT_INVALID"
        );
    }
}

#[test]
fn flat_search_rejects_a_contradictory_returned_count() {
    let body = json!({
        "structuredContent": {
            "total": 2,
            "count": 2,
            "cols": ["name"],
            "rows": [["x"]],
            "has_more": true
        },
        "isError": false
    });

    assert_eq!(
        decode_search_graph(&serde_json::to_vec(&body).unwrap(), DecodeLimits::default(),)
            .unwrap_err()
            .code(),
        "CBM_STRUCTURED_CONTENT_INVALID"
    );
}

#[test]
fn pagination_rejects_drift_loops_limits_and_unfinished_results() {
    let first = decode_search_graph(SEARCH_FIXTURE, DecodeLimits::default()).unwrap();
    let mut changed_total = search_page(44, &["qn", "label", "file", "lines", "rank"], false);
    let mut pagination = SearchPagination::new(DecodeLimits::default());
    pagination
        .push(PagePosition::Offset(0), first.clone())
        .unwrap();
    assert_eq!(
        pagination
            .push(PagePosition::Offset(1), changed_total.clone())
            .unwrap_err()
            .code(),
        "CBM_TOTAL_DRIFT"
    );

    changed_total = search_page(43, &["different"], false);
    let mut pagination = SearchPagination::new(DecodeLimits::default());
    pagination
        .push(PagePosition::Cursor("page-a".to_owned()), first.clone())
        .unwrap();
    assert_eq!(
        pagination
            .push(PagePosition::Cursor("page-b".to_owned()), changed_total)
            .unwrap_err()
            .code(),
        "CBM_COLUMNS_DRIFT"
    );

    let mut pagination = SearchPagination::new(DecodeLimits::default());
    pagination
        .push(PagePosition::Offset(0), first.clone())
        .unwrap();
    assert_eq!(
        pagination
            .push(PagePosition::Offset(0), first.clone())
            .unwrap_err()
            .code(),
        "CBM_PAGE_POSITION_LOOP"
    );

    let mut pagination = SearchPagination::new(DecodeLimits::default());
    pagination
        .push(PagePosition::Cursor("same".to_owned()), first.clone())
        .unwrap();
    assert_eq!(
        pagination
            .push(PagePosition::Cursor("same".to_owned()), first.clone())
            .unwrap_err()
            .code(),
        "CBM_PAGE_POSITION_LOOP"
    );

    let one_page = DecodeLimits {
        max_pages: 1,
        ..DecodeLimits::default()
    };
    let mut pagination = SearchPagination::new(one_page);
    pagination
        .push(PagePosition::Offset(0), first.clone())
        .unwrap();
    assert_eq!(
        pagination
            .push(
                PagePosition::Offset(1),
                search_page(43, &["qn", "label", "file", "lines", "rank"], false),
            )
            .unwrap_err()
            .code(),
        "CBM_PAGE_LIMIT_EXCEEDED"
    );

    let byte_limits = DecodeLimits {
        max_total_bytes: SEARCH_FIXTURE.len(),
        ..DecodeLimits::default()
    };
    let mut pagination = SearchPagination::new(byte_limits);
    pagination
        .push(PagePosition::Offset(0), first.clone())
        .unwrap();
    assert_eq!(
        pagination
            .push(
                PagePosition::Offset(1),
                search_page(43, &["qn", "label", "file", "lines", "rank"], false),
            )
            .unwrap_err()
            .code(),
        "CBM_PAGINATION_BYTES_EXCEEDED"
    );

    let mut pagination = SearchPagination::new(DecodeLimits::default());
    pagination.push(PagePosition::Offset(0), first).unwrap();
    assert_eq!(
        pagination.finish().unwrap_err().code(),
        "CBM_PAGINATION_UNFINISHED"
    );

    let mut pagination = SearchPagination::new(DecodeLimits::default());
    pagination
        .push(PagePosition::Offset(0), search_page(43, &["name"], false))
        .unwrap();
    assert_eq!(
        pagination.finish().unwrap_err().code(),
        "CBM_RESULT_COUNT_MISMATCH"
    );

    let first = search_page_with_names(1, &["a"], true);
    let second = search_page_with_names(1, &["b"], false);
    let mut pagination = SearchPagination::new(DecodeLimits::default());
    pagination.push(PagePosition::Offset(0), first).unwrap();
    assert_eq!(
        pagination
            .push(PagePosition::Offset(1), second)
            .unwrap_err()
            .code(),
        "CBM_RESULT_COUNT_MISMATCH"
    );

    let mut pagination = SearchPagination::new(DecodeLimits::default());
    pagination
        .push(
            PagePosition::Offset(0),
            search_page_with_names(2, &["a"], true),
        )
        .unwrap();
    assert_eq!(
        pagination
            .push(
                PagePosition::Cursor("retry-position".to_owned()),
                search_page_with_names(3, &["b"], false),
            )
            .unwrap_err()
            .code(),
        "CBM_TOTAL_DRIFT"
    );
    pagination
        .push(
            PagePosition::Cursor("retry-position".to_owned()),
            search_page_with_names(2, &["b"], false),
        )
        .unwrap();
    assert_eq!(pagination.finish().unwrap().len(), 2);
}

#[test]
fn pagination_reapplies_its_own_limits_to_pages_decoded_elsewhere() {
    let body = json!({
        "structuredContent": {"total": 2, "cols": ["name", "file"], "rows": [["a", "a.rs"], ["b", "b.rs"]], "has_more": false},
        "isError": false
    });
    let encoded = serde_json::to_vec(&body).unwrap();
    let page = decode_search_graph(&encoded, DecodeLimits::default()).unwrap();

    let row_limits = DecodeLimits {
        max_rows_per_page: 1,
        ..DecodeLimits::default()
    };
    assert_eq!(
        SearchPagination::new(row_limits)
            .push(PagePosition::Offset(0), page.clone())
            .unwrap_err()
            .code(),
        "CBM_PAGE_ROWS_EXCEEDED"
    );

    let column_limits = DecodeLimits {
        max_columns: 1,
        ..DecodeLimits::default()
    };
    assert_eq!(
        SearchPagination::new(column_limits)
            .push(PagePosition::Offset(0), page.clone())
            .unwrap_err()
            .code(),
        "CBM_PAGE_COLUMNS_EXCEEDED"
    );

    let byte_limits = DecodeLimits {
        max_result_bytes: encoded.len() - 1,
        ..DecodeLimits::default()
    };
    assert_eq!(
        SearchPagination::new(byte_limits)
            .push(PagePosition::Offset(0), page.clone())
            .unwrap_err()
            .code(),
        "CBM_RESULT_BYTES_EXCEEDED"
    );

    let cursor_limits = DecodeLimits {
        max_cursor_bytes: 4,
        ..DecodeLimits::default()
    };
    assert_eq!(
        SearchPagination::new(cursor_limits)
            .push(PagePosition::Cursor("12345".to_owned()), page)
            .unwrap_err()
            .code(),
        "CBM_CURSOR_BYTES_EXCEEDED"
    );
}

#[test]
fn coverage_pagination_and_ranges_are_bounded() {
    let scope_has_more = json!({
        "structuredContent": {
            "project": "p",
            "signal": "best_effort",
            "metadata": {"generation": "g"},
            "paths": [],
            "scopes": [{"requested_scope":"src", "scope":"src", "total":1, "has_more":true, "entries":[], "status":"known_gaps"}]
        },
        "isError": false
    });
    let page = decode_index_coverage(
        &serde_json::to_vec(&scope_has_more).unwrap(),
        DecodeLimits::default(),
    )
    .unwrap();
    assert!(page.has_more());

    let too_many_ranges = json!({
        "structuredContent": {
            "project": "p",
            "signal": "best_effort",
            "metadata": {"generation": "g"},
            "paths": [],
            "scopes": [{"requested_scope":"src", "scope":"src", "total":1, "has_more":false, "entries":[{"path":"src/x.rs","kind":"parse_partial","ranges":[{"start":1,"end":1},{"start":2,"end":2}]}], "status":"known_gaps"}]
        },
        "isError": false
    });
    let limits = DecodeLimits {
        max_coverage_ranges: 1,
        ..DecodeLimits::default()
    };
    assert_eq!(
        decode_index_coverage(&serde_json::to_vec(&too_many_ranges).unwrap(), limits)
            .unwrap_err()
            .code(),
        "CBM_COVERAGE_RANGES_EXCEEDED"
    );

    let invalid_range = json!({
        "structuredContent": {
            "project": "p",
            "signal": "best_effort",
            "metadata": {"generation": "g"},
            "paths": [],
            "scopes": [{"requested_scope":"src", "scope":"src", "total":1, "has_more":false, "entries":[{"path":"src/x.rs","kind":"parse_partial","ranges":[{"start":9,"end":3}]}], "status":"known_gaps"}]
        },
        "isError": false
    });
    assert_eq!(
        decode_index_coverage(
            &serde_json::to_vec(&invalid_range).unwrap(),
            DecodeLimits::default(),
        )
        .unwrap_err()
        .code(),
        "CBM_STRUCTURED_CONTENT_INVALID"
    );

    for (start, end) in [(0, 1), (1, 0)] {
        let zero_range = json!({
            "structuredContent": {
                "project": "p",
                "signal": "best_effort",
                "metadata": {"generation": "g"},
                "paths": [],
                "scopes": [{"requested_scope":"src", "scope":"src", "total":1, "has_more":false, "entries":[{"path":"src/x.rs","kind":"parse_partial","ranges":[{"start":start,"end":end}]}], "status":"known_gaps"}]
            },
            "isError": false
        });
        assert_eq!(
            decode_index_coverage(
                &serde_json::to_vec(&zero_range).unwrap(),
                DecodeLimits::default(),
            )
            .unwrap_err()
            .code(),
            "CBM_STRUCTURED_CONTENT_INVALID"
        );
    }

    let two_entries = json!({
        "structuredContent": {
            "project": "p",
            "signal": "best_effort",
            "metadata": {"generation": "g"},
            "paths": [],
            "scopes": [{"requested_scope":"src", "scope":"src", "total":2, "has_more":false, "entries":[{"path":"src/a.rs","kind":"parse_partial"},{"path":"src/b.rs","kind":"parse_partial"}], "status":"known_gaps"}]
        },
        "isError": false
    });
    let row_limits = DecodeLimits {
        max_rows_per_page: 1,
        ..DecodeLimits::default()
    };
    assert_eq!(
        decode_index_coverage(&serde_json::to_vec(&two_entries).unwrap(), row_limits)
            .unwrap_err()
            .code(),
        "CBM_PAGE_ROWS_EXCEEDED"
    );

    let two_paths = json!({
        "structuredContent": {
            "project": "p",
            "signal": "best_effort",
            "metadata": {"generation": "g"},
            "paths": [
                {"requested_path":"src/a.rs","path":"src/a.rs","status":"no_recorded_issue","freshness":"metadata_match","recommended_action":"use_graph_with_best_effort_caveat","coverage":[]},
                {"requested_path":"src/b.rs","path":"src/b.rs","status":"no_recorded_issue","freshness":"metadata_match","recommended_action":"use_graph_with_best_effort_caveat","coverage":[]}
            ],
            "scopes": []
        },
        "isError": false
    });
    assert_eq!(
        decode_index_coverage(&serde_json::to_vec(&two_paths).unwrap(), row_limits)
            .unwrap_err()
            .code(),
        "CBM_PAGE_ROWS_EXCEEDED"
    );
}

#[test]
fn receipt_contains_only_recorded_evidence_and_marks_missing_identity_unavailable() {
    let record = ToolCallRecord {
        tool: "mcp".to_owned(),
        action: "search_graph".to_owned(),
        actor: "agent:test".to_owned(),
        program_allowlist: Default::default(),
        tier: IsolationTier::Tier1,
        disposition: ToolDisposition::Completed { exit_code: 0 },
        stdout_sha256: "a".repeat(64),
        stdout_bytes: 123,
        stderr_sha256: "b".repeat(64),
        stderr_bytes: 0,
        truncated: false,
        reused: false,
        verified_executable: None,
        contained_session: None,
        commit: None,
        landed_ref: None,
        recovered_workspace: false,
    };

    let receipt = TransportReceipt::from_record(&record);
    assert_eq!(receipt.record(), &record);
    assert_eq!(receipt.invocation_identity(), IdentityEvidence::Unavailable);
    assert_eq!(receipt.executable_identity(), IdentityEvidence::Unavailable);

    let encoded = serde_json::to_value(receipt).unwrap();
    assert_eq!(encoded["invocationIdentity"], "unavailable");
    assert_eq!(encoded["executableIdentity"], "unavailable");
    assert_eq!(encoded["brokerRecord"]["stdoutSha256"], "a".repeat(64));
    assert!(encoded.get("invocationId").is_none());
    assert!(encoded.get("executableDigest").is_none());
}

fn search_page(
    total: u64,
    columns: &[&str],
    has_more: bool,
) -> graphhelm_codebase_memory_mcp::SearchPage {
    let body = json!({
        "structuredContent": {"total": total, "cols": columns, "rows": [], "has_more": has_more},
        "isError": false
    });
    decode_search_graph(&serde_json::to_vec(&body).unwrap(), DecodeLimits::default()).unwrap()
}

fn search_page_with_names(
    total: u64,
    names: &[&str],
    has_more: bool,
) -> graphhelm_codebase_memory_mcp::SearchPage {
    let rows = names.iter().map(|name| json!([name])).collect::<Vec<_>>();
    let body = json!({
        "structuredContent": {"total": total, "cols": ["name"], "rows": rows, "has_more": has_more},
        "isError": false
    });
    decode_search_graph(&serde_json::to_vec(&body).unwrap(), DecodeLimits::default()).unwrap()
}

#[test]
fn decode_errors_are_stable_and_do_not_echo_provider_payloads() {
    let secret = "secret-provider-payload";
    let malformed =
        format!("{{\"structuredContent\":{{\"total\":\"{secret}\"}},\"isError\":false}}");
    let error: DecodeError =
        decode_search_graph(malformed.as_bytes(), DecodeLimits::default()).unwrap_err();
    assert_eq!(error.code(), "CBM_STRUCTURED_CONTENT_INVALID");
    assert!(!error.to_string().contains(secret));
}
