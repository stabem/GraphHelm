// One integration-test executable; keep each suite in its original file.
mod registration {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;

    #[test]
    fn every_top_level_test_file_is_registered() {
        let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        let bundle = fs::read_to_string(tests.join("cli.rs")).expect("read CLI test bundle");
        let registered: BTreeSet<_> = bundle
            .lines()
            .filter_map(|line| line.trim().strip_prefix("#[path = \"")?.strip_suffix("\"]"))
            .map(str::to_owned)
            .collect();
        let present: BTreeSet<_> = fs::read_dir(&tests)
            .expect("read CLI tests directory")
            .map(|entry| entry.expect("read CLI test entry").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
            .filter(|path| path.file_name().is_some_and(|name| name != "cli.rs"))
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        let present: BTreeSet<_> = present
            .into_iter()
            .filter(|name| !name.starts_with("support"))
            .collect();
        let missing: Vec<_> = present.difference(&registered).cloned().collect();
        let stale: Vec<_> = registered.difference(&present).cloned().collect();
        assert!(missing.is_empty(), "unregistered CLI test files: {missing:?}");
        assert!(stale.is_empty(), "missing CLI test files: {stale:?}");
    }
}

#[path = "adoption_apply.rs"]
mod adoption_apply;
#[path = "adoption_backup.rs"]
mod adoption_backup;
#[path = "adoption_hosts.rs"]
mod adoption_hosts;
#[path = "adoption_inventory.rs"]
mod adoption_inventory;
#[path = "adoption_journey.rs"]
mod adoption_journey;
#[path = "adoption_resolve.rs"]
mod adoption_resolve;
#[path = "adoption_restore.rs"]
mod adoption_restore;
#[path = "amend_budget.rs"]
mod amend_budget;
#[path = "api_http.rs"]
mod api_http;
#[path = "architect_cli.rs"]
mod architect_cli;
#[path = "attention_inputs_one_feed.rs"]
mod attention_inputs_one_feed;
#[path = "attention_tag_domain.rs"]
mod attention_tag_domain;
#[path = "cli_smoke.rs"]
mod cli_smoke;
#[path = "context_citation_fixtures.rs"]
mod context_citation_fixtures;
#[path = "context_journey.rs"]
mod context_journey;
#[path = "customs_cli.rs"]
mod customs_cli;
#[path = "delivery_cli.rs"]
mod delivery_cli;
#[path = "development_benchmark.rs"]
mod development_benchmark;
#[path = "development_cli.rs"]
mod development_cli;
#[path = "development_concurrency.rs"]
mod development_concurrency;
#[path = "development_context_budget.rs"]
mod development_context_budget;
#[path = "development_contract_schemas.rs"]
mod development_contract_schemas;
#[path = "development_journey.rs"]
mod development_journey;
#[path = "development_package_inventory.rs"]
mod development_package_inventory;
#[path = "development_plugin.rs"]
mod development_plugin;
#[path = "development_redaction.rs"]
mod development_redaction;
#[path = "development_sabotage.rs"]
mod development_sabotage;
#[path = "development_surface_parity.rs"]
mod development_surface_parity;
#[path = "documents_cli.rs"]
mod documents_cli;
#[path = "documents_http.rs"]
mod documents_http;
#[path = "event_store_cli.rs"]
mod event_store_cli;
#[path = "event_store_local_backup.rs"]
mod event_store_local_backup;
#[path = "execution_cli.rs"]
mod execution_cli;
#[path = "extension_cli.rs"]
mod extension_cli;
#[path = "extension_lifecycle.rs"]
mod extension_lifecycle;
#[path = "gate_classify_red.rs"]
mod gate_classify_red;
#[path = "gate_http.rs"]
mod gate_http;
#[path = "gateway_cli.rs"]
mod gateway_cli;
#[path = "gateway_setup.rs"]
mod gateway_setup;
#[path = "http_helper_inventory.rs"]
mod http_helper_inventory;
#[path = "human_output.rs"]
mod human_output;
#[path = "init_cli.rs"]
mod init_cli;
#[path = "journey_explore_browser.rs"]
mod journey_explore_browser;
#[path = "journey_explore_cli.rs"]
mod journey_explore_cli;
#[path = "journey_flow_cli.rs"]
mod journey_flow_cli;
#[path = "journey_flow_surfaces.rs"]
mod journey_flow_surfaces;
#[path = "journey_live_browser.rs"]
mod journey_live_browser;
#[path = "journey_live_cli.rs"]
mod journey_live_cli;
#[path = "journey_preview_browser.rs"]
mod journey_preview_browser;
#[path = "journey_preview_cli.rs"]
mod journey_preview_cli;
#[path = "journey_producers_cli.rs"]
mod journey_producers_cli;
#[path = "journey_replay_browser.rs"]
mod journey_replay_browser;
#[path = "journey_replay_cli.rs"]
mod journey_replay_cli;
#[path = "journey_scope_guard.rs"]
mod journey_scope_guard;
#[path = "journey_validate_cli.rs"]
mod journey_validate_cli;
#[path = "journeys_surfaces.rs"]
mod journeys_surfaces;
#[path = "jpd_capsule_authority.rs"]
mod jpd_capsule_authority;
#[path = "jpd_graph_contract.rs"]
mod jpd_graph_contract;
#[path = "jpd_observer_trust.rs"]
mod jpd_observer_trust;
#[path = "jpd_plugin.rs"]
mod jpd_plugin;
#[path = "jpd_promotion_policy.rs"]
mod jpd_promotion_policy;
#[path = "jpd_retry_semantics.rs"]
mod jpd_retry_semantics;
#[path = "keel_check.rs"]
mod keel_check;
#[path = "keel_index.rs"]
mod keel_index;
#[path = "mcp_capability.rs"]
mod mcp_capability;
#[path = "mcp_cli.rs"]
mod mcp_cli;
#[path = "mcp_discovery.rs"]
mod mcp_discovery;
#[path = "mcp_stdio.rs"]
mod mcp_stdio;
#[path = "mcp_url.rs"]
mod mcp_url;
#[path = "monitor_http.rs"]
mod monitor_http;
#[path = "owner_records_cli.rs"]
mod owner_records_cli;
#[path = "providerless_journey.rs"]
mod providerless_journey;
#[path = "resume_atomicity.rs"]
mod resume_atomicity;
#[path = "resume_project_default.rs"]
mod resume_project_default;
#[path = "runtime_http.rs"]
mod runtime_http;
#[path = "schema_cli.rs"]
mod schema_cli;
#[path = "shipped_package_coverage.rs"]
mod shipped_package_coverage;
#[path = "signal_image_evidence_cli.rs"]
mod signal_image_evidence_cli;
#[path = "signal_image_evidence_http.rs"]
mod signal_image_evidence_http;
#[path = "skills_sync_cli.rs"]
mod skills_sync_cli;
#[path = "source_invariants.rs"]
mod source_invariants;
#[path = "surface_completeness.rs"]
mod surface_completeness;
#[path = "sweep_cli.rs"]
mod sweep_cli;
#[path = "task_event_cli.rs"]
mod task_event_cli;
#[path = "test_support_has_no_dead_helpers.rs"]
mod test_support_has_no_dead_helpers;
#[path = "tool_cli.rs"]
mod tool_cli;
#[path = "wake_http.rs"]
mod wake_http;
#[path = "workspace_cli.rs"]
mod workspace_cli;
