//! The contained index producer (#219): the composition every gate this month was built for.
//!
//! `StructuralCodeIndex` has had exactly one implementor — a refusing placeholder — because
//! D-042 forbade a live provider until the containment existed. It exists now, and this type is
//! the assembly: a [`ContainedProviderSession`] (verified executable, pinned snapshot, confined
//! workspace — #552/#540/#539/#538) speaks one-shot newline-delimited JSON-RPC (the MCP stdio
//! framing the house already speaks in `apps/cli/src/commands/mcp/rpc.rs`) to the provider
//! binary, and the reply is decoded by this crate's own bounded [`decode_search_graph`].
//!
//! What the response carries is the whole point:
//!
//! - `snapshots` — both halves carry `indexed_over`, the repository snapshot the index was
//!   BUILT from (the protocol's freshness definition: `is_fresh` compares the two ids, so a
//!   fresh index's generation IS the repo identity). The pinned copy's own content digest is a
//!   different fact — provenance of the served bytes — and lives on the broker record's session
//!   identity instead. A plan compiled over any other repo snapshot refuses `index_stale` at the
//!   consumer; the composition, not this code, makes that true.
//! - `broker_record` — carries `verified_executable` (path + digest, from the session) AND
//!   `contained_session` (the session identity): a receipt bound to this record is bound to *a
//!   named program in a named session*, D-042's closing demand, now produced rather than argued.
//!
//! **The query rule, mechanical:** select the requested File nodes by their complete qualified
//! names, with the project and literal paths escaped. No reformulation or second call; the
//! resulting file receipt remains partial structural evidence.

use std::collections::BTreeSet;

use graphhelm_runtime::ports::{StructuralCodeIndex, StructuralCodeIndexError};
use graphhelm_runtime::retrieval::StructuralIndexRequest;
use graphhelm_tool_broker::record::{ToolCallRecord, ToolDisposition, digest_hex};
use graphhelm_tool_host::process::{CancelSignal, CapturedProcess, ProcessLimits};
use graphhelm_tool_host::session::ContainedProviderSession;

use crate::{DecodeLimits, decode_search_graph};

/// A `StructuralCodeIndex` backed by a contained provider process.
pub struct ContainedIndexProvider {
    session: ContainedProviderSession,
    /// The repository snapshot this index was BUILT OVER — the semantic index generation.
    ///
    /// Two identities meet here and they are deliberately not the same thing: the SESSION's
    /// pinned-snapshot generation is the digest of the index's own bytes (provenance of what is
    /// served, #539), while the protocol's `index_generation` is defined by
    /// `SnapshotBinding::is_fresh` as EQUAL to the repo snapshot when fresh — the identity of
    /// the repository the index was built from. The builder records this beside the index; the
    /// producer transmits it. Conflating the two made every honest response read stale.
    indexed_over: graphhelm_protocols::OpaqueId,
    /// argv handed to the provider binary on every one-shot call (host configuration, never
    /// caller input — the request never reaches argv).
    provider_arguments: Vec<String>,
    /// The project name the provider indexes under; part of the tools/call arguments.
    project: String,
    actor: String,
    limits: DecodeLimits,
    process_limits: ProcessLimits,
    cancel_signal: Option<CancelSignal>,
    last_invocation: std::sync::Mutex<Option<ToolCallRecord>>,
}

impl ContainedIndexProvider {
    #[must_use]
    pub fn new(
        session: ContainedProviderSession,
        indexed_over: graphhelm_protocols::OpaqueId,
        provider_arguments: Vec<String>,
        project: String,
        actor: String,
        limits: DecodeLimits,
        process_limits: ProcessLimits,
    ) -> Self {
        Self {
            session,
            indexed_over,
            provider_arguments,
            project,
            actor,
            limits,
            process_limits,
            cancel_signal: None,
            last_invocation: std::sync::Mutex::new(None),
        }
    }

    #[must_use]
    pub fn with_cancel_signal(mut self, signal: CancelSignal) -> Self {
        self.cancel_signal = Some(signal);
        self
    }

    #[must_use]
    pub fn last_invocation(&self) -> Option<ToolCallRecord> {
        self.last_invocation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn invocation_record(
        &self,
        captured: &CapturedProcess,
        request: &StructuralIndexRequest,
    ) -> ToolCallRecord {
        let disposition = if captured.readers_abandoned || captured.reader_lost {
            ToolDisposition::HostError {
                code: "GHTOOL013_CAPTURE_LOST".to_owned(),
            }
        } else if captured.cancelled {
            ToolDisposition::HostError {
                code: "GHTOOL011_CANCELLED".to_owned(),
            }
        } else if captured.timed_out {
            ToolDisposition::TimedOut
        } else {
            match captured.exit_code {
                Some(code) => ToolDisposition::Completed { exit_code: code },
                None => ToolDisposition::HostError {
                    code: "GHTOOL007_EXIT_UNKNOWN".to_owned(),
                },
            }
        };
        ToolCallRecord {
            tool: request.provider.tool.clone(),
            action: request.provider.action.clone(),
            actor: self.actor.clone(),
            program_allowlist: BTreeSet::new(),
            tier: graphhelm_tool_broker::effect::IsolationTier::Tier1,
            disposition,
            stdout_sha256: digest_hex(&captured.stdout),
            stdout_bytes: captured.stdout.len() as u64,
            stderr_sha256: digest_hex(&captured.stderr),
            stderr_bytes: captured.stderr.len() as u64,
            truncated: captured.truncated,
            reused: false,
            verified_executable: Some(graphhelm_tool_broker::record::VerifiedExecutableIdentity {
                path: self.session.executable().path().display().to_string(),
                sha256: self.session.executable().sha256().to_owned(),
            }),
            contained_session: Some(self.session.identity().clone()),
            commit: None,
            landed_ref: None,
            recovered_workspace: false,
        }
    }

    /// The one-shot MCP conversation: initialize, initialized, one tools/call. Newline-delimited
    /// JSON-RPC, the transport's own framing.
    fn request_lines(&self, request: &StructuralIndexRequest) -> Vec<u8> {
        let qn_pattern = qn_file_pattern(&self.project, &request.requested_paths);
        let arguments = serde_json::json!({
            "project": self.project,
            "qn_pattern": qn_pattern,
            "limit": request.limits.max_results,
            "format": "json",
            "label": "File"
        });
        let lines = [
            serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                            "clientInfo": {"name": "graphhelm-contained-producer", "version": "1"}}
            }),
            serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            serde_json::json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                // "format":"json" is what makes the real provider answer structuredContent at
                // all (measured, 0.10.8): without it search_graph returns a text block the
                // house decoder refuses. "detail" is not in the tool's schema and was silently
                // ignored — dropped rather than transmitted as a false claim.
                "params": {"name": "search_graph", "arguments": arguments}
            }),
        ];
        let mut bytes = Vec::new();
        for line in lines {
            bytes.extend_from_slice(line.to_string().as_bytes());
            bytes.push(b'\n');
        }
        bytes
    }
}

impl StructuralCodeIndex for ContainedIndexProvider {
    fn retrieve(
        &self,
        request: &StructuralIndexRequest,
    ) -> Result<graphhelm_runtime::ports::StructuralIndexResponse, StructuralCodeIndexError> {
        *self
            .last_invocation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        let stdin = self.request_lines(request);
        let captured = self
            .session
            .call(
                &self.provider_arguments,
                Some(&stdin),
                &self.process_limits,
                self.cancel_signal.as_ref(),
            )
            .map_err(|_| StructuralCodeIndexError::Unavailable)?;

        let record = self.invocation_record(&captured, request);
        *self
            .last_invocation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(record.clone());

        // A reply is usable only when the host can account for the complete, successful
        // process. A valid reply emitted before a timeout or cancellation is still refused.
        if captured.exit_code != Some(0)
            || captured.truncated
            || captured.timed_out
            || captured.cancelled
            || captured.readers_abandoned
            || captured.reader_lost
        {
            return Err(StructuralCodeIndexError::Unavailable);
        }

        // The reply to id 2 is the LAST id-bearing line; provider prose on other lines is not
        // consulted. A missing or malformed reply is unavailability, not a partial answer.
        let reply = String::from_utf8_lossy(&captured.stdout);
        let result = reply
            .lines()
            .rev()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .find(|value| value.get("id") == Some(&serde_json::json!(2)))
            .and_then(|value| value.get("result").cloned())
            .ok_or(StructuralCodeIndexError::Unavailable)?;
        let page = decode_search_graph(
            &serde_json::to_vec(&result).map_err(|_| StructuralCodeIndexError::Unavailable)?,
            self.limits,
        )
        .map_err(|_| StructuralCodeIndexError::Unavailable)?;

        let row_count = page.rows().len()
            + page
                .groups()
                .iter()
                .map(|group| group.rows().len())
                .sum::<usize>();
        if !page.has_more() && page.total() != row_count as u64 {
            return Err(StructuralCodeIndexError::Unavailable);
        }

        // Hits: the `file` column of each row (detail "ids" carries qn only, so fall back to the
        // row's last string cell when no file column exists); unique, first-appearance order.
        let file_column = page.columns().iter().position(|name| name == "file");
        let mut seen = BTreeSet::new();
        let mut hits = Vec::new();
        // GROUPED pages first. The provider has two valid encodings and the decoder supports
        // both: a flat `rows` list, and `groups` where a shared (prefix, file) header is printed
        // once and `rows` is left EMPTY. Reading only `page.rows()` turned every grouped answer
        // into zero hits -- silently, because an empty hit list is a legal answer that means
        // something completely different (Codex P1 on #579). The group's own `file` is the path;
        // that is what the header exists to carry.
        for group in page.groups() {
            let path = group.file();
            if !path.is_empty() && seen.insert(path.to_owned()) {
                hits.push(path.to_owned());
            }
        }
        for row in page.rows() {
            let candidate = file_column
                .and_then(|index| row.get(index))
                .and_then(|cell| cell.as_str())
                .or_else(|| row.iter().rev().find_map(|cell| cell.as_str()));
            if let Some(path) = candidate
                && seen.insert(path.to_owned())
            {
                hits.push(path.to_owned());
            }
        }

        let requested = request.requested_paths.iter().collect::<BTreeSet<_>>();
        if hits.iter().any(|path| !requested.contains(path)) {
            return Err(StructuralCodeIndexError::Unavailable);
        }

        // ALWAYS Partial, and the consumer enforces exactly this: a BestEffort provider claiming
        // Complete is CoveragePromotion, refused. The index's own status says "best-effort, not
        // a completeness guarantee" — has_more=false means the PAGE ended, not that the search
        // was exhaustive. Verified absence stays out of reach by design until a provider can
        // support it.
        let coverage = graphhelm_protocols::CoverageState::Partial;
        // The semantic pair: this index serves coordinates over the repository snapshot it was
        // BUILT from, so both halves carry `indexed_over` — fresh by construction against a
        // plan compiled over the same repo, stale against any other. The pinned copy's own
        // digest stays where provenance lives: the session identity on the broker record.
        let snapshots = graphhelm_protocols::SnapshotBinding {
            repo_snapshot: self.indexed_over.clone(),
            index_generation: self.indexed_over.clone(),
        };

        let entries: Vec<graphhelm_protocols::RetrievalCoverageEntry> = request
            .requested_paths
            .iter()
            .map(|value| (graphhelm_protocols::RetrievalCoverageTarget::Path, value))
            .chain(request.negative_scopes.iter().map(|value| {
                (
                    graphhelm_protocols::RetrievalCoverageTarget::NegativeScope,
                    value,
                )
            }))
            .map(
                |(target, value)| graphhelm_protocols::RetrievalCoverageEntry {
                    target,
                    value: value.clone(),
                    coverage,
                    gap_ranges: Vec::new(),
                },
            )
            .collect();

        // Runtime receipts represent file hits, while the provider's page total and rows count
        // symbols. Several symbols can therefore map to one hit; report the deduplicated count
        // so the receipt's bounded page, total, and hit counts describe the same evidence.
        let results = u32::try_from(hits.len()).unwrap_or(u32::MAX);
        let stdout_bytes = captured.stdout.len() as u64;
        Ok(graphhelm_runtime::ports::StructuralIndexResponse {
            plan_binding: request.plan_binding.clone(),
            step: request.step.clone(),
            scope: request.plan_binding.scope.clone(),
            snapshots,
            provider: request.provider.clone(),
            broker_record: record,
            confidence: graphhelm_protocols::ProviderCoverageConfidence::BestEffort,
            coverage,
            entries,
            pages: vec![graphhelm_protocols::RetrievalPageEvidence {
                position: "page-1".to_owned(),
                results,
                bytes: stdout_bytes,
                has_more: page.has_more(),
            }],
            total_results: results,
            hits,
        })
    }
}

fn qn_file_pattern(project: &str, paths: &[String]) -> String {
    let alternatives = paths
        .iter()
        .map(|path| {
            let dotted = path.replace('/', ".");
            escape_regex(&dotted)
        })
        .collect::<Vec<_>>()
        .join("|");
    format!(r"^{}\.({alternatives})\.__file__$", escape_regex(project))
}

fn escape_regex(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(
            character,
            '\\' | '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|'
        ) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}
