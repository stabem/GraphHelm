use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path};

#[cfg(test)]
use std::path::PathBuf;

use graphhelm_protocols::Diagnostic;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::bounded_value::{BoundedValueError, ValueBudget, ValueLimits, parse_json, parse_yaml};

const SOURCE: &str = "extension-package";
const MANIFEST_NAME: &str = "extension.json";
const MAX_CONTRIBUTIONS: usize = 256;
const MAX_RESOURCE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_PACKAGE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 512;
const MAX_SURFACES: usize = 32;
const MAX_AUTHORITY_ITEMS: usize = 64;
const MAX_ARTIFACT_FLOWS: usize = 64;
const MAX_ARTIFACT_FLOW_REFS: usize = 64;
const MAX_INVENTORY_ENTRIES: usize = 1024;
const MAX_INVENTORY_DEPTH: usize = 16;
const MAX_PACKAGE_DIAGNOSTICS: usize = 256;
const MANIFEST_VALUE_LIMITS: ValueLimits = ValueLimits {
    max_depth: 128,
    max_values: 128 * 1024,
    max_key_bytes: MAX_RESOURCE_BYTES as usize,
    max_string_bytes: MAX_RESOURCE_BYTES as usize,
};
const RESOURCE_VALUE_LIMITS: ValueLimits = ValueLimits {
    max_depth: 64,
    max_values: 32 * 1024,
    max_key_bytes: 512 * 1024,
    max_string_bytes: MAX_RESOURCE_BYTES as usize,
};
const PACKAGE_SCHEMA_VALUE_LIMITS: ValueLimits = ValueLimits {
    max_depth: 64,
    max_values: 128 * 1024,
    max_key_bytes: 2 * 1024 * 1024,
    max_string_bytes: MAX_PACKAGE_BYTES as usize,
};
const ARTIFACT_FLOW_FORMAT: &str = "p50.dev/jpd/artifact-flow/v1";
const DIAGNOSTIC_LIMIT_MESSAGE: &str =
    "extension package diagnostic limit reached; remaining validation omitted";

const ARTIFACT_REF_PREFIXES: &[&str] = &[
    "task",
    "runtime",
    "schema",
    "evaluator",
    "observer",
    "policy",
    "artifact",
];

const CONTRIBUTION_DIRECTORIES: &[&str] = &[
    "skills",
    "agents",
    "schemas",
    "policies",
    "evaluators",
    "observers",
    "fixtures",
    "graphs",
    ".claude-plugin",
    ".codex-plugin",
];

const CONTRIBUTION_KINDS: &[&str] = &[
    "skill",
    "agent",
    "schema",
    "policy",
    "evaluator",
    "observer",
    "graph",
    "fixture",
    "host-adapter",
];

const CONTRIBUTION_EFFECTS: &[&str] = &[
    "artifact.local.write",
    "artifact.propose",
    "external.read",
    "host.discover",
    "runtime.connect",
    "runtime.mutate",
    "runtime.read",
];

const CONTRIBUTION_PERMISSIONS: &[&str] = &[
    "network.external",
    "network.loopback",
    "owner.decision.request",
    "package.read",
    "runtime.read",
    "runtime.write",
    "token.reference.read",
    "workspace.artifact.write",
];

/// Every entry here and in `CLI_COMMANDS` below is a membership test against a hand-maintained
/// literal - neither list is compared to the real CLI/MCP surface anywhere on its own. The
/// `cli_and_mcp_surface_allowlists_stay_a_subset_of_the_real_derived_surface` guard in
/// `apps/cli/tests/extension_cli.rs` derives the real surface from the built binary's own
/// `--help` output and asserts both lists stay a SUBSET of it - that catches the dangerous
/// direction (naming a surface that does not exist) whether the cause is a rename, a removal, or
/// a stale hand entry, without requiring either list to be the FULL surface (see
/// `CLI_COMMANDS`'s own policy note below for why it deliberately is not).
const MCP_TOOLS: &[&str] = &[
    "start",
    "status",
    "events",
    "signal",
    "approve",
    "pause",
    "resume",
    "cancel",
    "routes",
    "wake_arm",
    "wake_status",
    "amend_budget",
    "wake_wait",
    "probe",
    "resolve_contract",
    "memory_status",
    "present",
    "compile_context",
    "memory_propose",
    "accounting",
];

/// DELIBERATE ALLOWLIST, not an enumeration of every real command - a skill's declared `surface`
/// is a claim about what it uses, and this list is the DOMAIN surface a journey may legitimately
/// claim to drive or observe (graph/execution/schema objects, plus the read-shaped `events
/// verify`), not the full CLI. Excluded on purpose, and enumerated here so the gap is declared
/// rather than accidental (checked against `args.rs` on this branch; the guard below keeps this
/// list a subset of the real surface even if the real surface's OWN shape changes):
///
/// - `schema check`, `schema migrate`, `graph draft apply`, `gateway credential set|remove`,
///   `events rebuild|backup|restore` - mutating or destructive infrastructure operations
///   (credential storage, event-store disaster recovery, schema/graph rewrites). A skill
///   claiming these as its surface is claiming administrative authority this PR's own refusal
///   family (capsules cannot self-install/activate/promote) exists to deny elsewhere; letting the
///   allowlist grant it here would be the same authority boundary with a hole in one corner.
/// - `gateway routes`, `gateway probe`, `tool invoke` - operator/infrastructure surfaces (model
///   routing, arbitrary brokered tool execution), not domain objects a journey observes.
/// - `serve`, `wake-wait` - process-lifecycle primitives (starting the API server, an internal
///   doorbell wait), not an action a skill's journey narrates.
///
/// If a future skill genuinely needs one of these, that is a decision to widen this list on
/// purpose - not a defect in this comment or the guard that checks it.
const CLI_COMMANDS: &[&str] = &[
    "graph validate",
    "graph lint",
    "graph hash",
    "graph simulate",
    "graph replay",
    "schema catalog",
    "schema conformance",
    "schema view",
    "schema digest",
    "execution start",
    "execution status",
    "execution signal",
    "execution approve",
    "execution pause",
    "execution resume",
    "execution cancel",
    "execution amend-budget",
    "events verify",
    "quality certify",
    "extension validate",
    "mcp",
];

/// Test-support only: exposes the two allowlists so a cross-crate test
/// (`apps/cli/tests/extension_cli.rs`) can assert they stay a subset of the real, derived CLI/MCP
/// surface. Not part of the crate's ordinary public API - `#[doc(hidden)]` keeps it out of
/// rendered docs, and the name says what it is for.
#[doc(hidden)]
pub fn __surface_allowlists_for_testing() -> (&'static [&'static str], &'static [&'static str]) {
    (CLI_COMMANDS, MCP_TOOLS)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Contribution {
    id: String,
    kind: String,
    path: String,
    sha256: String,
    effects: Vec<String>,
    permissions: Vec<String>,
    requires: ContributionRequires,
    #[serde(default)]
    surfaces: Vec<String>,
    #[serde(default)]
    family: Option<String>,
    #[serde(default)]
    schema: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContributionRequires {
    capabilities: Vec<String>,
    observers: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactFlow {
    family: String,
    inputs: Vec<String>,
    outputs: Vec<String>,
}

struct PackageRecord {
    path: String,
    digest: String,
}

struct ValidatedResource {
    contribution: Contribution,
    // Compatibility field for focused graph tests. Production validation derives
    // the logical source label from contribution.path and never retains a host path.
    #[cfg(test)]
    #[allow(dead_code)]
    path: PathBuf,
    bytes: Vec<u8>,
    base_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapabilityError {
    InvalidPath,
    NotFound,
    LinkedOrWrongType,
    LimitExceeded,
    Io,
}

#[derive(Debug)]
struct PackageCapability {
    // Unix retains every directory used to reach the package root. Windows opens
    // the root with no delete sharing, so the single root handle pins its name.
    _root_ancestors: Vec<File>,
    root: File,
}

#[derive(Debug)]
struct OpenedRegular {
    file: File,
    // Keep every package-relative ancestor live until the captured file is done.
    _ancestors: Vec<File>,
}

enum InventoryChild {
    Directory(File),
    Regular(File),
}

enum EnumeratedName {
    Canonical(String),
    NonCanonical,
}

impl PackageCapability {
    fn open(path: &Path) -> Result<Self, CapabilityError> {
        let (_root_ancestors, root) = open_package_root(path)?;
        Ok(Self {
            _root_ancestors,
            root,
        })
    }

    fn open_manifest(&self) -> Result<OpenedRegular, CapabilityError> {
        self.open_regular_components(MANIFEST_NAME)
    }

    fn open_regular(&self, relative: &str) -> Result<OpenedRegular, CapabilityError> {
        if !valid_relative_path(relative) {
            return Err(CapabilityError::InvalidPath);
        }
        self.open_regular_components(relative)
    }

    fn open_regular_components(&self, relative: &str) -> Result<OpenedRegular, CapabilityError> {
        let mut components = relative.split('/').peekable();
        let mut current = self.root.try_clone().map_err(|_| CapabilityError::Io)?;
        let mut ancestors = Vec::new();
        while let Some(component) = components.next() {
            if components.peek().is_none() {
                let file = open_child_regular(&current, component)?;
                ancestors.push(current);
                return Ok(OpenedRegular {
                    file,
                    _ancestors: ancestors,
                });
            }
            let next = open_child_directory(&current, component)?;
            ancestors.push(current);
            current = next;
        }
        Err(CapabilityError::InvalidPath)
    }
}

impl OpenedRegular {
    fn read_bounded(&mut self, limit: u64) -> Result<Vec<u8>, CapabilityError> {
        let metadata = self.file.metadata().map_err(|_| CapabilityError::Io)?;
        if !metadata.is_file() {
            return Err(CapabilityError::LinkedOrWrongType);
        }
        if metadata.len() > limit {
            return Err(CapabilityError::LimitExceeded);
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        Read::by_ref(&mut self.file)
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| CapabilityError::Io)?;
        if bytes.len() as u64 > limit {
            return Err(CapabilityError::LimitExceeded);
        }
        Ok(bytes)
    }
}

#[derive(Default)]
struct DiagnosticCollector {
    diagnostics: Vec<Diagnostic>,
    omitted: bool,
}

impl DiagnosticCollector {
    fn push(&mut self, diagnostic: Diagnostic) {
        if self.diagnostics.len() < MAX_PACKAGE_DIAGNOSTICS {
            self.diagnostics.push(diagnostic);
        } else {
            self.omitted = true;
        }
    }

    fn extend(&mut self, diagnostics: impl IntoIterator<Item = Diagnostic>) {
        for diagnostic in diagnostics {
            if self.should_stop() {
                break;
            }
            self.push(diagnostic);
        }
    }

    fn should_stop(&mut self) -> bool {
        if self.diagnostics.len() < MAX_PACKAGE_DIAGNOSTICS {
            false
        } else {
            self.omitted = true;
            true
        }
    }

    fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    fn into_diagnostics(mut self) -> Vec<Diagnostic> {
        if self.omitted {
            self.diagnostics.push(error(
                "GHEX011_LIMIT",
                DIAGNOSTIC_LIMIT_MESSAGE,
                "/diagnostics",
            ));
        }
        self.diagnostics
    }
}

struct PackageGrants {
    package_read: bool,
    workspace_artifact_write: bool,
    network_external: bool,
    network_loopback: bool,
    runtime_read: bool,
    runtime_mutations: BTreeSet<String>,
    owner_confirmation: bool,
    token_reference_read: bool,
}

impl PackageGrants {
    fn from_manifest(manifest: &serde_json::Value) -> Self {
        let runtime_mutations = manifest
            .pointer("/spec/permissions/runtime/mutations")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_owned)
            .collect();
        let owner_confirmation = manifest
            .pointer("/spec/permissions/runtime/ownerConfirmationRequired")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|values| !values.is_empty());
        Self {
            package_read: manifest
                .pointer("/spec/permissions/filesystem/package")
                .and_then(serde_json::Value::as_str)
                == Some("read"),
            workspace_artifact_write: manifest
                .pointer("/spec/permissions/filesystem/workspaceArtifacts")
                .and_then(serde_json::Value::as_str)
                == Some("proposal-write"),
            network_external: manifest
                .pointer("/spec/permissions/network/external")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            network_loopback: manifest
                .pointer("/spec/permissions/network/loopbackRuntimeApi")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            runtime_read: manifest
                .pointer("/spec/permissions/runtime/read")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            runtime_mutations,
            owner_confirmation,
            token_reference_read: manifest
                .pointer("/spec/permissions/secrets/tokenFile")
                .and_then(serde_json::Value::as_str)
                == Some("reference-only"),
        }
    }

    fn grants_effect(&self, effect: &str) -> bool {
        match effect {
            "artifact.local.write" => self.workspace_artifact_write,
            "artifact.propose" => true,
            "external.read" => self.network_external,
            "host.discover" => self.package_read,
            "runtime.connect" => self.network_loopback,
            "runtime.mutate" => !self.runtime_mutations.is_empty(),
            "runtime.read" => self.runtime_read,
            _ => false,
        }
    }

    fn grants_permission(&self, permission: &str) -> bool {
        match permission {
            "network.external" => self.network_external,
            "network.loopback" => self.network_loopback,
            "owner.decision.request" => self.owner_confirmation,
            "package.read" => self.package_read,
            "runtime.read" => self.runtime_read,
            "runtime.write" => !self.runtime_mutations.is_empty(),
            "token.reference.read" => self.token_reference_read,
            "workspace.artifact.write" => self.workspace_artifact_write,
            _ => false,
        }
    }
}

/// One contribution's own declared authority, surfaced (never re-derived) from what the
/// validator already parsed and checked. `surfaces` is the closed-allowlist-checked claim about
/// which MCP tools / CLI commands this contribution drives (§`MCP_TOOLS`/`CLI_COMMANDS` above);
/// consumers that need only the MCP subset intersect it themselves rather than this type
/// filtering it, so a consumer added later is not silently handed a narrower view than the one
/// the manifest actually declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedContribution {
    pub id: String,
    pub surfaces: Vec<String>,
    pub effects: Vec<String>,
    pub permissions: Vec<String>,
    pub required_capabilities: Vec<String>,
}

/// Validated, presentation-neutral summary of an extension package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedExtensionPackage {
    pub id: String,
    pub version: String,
    pub contribution_count: usize,
    pub package_digest: String,
    pub contributions: Vec<ValidatedContribution>,
}

/// Validate an extension package using only local, bounded resources.
pub fn validate_extension_package(
    package: &Path,
) -> Result<ValidatedExtensionPackage, Vec<Diagnostic>> {
    let mut collector = DiagnosticCollector::default();
    match validate_package(package, &mut collector) {
        Ok((manifest, records)) => Ok(ValidatedExtensionPackage {
            id: manifest["metadata"]["id"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            version: manifest["metadata"]["version"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            contribution_count: records.len(),
            package_digest: package_digest(&manifest, &records),
            contributions: validated_contributions(&manifest),
        }),
        Err(()) => {
            let mut diagnostics = collector.into_diagnostics();
            diagnostics.sort_by(|left, right| {
                (&left.path, &left.code, &left.message).cmp(&(
                    &right.path,
                    &right.code,
                    &right.message,
                ))
            });
            Err(diagnostics)
        }
    }
}

/// Reads `spec.contracts.contributions` back through the SAME `Contribution` deserialization the
/// validator already ran, rather than a second hand-rolled JSON reader — a re-parse of already-
/// validated bytes cannot drift from what the validator itself checked, which a parallel reader
/// of the same field names could.
fn validated_contributions(manifest: &serde_json::Value) -> Vec<ValidatedContribution> {
    manifest["spec"]["contracts"]["contributions"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|value| {
            let contribution: Contribution = serde_json::from_value(value.clone())
                .expect("every contribution here already passed validation");
            ValidatedContribution {
                id: contribution.id,
                surfaces: contribution.surfaces,
                effects: contribution.effects,
                permissions: contribution.permissions,
                required_capabilities: contribution.requires.capabilities,
            }
        })
        .collect()
}

fn validate_package(
    package: &Path,
    diagnostics: &mut DiagnosticCollector,
) -> Result<(serde_json::Value, Vec<PackageRecord>), ()> {
    let root = PackageCapability::open(package).map_err(|failure| {
        diagnostics.push(root_diagnostic(failure));
    })?;
    let mut opened_manifest = root.open_manifest().map_err(|failure| {
        diagnostics.push(manifest_open_diagnostic(failure));
    })?;
    let manifest =
        parse_opened_manifest(&mut opened_manifest).map_err(|errors| diagnostics.extend(errors))?;
    drop(opened_manifest);
    let raw_contributions = manifest
        .pointer("/spec/contracts/contributions")
        .cloned()
        .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
    let Some(items) = raw_contributions.as_array() else {
        diagnostics.push(error(
            "GHEX003_CONTRIBUTION",
            "contracts.contributions must be an array",
            "/spec/contracts/contributions",
        ));
        return Err(());
    };
    if items.len() > MAX_CONTRIBUTIONS {
        diagnostics.push(error(
            "GHEX011_LIMIT",
            "extension exceeds the contribution count limit",
            "/spec/contracts/contributions",
        ));
        return Err(());
    }
    let extension_id = manifest["metadata"]["id"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let extension_version = manifest["metadata"]["version"]
        .as_str()
        .unwrap_or_default()
        .to_owned();

    let package_grants = PackageGrants::from_manifest(&manifest);
    let mut contributions = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        if diagnostics.should_stop() {
            break;
        }
        match serde_json::from_value::<Contribution>(item.clone()) {
            Ok(contribution) => contributions.push((index, contribution)),
            Err(_) => diagnostics.push(error(
                "GHEX003_CONTRIBUTION",
                "contribution does not satisfy the closed contribution contract",
                format!("/spec/contracts/contributions/{index}"),
            )),
        }
    }
    if diagnostics.should_stop() {
        return Err(());
    }
    let artifact_flow_refs = contribution_artifact_flow_refs(&contributions);
    validate_artifact_flows(&manifest, &artifact_flow_refs, diagnostics);
    if diagnostics.should_stop() {
        return Err(());
    }
    validate_contracts_fields(&manifest, diagnostics);
    if diagnostics.should_stop() {
        return Err(());
    }

    let mut ids = BTreeSet::new();
    let mut contribution_kinds = BTreeMap::new();
    let mut paths = BTreeSet::new();
    let mut portable_paths = BTreeSet::new();
    let declared_schema_paths = contributions
        .iter()
        .filter(|(_, contribution)| contribution.kind == "schema")
        .map(|(_, contribution)| contribution.path.clone())
        .collect::<BTreeSet<_>>();
    let mut aggregate_bytes = 0u64;
    let mut records = Vec::with_capacity(contributions.len());
    let mut resources = Vec::with_capacity(contributions.len());
    for (index, contribution) in contributions {
        if diagnostics.should_stop() {
            break;
        }
        let base_path = format!("/spec/contracts/contributions/{index}");
        if !valid_identifier(&contribution.id) || !ids.insert(contribution.id.clone()) {
            diagnostics.push(error(
                "GHEX003_CONTRIBUTION",
                "contribution id must be bounded, non-empty, and unique",
                format!("{base_path}/id"),
            ));
        } else {
            contribution_kinds.insert(contribution.id.clone(), contribution.kind.clone());
        }
        if !CONTRIBUTION_KINDS.contains(&contribution.kind.as_str()) {
            diagnostics.push(error(
                "GHEX003_CONTRIBUTION",
                "contribution kind is not supported",
                format!("{base_path}/kind"),
            ));
        }
        if is_host_adapter_path(&contribution.path) != (contribution.kind == "host-adapter") {
            diagnostics.push(error(
                "GHEX013_HOST",
                "host adapter paths and contribution kinds must match",
                format!("{base_path}/kind"),
            ));
        }
        if contribution
            .family
            .as_deref()
            .is_some_and(|family| !valid_identifier(family))
        {
            diagnostics.push(error(
                "GHEX003_CONTRIBUTION",
                "contribution family must be bounded and non-empty",
                format!("{base_path}/family"),
            ));
        }
        if matches!(
            contribution.kind.as_str(),
            "policy" | "evaluator" | "observer"
        ) && contribution.schema.is_none()
        {
            diagnostics.push(error(
                "GHEX014_SCHEMA_REF",
                "typed data contribution must declare a package schema",
                format!("{base_path}/schema"),
            ));
        }
        validate_declared_surfaces(&contribution.surfaces, &base_path, diagnostics);
        if diagnostics.should_stop() {
            break;
        }
        validate_authority(&contribution, &base_path, diagnostics);
        if diagnostics.should_stop() {
            break;
        }
        validate_authority_subset(&contribution, &package_grants, &base_path, diagnostics);
        if diagnostics.should_stop() {
            break;
        }

        if !paths.insert(contribution.path.clone())
            || !portable_paths.insert(contribution.path.to_ascii_lowercase())
        {
            diagnostics.push(error(
                "GHEX003_CONTRIBUTION",
                "contribution path must be unique",
                format!("{base_path}/path"),
            ));
            continue;
        }
        let mut opened_resource = match root.open_regular(&contribution.path) {
            Ok(file) => file,
            Err(diagnostic) => {
                diagnostics.push(at_contribution(
                    resource_open_diagnostic(diagnostic),
                    &base_path,
                ));
                continue;
            }
        };
        let bytes = match opened_resource.read_bounded(MAX_RESOURCE_BYTES) {
            Ok(bytes) => bytes,
            Err(diagnostic) => {
                diagnostics.push(at_contribution(
                    resource_read_diagnostic(diagnostic),
                    &base_path,
                ));
                continue;
            }
        };
        aggregate_bytes = aggregate_bytes.saturating_add(bytes.len() as u64);
        if aggregate_bytes > MAX_PACKAGE_BYTES {
            diagnostics.push(error(
                "GHEX011_LIMIT",
                "extension exceeds the aggregate resource size limit",
                format!("{base_path}/path"),
            ));
            continue;
        }

        if !valid_sha256(&contribution.sha256) {
            diagnostics.push(error(
                "GHEX005_DIGEST",
                "contribution sha256 must use canonical lowercase sha256 form",
                format!("{base_path}/sha256"),
            ));
            continue;
        }
        let actual_digest = sha256(&bytes);
        if actual_digest != contribution.sha256 {
            diagnostics.push(error(
                "GHEX005_DIGEST",
                "contribution digest does not match the declared sha256",
                format!("{base_path}/sha256"),
            ));
            continue;
        }

        records.push(PackageRecord {
            path: contribution.path.clone(),
            digest: actual_digest,
        });
        resources.push(ValidatedResource {
            #[cfg(test)]
            path: PathBuf::from(&contribution.path),
            contribution,
            bytes,
            base_path,
        });
    }
    if diagnostics.should_stop() {
        return Err(());
    }
    validate_contribution_contents_bounded(
        &resources,
        &declared_schema_paths,
        &contribution_kinds,
        &extension_id,
        &extension_version,
        diagnostics,
    );
    if diagnostics.should_stop() {
        return Err(());
    }
    validate_inventory(&root, &paths, diagnostics);

    if diagnostics.is_empty() {
        Ok((manifest, records))
    } else {
        Err(())
    }
}

fn validate_inventory(
    root: &PackageCapability,
    declared_paths: &BTreeSet<String>,
    diagnostics: &mut DiagnosticCollector,
) {
    if diagnostics.should_stop() {
        return;
    }
    let entries = match enumerate_directory(&root.root, MAX_INVENTORY_ENTRIES) {
        Ok(entries) => entries,
        Err(CapabilityError::LimitExceeded) => {
            diagnostics.push(error(
                "GHEX011_LIMIT",
                "extension package inventory exceeds its entry limit",
                "/inventory",
            ));
            return;
        }
        Err(_) => {
            diagnostics.push(error(
                "GHEX001_PACKAGE",
                "extension package inventory cannot be read",
                "/inventory",
            ));
            return;
        }
    };
    let mut entry_count = entries.len();
    for entry in entries {
        if diagnostics.should_stop() {
            break;
        }
        let EnumeratedName::Canonical(name) = entry else {
            diagnostics.push(error(
                "GHEX012_INVENTORY",
                "extension package contains a non-canonical entry name",
                "/inventory",
            ));
            continue;
        };
        let child = match inspect_inventory_child(&root.root, &name) {
            Ok(child) => child,
            Err(CapabilityError::LinkedOrWrongType) => {
                diagnostics.push(error(
                    "GHEX004_PATH",
                    "extension package inventory cannot contain links or special files",
                    inventory_path(&name),
                ));
                continue;
            }
            Err(_) => {
                diagnostics.push(inventory_error(
                    &name,
                    "extension package entry cannot be inspected",
                ));
                continue;
            }
        };
        match (name.as_str(), child) {
            (MANIFEST_NAME, InventoryChild::Regular(_)) => {}
            (MANIFEST_NAME, InventoryChild::Directory(_)) => {
                diagnostics.push(inventory_error(
                    &name,
                    "extension.json must be a regular file",
                ));
            }
            ("README.md", InventoryChild::Regular(file)) => {
                if file
                    .metadata()
                    .map_or(true, |metadata| metadata.len() > MAX_RESOURCE_BYTES)
                {
                    diagnostics.push(inventory_error(
                        &name,
                        "README.md must be a bounded regular file",
                    ));
                }
            }
            ("README.md", InventoryChild::Directory(_)) => {
                diagnostics.push(inventory_error(
                    &name,
                    "README.md must be a bounded regular file",
                ));
            }
            (".mcp.json", InventoryChild::Regular(_)) => {
                if !declared_paths.contains(&name) {
                    diagnostics.push(inventory_error(
                        &name,
                        "discoverable extension file is not declared by the manifest",
                    ));
                }
            }
            (".mcp.json", InventoryChild::Directory(_)) => {
                diagnostics.push(inventory_error(
                    &name,
                    "host adapter must be a regular file",
                ));
            }
            (directory, InventoryChild::Directory(handle))
                if CONTRIBUTION_DIRECTORIES.contains(&directory) =>
            {
                scan_contribution_directory(
                    handle,
                    name,
                    1,
                    declared_paths,
                    &mut entry_count,
                    diagnostics,
                );
            }
            (directory, InventoryChild::Regular(_))
                if CONTRIBUTION_DIRECTORIES.contains(&directory) =>
            {
                diagnostics.push(inventory_error(
                    &name,
                    "contribution namespace must be a directory",
                ));
            }
            (_, _) => diagnostics.push(inventory_error(
                &name,
                "extension package contains an unknown top-level entry",
            )),
        }
    }
}

fn scan_contribution_directory(
    directory: File,
    relative_directory: String,
    depth: usize,
    declared_paths: &BTreeSet<String>,
    entry_count: &mut usize,
    diagnostics: &mut DiagnosticCollector,
) {
    if diagnostics.should_stop() {
        return;
    }
    if depth > MAX_INVENTORY_DEPTH {
        diagnostics.push(error(
            "GHEX011_LIMIT",
            "extension package inventory exceeds its depth limit",
            inventory_path(&relative_directory),
        ));
        return;
    }
    let remaining = MAX_INVENTORY_ENTRIES.saturating_sub(*entry_count);
    let entries = match enumerate_directory(&directory, remaining) {
        Ok(entries) => entries,
        Err(CapabilityError::LimitExceeded) => {
            diagnostics.push(error(
                "GHEX011_LIMIT",
                "extension package inventory exceeds its entry limit",
                "/inventory",
            ));
            return;
        }
        Err(_) => {
            diagnostics.push(inventory_error(
                &relative_directory,
                "contribution directory cannot be read",
            ));
            return;
        }
    };
    *entry_count = entry_count.saturating_add(entries.len());
    for entry in entries {
        if diagnostics.should_stop() {
            break;
        }
        let EnumeratedName::Canonical(name) = entry else {
            diagnostics.push(error(
                "GHEX012_INVENTORY",
                "extension package contains a non-canonical entry name",
                "/inventory",
            ));
            continue;
        };
        let relative = format!("{relative_directory}/{name}");
        match inspect_inventory_child(&directory, &name) {
            Ok(InventoryChild::Directory(handle)) => scan_contribution_directory(
                handle,
                relative,
                depth + 1,
                declared_paths,
                entry_count,
                diagnostics,
            ),
            Ok(InventoryChild::Regular(_)) => {
                if !declared_paths.contains(&relative) {
                    diagnostics.push(inventory_error(
                        &relative,
                        "discoverable extension file is not declared by the manifest",
                    ));
                }
            }
            Err(CapabilityError::LinkedOrWrongType) => diagnostics.push(error(
                "GHEX004_PATH",
                "extension package inventory cannot contain links or special files",
                inventory_path(&relative),
            )),
            Err(_) => {
                diagnostics.push(inventory_error(
                    &relative,
                    "contribution entry cannot be inspected",
                ));
            }
        }
    }
}

fn inventory_error(relative: &str, message: &'static str) -> Diagnostic {
    error("GHEX012_INVENTORY", message, inventory_path(relative))
}

fn inventory_path(relative: &str) -> String {
    format!(
        "/inventory/{}",
        relative.replace('~', "~0").replace('/', "~1")
    )
}

fn root_diagnostic(failure: CapabilityError) -> Diagnostic {
    let message = match failure {
        CapabilityError::LinkedOrWrongType => "extension package must be a real directory",
        _ => "extension package directory cannot be inspected",
    };
    error("GHEX001_PACKAGE", message, "/package")
}

fn manifest_open_diagnostic(failure: CapabilityError) -> Diagnostic {
    let message = match failure {
        CapabilityError::NotFound => "extension.json is required",
        _ => "extension.json must be a bounded regular file",
    };
    error("GHEX001_PACKAGE", message, "/extension.json")
}

fn resource_open_diagnostic(failure: CapabilityError) -> Diagnostic {
    match failure {
        CapabilityError::InvalidPath => error(
            "GHEX004_PATH",
            "contribution path is not a canonical package-relative path",
            "/path",
        ),
        CapabilityError::LinkedOrWrongType => error(
            "GHEX004_PATH",
            "contribution path must name a regular file, not a link",
            "/path",
        ),
        _ => error(
            "GHEX004_PATH",
            "contribution path does not name a readable package file",
            "/path",
        ),
    }
}

fn resource_read_diagnostic(failure: CapabilityError) -> Diagnostic {
    match failure {
        CapabilityError::LimitExceeded => error(
            "GHEX011_LIMIT",
            "contribution exceeds the resource size limit",
            "/path",
        ),
        _ => error("GHEX004_PATH", "contribution file cannot be read", "/path"),
    }
}

fn parse_opened_manifest(
    manifest: &mut OpenedRegular,
) -> Result<serde_json::Value, Vec<Diagnostic>> {
    let bytes = manifest
        .read_bounded(MAX_RESOURCE_BYTES)
        .map_err(|failure| vec![manifest_open_diagnostic(failure)])?;
    let mut budget = ValueBudget::default();
    let raw = parse_json(&bytes, MANIFEST_VALUE_LIMITS, &mut budget).map_err(|failure| {
        let (code, message) = if bounded_value_limit_exceeded(failure) {
            (
                "GHEX011_LIMIT",
                "extension manifest exceeds deterministic JSON structure limits",
            )
        } else {
            ("GHEX001_PACKAGE", "extension manifest cannot be loaded")
        };
        vec![error(code, message, "/extension.json")]
    })?;
    let diagnostics = crate::validate_extension_value(&raw, MANIFEST_NAME);
    if diagnostics.is_empty() {
        Ok(raw)
    } else {
        Err(diagnostics
            .into_iter()
            .map(|diagnostic| {
                if diagnostic.code == "GHS002_SCHEMA" {
                    error(
                        "GHEX002_MANIFEST",
                        "extension manifest does not satisfy its schema",
                        diagnostic.path,
                    )
                } else {
                    error(
                        "GHEX001_PACKAGE",
                        "extension manifest cannot be loaded",
                        "/extension.json",
                    )
                }
            })
            .collect())
    }
}

fn valid_relative_path(relative: &str) -> bool {
    !relative.is_empty()
        && relative.len() <= MAX_PATH_BYTES
        && relative != MANIFEST_NAME
        && !relative.starts_with('/')
        && !relative.ends_with('/')
        && !relative.contains('\0')
        && !relative.contains('\\')
        && !relative.split('/').any(|segment| {
            segment.is_empty()
                || matches!(segment, "." | "..")
                || segment.contains(':')
                || segment.chars().any(char::is_control)
        })
        && Path::new(relative)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn valid_capability_component(name: &str) -> bool {
    !name.is_empty()
        && !matches!(name, "." | "..")
        && !name.contains(['\0', '/', '\\'])
        && !name.chars().any(char::is_control)
}

#[cfg(unix)]
fn open_package_root(path: &Path) -> Result<(Vec<File>, File), CapabilityError> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| CapabilityError::Io)?
            .join(path)
    };
    let root_name = CString::new("/").map_err(|_| CapabilityError::Io)?;
    // SAFETY: the fixed root path is NUL-terminated and the returned descriptor is owned.
    let descriptor = unsafe {
        libc::open(
            root_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if descriptor < 0 {
        return Err(capability_error_from_io(std::io::Error::last_os_error()));
    }
    // SAFETY: the fresh descriptor is transferred exactly once.
    let mut current = unsafe { File::from_raw_fd(descriptor) };
    let mut ancestors = Vec::new();
    for component in absolute.components() {
        let part = match component {
            Component::RootDir | Component::CurDir => continue,
            Component::ParentDir => std::ffi::OsStr::new(".."),
            Component::Normal(part) => part,
            Component::Prefix(_) => return Err(CapabilityError::InvalidPath),
        };
        let name = CString::new(part.as_bytes()).map_err(|_| CapabilityError::InvalidPath)?;
        // SAFETY: current is a retained directory and name is a live NUL-terminated component.
        let next = unsafe {
            libc::openat(
                current.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if next < 0 {
            return Err(capability_error_from_io(std::io::Error::last_os_error()));
        }
        ancestors.push(current);
        // SAFETY: the fresh descriptor is transferred exactly once.
        current = unsafe { File::from_raw_fd(next) };
    }
    validate_opened_directory(&current)?;
    Ok((ancestors, current))
}

#[cfg(windows)]
fn open_package_root(path: &Path) -> Result<(Vec<File>, File), CapabilityError> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| CapabilityError::Io)?
            .join(path)
    };
    let root = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(absolute)
        .map_err(capability_error_from_io)?;
    validate_opened_directory(&root)?;
    Ok((Vec::new(), root))
}

#[cfg(unix)]
fn open_child_directory(directory: &File, name: &str) -> Result<File, CapabilityError> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};

    if !valid_capability_component(name) {
        return Err(CapabilityError::InvalidPath);
    }
    let name = CString::new(name).map_err(|_| CapabilityError::InvalidPath)?;
    // SAFETY: directory and the NUL-terminated package component remain live for the call.
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if descriptor < 0 {
        return Err(capability_error_from_io(std::io::Error::last_os_error()));
    }
    // SAFETY: the fresh descriptor is transferred exactly once.
    let file = unsafe { File::from_raw_fd(descriptor) };
    validate_opened_directory(&file)?;
    Ok(file)
}

#[cfg(unix)]
fn open_child_regular(directory: &File, name: &str) -> Result<File, CapabilityError> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};

    if !valid_capability_component(name) {
        return Err(CapabilityError::InvalidPath);
    }
    let name = CString::new(name).map_err(|_| CapabilityError::InvalidPath)?;
    // SAFETY: directory and the NUL-terminated package component remain live for the call.
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    if descriptor < 0 {
        return Err(capability_error_from_io(std::io::Error::last_os_error()));
    }
    // SAFETY: the fresh descriptor is transferred exactly once.
    let file = unsafe { File::from_raw_fd(descriptor) };
    validate_opened_regular(&file)?;
    Ok(file)
}

#[cfg(windows)]
fn open_child_directory(directory: &File, name: &str) -> Result<File, CapabilityError> {
    let file = nt_open_child(directory, name, true)?;
    validate_opened_directory(&file)?;
    Ok(file)
}

#[cfg(windows)]
fn open_child_regular(directory: &File, name: &str) -> Result<File, CapabilityError> {
    let file = nt_open_child(directory, name, false)?;
    validate_opened_regular(&file)?;
    Ok(file)
}

#[cfg(windows)]
fn nt_open_child(
    directory: &File,
    name: &str,
    directory_only: bool,
) -> Result<File, CapabilityError> {
    use std::mem::{size_of, zeroed};
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT,
        FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
    };
    use windows_sys::Win32::Foundation::{HANDLE, OBJ_CASE_INSENSITIVE, UNICODE_STRING};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_NORMAL, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_READ_DATA,
        FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, SYNCHRONIZE,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    if !valid_capability_component(name) {
        return Err(CapabilityError::InvalidPath);
    }
    let mut wide = name.encode_utf16().collect::<Vec<_>>();
    let byte_length = wide
        .len()
        .checked_mul(2)
        .and_then(|length| u16::try_from(length).ok())
        .ok_or(CapabilityError::InvalidPath)?;
    let unicode = UNICODE_STRING {
        Length: byte_length,
        MaximumLength: byte_length,
        Buffer: wide.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: directory.as_raw_handle(),
        ObjectName: &unicode,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: std::ptr::null(),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut handle: HANDLE = std::ptr::null_mut();
    // SAFETY: zero is the documented initial state for IO_STATUS_BLOCK.
    let mut status: IO_STATUS_BLOCK = unsafe { zeroed() };
    let desired_access = if directory_only {
        FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE
    } else {
        FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE
    };
    let options = if directory_only {
        FILE_DIRECTORY_FILE
    } else {
        FILE_NON_DIRECTORY_FILE
    } | FILE_OPEN_REPARSE_POINT
        | FILE_SYNCHRONOUS_IO_NONALERT;
    // SAFETY: every pointer references live storage for this synchronous call; a
    // successful handle is transferred to File exactly once below.
    let result = unsafe {
        NtCreateFile(
            &mut handle,
            desired_access,
            &attributes,
            &mut status,
            std::ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_OPEN,
            options,
            std::ptr::null(),
            0,
        )
    };
    if result < 0 || handle.is_null() {
        return Err(capability_error_from_ntstatus(result));
    }
    // SAFETY: successful NtCreateFile returned a fresh owned handle.
    Ok(unsafe { File::from_raw_handle(handle) })
}

#[cfg(unix)]
fn inspect_inventory_child(
    directory: &File,
    name: &str,
) -> Result<InventoryChild, CapabilityError> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};

    if !valid_capability_component(name) {
        return Err(CapabilityError::InvalidPath);
    }
    let name = CString::new(name).map_err(|_| CapabilityError::InvalidPath)?;
    // O_NONBLOCK prevents a malicious FIFO from stalling validation. O_NOFOLLOW
    // makes the returned descriptor itself authoritative for the type decision.
    // SAFETY: retained directory and NUL-terminated component are live.
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    if descriptor < 0 {
        return Err(capability_error_from_io(std::io::Error::last_os_error()));
    }
    // SAFETY: the fresh descriptor is transferred exactly once.
    let file = unsafe { File::from_raw_fd(descriptor) };
    let metadata = file.metadata().map_err(|_| CapabilityError::Io)?;
    if metadata.is_dir() {
        Ok(InventoryChild::Directory(file))
    } else if metadata.is_file() {
        Ok(InventoryChild::Regular(file))
    } else {
        Err(CapabilityError::LinkedOrWrongType)
    }
}

#[cfg(windows)]
fn inspect_inventory_child(
    directory: &File,
    name: &str,
) -> Result<InventoryChild, CapabilityError> {
    let marker = nt_open_child_untyped(directory, name)?;
    let metadata = marker.metadata().map_err(|_| CapabilityError::Io)?;
    if is_reparse_point(&metadata) {
        return Err(CapabilityError::LinkedOrWrongType);
    }
    if metadata.is_dir() {
        let typed = open_child_directory(directory, name)?;
        if file_identity(&marker)? != file_identity(&typed)? {
            return Err(CapabilityError::Io);
        }
        Ok(InventoryChild::Directory(typed))
    } else if metadata.is_file() {
        let typed = open_child_regular(directory, name)?;
        if file_identity(&marker)? != file_identity(&typed)? {
            return Err(CapabilityError::Io);
        }
        Ok(InventoryChild::Regular(typed))
    } else {
        Err(CapabilityError::LinkedOrWrongType)
    }
}

#[cfg(windows)]
fn nt_open_child_untyped(directory: &File, name: &str) -> Result<File, CapabilityError> {
    use std::mem::{size_of, zeroed};
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_OPEN, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
    };
    use windows_sys::Win32::Foundation::{HANDLE, OBJ_CASE_INSENSITIVE, UNICODE_STRING};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_NORMAL, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    if !valid_capability_component(name) {
        return Err(CapabilityError::InvalidPath);
    }
    let mut wide = name.encode_utf16().collect::<Vec<_>>();
    let byte_length = wide
        .len()
        .checked_mul(2)
        .and_then(|length| u16::try_from(length).ok())
        .ok_or(CapabilityError::InvalidPath)?;
    let unicode = UNICODE_STRING {
        Length: byte_length,
        MaximumLength: byte_length,
        Buffer: wide.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: directory.as_raw_handle(),
        ObjectName: &unicode,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: std::ptr::null(),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut handle: HANDLE = std::ptr::null_mut();
    // SAFETY: zero is the documented initial state for IO_STATUS_BLOCK.
    let mut status: IO_STATUS_BLOCK = unsafe { zeroed() };
    // SAFETY: pointers reference live storage for this synchronous call; the
    // successful handle is transferred exactly once below.
    let result = unsafe {
        NtCreateFile(
            &mut handle,
            FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            &attributes,
            &mut status,
            std::ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_OPEN,
            FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            std::ptr::null(),
            0,
        )
    };
    if result < 0 || handle.is_null() {
        return Err(capability_error_from_ntstatus(result));
    }
    // SAFETY: successful NtCreateFile returned a fresh owned handle.
    Ok(unsafe { File::from_raw_handle(handle) })
}

#[cfg(unix)]
fn enumerate_directory(
    directory: &File,
    limit: usize,
) -> Result<Vec<EnumeratedName>, CapabilityError> {
    use std::ffi::{CStr, CString};
    use std::os::fd::AsRawFd;

    let dot = CString::new(".").map_err(|_| CapabilityError::Io)?;
    // SAFETY: this makes an independent open description rooted at the retained handle.
    let duplicate = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            dot.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if duplicate < 0 {
        return Err(capability_error_from_io(std::io::Error::last_os_error()));
    }
    // SAFETY: fdopendir consumes duplicate on success.
    let stream = unsafe { libc::fdopendir(duplicate) };
    if stream.is_null() {
        // SAFETY: fdopendir failed and therefore did not consume duplicate.
        unsafe { libc::close(duplicate) };
        return Err(CapabilityError::Io);
    }
    let result = (|| {
        let mut names = Vec::with_capacity(limit.min(64));
        loop {
            // SAFETY: stream remains live until closed below.
            let entry = unsafe { libc::readdir(stream) };
            if entry.is_null() {
                break;
            }
            // SAFETY: d_name is NUL-terminated on the live dirent.
            let bytes = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if matches!(bytes, b"." | b"..") {
                continue;
            }
            if names.len() == limit {
                return Err(CapabilityError::LimitExceeded);
            }
            names.push(match std::str::from_utf8(bytes) {
                Ok(name) => EnumeratedName::Canonical(name.to_owned()),
                Err(_) => EnumeratedName::NonCanonical,
            });
        }
        sort_enumerated_names(&mut names);
        Ok(names)
    })();
    // SAFETY: stream is closed exactly once and owns duplicate.
    if unsafe { libc::closedir(stream) } != 0 {
        return Err(CapabilityError::Io);
    }
    result
}

#[cfg(windows)]
fn enumerate_directory(
    directory: &File,
    limit: usize,
) -> Result<Vec<EnumeratedName>, CapabilityError> {
    use std::mem::{size_of, zeroed};
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_NAMES_INFORMATION, FileNamesInformation, NtQueryDirectoryFile,
    };
    use windows_sys::Win32::Foundation::STATUS_NO_MORE_FILES;
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    let mut names = Vec::with_capacity(limit.min(64));
    let mut restart = true;
    loop {
        let mut buffer = [0u64; 1024];
        // SAFETY: zero is the documented initial state for IO_STATUS_BLOCK.
        let mut status: IO_STATUS_BLOCK = unsafe { zeroed() };
        // SAFETY: the directory handle and aligned output buffer are live. The call
        // is synchronous and returns at most one FILE_NAMES_INFORMATION entry.
        let result = unsafe {
            NtQueryDirectoryFile(
                directory.as_raw_handle(),
                std::ptr::null_mut(),
                None,
                std::ptr::null(),
                &mut status,
                buffer.as_mut_ptr().cast(),
                u32::try_from(buffer.len() * size_of::<u64>()).map_err(|_| CapabilityError::Io)?,
                FileNamesInformation,
                true,
                std::ptr::null(),
                restart,
            )
        };
        restart = false;
        if result == STATUS_NO_MORE_FILES {
            break;
        }
        if result < 0 {
            return Err(CapabilityError::Io);
        }
        let header_bytes = std::mem::offset_of!(FILE_NAMES_INFORMATION, FileName);
        if status.Information < header_bytes {
            return Err(CapabilityError::Io);
        }
        // SAFETY: the successful call initialized at least the fixed header, checked above.
        let information = unsafe { &*buffer.as_ptr().cast::<FILE_NAMES_INFORMATION>() };
        let name_bytes =
            usize::try_from(information.FileNameLength).map_err(|_| CapabilityError::Io)?;
        if name_bytes % 2 != 0
            || header_bytes.saturating_add(name_bytes) > status.Information
            || header_bytes.saturating_add(name_bytes) > buffer.len() * size_of::<u64>()
        {
            return Err(CapabilityError::Io);
        }
        // SAFETY: FileNameLength was bounded by both returned and allocated storage.
        let wide = unsafe {
            std::slice::from_raw_parts(
                std::ptr::addr_of!(information.FileName).cast::<u16>(),
                name_bytes / 2,
            )
        };
        if matches!(wide, [46] | [46, 46]) {
            continue;
        }
        if names.len() == limit {
            return Err(CapabilityError::LimitExceeded);
        }
        names.push(match String::from_utf16(wide) {
            Ok(name) => EnumeratedName::Canonical(name),
            Err(_) => EnumeratedName::NonCanonical,
        });
    }
    sort_enumerated_names(&mut names);
    Ok(names)
}

fn sort_enumerated_names(names: &mut [EnumeratedName]) {
    names.sort_by(|left, right| match (left, right) {
        (EnumeratedName::Canonical(left), EnumeratedName::Canonical(right)) => left.cmp(right),
        (EnumeratedName::NonCanonical, EnumeratedName::Canonical(_)) => std::cmp::Ordering::Less,
        (EnumeratedName::Canonical(_), EnumeratedName::NonCanonical) => std::cmp::Ordering::Greater,
        (EnumeratedName::NonCanonical, EnumeratedName::NonCanonical) => std::cmp::Ordering::Equal,
    });
}

fn validate_opened_regular(file: &File) -> Result<(), CapabilityError> {
    let metadata = file.metadata().map_err(|_| CapabilityError::Io)?;
    if !metadata.is_file() || metadata_is_reparse_point(&metadata) {
        return Err(CapabilityError::LinkedOrWrongType);
    }
    Ok(())
}

fn validate_opened_directory(file: &File) -> Result<(), CapabilityError> {
    let metadata = file.metadata().map_err(|_| CapabilityError::Io)?;
    if !metadata.is_dir() || metadata_is_reparse_point(&metadata) {
        return Err(CapabilityError::LinkedOrWrongType);
    }
    Ok(())
}

#[cfg(unix)]
fn metadata_is_reparse_point(_metadata: &std::fs::Metadata) -> bool {
    false
}

#[cfg(windows)]
fn metadata_is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    is_reparse_point(metadata)
}

#[cfg(windows)]
fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileIdentity {
    volume: u64,
    file: u64,
}

#[cfg(windows)]
fn file_identity(file: &File) -> Result<FileIdentity, CapabilityError> {
    use std::mem::MaybeUninit;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let mut information = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: file owns a live handle and the output is read only after success.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) } == 0 {
        return Err(CapabilityError::Io);
    }
    // SAFETY: the successful API call initialized all fields.
    let information = unsafe { information.assume_init() };
    Ok(FileIdentity {
        volume: u64::from(information.dwVolumeSerialNumber),
        file: (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
    })
}

#[cfg(unix)]
fn capability_error_from_io(error: std::io::Error) -> CapabilityError {
    match error.raw_os_error() {
        Some(libc::ENOENT) => CapabilityError::NotFound,
        Some(libc::ELOOP | libc::ENOTDIR | libc::EISDIR) => CapabilityError::LinkedOrWrongType,
        _ => CapabilityError::Io,
    }
}

#[cfg(windows)]
fn capability_error_from_io(error: std::io::Error) -> CapabilityError {
    if error.kind() == std::io::ErrorKind::NotFound {
        CapabilityError::NotFound
    } else {
        CapabilityError::Io
    }
}

#[cfg(windows)]
fn capability_error_from_ntstatus(status: i32) -> CapabilityError {
    use windows_sys::Win32::Foundation::{
        STATUS_FILE_IS_A_DIRECTORY, STATUS_NOT_A_DIRECTORY, STATUS_OBJECT_NAME_NOT_FOUND,
        STATUS_OBJECT_PATH_NOT_FOUND, STATUS_REPARSE_POINT_ENCOUNTERED, STATUS_STOPPED_ON_SYMLINK,
    };
    match status {
        STATUS_OBJECT_NAME_NOT_FOUND | STATUS_OBJECT_PATH_NOT_FOUND => CapabilityError::NotFound,
        STATUS_FILE_IS_A_DIRECTORY
        | STATUS_NOT_A_DIRECTORY
        | STATUS_REPARSE_POINT_ENCOUNTERED
        | STATUS_STOPPED_ON_SYMLINK => CapabilityError::LinkedOrWrongType,
        _ => CapabilityError::Io,
    }
}

fn validate_declared_surfaces(
    surfaces: &[String],
    base_path: &str,
    diagnostics: &mut DiagnosticCollector,
) {
    if diagnostics.should_stop() {
        return;
    }
    if surfaces.len() > MAX_SURFACES {
        diagnostics.push(error(
            "GHEX011_LIMIT",
            "contribution exceeds the public surface count limit",
            format!("{base_path}/surfaces"),
        ));
        return;
    }
    let mut unique = BTreeSet::new();
    for (index, surface) in surfaces.iter().enumerate() {
        if diagnostics.should_stop() {
            break;
        }
        if !known_surface(surface) || !unique.insert(surface) {
            diagnostics.push(error(
                "GHEX006_SURFACE",
                "contribution declares an unsupported or duplicate public surface",
                format!("{base_path}/surfaces/{index}"),
            ));
        }
    }
}

fn contribution_artifact_flow_refs(contributions: &[(usize, Contribution)]) -> BTreeSet<String> {
    contributions
        .iter()
        .filter(|(_, contribution)| {
            matches!(
                contribution.kind.as_str(),
                "schema" | "evaluator" | "observer" | "policy"
            )
        })
        .map(|(_, contribution)| {
            let namespace = format!("{}/", contribution.kind);
            let name = contribution
                .id
                .strip_prefix(&namespace)
                .unwrap_or(&contribution.id);
            format!("{}:{name}", contribution.kind)
        })
        .collect()
}

/// #285: six `spec.contracts` fields were declared in every shipped manifest and read by
/// nothing — `additionalProperties: true` on `contracts` (`schemas/extension.schema.json`) means
/// the schema layer never knew these keys existed either, so this is the only place any of them
/// is ever checked. Per-field verdicts, not one shared rule (design note #285, §2):
///
/// - `publication`: ENFORCED below. A package declaring anything but `"governor-only"`
///   misrepresents how it may be published — B's own demonstrated exploit
///   (`"publication": "self"` validated clean before this check existed).
/// - `hostViews`: ENFORCED below, shape only. The two shipped packages disagreed not on value but
///   on TYPE (array vs. a bare string) — a structural contradiction independent of what the field
///   eventually means (issue #223's territory, left untouched).
/// - `formatVersion`: ADVISORY. No consumer exists or is planned; enforcing would be inventing
///   significance from stylistic analogy to `apiVersion`/`DEVELOPMENT_API_MAJOR`, not from a
///   demonstrated need. Wakes when a real design need for a contracts-format version appears.
/// - `missingCapabilityResult`: ADVISORY. The two shipped values (`"refuse"`, `"unresolved"`) are
///   plausible but undecided — no capability registry exists yet to make either one meaningful
///   (`ValidatedContribution.required_capabilities` is parsed, consumed by nothing). Wakes when
///   that registry is built.
/// - `composition`: ADVISORY. The two shipped values (`"atomic"`, `"adaptive"`) disagree with zero
///   documented distinction between them — enforcing a closed enum now would freeze a choice
///   nobody has made. Wakes when `atomic` vs `adaptive` gets a documented behavioral distinction.
/// - `activation`: ADVISORY. Issue #212 ("atomic extension installation, activation, rollback")
///   owns this concept by name and is open, unimplemented — enforcing an enum here risks
///   contradicting a decision #212 hasn't made yet. Wakes when #212 lands its state machine.
fn validate_contracts_fields(manifest: &serde_json::Value, diagnostics: &mut DiagnosticCollector) {
    if diagnostics.should_stop() {
        return;
    }
    let Some(contracts) = manifest
        .pointer("/spec/contracts")
        .and_then(serde_json::Value::as_object)
    else {
        return;
    };

    if let Some(publication) = contracts.get("publication")
        && publication != "governor-only"
    {
        diagnostics.push(error(
            "GHEX021_PUBLICATION",
            "publication must be \"governor-only\" -- this manifest field is a claim about how \
             the package may be published, not an operational grant, and only that one value is \
             recognized",
            "/spec/contracts/publication",
        ));
    }

    if let Some(host_views) = contracts.get("hostViews")
        && !host_views.is_array()
    {
        diagnostics.push(error(
            "GHEX022_HOST_VIEWS",
            "hostViews must be an array",
            "/spec/contracts/hostViews",
        ));
    }
}

fn validate_artifact_flows(
    manifest: &serde_json::Value,
    contribution_refs: &BTreeSet<String>,
    diagnostics: &mut DiagnosticCollector,
) {
    if diagnostics.should_stop() {
        return;
    }
    let Some(contracts) = manifest
        .pointer("/spec/contracts")
        .and_then(serde_json::Value::as_object)
    else {
        return;
    };
    let Some(raw_flows) = contracts.get("artifactFlows") else {
        return;
    };

    if contracts
        .get("artifactFlowFormat")
        .and_then(serde_json::Value::as_str)
        != Some(ARTIFACT_FLOW_FORMAT)
    {
        diagnostics.push(error(
            "GHEX019_ARTIFACT_FLOW",
            "artifact flows require the supported artifact flow format",
            "/spec/contracts/artifactFlowFormat",
        ));
    }

    let entry_families = contracts
        .get("entryFamilies")
        .and_then(serde_json::Value::as_array);
    let mut declared_families = BTreeMap::new();
    match entry_families {
        Some(values) if !values.is_empty() && values.len() <= MAX_ARTIFACT_FLOWS => {
            for (index, value) in values.iter().enumerate() {
                if diagnostics.should_stop() {
                    return;
                }
                let Some(family) = value.as_str() else {
                    diagnostics.push(error(
                        "GHEX019_ARTIFACT_FLOW",
                        "artifact flow entry families must be bounded unique identifiers",
                        format!("/spec/contracts/entryFamilies/{index}"),
                    ));
                    continue;
                };
                if !valid_identifier(family)
                    || declared_families.insert(family.to_owned(), index).is_some()
                {
                    diagnostics.push(error(
                        "GHEX019_ARTIFACT_FLOW",
                        "artifact flow entry families must be bounded unique identifiers",
                        format!("/spec/contracts/entryFamilies/{index}"),
                    ));
                }
            }
        }
        _ => diagnostics.push(error(
            "GHEX019_ARTIFACT_FLOW",
            "artifact flows require a non-empty bounded entry family list",
            "/spec/contracts/entryFamilies",
        )),
    }

    let Some(flow_values) = raw_flows.as_array() else {
        diagnostics.push(error(
            "GHEX019_ARTIFACT_FLOW",
            "artifactFlows must be a bounded array",
            "/spec/contracts/artifactFlows",
        ));
        return;
    };
    if flow_values.is_empty() || flow_values.len() > MAX_ARTIFACT_FLOWS {
        diagnostics.push(error(
            "GHEX019_ARTIFACT_FLOW",
            "artifactFlows must be a non-empty bounded array",
            "/spec/contracts/artifactFlows",
        ));
    }

    let mut flow_families = BTreeMap::new();
    for (index, value) in flow_values.iter().take(MAX_ARTIFACT_FLOWS).enumerate() {
        if diagnostics.should_stop() {
            return;
        }
        let base_path = format!("/spec/contracts/artifactFlows/{index}");
        let Ok(flow) = serde_json::from_value::<ArtifactFlow>(value.clone()) else {
            diagnostics.push(error(
                "GHEX019_ARTIFACT_FLOW",
                "artifact flow must satisfy the closed flow contract",
                &base_path,
            ));
            continue;
        };
        if !valid_identifier(&flow.family)
            || flow_families.insert(flow.family.clone(), index).is_some()
        {
            diagnostics.push(error(
                "GHEX019_ARTIFACT_FLOW",
                "artifact flow family must be a bounded unique identifier",
                format!("{base_path}/family"),
            ));
        }
        if !declared_families.is_empty() && !declared_families.contains_key(&flow.family) {
            diagnostics.push(error(
                "GHEX019_ARTIFACT_FLOW",
                "artifact flow family is not an entry family",
                format!("{base_path}/family"),
            ));
        }
        validate_artifact_flow_refs(
            &flow.inputs,
            "inputs",
            &base_path,
            contribution_refs,
            diagnostics,
        );
        if diagnostics.should_stop() {
            return;
        }
        validate_artifact_flow_refs(
            &flow.outputs,
            "outputs",
            &base_path,
            contribution_refs,
            diagnostics,
        );
    }

    for (family, index) in declared_families {
        if diagnostics.should_stop() {
            return;
        }
        if !flow_families.contains_key(&family) {
            diagnostics.push(error(
                "GHEX019_ARTIFACT_FLOW",
                "entry family requires exactly one artifact flow",
                format!("/spec/contracts/entryFamilies/{index}"),
            ));
        }
    }
}

fn validate_artifact_flow_refs(
    values: &[String],
    field: &str,
    base_path: &str,
    contribution_refs: &BTreeSet<String>,
    diagnostics: &mut DiagnosticCollector,
) {
    if diagnostics.should_stop() {
        return;
    }
    if values.is_empty() || values.len() > MAX_ARTIFACT_FLOW_REFS {
        diagnostics.push(error(
            "GHEX019_ARTIFACT_FLOW",
            "artifact flow references must be non-empty and bounded",
            format!("{base_path}/{field}"),
        ));
    }
    let mut unique = BTreeSet::new();
    for (index, value) in values.iter().take(MAX_ARTIFACT_FLOW_REFS).enumerate() {
        if diagnostics.should_stop() {
            break;
        }
        if !valid_artifact_flow_ref(value)
            || !artifact_flow_ref_resolves(value, contribution_refs)
            || !unique.insert(value)
        {
            diagnostics.push(error(
                "GHEX019_ARTIFACT_FLOW",
                "artifact flow references must use the closed vocabulary and be unique",
                format!("{base_path}/{field}/{index}"),
            ));
        }
    }
}

fn artifact_flow_ref_resolves(value: &str, contribution_refs: &BTreeSet<String>) -> bool {
    value
        .split_once(':')
        .is_some_and(|(prefix, _)| match prefix {
            "task" | "runtime" | "artifact" => true,
            "schema" | "evaluator" | "observer" | "policy" => contribution_refs.contains(value),
            _ => false,
        })
}

fn valid_artifact_flow_ref(value: &str) -> bool {
    let Some((prefix, name)) = value.split_once(':') else {
        return false;
    };
    ARTIFACT_REF_PREFIXES.contains(&prefix)
        && !name.is_empty()
        && name.len() <= 128
        && !name.contains(':')
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'_' | b'.' | b'/')
        })
        && name
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && name
            .bytes()
            .next_back()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && !name
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
}

fn validate_authority(
    contribution: &Contribution,
    base_path: &str,
    diagnostics: &mut DiagnosticCollector,
) {
    for (field, values, vocabulary) in [
        (
            "effects",
            contribution.effects.as_slice(),
            Some(CONTRIBUTION_EFFECTS),
        ),
        (
            "permissions",
            contribution.permissions.as_slice(),
            Some(CONTRIBUTION_PERMISSIONS),
        ),
        (
            "requires/capabilities",
            contribution.requires.capabilities.as_slice(),
            None,
        ),
        (
            "requires/observers",
            contribution.requires.observers.as_slice(),
            None,
        ),
    ] {
        if diagnostics.should_stop() {
            return;
        }
        if values.len() > MAX_AUTHORITY_ITEMS {
            diagnostics.push(error(
                "GHEX011_LIMIT",
                "contribution authority list exceeds its item limit",
                format!("{base_path}/{field}"),
            ));
            continue;
        }
        for (index, value) in values.iter().enumerate() {
            if diagnostics.should_stop() {
                return;
            }
            if !valid_identifier(value)
                || vocabulary.is_some_and(|allowed| !allowed.contains(&value.as_str()))
                || index > 0 && values[index - 1].as_str() >= value.as_str()
            {
                diagnostics.push(error(
                    "GHEX017_AUTHORITY",
                    "contribution authority values must be valid, unique, and sorted",
                    format!("{base_path}/{field}/{index}"),
                ));
            }
        }
    }
}

fn validate_authority_subset(
    contribution: &Contribution,
    grants: &PackageGrants,
    base_path: &str,
    diagnostics: &mut DiagnosticCollector,
) {
    for (index, effect) in contribution.effects.iter().enumerate() {
        if diagnostics.should_stop() {
            return;
        }
        if CONTRIBUTION_EFFECTS.contains(&effect.as_str()) && !grants.grants_effect(effect) {
            diagnostics.push(error(
                "GHEX018_AUTHORITY_ESCALATION",
                "contribution effect exceeds the package authority grant",
                format!("{base_path}/effects/{index}"),
            ));
        }
        if let Some(permission) = required_permission_for_effect(effect)
            && !contribution
                .permissions
                .iter()
                .any(|declared| declared == permission)
        {
            diagnostics.push(error(
                "GHEX018_AUTHORITY_ESCALATION",
                "contribution effect requires a matching contribution permission",
                format!("{base_path}/effects/{index}"),
            ));
        }
    }
    for (index, permission) in contribution.permissions.iter().enumerate() {
        if diagnostics.should_stop() {
            return;
        }
        if CONTRIBUTION_PERMISSIONS.contains(&permission.as_str())
            && !grants.grants_permission(permission)
        {
            diagnostics.push(error(
                "GHEX018_AUTHORITY_ESCALATION",
                "contribution permission exceeds the package authority grant",
                format!("{base_path}/permissions/{index}"),
            ));
        }
    }

    let declares_mutation_effect = contribution
        .effects
        .iter()
        .any(|effect| effect == "runtime.mutate");
    let declares_mutation_permission = contribution
        .permissions
        .iter()
        .any(|permission| permission == "runtime.write");
    let declares_connection_effect = contribution
        .effects
        .iter()
        .any(|effect| effect == "runtime.connect");
    let declares_loopback_permission = contribution
        .permissions
        .iter()
        .any(|permission| permission == "network.loopback");
    let declares_read_effect = contribution
        .effects
        .iter()
        .any(|effect| effect == "runtime.read");
    let declares_read_permission = contribution
        .permissions
        .iter()
        .any(|permission| permission == "runtime.read");
    let declares_owner_decision = contribution
        .permissions
        .iter()
        .any(|permission| permission == "owner.decision.request");
    let mut has_mutation_surface = false;
    for (index, surface) in contribution.surfaces.iter().enumerate() {
        if diagnostics.should_stop() {
            return;
        }
        if surface_requires_loopback(surface)
            && (!declares_connection_effect || !declares_loopback_permission)
        {
            diagnostics.push(error(
                "GHEX018_AUTHORITY_ESCALATION",
                "MCP surface omits its explicit loopback connection authority",
                format!("{base_path}/surfaces/{index}"),
            ));
        }
        if runtime_read_surface(surface) && (!declares_read_effect || !declares_read_permission) {
            diagnostics.push(error(
                "GHEX018_AUTHORITY_ESCALATION",
                "runtime read surface omits its explicit read authority",
                format!("{base_path}/surfaces/{index}"),
            ));
        }
        if let Some(mutation) = mutation_for_surface(surface) {
            has_mutation_surface = true;
            if !grants.runtime_mutations.contains(mutation)
                || !declares_mutation_effect
                || !declares_mutation_permission
                || !declares_owner_decision
            {
                diagnostics.push(error(
                    "GHEX018_AUTHORITY_ESCALATION",
                    "runtime mutation surface exceeds or omits its explicit authority grant",
                    format!("{base_path}/surfaces/{index}"),
                ));
            }
        }
    }
    if diagnostics.should_stop() {
        return;
    }
    if (declares_mutation_effect || declares_mutation_permission) && !has_mutation_surface {
        diagnostics.push(error(
            "GHEX018_AUTHORITY_ESCALATION",
            "runtime mutation authority requires at least one declared mutation surface",
            format!("{base_path}/surfaces"),
        ));
    }
    if declares_mutation_effect && !declares_owner_decision {
        diagnostics.push(error(
            "GHEX018_AUTHORITY_ESCALATION",
            "runtime mutation authority requires owner decision permission",
            format!("{base_path}/permissions"),
        ));
    }
}

fn required_permission_for_effect(effect: &str) -> Option<&'static str> {
    match effect {
        "artifact.local.write" => Some("workspace.artifact.write"),
        "external.read" => Some("network.external"),
        "host.discover" => Some("package.read"),
        "runtime.connect" => Some("network.loopback"),
        "runtime.mutate" => Some("runtime.write"),
        "runtime.read" => Some("runtime.read"),
        _ => None,
    }
}

fn mutation_for_surface(surface: &str) -> Option<&'static str> {
    match surface {
        "tool:start" | "cli:execution start" => Some("start"),
        "tool:signal" | "cli:execution signal" => Some("signal"),
        "tool:approve" | "cli:execution approve" => Some("approve"),
        "tool:pause" | "cli:execution pause" => Some("pause"),
        "tool:resume" | "cli:execution resume" => Some("resume"),
        "tool:cancel" | "cli:execution cancel" => Some("cancel"),
        "tool:wake_arm" => Some("wake_arm"),
        "tool:amend_budget" | "cli:execution amend-budget" => Some("amend_budget"),
        "cli:quality certify" => Some("quality_certify"),
        _ => None,
    }
}

fn surface_requires_loopback(surface: &str) -> bool {
    surface.starts_with("tool:") || surface == "cli:mcp"
}

fn runtime_read_surface(surface: &str) -> bool {
    matches!(
        surface,
        "tool:status"
            | "tool:events"
            | "tool:routes"
            | "tool:wake_status"
            | "tool:wake_wait"
            | "tool:probe"
            | "cli:execution status"
    )
}

fn known_surface(surface: &str) -> bool {
    surface
        .strip_prefix("tool:")
        .is_some_and(|tool| MCP_TOOLS.contains(&tool))
        || surface
            .strip_prefix("cli:")
            .is_some_and(|command| CLI_COMMANDS.contains(&command))
}

fn validate_contribution_contents_bounded(
    resources: &[ValidatedResource],
    declared_schema_paths: &BTreeSet<String>,
    contribution_kinds: &BTreeMap<String, String>,
    extension_id: &str,
    extension_version: &str,
    diagnostics: &mut DiagnosticCollector,
) {
    if diagnostics.should_stop() {
        return;
    }
    let mut schema_documents = BTreeMap::new();
    let mut schema_ids = BTreeMap::new();
    let mut schema_value_budget = ValueBudget::default();
    for resource in resources
        .iter()
        .filter(|resource| resource.contribution.kind == "schema")
    {
        if diagnostics.should_stop() {
            return;
        }
        let schema = match parse_schema(&resource.bytes, &mut schema_value_budget) {
            Ok(schema) => schema,
            Err(failure) if bounded_value_limit_exceeded(failure) => {
                diagnostics.push(error(
                    "GHEX011_LIMIT",
                    "extension schemas exceed deterministic aggregate JSON structure limits",
                    format!("{}/path", resource.base_path),
                ));
                return;
            }
            Err(_) => {
                diagnostics.push(error(
                    "GHEX008_SCHEMA",
                    "schema contribution must be valid offline Draft 2020-12 JSON",
                    format!("{}/path", resource.base_path),
                ));
                continue;
            }
        };
        if let Some(id) = schema
            .get("$id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
        {
            schema_ids.insert(resource.contribution.path.clone(), id);
        }
        schema_documents.insert(resource.contribution.path.clone(), schema);
    }
    let schemas = if schema_documents.is_empty() {
        None
    } else {
        if diagnostics.should_stop() {
            return;
        }
        match crate::OfflineSchemaSet::compile(schema_documents) {
            Ok(schemas) => Some(schemas),
            Err(_) => {
                diagnostics.push(error(
                    "GHEX008_SCHEMA",
                    "contributed schemas do not form a valid offline schema set",
                    "/spec/contracts/contributions",
                ));
                None
            }
        }
    };

    for resource in resources {
        if diagnostics.should_stop() {
            return;
        }
        let contribution = &resource.contribution;
        match contribution.kind.as_str() {
            "skill" => validate_skill(
                contribution,
                &resource.bytes,
                &resource.base_path,
                diagnostics,
            ),
            "schema" => {}
            "agent" => validate_agent(
                contribution,
                &resource.bytes,
                &resource.base_path,
                diagnostics,
            ),
            "policy" | "evaluator" | "observer" => validate_typed_data(
                contribution,
                &resource.bytes,
                &resource.base_path,
                declared_schema_paths,
                &schema_ids,
                schemas.as_ref(),
                diagnostics,
            ),
            "graph" => validate_graph(
                &resource.bytes,
                Path::new(&contribution.path),
                &resource.base_path,
                extension_id,
                contribution_kinds,
                diagnostics,
            ),
            "host-adapter" => validate_host_adapter(
                contribution,
                &resource.bytes,
                &resource.base_path,
                extension_id,
                extension_version,
                diagnostics,
            ),
            _ => {}
        }
    }
}

#[cfg(test)]
fn validate_contribution_contents(
    resources: &[ValidatedResource],
    declared_schema_paths: &BTreeSet<String>,
    contribution_kinds: &BTreeMap<String, String>,
    extension_id: &str,
    extension_version: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut collector = DiagnosticCollector::default();
    validate_contribution_contents_bounded(
        resources,
        declared_schema_paths,
        contribution_kinds,
        extension_id,
        extension_version,
        &mut collector,
    );
    diagnostics.extend(collector.into_diagnostics());
}

fn is_host_adapter_path(path: &str) -> bool {
    matches!(
        path,
        ".claude-plugin/plugin.json" | ".codex-plugin/plugin.json" | ".mcp.json"
    )
}

fn validate_host_adapter(
    contribution: &Contribution,
    bytes: &[u8],
    base_path: &str,
    extension_id: &str,
    extension_version: &str,
    diagnostics: &mut DiagnosticCollector,
) {
    if diagnostics.should_stop() {
        return;
    }
    let mut budget = ValueBudget::default();
    let value = match parse_json(bytes, RESOURCE_VALUE_LIMITS, &mut budget) {
        Ok(value) => value,
        Err(failure) if bounded_value_limit_exceeded(failure) => {
            diagnostics.push(error(
                "GHEX011_LIMIT",
                "host adapter exceeds deterministic JSON structure limits",
                format!("{base_path}/path"),
            ));
            return;
        }
        Err(_) => {
            diagnostics.push(error(
                "GHEX013_HOST",
                "host adapter must be valid JSON",
                format!("{base_path}/path"),
            ));
            return;
        }
    };
    let valid = match contribution.path.as_str() {
        ".claude-plugin/plugin.json" => {
            value.get("name").and_then(serde_json::Value::as_str) == Some(extension_id)
                && value.get("version").and_then(serde_json::Value::as_str)
                    == Some(extension_version)
        }
        ".codex-plugin/plugin.json" => {
            value.get("id").and_then(serde_json::Value::as_str) == Some(extension_id)
                && value.get("name").and_then(serde_json::Value::as_str) == Some(extension_id)
                && value.get("version").and_then(serde_json::Value::as_str)
                    == Some(extension_version)
        }
        ".mcp.json" => valid_mcp_registration(&value),
        _ => false,
    };
    if !valid {
        diagnostics.push(error(
            "GHEX013_HOST",
            "host adapter does not match the extension envelope",
            format!("{base_path}/path"),
        ));
    }
}

fn valid_mcp_registration(value: &serde_json::Value) -> bool {
    let Some(root) = value.as_object() else {
        return false;
    };
    if root.len() != 1 {
        return false;
    }
    let Some(servers) = root
        .get("mcpServers")
        .and_then(serde_json::Value::as_object)
    else {
        return false;
    };
    if servers.len() != 1 {
        return false;
    }
    let Some(server) = servers
        .get("graphhelm")
        .and_then(serde_json::Value::as_object)
    else {
        return false;
    };
    if server.len() != 2
        || server.get("command").and_then(serde_json::Value::as_str) != Some("${GRAPHHELM_CLI}")
    {
        return false;
    }
    let Some(args) = server.get("args").and_then(serde_json::Value::as_array) else {
        return false;
    };
    let args = args
        .iter()
        .map(serde_json::Value::as_str)
        .collect::<Option<Vec<_>>>();
    let Some(args) = args else {
        return false;
    };
    args.len() == 7
        && args[0] == "mcp"
        && args[1] == "--url"
        && loopback_url(args[2])
        && args[3] == "--token-file"
        && args[4] == "${GRAPHHELM_TOKEN_FILE}"
        && args[5] == "--actor"
        && args[6] == "${GRAPHHELM_ACTOR}"
}

fn loopback_url(url: &str) -> bool {
    url.strip_prefix("http://127.0.0.1:")
        .or_else(|| url.strip_prefix("http://[::1]:"))
        .is_some_and(|port| {
            !port.is_empty()
                && port.bytes().all(|byte| byte.is_ascii_digit())
                && port.parse::<u16>().is_ok_and(|port| port != 0)
        })
}

fn validate_skill(
    contribution: &Contribution,
    bytes: &[u8],
    base_path: &str,
    diagnostics: &mut DiagnosticCollector,
) {
    if diagnostics.should_stop() {
        return;
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        diagnostics.push(error(
            "GHEX007_SKILL",
            "skill must be UTF-8 with YAML frontmatter",
            format!("{base_path}/path"),
        ));
        return;
    };
    let normalized = text.replace("\r\n", "\n");
    let Some(rest) = normalized.strip_prefix("---\n") else {
        diagnostics.push(error(
            "GHEX007_SKILL",
            "skill must start with YAML frontmatter",
            format!("{base_path}/path"),
        ));
        return;
    };
    let Some(frontmatter_end) = rest.find("\n---\n") else {
        diagnostics.push(error(
            "GHEX007_SKILL",
            "skill frontmatter must have a closing delimiter",
            format!("{base_path}/path"),
        ));
        return;
    };
    let frontmatter = &rest[..frontmatter_end];
    let body = &rest[frontmatter_end + "\n---\n".len()..];
    let value = match parse_resource_value(frontmatter.as_bytes(), Some("yaml")) {
        Ok(value) => value,
        Err(failure) if bounded_value_limit_exceeded(failure) => {
            diagnostics.push(error(
                "GHEX011_LIMIT",
                "skill frontmatter exceeds deterministic YAML structure limits",
                format!("{base_path}/path"),
            ));
            return;
        }
        Err(_) => {
            diagnostics.push(error(
                "GHEX007_SKILL",
                "skill frontmatter is invalid YAML",
                format!("{base_path}/path"),
            ));
            return;
        }
    };
    let valid_name = value
        .get("name")
        .and_then(serde_json::Value::as_str)
        .is_some_and(valid_skill_name);
    let valid_description = value
        .get("description")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|description| !description.trim().is_empty() && description.len() <= 1024);
    if !valid_name || !valid_description {
        diagnostics.push(error(
            "GHEX007_SKILL",
            "skill frontmatter requires a bounded name and description",
            format!("{base_path}/path"),
        ));
        return;
    }

    let declared = contribution
        .surfaces
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let Ok(markers) = skill_markers(body) else {
        diagnostics.push(error(
            "GHEX006_SURFACE",
            "skill public surfaces must use explicit Markdown code markers",
            format!("{base_path}/surfaces"),
        ));
        return;
    };
    for marker in markers {
        if diagnostics.should_stop() {
            break;
        }
        if !known_surface(marker) || !declared.contains(marker) {
            diagnostics.push(error(
                "GHEX006_SURFACE",
                "skill uses a public surface that is unknown or undeclared",
                format!("{base_path}/surfaces"),
            ));
        }
    }
}

fn valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
}

fn skill_markers(body: &str) -> Result<BTreeSet<&str>, ()> {
    let mut markers = BTreeSet::new();
    let bytes = body.as_bytes();
    let mut cursor = 0;
    let mut plain_start = 0;

    while cursor < bytes.len() {
        if bytes[cursor] != b'`' || markdown_delimiter_is_escaped(bytes, cursor) {
            cursor += 1;
            continue;
        }
        let delimiter_len = backtick_run_len(bytes, cursor);
        let content_start = cursor + delimiter_len;
        let mut search = content_start;
        let mut closing = None;
        while search < bytes.len() {
            if bytes[search] != b'`' || markdown_delimiter_is_escaped(bytes, search) {
                search += 1;
                continue;
            }
            let candidate_len = backtick_run_len(bytes, search);
            if candidate_len == delimiter_len {
                closing = Some(search);
                break;
            }
            search += candidate_len;
        }

        let Some(closing) = closing else {
            return Err(());
        };
        if contains_public_surface_marker(&body[plain_start..cursor]) {
            return Err(());
        }
        let code = &body[content_start..closing];
        if contains_public_surface_marker(code) {
            let marker = code.trim();
            if marker.contains(['\r', '\n'])
                || !(marker.starts_with("tool:") || marker.starts_with("cli:"))
            {
                return Err(());
            }
            markers.insert(marker);
        }
        cursor = closing + delimiter_len;
        plain_start = cursor;
    }

    if contains_public_surface_marker(&body[plain_start..]) {
        return Err(());
    }
    Ok(markers)
}

fn contains_public_surface_marker(value: &str) -> bool {
    value.contains("tool:") || value.contains("cli:")
}

fn markdown_delimiter_is_escaped(bytes: &[u8], index: usize) -> bool {
    let mut backslashes = 0;
    let mut cursor = index;
    while cursor > 0 && bytes[cursor - 1] == b'\\' {
        backslashes += 1;
        cursor -= 1;
    }
    backslashes % 2 == 1
}

fn backtick_run_len(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .take_while(|byte| **byte == b'`')
        .count()
}

fn parse_schema(
    bytes: &[u8],
    budget: &mut ValueBudget,
) -> Result<serde_json::Value, BoundedValueError> {
    parse_json(bytes, PACKAGE_SCHEMA_VALUE_LIMITS, budget)
}

fn parse_resource_value(
    bytes: &[u8],
    extension: Option<&str>,
) -> Result<serde_json::Value, BoundedValueError> {
    let mut budget = ValueBudget::default();
    match extension {
        Some("json") => parse_json(bytes, RESOURCE_VALUE_LIMITS, &mut budget),
        Some("yaml" | "yml") => parse_yaml(bytes, RESOURCE_VALUE_LIMITS, &mut budget),
        _ => Err(BoundedValueError::Invalid),
    }
}

fn bounded_value_limit_exceeded(failure: BoundedValueError) -> bool {
    matches!(
        failure,
        BoundedValueError::DepthLimit
            | BoundedValueError::ValueLimit
            | BoundedValueError::KeyBytesLimit
            | BoundedValueError::StringBytesLimit
    )
}

fn validate_typed_data(
    contribution: &Contribution,
    bytes: &[u8],
    base_path: &str,
    declared_schema_paths: &BTreeSet<String>,
    schema_ids: &BTreeMap<String, String>,
    schemas: Option<&crate::OfflineSchemaSet>,
    diagnostics: &mut DiagnosticCollector,
) {
    if diagnostics.should_stop() {
        return;
    }
    let Some(schema_path) = contribution.schema.as_deref() else {
        return;
    };
    if !valid_relative_path(schema_path) || !declared_schema_paths.contains(schema_path) {
        diagnostics.push(error(
            "GHEX014_SCHEMA_REF",
            "typed data schema must reference a declared package schema contribution",
            format!("{base_path}/schema"),
        ));
        return;
    }
    let (Some(schema_id), Some(schemas)) = (schema_ids.get(schema_path), schemas) else {
        diagnostics.push(error(
            "GHEX014_SCHEMA_REF",
            "typed data references a schema that did not compile",
            format!("{base_path}/schema"),
        ));
        return;
    };
    let extension = Path::new(&contribution.path)
        .extension()
        .and_then(|value| value.to_str());
    let value = match parse_resource_value(bytes, extension) {
        Ok(value) => value,
        Err(failure) if bounded_value_limit_exceeded(failure) => {
            diagnostics.push(error(
                "GHEX011_LIMIT",
                "typed data contribution exceeds deterministic structure limits",
                format!("{base_path}/path"),
            ));
            return;
        }
        Err(_) => {
            diagnostics.push(error(
                "GHEX015_DATA_PARSE",
                "typed data contribution must be valid JSON or YAML",
                format!("{base_path}/path"),
            ));
            return;
        }
    };
    for diagnostic in schemas.validate(schema_id, &value, SOURCE) {
        if diagnostics.should_stop() {
            break;
        }
        diagnostics.push(error(
            "GHEX016_DATA_SCHEMA",
            "typed data contribution does not satisfy its declared schema",
            format!("{base_path}/resource{}", diagnostic.path),
        ));
    }
}

fn validate_agent(
    contribution: &Contribution,
    bytes: &[u8],
    base_path: &str,
    diagnostics: &mut DiagnosticCollector,
) {
    if diagnostics.should_stop() {
        return;
    }
    let extension = Path::new(&contribution.path)
        .extension()
        .and_then(|value| value.to_str());
    let value = match parse_resource_value(bytes, extension) {
        Ok(value) => value,
        Err(failure) if bounded_value_limit_exceeded(failure) => {
            diagnostics.push(error(
                "GHEX011_LIMIT",
                "agent contribution exceeds deterministic structure limits",
                format!("{base_path}/path"),
            ));
            return;
        }
        Err(_) => {
            diagnostics.push(error(
                "GHEX009_AGENT",
                "agent contribution must be valid JSON or YAML",
                format!("{base_path}/path"),
            ));
            return;
        }
    };
    for diagnostic in crate::validate_agent_value(&value, SOURCE) {
        if diagnostics.should_stop() {
            return;
        }
        diagnostics.push(error(
            "GHEX009_AGENT",
            "agent contribution does not satisfy the agent schema",
            format!("{base_path}/resource{}", diagnostic.path),
        ));
    }
    if diagnostics.should_stop() {
        return;
    }
    let declared = contribution
        .surfaces
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let Some(allowed_tools) = value
        .get("allowedTools")
        .and_then(serde_json::Value::as_array)
    else {
        diagnostics.push(error(
            "GHEX006_SURFACE",
            "agent contributions must declare allowedTools explicitly",
            format!("{base_path}/resource/allowedTools"),
        ));
        return;
    };
    let mut allowed_surfaces = BTreeSet::new();
    for (index, tool) in allowed_tools.iter().enumerate() {
        if diagnostics.should_stop() {
            return;
        }
        let Some(tool) = tool.as_str() else {
            continue;
        };
        let surface = if tool.starts_with("tool:") || tool.starts_with("cli:") {
            Some(tool.to_owned())
        } else if tool.starts_with("graphhelm.") {
            graphhelm_tool_surface(tool)
        } else {
            continue;
        };
        if surface
            .as_deref()
            .is_none_or(|surface| !known_surface(surface) || !declared.contains(surface))
        {
            diagnostics.push(error(
                "GHEX006_SURFACE",
                "agent uses a GraphHelm public surface that is unknown or undeclared",
                format!("{base_path}/resource/allowedTools/{index}"),
            ));
        } else if let Some(surface) = surface {
            allowed_surfaces.insert(surface);
        }
    }
    if diagnostics.should_stop() {
        return;
    }

    if let Some(instructions) = value
        .get("instructions")
        .and_then(serde_json::Value::as_str)
    {
        let Ok(markers) = skill_markers(instructions) else {
            diagnostics.push(error(
                "GHEX006_SURFACE",
                "agent public surfaces must use explicit Markdown code markers",
                format!("{base_path}/resource/instructions"),
            ));
            return;
        };
        for marker in markers {
            if diagnostics.should_stop() {
                return;
            }
            if !known_surface(marker)
                || !declared.contains(marker)
                || !allowed_surfaces.contains(marker)
            {
                diagnostics.push(error(
                    "GHEX006_SURFACE",
                    "agent instructions use a public surface that is not allowed and declared",
                    format!("{base_path}/resource/instructions"),
                ));
            }
        }
    }
}

fn graphhelm_tool_surface(tool: &str) -> Option<String> {
    if let Some(name) = tool.strip_prefix("graphhelm.mcp.") {
        return Some(format!("tool:{name}"));
    }
    tool.strip_prefix("graphhelm.cli.")
        .map(|command| format!("cli:{}", command.replace('.', " ")))
}

fn validate_graph(
    bytes: &[u8],
    resource_path: &Path,
    base_path: &str,
    extension_id: &str,
    contribution_kinds: &BTreeMap<String, String>,
    diagnostics: &mut DiagnosticCollector,
) {
    if diagnostics.should_stop() {
        return;
    }
    let extension = resource_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    match crate::document::load_graph_bytes(bytes, extension, SOURCE) {
        Ok(graph) => validate_graph_extension_refs(
            &graph.raw,
            base_path,
            extension_id,
            contribution_kinds,
            diagnostics,
        ),
        Err(graph_diagnostics) => {
            for diagnostic in graph_diagnostics {
                if diagnostics.should_stop() {
                    break;
                }
                diagnostics.push(error(
                    "GHEX010_GRAPH",
                    "graph contribution is invalid",
                    format!("{base_path}/resource{}", diagnostic.path),
                ));
            }
        }
    }
}

fn validate_graph_extension_refs(
    graph: &serde_json::Value,
    base_path: &str,
    extension_id: &str,
    contribution_kinds: &BTreeMap<String, String>,
    diagnostics: &mut DiagnosticCollector,
) {
    if diagnostics.should_stop() {
        return;
    }
    if let Some(nodes) = graph
        .pointer("/spec/nodes")
        .and_then(serde_json::Value::as_object)
    {
        for (node_id, node) in nodes {
            if diagnostics.should_stop() {
                return;
            }
            if let Some(reference) = node
                .pointer("/agent/ref")
                .and_then(serde_json::Value::as_str)
            {
                validate_graph_extension_ref(
                    reference,
                    "agent",
                    extension_id,
                    contribution_kinds,
                    format!(
                        "{base_path}/resource/spec/nodes/{}/agent/ref",
                        escape_json_pointer_token(node_id)
                    ),
                    diagnostics,
                );
            }
            // #1049: the crew is the same binding in the plural, and every member carries the
            // same reference obligation as the primary - named by its own index, so a package
            // with several members is told WHICH one is dangling.
            if let Some(crew) = node
                .pointer("/agents")
                .and_then(serde_json::Value::as_array)
            {
                for (index, member) in crew.iter().enumerate() {
                    if diagnostics.should_stop() {
                        return;
                    }
                    if let Some(reference) =
                        member.pointer("/ref").and_then(serde_json::Value::as_str)
                    {
                        validate_graph_extension_ref(
                            reference,
                            "agent",
                            extension_id,
                            contribution_kinds,
                            format!(
                                "{base_path}/resource/spec/nodes/{}/agents/{index}/ref",
                                escape_json_pointer_token(node_id)
                            ),
                            diagnostics,
                        );
                    }
                }
            }
        }
    }
    if let Some(policies) = graph
        .pointer("/spec/policies")
        .and_then(serde_json::Value::as_array)
    {
        for (index, policy) in policies.iter().enumerate() {
            if diagnostics.should_stop() {
                return;
            }
            let path = format!("{base_path}/resource/spec/policies/{index}");
            if let Some(reference) = policy.as_str() {
                validate_graph_extension_ref(
                    reference,
                    "policy",
                    extension_id,
                    contribution_kinds,
                    path,
                    diagnostics,
                );
            } else {
                diagnostics.push(error(
                    "GHEX020_EXTENSION_REF",
                    "extension graph policy must be a canonical package contribution reference",
                    path,
                ));
            }
        }
    }
}

fn validate_graph_extension_ref(
    reference: &str,
    expected_kind: &str,
    extension_id: &str,
    contribution_kinds: &BTreeMap<String, String>,
    path: String,
    diagnostics: &mut DiagnosticCollector,
) {
    let prefix = format!("extension://{extension_id}/");
    let valid = reference.strip_prefix(&prefix).is_some_and(|id| {
        contribution_kinds
            .get(id)
            .is_some_and(|kind| kind == expected_kind)
            && reference == format!("{prefix}{id}")
    });
    if !valid {
        diagnostics.push(error(
            "GHEX020_EXTENSION_REF",
            "graph extension reference must name this package and an exact typed contribution id",
            path,
        ));
    }
}

fn escape_json_pointer_token(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn valid_identifier(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|character| !character.is_control() && !character.is_whitespace())
}

fn valid_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn package_digest(manifest: &serde_json::Value, records: &[PackageRecord]) -> String {
    let mut sorted = records.iter().collect::<Vec<_>>();
    sorted.sort_by(|left, right| left.path.cmp(&right.path));
    let canonical_manifest = canonical_manifest(manifest);
    let mut hasher = Sha256::new();
    hasher.update(b"graphhelm:extension-package:v2\0");
    hash_digest_segment(&mut hasher, b"manifest", &canonical_manifest);
    for record in sorted {
        hasher.update(b"resource\0");
        hash_digest_segment(&mut hasher, b"path", record.path.as_bytes());
        hash_digest_segment(&mut hasher, b"sha256", record.digest.as_bytes());
    }
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

#[cfg(test)]
#[path = "extension_security_tests.rs"]
mod security_tests;

#[cfg(test)]
#[path = "extension_capability_tests.rs"]
mod capability_tests;

fn canonical_manifest(manifest: &serde_json::Value) -> Vec<u8> {
    let mut normalized = manifest.clone();
    if let Some(contributions) = normalized
        .pointer_mut("/spec/contracts/contributions")
        .and_then(serde_json::Value::as_array_mut)
    {
        contributions.sort_by(|left, right| {
            let left_path = left
                .get("path")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let right_path = right
                .get("path")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let left_id = left
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let right_id = right
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            (left_path, left_id).cmp(&(right_path, right_id))
        });
    }
    let mut output = Vec::new();
    write_canonical_json(&normalized, &mut output);
    output
}

fn write_canonical_json(value: &serde_json::Value, output: &mut Vec<u8>) {
    match value {
        serde_json::Value::Null => output.extend_from_slice(b"null"),
        serde_json::Value::Bool(value) => {
            output.extend_from_slice(if *value { b"true" } else { b"false" });
        }
        serde_json::Value::Number(value) => output.extend_from_slice(value.to_string().as_bytes()),
        serde_json::Value::String(value) => {
            serde_json::to_writer(output, value).expect("writing JSON to Vec cannot fail");
        }
        serde_json::Value::Array(values) => {
            output.push(b'[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                write_canonical_json(value, output);
            }
            output.push(b']');
        }
        serde_json::Value::Object(values) => {
            output.push(b'{');
            let mut members = values.iter().collect::<Vec<_>>();
            members.sort_by(|left, right| left.0.cmp(right.0));
            for (index, (key, value)) in members.into_iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                serde_json::to_writer(&mut *output, key).expect("writing JSON to Vec cannot fail");
                output.push(b':');
                write_canonical_json(value, output);
            }
            output.push(b'}');
        }
    }
}

fn hash_digest_segment(hasher: &mut Sha256, label: &[u8], bytes: &[u8]) {
    hasher.update((label.len() as u64).to_be_bytes());
    hasher.update(label);
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn at_contribution(mut diagnostic: Diagnostic, base_path: &str) -> Diagnostic {
    diagnostic.path = format!("{base_path}{}", diagnostic.path);
    diagnostic
}

fn error(code: &str, message: impl Into<String>, path: impl Into<String>) -> Diagnostic {
    Diagnostic::error(code, message, path, SOURCE)
}
