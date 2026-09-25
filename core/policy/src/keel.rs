//! Keel: the deterministic half of the development model (#1212).
//!
//! A keel is laid before any plank and is the part of the hull that resists drift. This module is
//! that part for code written by agents: it RECEIVES a unified diff and a versioned policy, and
//! returns what the diff SPENDS of the node's write surface (new modules, types, public functions,
//! dependencies, tests), which budgets it exceeds, and what mode the offending seat is in after
//! its history. No LLM, no filesystem, no clock: the same diff and the same policy always give the
//! same answer, which is what makes a refusal replayable and a penalty auditable.
//!
//! **What this is NOT, stated so the reader does not oversell it.** It is a line grammar over a
//! diff, not a parser. It counts declarations it can recognise from a single added line in Rust,
//! TypeScript/JavaScript and Python; it does not see a function grow inside an existing body
//! (the "inward sprawl" twin is bounded here only by the per-file added-line cap), it does not
//! resolve re-exports, macros or dynamic attributes, and it reads dependency manifests by section
//! heading. Every one of those is a SEALED LIMIT named in `keel.yaml`; a reader who needs the
//! stronger claim writes the parser and the pathogen that fails without it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The write surface a node may spend, per kind. Zero is a valid budget and means "none".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SurfaceBudget {
    pub new_module: u32,
    pub new_type: u32,
    pub new_public_fn: u32,
    pub new_dependency: u32,
    pub new_test: u32,
}

impl SurfaceBudget {
    const fn get(&self, debit: Debit) -> u32 {
        match debit {
            Debit::NewModule => self.new_module,
            Debit::NewType => self.new_type,
            Debit::NewPublicFn => self.new_public_fn,
            Debit::NewDependency => self.new_dependency,
            Debit::NewTest => self.new_test,
        }
    }

    /// Component-wise sum, saturating: an allowance never wraps a budget to zero.
    #[must_use]
    pub const fn plus(self, other: Self) -> Self {
        Self {
            new_module: self.new_module.saturating_add(other.new_module),
            new_type: self.new_type.saturating_add(other.new_type),
            new_public_fn: self.new_public_fn.saturating_add(other.new_public_fn),
            new_dependency: self.new_dependency.saturating_add(other.new_dependency),
            new_test: self.new_test.saturating_add(other.new_test),
        }
    }

    /// Component-wise minimum: an allowance may not exceed the policy's ceiling.
    #[must_use]
    pub fn capped_by(self, ceiling: Self) -> Self {
        Self {
            new_module: self.new_module.min(ceiling.new_module),
            new_type: self.new_type.min(ceiling.new_type),
            new_public_fn: self.new_public_fn.min(ceiling.new_public_fn),
            new_dependency: self.new_dependency.min(ceiling.new_dependency),
            new_test: self.new_test.min(ceiling.new_test),
        }
    }
}

/// The per-file cap that stands in for the parser this slice does not have.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BodyLimits {
    /// Added lines a single file may receive in one diff. Above it the change is refused as
    /// `keel.body.oversized_change`: a promise that needs more than this is two promises.
    pub max_file_lines_delta: u32,
}

/// How drift changes what a seat may write next. Strikes and credits are folded over the last
/// `window_nodes` records; every `strikes_per_rung` net strikes drop the seat one rung.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ladder {
    /// Off until a controlled comparison shows the ladder adds value over guidance alone (owner
    /// decision 2026-09-23, paper section 7a). While off, [`effective_mode`] is always `Full`; the
    /// fold in [`ladder`] still runs, so the rung is measured and reported, never applied.
    pub enabled: bool,
    pub strikes_per_rung: u32,
    pub credit_per_proven_promise: u32,
    pub window_nodes: u32,
}

/// The contract card bounds: what an agent loads instead of the tree must stay loadable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CardLimits {
    pub max_scope_paths: u32,
    pub max_exported_symbols: u32,
    pub max_card_bytes: u32,
}

/// Whether count-based rules (surface budgets, the per-file cap) block or only report.
///
/// Counts are signals for the reviewer by default: a hard quota invites an agent to inflate one
/// function or drop a test to stay under a number. Objective contracts (an unparseable diff, a card
/// scope that is not a list of paths) block in either mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Enforcement {
    Signal,
    Block,
}

/// The whole versioned rules file. `version` travels with every verdict.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KeelPolicy {
    pub version: String,
    pub card: CardLimits,
    pub surface: SurfaceBudget,
    pub max_allowance: SurfaceBudget,
    pub body: BodyLimits,
    pub surface_enforcement: Enforcement,
    pub ladder: Ladder,
}

/// What a diff spends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Debit {
    NewModule,
    NewType,
    NewPublicFn,
    NewDependency,
    NewTest,
}

impl Debit {
    const ALL: [Self; 5] = [
        Self::NewModule,
        Self::NewType,
        Self::NewPublicFn,
        Self::NewDependency,
        Self::NewTest,
    ];

    /// The rule id a budget overrun is reported under.
    #[must_use]
    pub const fn over_budget_rule(self) -> &'static str {
        match self {
            Self::NewModule => "keel.surface.new_module_over_budget",
            Self::NewType => "keel.surface.new_type_over_budget",
            Self::NewPublicFn => "keel.surface.new_public_fn_over_budget",
            Self::NewDependency => "keel.surface.new_dependency_over_budget",
            Self::NewTest => "keel.surface.new_test_over_budget",
        }
    }
}

/// One charge against the write surface, with the line that caused it so a refusal can point.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Charge {
    pub debit: Debit,
    pub path: String,
    /// 1-based line number within the added side of the diff for that file.
    pub line: u32,
    pub symbol: String,
}

/// A rule the diff breaks. `rule` is a stable id; `detail` is for a human.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub rule: String,
    pub path: Option<String>,
    pub detail: String,
    /// Whether this finding refuses the diff. Count findings block only under
    /// `surfaceEnforcement: block`; objective contracts always do.
    pub blocking: bool,
}

/// The verdict on one diff under one policy version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Classification {
    pub policy_version: String,
    pub charges: Vec<Charge>,
    pub totals: BTreeMap<Debit, u32>,
    pub budget: SurfaceBudget,
    pub findings: Vec<Finding>,
    /// True when any BLOCKING finding exists. A refusal is a property of the findings, never a
    /// separate flag someone could set without one.
    pub refused: bool,
}

/// The rung a seat sits on. Each rung is a smaller write surface, not a smaller number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// May spend the full budget plus a declared allowance.
    Full,
    /// May edit inside existing files and types only: no new module, type or dependency.
    ContractOnly,
    /// May edit existing function bodies only: additionally no new public function or test.
    PatchOnly,
    /// May not write; may propose a plan for another seat.
    ProposeOnly,
}

impl Mode {
    /// The budget a mode leaves open. Independent of policy numbers on purpose: a rung is a
    /// shape, and the shapes are the paradigm, not a tunable.
    #[must_use]
    pub const fn cap(self, full: SurfaceBudget) -> SurfaceBudget {
        match self {
            Self::Full => full,
            Self::ContractOnly => SurfaceBudget {
                new_module: 0,
                new_type: 0,
                new_public_fn: full.new_public_fn,
                new_dependency: 0,
                new_test: full.new_test,
            },
            Self::PatchOnly | Self::ProposeOnly => SurfaceBudget {
                new_module: 0,
                new_type: 0,
                new_public_fn: 0,
                new_dependency: 0,
                new_test: 0,
            },
        }
    }
}

/// One node's outcome as the ladder sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeRecord {
    /// The watchdog refused or repaired this node's write.
    pub drifted: bool,
    /// The node's promise passed its named proof instrument.
    pub proven: bool,
}

/// Folds a seat's history into its mode. Newest record last. Only the last `window_nodes` count;
/// strikes never go below zero, so credit is never banked ahead of drift.
#[must_use]
pub fn ladder(history: &[NodeRecord], policy: &Ladder) -> Mode {
    let window = usize::try_from(policy.window_nodes).unwrap_or(usize::MAX);
    let start = history.len().saturating_sub(window);
    let mut strikes: u32 = 0;
    for record in &history[start..] {
        if record.drifted {
            strikes = strikes.saturating_add(1);
        }
        if record.proven {
            strikes = strikes.saturating_sub(policy.credit_per_proven_promise);
        }
    }
    if policy.strikes_per_rung == 0 {
        // A zero divisor is a policy that never demotes, not a panic. The schema forbids it;
        // this line is what makes the schema's promise hold even if the schema is bypassed.
        return Mode::Full;
    }
    match strikes / policy.strikes_per_rung {
        0 => Mode::Full,
        1 => Mode::ContractOnly,
        2 => Mode::PatchOnly,
        _ => Mode::ProposeOnly,
    }
}

/// The mode a seat actually writes in: the folded rung when the ladder is enabled, `Full` otherwise.
#[must_use]
pub fn effective_mode(history: &[NodeRecord], policy: &Ladder) -> Mode {
    if policy.enabled {
        ladder(history, policy)
    } else {
        Mode::Full
    }
}

/// Classifies a unified diff against the policy. `allowance` is what the promise declared at
/// planning time; it is capped by `policy.max_allowance` and added to the base budget, then the
/// whole is capped by `mode`.
#[must_use]
pub fn classify_write(
    diff: &str,
    policy: &KeelPolicy,
    allowance: Option<SurfaceBudget>,
    mode: Mode,
) -> Classification {
    let budget = mode.cap(
        policy.surface.plus(
            allowance
                .unwrap_or_default()
                .capped_by(policy.max_allowance),
        ),
    );
    let counts_block = policy.surface_enforcement == Enforcement::Block;
    let mut charges = Vec::new();
    let mut findings = Vec::new();
    let mut totals: BTreeMap<Debit, u32> = Debit::ALL.iter().map(|d| (*d, 0)).collect();
    let files = parse(diff);
    if files.is_empty() && !diff.trim().is_empty() {
        findings.push(Finding {
            rule: "keel.diff.unparseable".into(),
            path: None,
            detail: "no `+++ b/<path>` header found; nothing was classified".into(),
            blocking: true,
        });
    }
    for file in &files {
        if file.added.len() as u64 > u64::from(policy.body.max_file_lines_delta) {
            findings.push(Finding {
                rule: "keel.body.oversized_change".into(),
                blocking: counts_block,
                path: Some(file.path.clone()),
                detail: format!(
                    "{} added lines, limit {}",
                    file.added.len(),
                    policy.body.max_file_lines_delta
                ),
            });
        }
        let language = Language::of(&file.path);
        let test_file = is_test_path(&file.path);
        if file.is_new && language.is_some() && !test_file {
            charges.push(Charge {
                debit: Debit::NewModule,
                path: file.path.clone(),
                line: 0,
                symbol: file.path.clone(),
            });
        }
        if let Some(language) = language {
            for (index, line) in file.added.iter().enumerate() {
                let line_number = u32::try_from(index + 1).unwrap_or(u32::MAX);
                if let Some((debit, symbol)) = language.classify(line, test_file) {
                    charges.push(Charge {
                        debit,
                        path: file.path.clone(),
                        line: line_number,
                        symbol,
                    });
                }
            }
        } else if let Some(manifest) = Manifest::of(&file.path) {
            for (index, line) in file.added_with_context.iter().enumerate() {
                if let Some(symbol) = manifest.dependency(line, &file.added_with_context[..index]) {
                    charges.push(Charge {
                        debit: Debit::NewDependency,
                        path: file.path.clone(),
                        line: u32::try_from(index + 1).unwrap_or(u32::MAX),
                        symbol,
                    });
                }
            }
        }
    }
    for charge in &charges {
        *totals.entry(charge.debit).or_insert(0) += 1;
    }
    for debit in Debit::ALL {
        let charged = totals.get(&debit).copied().unwrap_or(0);
        let allowed = budget.get(debit);
        if charged > allowed {
            findings.push(Finding {
                rule: debit.over_budget_rule().into(),
                blocking: counts_block,
                path: None,
                detail: format!("charged {charged}, budget {allowed}"),
            });
        }
    }
    let refused = findings.iter().any(|finding| finding.blocking);
    Classification {
        policy_version: policy.version.clone(),
        charges,
        totals,
        budget,
        findings,
        refused,
    }
}

/// Checks a contract card's bounds: paths listed, not globbed; symbols listed; bytes bounded.
/// Returns the rule ids broken, empty when the card is within the policy.
#[must_use]
pub fn check_card(
    scope_paths: &[String],
    exported_symbols: &[String],
    card_bytes: u64,
    policy: &CardLimits,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    if scope_paths.is_empty() {
        findings.push(Finding {
            rule: "keel.card.scope_empty".into(),
            blocking: true,
            path: None,
            detail: "a card names at least one path".into(),
        });
    }
    if scope_paths.len() as u64 > u64::from(policy.max_scope_paths) {
        findings.push(Finding {
            rule: "keel.card.scope_too_wide".into(),
            blocking: false,
            path: None,
            detail: format!(
                "{} paths, limit {}",
                scope_paths.len(),
                policy.max_scope_paths
            ),
        });
    }
    for path in scope_paths {
        if path.contains(['*', '?', '[']) || path.contains("..") {
            findings.push(Finding {
                rule: "keel.card.scope_not_a_path".into(),
                blocking: true,
                path: Some(path.clone()),
                detail: "a scope is a list of paths; a glob is a description".into(),
            });
        }
    }
    if exported_symbols.len() as u64 > u64::from(policy.max_exported_symbols) {
        findings.push(Finding {
            rule: "keel.card.too_many_symbols".into(),
            blocking: false,
            path: None,
            detail: format!(
                "{} exported symbols, limit {}",
                exported_symbols.len(),
                policy.max_exported_symbols
            ),
        });
    }
    if card_bytes > u64::from(policy.max_card_bytes) {
        findings.push(Finding {
            rule: "keel.card.too_large".into(),
            blocking: false,
            path: None,
            detail: format!("{card_bytes} bytes, limit {}", policy.max_card_bytes),
        });
    }
    findings
}

struct DiffFile {
    path: String,
    is_new: bool,
    /// Added lines only (without the leading `+`).
    added: Vec<String>,
    /// Added and context lines in order, with a leading marker `+` or ` ` so manifest section
    /// tracking can see headings that were not themselves added.
    added_with_context: Vec<String>,
}

fn parse(diff: &str) -> Vec<DiffFile> {
    let mut files: Vec<DiffFile> = Vec::new();
    let mut pending_new = false;
    let mut in_hunk = false;
    for raw in diff.lines() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.starts_with("diff --git ") {
            pending_new = false;
            in_hunk = false;
            continue;
        }
        if line.starts_with("new file mode") || line == "--- /dev/null" {
            pending_new = true;
            continue;
        }
        if let Some(path) = line.strip_prefix("+++ ") {
            let path = path.strip_prefix("b/").unwrap_or(path);
            let path = path.split('\t').next().unwrap_or(path).to_owned();
            files.push(DiffFile {
                path,
                is_new: pending_new,
                added: Vec::new(),
                added_with_context: Vec::new(),
            });
            in_hunk = false;
            continue;
        }
        if line.starts_with("--- ") {
            continue;
        }
        if line.starts_with("@@") {
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if let Some(added) = line.strip_prefix('+') {
            file.added.push(added.to_owned());
            file.added_with_context.push(format!("+{added}"));
        } else if let Some(context) = line.strip_prefix(' ') {
            file.added_with_context.push(format!(" {context}"));
        } else if line.is_empty() {
            file.added_with_context.push(" ".into());
        }
    }
    files
}

#[derive(Clone, Copy)]
enum Language {
    Rust,
    TypeScript,
    Python,
}

impl Language {
    fn of(path: &str) -> Option<Self> {
        let extension = path.rsplit('.').next()?;
        match extension {
            "rs" => Some(Self::Rust),
            "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" => Some(Self::TypeScript),
            "py" | "pyi" => Some(Self::Python),
            _ => None,
        }
    }

    /// One added line, one charge at most. Test declarations are charged even in test files;
    /// everything else in a test file is free, because a test file is not public surface.
    fn classify(self, line: &str, test_file: bool) -> Option<(Debit, String)> {
        let trimmed = line.trim_start();
        match self {
            Self::Rust => {
                if trimmed.starts_with("#[test]") || trimmed.starts_with("#[tokio::test") {
                    return Some((Debit::NewTest, "#[test]".into()));
                }
                if test_file {
                    return None;
                }
                let rest = trimmed.strip_prefix("pub ")?;
                if rest.starts_with('(') {
                    // pub(crate), pub(super): not public surface.
                    return None;
                }
                let rest = rest.strip_prefix("async ").unwrap_or(rest);
                let rest = rest.strip_prefix("unsafe ").unwrap_or(rest);
                let rest = rest.strip_prefix("const ").unwrap_or(rest);
                if let Some(name) = rest.strip_prefix("fn ") {
                    return Some((Debit::NewPublicFn, identifier(name)));
                }
                for keyword in ["struct ", "enum ", "trait ", "type ", "union "] {
                    if let Some(name) = rest.strip_prefix(keyword) {
                        return Some((Debit::NewType, identifier(name)));
                    }
                }
                None
            }
            Self::TypeScript => {
                if trimmed.starts_with("it(")
                    || trimmed.starts_with("it.each(")
                    || trimmed.starts_with("test(")
                    || trimmed.starts_with("test.each(")
                {
                    return Some((Debit::NewTest, first_string_literal(trimmed)));
                }
                if test_file {
                    return None;
                }
                let rest = trimmed.strip_prefix("export ")?;
                let rest = rest.strip_prefix("default ").unwrap_or(rest);
                let rest = rest.strip_prefix("declare ").unwrap_or(rest);
                let rest = rest.strip_prefix("abstract ").unwrap_or(rest);
                let rest = rest.strip_prefix("async ").unwrap_or(rest);
                if let Some(name) = rest.strip_prefix("function ") {
                    return Some((Debit::NewPublicFn, identifier(name.trim_start_matches('*'))));
                }
                for keyword in ["interface ", "type ", "class ", "enum "] {
                    if let Some(name) = rest.strip_prefix(keyword) {
                        return Some((Debit::NewType, identifier(name)));
                    }
                }
                for keyword in ["const ", "let ", "var "] {
                    if let Some(binding) = rest.strip_prefix(keyword) {
                        let name = identifier(binding);
                        let after = binding[name.len()..].trim_start();
                        let after = after.strip_prefix(':').map_or(after, |typed| {
                            typed
                                .split_once('=')
                                .map_or("", |(_, value)| value.trim_start())
                        });
                        let value = after.strip_prefix('=').map_or(after, str::trim_start);
                        let value = value.strip_prefix("async ").unwrap_or(value);
                        if value.starts_with('(') || value.starts_with("function") {
                            return Some((Debit::NewPublicFn, name));
                        }
                        return None;
                    }
                }
                None
            }
            Self::Python => {
                if let Some(name) = trimmed.strip_prefix("def test_") {
                    return Some((Debit::NewTest, format!("test_{}", identifier(name))));
                }
                if test_file || line.starts_with(char::is_whitespace) {
                    return None;
                }
                if let Some(name) = trimmed
                    .strip_prefix("def ")
                    .or_else(|| trimmed.strip_prefix("async def "))
                {
                    let name = identifier(name);
                    return (!name.starts_with('_')).then_some((Debit::NewPublicFn, name));
                }
                if let Some(name) = trimmed.strip_prefix("class ") {
                    let name = identifier(name);
                    return (!name.starts_with('_')).then_some((Debit::NewType, name));
                }
                None
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Manifest {
    Cargo,
    PackageJson,
    Requirements,
    PyProject,
}

impl Manifest {
    fn of(path: &str) -> Option<Self> {
        let name = path.rsplit('/').next()?;
        match name {
            "Cargo.toml" => Some(Self::Cargo),
            "package.json" => Some(Self::PackageJson),
            "pyproject.toml" => Some(Self::PyProject),
            _ if name.starts_with("requirements") && name.ends_with(".txt") => {
                Some(Self::Requirements)
            }
            _ => None,
        }
    }

    /// Whether `line` (with its `+`/` ` marker) adds a dependency, given the lines before it in
    /// the same file so the section heading — added or merely context — is known.
    fn dependency(self, line: &str, before: &[String]) -> Option<String> {
        let added = line.strip_prefix('+')?;
        let trimmed = added.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            return None;
        }
        match self {
            Self::Requirements => Some(identifier_loose(trimmed)),
            Self::Cargo | Self::PyProject => {
                if trimmed.starts_with('[') {
                    return None;
                }
                let section = before
                    .iter()
                    .rev()
                    .map(|entry| entry[1..].trim())
                    .find(|entry| entry.starts_with('['))
                    .unwrap_or("");
                let in_dependencies = match self {
                    Self::Cargo => section.contains("dependencies]"),
                    _ => {
                        section.starts_with("[tool.poetry.dependencies")
                            || section.starts_with("[tool.poetry.group")
                            || section.starts_with("[project.optional-dependencies")
                    }
                };
                if in_dependencies {
                    let (key, _) = trimmed.split_once('=')?;
                    return Some(key.trim().trim_matches('"').to_owned());
                }
                if matches!(self, Self::PyProject)
                    && section.starts_with("[project")
                    && trimmed.starts_with('"')
                    && before.iter().rev().any(|entry| {
                        let entry = entry[1..].trim();
                        entry.starts_with("dependencies")
                            || entry.starts_with("optional-dependencies")
                    })
                {
                    return Some(identifier_loose(trimmed.trim_matches(['"', ','])));
                }
                None
            }
            Self::PackageJson => {
                let mut depth = 0_i32;
                let mut in_dependencies = false;
                for entry in before {
                    let text = entry[1..].trim();
                    if depth == 1
                        && (text.starts_with("\"dependencies\"")
                            || text.starts_with("\"devDependencies\"")
                            || text.starts_with("\"peerDependencies\"")
                            || text.starts_with("\"optionalDependencies\""))
                    {
                        in_dependencies = true;
                    }
                    for character in text.chars() {
                        match character {
                            '{' => depth += 1,
                            '}' => {
                                depth -= 1;
                                if depth <= 1 {
                                    in_dependencies = false;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                if in_dependencies && trimmed.starts_with('"') {
                    let name = trimmed.trim_start_matches('"');
                    let name = name.split('"').next()?;
                    return Some(name.to_owned());
                }
                None
            }
        }
    }
}

fn is_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    lower.contains("/tests/")
        || lower.contains("/test/")
        || lower.contains("/__tests__/")
        || lower.starts_with("tests/")
        || lower.starts_with("test/")
        || name.starts_with("test_")
        || name.ends_with("_test.py")
        || name.ends_with("_test.rs")
        || name.contains(".test.")
        || name.contains(".spec.")
}

fn identifier(text: &str) -> String {
    text.chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .collect()
}

fn identifier_loose(text: &str) -> String {
    text.chars()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '@' | '/'))
        .collect()
}

fn first_string_literal(text: &str) -> String {
    for quote in ['"', '\'', '`'] {
        if let Some((_, rest)) = text.split_once(quote)
            && let Some((literal, _)) = rest.split_once(quote)
        {
            return literal.to_owned();
        }
    }
    String::new()
}
