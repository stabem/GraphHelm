//! Pure, fail-closed decoding for recorded codebase-memory-mcp tool results.
//!
//! This crate deliberately owns no MCP transport, process launch, index creation, cache path,
//! retry, or source fallback. ADR-028 reserves live access for a future broker-managed SDK session
//! over an immutable provider snapshot. The public functions here only decode bytes that an
//! authorized caller already obtained and recorded.

use std::collections::HashSet;

use graphhelm_tool_broker::record::ToolCallRecord;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Resource limits applied before and while decoding untrusted provider output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeLimits {
    pub max_result_bytes: usize,
    pub max_total_bytes: usize,
    pub max_nesting_depth: usize,
    pub max_pages: usize,
    pub max_rows_per_page: usize,
    pub max_columns: usize,
    pub max_coverage_ranges: usize,
    pub max_cursor_bytes: usize,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_result_bytes: 1024 * 1024,
            max_total_bytes: 8 * 1024 * 1024,
            max_nesting_depth: 64,
            max_pages: 64,
            max_rows_per_page: 5_000,
            max_columns: 64,
            max_coverage_ranges: 10_000,
            max_cursor_bytes: 4 * 1024,
        }
    }
}

/// Stable, content-free refusals. Messages never interpolate provider-controlled bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("provider result exceeds the configured byte limit")]
    ResultBytesExceeded,
    #[error("provider result exceeds the configured JSON nesting limit")]
    JsonNestingExceeded,
    #[error("provider result is not valid JSON")]
    InvalidJson,
    #[error("provider reported a tool error")]
    ProviderError,
    #[error("provider result has no structured content")]
    StructuredContentMissing,
    #[error("provider structured content does not match the required shape")]
    StructuredContentInvalid,
    #[error("provider page exceeds the configured row limit")]
    PageRowsExceeded,
    #[error("provider page exceeds the configured column limit")]
    PageColumnsExceeded,
    #[error("provider page total changed during pagination")]
    TotalDrift,
    #[error("provider page columns changed during pagination")]
    ColumnsDrift,
    #[error("provider repeated a page cursor or offset")]
    PagePositionLoop,
    #[error("provider cursor exceeds the configured byte limit")]
    CursorBytesExceeded,
    #[error("provider pagination exceeds the configured page limit")]
    PageLimitExceeded,
    #[error("provider pagination exceeds the configured aggregate byte limit")]
    PaginationBytesExceeded,
    #[error("provider returned another page after pagination completed")]
    PaginationAlreadyComplete,
    #[error("provider pagination ended while more pages were advertised")]
    PaginationUnfinished,
    #[error("provider pagination row count does not match its declared total")]
    ResultCountMismatch,
    #[error("provider pagination contained no pages")]
    PaginationEmpty,
    #[error("provider coverage output exceeds the configured range limit")]
    CoverageRangesExceeded,
    #[error("provider seal is missing its project or generation")]
    SealUnavailable,
}

impl DecodeError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ResultBytesExceeded => "CBM_RESULT_BYTES_EXCEEDED",
            Self::JsonNestingExceeded => "CBM_JSON_NESTING_EXCEEDED",
            Self::InvalidJson => "CBM_JSON_INVALID",
            Self::ProviderError => "CBM_PROVIDER_ERROR",
            Self::StructuredContentMissing => "CBM_STRUCTURED_CONTENT_MISSING",
            Self::StructuredContentInvalid => "CBM_STRUCTURED_CONTENT_INVALID",
            Self::PageRowsExceeded => "CBM_PAGE_ROWS_EXCEEDED",
            Self::PageColumnsExceeded => "CBM_PAGE_COLUMNS_EXCEEDED",
            Self::TotalDrift => "CBM_TOTAL_DRIFT",
            Self::ColumnsDrift => "CBM_COLUMNS_DRIFT",
            Self::PagePositionLoop => "CBM_PAGE_POSITION_LOOP",
            Self::CursorBytesExceeded => "CBM_CURSOR_BYTES_EXCEEDED",
            Self::PageLimitExceeded => "CBM_PAGE_LIMIT_EXCEEDED",
            Self::PaginationBytesExceeded => "CBM_PAGINATION_BYTES_EXCEEDED",
            Self::PaginationAlreadyComplete => "CBM_PAGINATION_ALREADY_COMPLETE",
            Self::PaginationUnfinished => "CBM_PAGINATION_UNFINISHED",
            Self::ResultCountMismatch => "CBM_RESULT_COUNT_MISMATCH",
            Self::PaginationEmpty => "CBM_PAGINATION_EMPTY",
            Self::CoverageRangesExceeded => "CBM_COVERAGE_RANGES_EXCEEDED",
            Self::SealUnavailable => "CBM_SEAL_UNAVAILABLE",
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolResultEnvelope {
    structured_content: Option<Value>,
    is_error: Option<bool>,
}

/// One decoded `search_graph(format="json")` page.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchPage {
    total: u64,
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
    groups: Vec<SearchGroup>,
    has_more: bool,
    encoded_bytes: usize,
    returned_rows: usize,
}

impl SearchPage {
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.total
    }

    #[must_use]
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    #[must_use]
    pub fn rows(&self) -> &[Vec<Value>] {
        &self.rows
    }

    #[must_use]
    pub fn groups(&self) -> &[SearchGroup] {
        &self.groups
    }

    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }
}

/// One prefix/file group from the provider's grouped search encoding.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchGroup {
    prefix: String,
    file: String,
    rows: Vec<Vec<Value>>,
}

impl SearchGroup {
    #[must_use]
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    #[must_use]
    pub fn file(&self) -> &str {
        &self.file
    }

    #[must_use]
    pub fn rows(&self) -> &[Vec<Value>] {
        &self.rows
    }
}

#[derive(Deserialize)]
struct SearchWire {
    total: u64,
    #[serde(default)]
    count: Option<u64>,
    cols: Vec<String>,
    #[serde(default)]
    rows: Option<Vec<Vec<Value>>>,
    #[serde(default)]
    groups: Option<Vec<SearchGroupWire>>,
    has_more: bool,
}

#[derive(Deserialize)]
struct SearchGroupWire {
    #[serde(alias = "qn_prefix")]
    prefix: String,
    file: String,
    rows: Vec<Vec<Value>>,
}

/// Decode exactly one provider page. Pagination and generation acceptance stay with callers.
pub fn decode_search_graph(bytes: &[u8], limits: DecodeLimits) -> Result<SearchPage, DecodeError> {
    let (structured, encoded_bytes) = structured_content(bytes, limits)?;
    let wire: SearchWire =
        serde_json::from_value(structured).map_err(|_| DecodeError::StructuredContentInvalid)?;

    if wire.cols.len() > limits.max_columns {
        return Err(DecodeError::PageColumnsExceeded);
    }
    if wire.cols.is_empty() && wire.total != 0 {
        return Err(DecodeError::StructuredContentInvalid);
    }

    let (rows, groups, grouped) = match (wire.rows, wire.groups) {
        (Some(rows), None) => (rows, Vec::new(), false),
        (None, Some(groups)) => {
            if groups.iter().any(|group| {
                group.prefix.trim().is_empty()
                    || group.file.trim().is_empty()
                    || group.rows.is_empty()
            }) {
                return Err(DecodeError::StructuredContentInvalid);
            }
            let groups = groups
                .into_iter()
                .map(|group| SearchGroup {
                    prefix: group.prefix,
                    file: group.file,
                    rows: group.rows,
                })
                .collect::<Vec<_>>();
            (Vec::new(), groups, true)
        }
        _ => return Err(DecodeError::StructuredContentInvalid),
    };

    let grouped_row_count = groups
        .iter()
        .try_fold(0usize, |count, group| count.checked_add(group.rows.len()))
        .ok_or(DecodeError::PageRowsExceeded)?;
    let row_count = rows
        .len()
        .checked_add(grouped_row_count)
        .ok_or(DecodeError::PageRowsExceeded)?;
    if row_count > limits.max_rows_per_page {
        return Err(DecodeError::PageRowsExceeded);
    }
    let returned_count = u64::try_from(row_count).map_err(|_| DecodeError::PageRowsExceeded)?;
    if returned_count > wire.total
        || wire.count.is_some_and(|count| count != returned_count)
        || (grouped && wire.count.is_none())
    {
        return Err(DecodeError::StructuredContentInvalid);
    }
    if rows
        .iter()
        .chain(groups.iter().flat_map(|group| group.rows.iter()))
        .any(|row| {
            row.len() != wire.cols.len()
                || row
                    .iter()
                    .any(|cell| matches!(cell, Value::Array(_) | Value::Object(_)))
        })
    {
        return Err(DecodeError::StructuredContentInvalid);
    }

    Ok(SearchPage {
        total: wire.total,
        columns: wire.cols,
        rows,
        groups,
        has_more: wire.has_more,
        encoded_bytes,
        returned_rows: row_count,
    })
}

/// Opaque position supplied by the caller for loop detection only.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PagePosition {
    Offset(u64),
    Cursor(String),
}

/// Bounded consistency checker for pages from one `search_graph` attempt.
///
/// It does not fetch, retry, inspect generations, or accept a retrieval attempt.
pub struct SearchPagination {
    limits: DecodeLimits,
    positions: HashSet<PagePosition>,
    pages: Vec<SearchPage>,
    expected_total: Option<u64>,
    expected_columns: Option<Vec<String>>,
    total_bytes: usize,
    returned_rows: usize,
}

impl SearchPagination {
    #[must_use]
    pub fn new(limits: DecodeLimits) -> Self {
        Self {
            limits,
            positions: HashSet::new(),
            pages: Vec::new(),
            expected_total: None,
            expected_columns: None,
            total_bytes: 0,
            returned_rows: 0,
        }
    }

    pub fn push(&mut self, position: PagePosition, page: SearchPage) -> Result<(), DecodeError> {
        if self.pages.len() >= self.limits.max_pages {
            return Err(DecodeError::PageLimitExceeded);
        }
        if page.encoded_bytes > self.limits.max_result_bytes {
            return Err(DecodeError::ResultBytesExceeded);
        }
        if page.columns.len() > self.limits.max_columns {
            return Err(DecodeError::PageColumnsExceeded);
        }
        if page.returned_rows > self.limits.max_rows_per_page {
            return Err(DecodeError::PageRowsExceeded);
        }
        if matches!(&position, PagePosition::Cursor(cursor) if cursor.len() > self.limits.max_cursor_bytes)
        {
            return Err(DecodeError::CursorBytesExceeded);
        }
        if self.pages.last().is_some_and(|previous| !previous.has_more) {
            return Err(DecodeError::PaginationAlreadyComplete);
        }
        if self.positions.contains(&position) {
            return Err(DecodeError::PagePositionLoop);
        }
        if self.expected_total.is_some_and(|total| total != page.total) {
            return Err(DecodeError::TotalDrift);
        }
        if self
            .expected_columns
            .as_ref()
            .is_some_and(|columns| columns != &page.columns)
        {
            return Err(DecodeError::ColumnsDrift);
        }
        let total_bytes = self
            .total_bytes
            .checked_add(page.encoded_bytes)
            .ok_or(DecodeError::PaginationBytesExceeded)?;
        if total_bytes > self.limits.max_total_bytes {
            return Err(DecodeError::PaginationBytesExceeded);
        }
        let returned_rows = self
            .returned_rows
            .checked_add(page.returned_rows)
            .ok_or(DecodeError::ResultCountMismatch)?;
        if u64::try_from(returned_rows)
            .ok()
            .is_none_or(|returned| returned > page.total)
        {
            return Err(DecodeError::ResultCountMismatch);
        }

        let inserted = self.positions.insert(position);
        debug_assert!(
            inserted,
            "position was checked before the atomic state commit"
        );
        self.expected_total.get_or_insert(page.total);
        self.expected_columns
            .get_or_insert_with(|| page.columns.clone());
        self.total_bytes = total_bytes;
        self.returned_rows = returned_rows;
        self.pages.push(page);
        Ok(())
    }

    pub fn finish(self) -> Result<Vec<SearchPage>, DecodeError> {
        let Some(last) = self.pages.last() else {
            return Err(DecodeError::PaginationEmpty);
        };
        if last.has_more {
            return Err(DecodeError::PaginationUnfinished);
        }
        if u64::try_from(self.returned_rows).ok() != self.expected_total {
            return Err(DecodeError::ResultCountMismatch);
        }
        Ok(self.pages)
    }
}

/// Coverage is deliberately not a proof of completeness.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoverageConfidence {
    Unknown,
}

/// Provider identity used to compare opening and closing coverage reads in a higher layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderSeal {
    project: String,
    generation: String,
    confidence: CoverageConfidence,
}

impl ProviderSeal {
    #[must_use]
    pub fn project(&self) -> &str {
        &self.project
    }

    #[must_use]
    pub fn generation(&self) -> &str {
        &self.generation
    }

    #[must_use]
    pub const fn confidence(&self) -> CoverageConfidence {
        self.confidence
    }
}

/// One exact-path coverage result, preserved as typed provider evidence.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct CoveragePath {
    pub requested_path: String,
    pub path: String,
    pub status: String,
    pub freshness: String,
    pub recommended_action: String,
    #[serde(default)]
    pub coverage: Vec<CoverageEntry>,
}

/// One source range reported by coverage metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct CoverageRange {
    pub start: u64,
    pub end: u64,
}

/// One entry inside a scoped coverage page.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct CoverageEntry {
    pub path: String,
    pub kind: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default, rename = "match")]
    pub match_kind: Option<String>,
    #[serde(default)]
    pub ranges: Vec<CoverageRange>,
}

/// One bounded provider scope result.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct CoverageScope {
    pub requested_scope: String,
    pub scope: String,
    pub total: u64,
    pub has_more: bool,
    #[serde(default)]
    pub entries: Vec<CoverageEntry>,
    pub status: String,
}

/// One decoded `check_index_coverage` result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoveragePage {
    project: String,
    generation: String,
    confidence: CoverageConfidence,
    paths: Vec<CoveragePath>,
    scopes: Vec<CoverageScope>,
}

impl CoveragePage {
    pub fn seal(&self) -> Result<ProviderSeal, DecodeError> {
        if self.project.trim().is_empty() || self.generation.trim().is_empty() {
            return Err(DecodeError::SealUnavailable);
        }
        Ok(ProviderSeal {
            project: self.project.clone(),
            generation: self.generation.clone(),
            confidence: self.confidence,
        })
    }

    #[must_use]
    pub fn paths(&self) -> &[CoveragePath] {
        &self.paths
    }

    #[must_use]
    pub fn scopes(&self) -> &[CoverageScope] {
        &self.scopes
    }

    #[must_use]
    pub fn has_more(&self) -> bool {
        self.scopes.iter().any(|scope| scope.has_more)
    }
}

#[derive(Deserialize)]
struct CoverageWire {
    project: String,
    #[serde(default)]
    signal: Option<String>,
    metadata: CoverageMetadataWire,
    paths: Vec<CoveragePath>,
    scopes: Vec<CoverageScope>,
}

#[derive(Deserialize)]
struct CoverageMetadataWire {
    generation: String,
}

/// Decode one coverage response. `best_effort` is evidence, never completeness.
pub fn decode_index_coverage(
    bytes: &[u8],
    limits: DecodeLimits,
) -> Result<CoveragePage, DecodeError> {
    let (structured, _) = structured_content(bytes, limits)?;
    let wire: CoverageWire =
        serde_json::from_value(structured).map_err(|_| DecodeError::StructuredContentInvalid)?;
    if wire
        .signal
        .as_deref()
        .is_some_and(|signal| signal != "best_effort")
    {
        return Err(DecodeError::StructuredContentInvalid);
    }

    let range_count = wire
        .paths
        .iter()
        .flat_map(|path| path.coverage.iter())
        .map(|entry| entry.ranges.len())
        .chain(
            wire.scopes
                .iter()
                .flat_map(|scope| scope.entries.iter())
                .map(|entry| entry.ranges.len()),
        )
        .try_fold(0usize, |total, count| total.checked_add(count))
        .ok_or(DecodeError::CoverageRangesExceeded)?;
    if range_count > limits.max_coverage_ranges {
        return Err(DecodeError::CoverageRangesExceeded);
    }
    if wire
        .paths
        .iter()
        .flat_map(|path| path.coverage.iter())
        .chain(wire.scopes.iter().flat_map(|scope| scope.entries.iter()))
        .flat_map(|entry| entry.ranges.iter())
        .any(|range| range.start == 0 || range.end == 0 || range.start > range.end)
    {
        return Err(DecodeError::StructuredContentInvalid);
    }
    let coverage_entry_count = wire
        .paths
        .iter()
        .map(|path| path.coverage.len())
        .chain(wire.scopes.iter().map(|scope| scope.entries.len()))
        .chain([wire.paths.len(), wire.scopes.len()])
        .try_fold(0usize, |total, count| total.checked_add(count))
        .ok_or(DecodeError::PageRowsExceeded)?;
    if coverage_entry_count > limits.max_rows_per_page {
        return Err(DecodeError::PageRowsExceeded);
    }
    if wire.scopes.iter().any(|scope| {
        usize::try_from(scope.total)
            .ok()
            .is_some_and(|total| scope.entries.len() > total)
    }) {
        return Err(DecodeError::StructuredContentInvalid);
    }

    Ok(CoveragePage {
        project: wire.project,
        generation: wire.metadata.generation,
        confidence: CoverageConfidence::Unknown,
        paths: wire.paths,
        scopes: wire.scopes,
    })
}

/// Explicit absence of identity evidence in the current broker record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityEvidence {
    Unavailable,
}

/// Immutable receipt over exactly the durable evidence already emitted by the Tool Broker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportReceipt {
    broker_record: ToolCallRecord,
    invocation_identity: IdentityEvidence,
    executable_identity: IdentityEvidence,
}

impl TransportReceipt {
    #[must_use]
    pub fn from_record(record: &ToolCallRecord) -> Self {
        Self {
            broker_record: record.clone(),
            invocation_identity: IdentityEvidence::Unavailable,
            executable_identity: IdentityEvidence::Unavailable,
        }
    }

    #[must_use]
    pub const fn record(&self) -> &ToolCallRecord {
        &self.broker_record
    }

    #[must_use]
    pub const fn invocation_identity(&self) -> IdentityEvidence {
        self.invocation_identity
    }

    #[must_use]
    pub const fn executable_identity(&self) -> IdentityEvidence {
        self.executable_identity
    }
}

fn structured_content(bytes: &[u8], limits: DecodeLimits) -> Result<(Value, usize), DecodeError> {
    if bytes.len() > limits.max_result_bytes {
        return Err(DecodeError::ResultBytesExceeded);
    }
    check_json_nesting(bytes, limits.max_nesting_depth)?;
    let envelope: ToolResultEnvelope =
        serde_json::from_slice(bytes).map_err(|_| DecodeError::InvalidJson)?;
    match envelope.is_error {
        Some(true) => return Err(DecodeError::ProviderError),
        Some(false) => {}
        None => return Err(DecodeError::StructuredContentInvalid),
    }
    let structured = envelope
        .structured_content
        .ok_or(DecodeError::StructuredContentMissing)?;
    if !structured.is_object() {
        return Err(DecodeError::StructuredContentInvalid);
    }
    Ok((structured, bytes.len()))
}

fn check_json_nesting(bytes: &[u8], maximum: usize) -> Result<(), DecodeError> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match *byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth = depth
                    .checked_add(1)
                    .ok_or(DecodeError::JsonNestingExceeded)?;
                if depth > maximum {
                    return Err(DecodeError::JsonNestingExceeded);
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

pub mod provider;
