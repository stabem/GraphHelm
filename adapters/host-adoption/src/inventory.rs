use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use graphhelm_protocols::adoption::{AdoptionError, AdoptionReason, Coverage};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_DISCOVERY_ENTRIES: usize = 10_000;
const MAX_DISCOVERY_DEPTH: usize = 16;
const MAX_DISCOVERY_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Default)]
struct DiscoveryBudget {
    entries: usize,
    bytes: u64,
    exhausted: bool,
    /// A link was met and recorded as an item; the link itself was never followed.
    linked: bool,
    /// An entry could not be read and was recorded as an item instead of losing its siblings.
    unreadable: bool,
}

impl DiscoveryBudget {
    fn complete(&self) -> bool {
        !self.exhausted && !self.linked && !self.unreadable
    }

    fn reason(&self) -> Value {
        if self.complete() {
            return Value::Null;
        }
        let mut reasons = Vec::new();
        if self.linked {
            reasons.push("linked");
        }
        if self.unreadable {
            reasons.push("unreadable");
        }
        if self.exhausted {
            reasons.push("bound");
        }
        json!(reasons.join("_or_"))
    }
}

/// Documented discovery roots per host. The Claude plugin cache under `~/.claude/plugins` is
/// deliberately not a root: the installed set is the `installed_plugins.json` record read by
/// [`installed_plugin_records`], and walking the cache reports every vendored `SKILL.md` of every
/// cached version as a plugin (measured 2026-09-22: 1205 items, 998 duplicates, on one machine).
fn discovery_roots(host: &str) -> &'static [(&'static str, &'static str, &'static str)] {
    match host {
        "claude" => &[
            ("project", ".claude/skills", "skill"),
            ("project", ".claude/plugins", "plugin"),
            ("home", ".claude/skills", "skill"),
            ("project", ".claude/rules", "rule"),
            ("home", ".claude/rules", "rule"),
            ("project", ".claude/agents", "agent"),
            ("home", ".claude/agents", "agent"),
            ("project", ".claude/commands", "command"),
            ("home", ".claude/commands", "command"),
        ],
        "codex" => &[
            ("project", ".agents/skills", "skill"),
            ("home", ".agents/skills", "skill"),
            ("home", ".codex/skills", "skill"),
        ],
        _ => &[],
    }
}

/// Only the named manifest file of each kind is ever opened; arbitrary package contents are not.
fn wanted_file(kind: &str, file_name: &str) -> bool {
    match kind {
        "skill" => file_name == "SKILL.md",
        "plugin" => matches!(file_name, "plugin.json" | "manifest.json" | "SKILL.md"),
        "rule" | "agent" | "command" => file_name.ends_with(".md"),
        _ => false,
    }
}

/// The documented plugin record. Each entry is installed by definition; enablement still comes
/// from the `enabledPlugins` settings keys, which are inventoried separately.
const INSTALLED_PLUGINS_RECORD: &str = ".claude/plugins/installed_plugins.json";
/// A plugin name is `name@marketplace`. A longer one is not a name this host writes, and each
/// emitted install copies the name twice, so an unbounded one multiplies the record's size.
const MAX_PLUGIN_NAME_BYTES: usize = 512;

fn installed_plugin_records(home: &Path, host: &str) -> (Vec<Value>, Option<Value>) {
    let bytes = match read_candidate(home, INSTALLED_PLUGINS_RECORD) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return (Vec::new(), None),
        Err(error) => {
            return (
                Vec::new(),
                Some(
                    json!({"scope":"plugin-record","state":"incomplete","reason":format!("{:?}", error.reason)}),
                ),
            );
        }
    };
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return (
            Vec::new(),
            Some(json!({"scope":"plugin-record","state":"incomplete","reason":"truncated"})),
        );
    }
    let Some(plugins) = serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|value| value.get("plugins").cloned())
        .and_then(|value| value.as_object().cloned())
    else {
        return (
            Vec::new(),
            Some(json!({"scope":"plugin-record","state":"incomplete","reason":"invalid"})),
        );
    };
    let digest = format!("sha256:{}", hex::encode(Sha256::digest(&bytes)));
    let mut entries = Vec::new();
    // ONE BUDGET ACROSS EVERY EMITTED INSTALL, not across names (review of #1210 by [757c],
    // confirmed by [3f90d6] and [b9deb2]): a record under 1 MiB with one name and a large
    // `installs` array expanded into that many items, each copying the name, so the memory grew
    // with installs x name length. Hitting either bound records the gap instead of truncating in
    // silence.
    let mut truncated = false;
    'records: for (name, installs) in &plugins {
        if name.len() > MAX_PLUGIN_NAME_BYTES {
            truncated = true;
            continue;
        }
        let installs: Vec<&Value> = match installs.as_array() {
            Some(array) => array.iter().collect(),
            None => vec![installs],
        };
        for install in installs {
            if entries.len() >= MAX_DISCOVERY_ENTRIES {
                truncated = true;
                break 'records;
            }
            let scope = install.get("scope").and_then(Value::as_str);
            let version = install.get("version").and_then(Value::as_str);
            entries.push(json!({
                "id": format!("home/{INSTALLED_PLUGINS_RECORD}/plugin/{name}"),
                "name": name, "root": "home", "path": INSTALLED_PLUGINS_RECORD,
                "host": host, "scope": scope.map_or(json!("user"), |scope| json!(scope)),
                "kind": "plugin", "surfaceKind": "plugin", "status": "recorded",
                "installed": true, "enabled": Value::Null, "loaded": Value::Null,
                "version": version.map_or(Value::Null, |version| json!(version)),
                "digest": digest,
                "origin": "installed", "protected": false, "duplicate": false, "managed": false
            }));
        }
    }
    let gap = truncated
        .then(|| json!({"scope":"plugin-record","state":"incomplete","reason":"truncated"}));
    (entries, gap)
}

pub fn inventory(project: &Path, home: &Path) -> Result<Value, AdoptionError> {
    validate_root(project)?;
    validate_root(home)?;
    let mut claude = inspect_host("claude", project, home)?;
    let mut codex = inspect_host("codex", project, home)?;
    let (claude_entries, claude_reason) = scan_known_roots(project, home, "claude")?;
    let (codex_entries, codex_reason) = scan_known_roots(project, home, "codex")?;
    append_scanned(&mut claude, claude_entries, claude_reason);
    append_scanned(&mut codex, codex_entries, codex_reason);
    let (plugin_records, plugin_gap) = installed_plugin_records(home, "claude");
    claude["items"]
        .as_array_mut()
        .expect("host items")
        .extend(plugin_records);
    if let Some(gap) = plugin_gap {
        claude["coverage"] = json!("incomplete");
        claude["coverageDetails"]
            .as_array_mut()
            .expect("coverage details")
            .push(gap);
    }
    mark_duplicates(&mut claude);
    mark_duplicates(&mut codex);
    let coverage = [claude["coverage"].as_str(), codex["coverage"].as_str()]
        .iter()
        .map(|value| match *value {
            Some("complete") => Coverage::Complete,
            Some("truncated") => Coverage::Truncated,
            Some("inaccessible") => Coverage::Inaccessible,
            _ => Coverage::Unsupported,
        })
        .collect::<Vec<_>>();
    let document = json!({
        "apiVersion": "p50.dev/adoption/v1",
        "kind": "Inventory",
        "id": "inventory/local",
        "spec": {
            "roots": [{"id": "project", "scope": "project"}, {"id": "home", "scope": "user"}],
            "rootBindings": crate::root_bindings(project, home)?,
            "hosts": [claude, codex],
            "coverage": if graphhelm_protocols::adoption::coverage_complete(&coverage) { "complete" } else { "incomplete" }
        }
    });
    // This adapter constructs the closed envelope itself. The repository's public schema
    // catalog is intentionally frozen at 1.0.0; a new host-local report cannot silently
    // enlarge that published wire surface before its versioned contract is accepted.
    Ok(document)
}

fn mark_duplicates(host: &mut Value) {
    let mut seen = std::collections::BTreeSet::new();
    let mut duplicate = std::collections::BTreeSet::new();
    if let Some(items) = host["items"].as_array() {
        for item in items {
            if let (Some(kind), Some(name)) = (item["kind"].as_str(), item["name"].as_str()) {
                let key = format!("{kind}:{name}");
                if !seen.insert(key.clone()) {
                    duplicate.insert(key);
                }
            }
        }
    }
    if let Some(items) = host["items"].as_array_mut() {
        for item in items {
            if let (Some(kind), Some(name)) = (item["kind"].as_str(), item["name"].as_str()) {
                item["duplicate"] = json!(duplicate.contains(&format!("{kind}:{name}")));
            }
        }
    }
}

fn append_scanned(host: &mut Value, entries: Vec<Value>, reason: Value) {
    host["items"]
        .as_array_mut()
        .expect("host items")
        .extend(entries);
    let host_name = host["host"].clone();
    let complete = reason.is_null();
    let roots = host["coverageDetails"]
        .as_array_mut()
        .expect("coverage details");
    roots.push(json!({
        "scope": host_name,
        "state": if complete { "complete" } else { "incomplete" },
        "reason": reason
    }));
    if !complete {
        host["coverage"] = json!("incomplete");
    }
}

/// Scan only documented discovery roots. Missing roots are complete (there is nothing
/// installed). A link or an unreadable entry is recorded as an item of its own and the scan
/// goes on with the siblings; only the entry and depth bounds stop it. Returns the entries and
/// the coverage reason (`Null` when complete).
fn scan_known_roots(
    project: &Path,
    home: &Path,
    host: &str,
) -> Result<(Vec<Value>, Value), AdoptionError> {
    let mut entries = Vec::new();
    let mut budget = DiscoveryBudget::default();
    for (scope, relative, kind) in discovery_roots(host) {
        let root = if *scope == "project" { project } else { home };
        let path = root.join(relative);
        if std::fs::symlink_metadata(&path).is_err() {
            continue;
        }
        let _ = scan_tree(&path, root, host, scope, kind, 0, &mut entries, &mut budget);
        if budget.exhausted {
            break;
        }
    }
    Ok((entries, budget.reason()))
}

fn scanned_item(
    root: &Path,
    path: &Path,
    host: &str,
    scope: &str,
    kind: &str,
    status: &str,
) -> Option<Value> {
    let rel = path
        .strip_prefix(root)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    let file_name = path.file_name().and_then(|name| name.to_str())?;
    Some(json!({
        "id": format!("{scope}/{kind}/{rel}/{file_name}"), "root": scope, "path": rel,
        "name": file_name, "host": host, "scope": semantic_scope(scope), "kind": kind,
        "surfaceKind": kind, "status": status, "installed": Value::Null, "enabled": Value::Null,
        "loaded": Value::Null, "digest": Value::Null, "origin": "local",
        "protected": kind == "rule", "duplicate": false, "managed": false
    }))
}

#[allow(clippy::too_many_arguments)]
fn scan_tree(
    path: &Path,
    root: &Path,
    host: &str,
    scope: &str,
    kind: &str,
    depth: usize,
    entries: &mut Vec<Value>,
    budget: &mut DiscoveryBudget,
) -> Result<(), ()> {
    if depth > MAX_DISCOVERY_DEPTH {
        budget.exhausted = true;
        return Err(());
    }
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        budget.unreadable = true;
        entries.extend(scanned_item(root, path, host, scope, kind, "inaccessible"));
        return Ok(());
    };
    // A symlink or a Windows junction is a fact about the host (this machine keeps 24 of them
    // under ~/.claude/skills, into ~/.agents/skills). It is recorded and never followed; the
    // siblings after it are still scanned. Aborting here used to hide every later entry and
    // made backup refuse the whole snapshot (measured 2026-09-22).
    if meta.file_type().is_symlink() || is_reparse_point(&meta) {
        budget.linked = true;
        entries.extend(scanned_item(root, path, host, scope, kind, "linked"));
        return Ok(());
    }
    if meta.is_file() {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        // Filter names before opening files. Arbitrary package contents are never read.
        if !wanted_file(kind, file_name) {
            return Ok(());
        }
        let rel = path
            .strip_prefix(root)
            .map_err(|_| ())?
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = match read_candidate(root, &rel) {
            Ok(Some(bytes)) if bytes.len() as u64 <= MAX_FILE_BYTES => bytes,
            Ok(Some(_)) => {
                budget.unreadable = true;
                entries.extend(scanned_item(root, path, host, scope, kind, "truncated"));
                return Ok(());
            }
            Ok(None) | Err(_) => {
                budget.unreadable = true;
                entries.extend(scanned_item(root, path, host, scope, kind, "inaccessible"));
                return Ok(());
            }
        };
        let unit_name = logical_package_name(kind, path, &bytes, file_name);
        let stable = format!("{scope}/{kind}/{rel}/{unit_name}");
        entries.push(json!({
            "id": stable, "root": scope, "path": rel,
            "name": unit_name, "host": host, "scope": semantic_scope(scope), "kind": kind, "surfaceKind": kind, "status": "observed",
            "installed": true, "enabled": Value::Null, "loaded": Value::Null,
            "digest": format!("sha256:{}", hex::encode(Sha256::digest(&bytes))),
            "origin": "local", "protected": kind == "rule", "duplicate": false, "managed": false
        }));
        return Ok(());
    }
    let Ok(children) = std::fs::read_dir(path) else {
        budget.unreadable = true;
        entries.extend(scanned_item(root, path, host, scope, kind, "inaccessible"));
        return Ok(());
    };
    for child in children {
        if budget.entries >= MAX_DISCOVERY_ENTRIES {
            budget.exhausted = true;
            return Err(());
        }
        budget.entries += 1;
        let Ok(child) = child else {
            budget.unreadable = true;
            continue;
        };
        scan_tree(
            &child.path(),
            root,
            host,
            scope,
            kind,
            depth + 1,
            entries,
            budget,
        )?;
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse_point(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_meta: &std::fs::Metadata) -> bool {
    false
}

fn logical_package_name(kind: &str, path: &Path, bytes: &[u8], file_name: &str) -> String {
    // A rule, agent or command is one Markdown file; Claude Code names it by its stem
    // (`deploy.md` is `/deploy`). A skill or plugin is a directory named by its parent.
    if matches!(kind, "rule" | "agent" | "command") {
        return file_name
            .strip_suffix(".md")
            .filter(|stem| !stem.is_empty())
            .unwrap_or(file_name)
            .to_owned();
    }
    if kind == "plugin"
        && let Ok(value) = serde_json::from_slice::<Value>(bytes)
        && let Some(name) = value.get("name").and_then(Value::as_str)
        && !name.is_empty()
        && name.len() <= 256
        && !name.contains(['/', '\\', ':'])
    {
        return name.to_owned();
    }
    let package_dir = path.parent().and_then(|parent| {
        if parent.file_name().and_then(|name| name.to_str()) == Some(".claude-plugin") {
            parent.parent()
        } else {
            Some(parent)
        }
    });
    package_dir
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(file_name)
        .to_owned()
}

/// Returns the bounded manifest bytes that inventory observed. These are recovery evidence for
/// discovered packages; they are deliberately not mutable adoption surfaces.
pub(crate) fn discovered_manifest_files(
    project: &crate::storage::Root,
    home: &crate::storage::Root,
    byte_limit: u64,
) -> Result<BTreeMap<String, Vec<u8>>, AdoptionError> {
    let mut result = BTreeMap::new();
    let mut budget = DiscoveryBudget::default();
    let byte_limit = byte_limit.min(MAX_DISCOVERY_BYTES);
    for host in ["claude", "codex"] {
        // Backup evidence is the package manifests only; rules, agents and commands are
        // instruction files, not packages, and the manifest validator names the package files.
        for (scope, relative, kind) in discovery_roots(host)
            .iter()
            .filter(|(_, _, kind)| matches!(*kind, "skill" | "plugin"))
        {
            let root = if *scope == "project" { project } else { home };
            let path = root.record.path.join(relative);
            if std::fs::symlink_metadata(&path).is_err() {
                continue;
            }
            collect_manifest_files(
                &path,
                root,
                scope,
                kind,
                0,
                byte_limit,
                &mut budget,
                &mut result,
            )?;
            if budget.exhausted {
                return Err(AdoptionError {
                    reason: AdoptionReason::LimitExceeded,
                });
            }
        }
    }
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn collect_manifest_files(
    path: &Path,
    root: &crate::storage::Root,
    scope: &str,
    kind: &str,
    depth: usize,
    byte_limit: u64,
    budget: &mut DiscoveryBudget,
    result: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), AdoptionError> {
    if depth > MAX_DISCOVERY_DEPTH {
        return Err(AdoptionError {
            reason: AdoptionReason::LimitExceeded,
        });
    }
    let meta = std::fs::symlink_metadata(path).map_err(|_| AdoptionError {
        reason: AdoptionReason::CoverageIncomplete,
    })?;
    // A linked entry is not followed and not snapshotted: its bytes belong to another root.
    // The inventory records it as `linked`; the backup must still cover the siblings.
    if meta.file_type().is_symlink() || is_reparse_point(&meta) {
        return Ok(());
    }
    if meta.is_file() {
        let file_name = path
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or_default();
        if !wanted_file(kind, file_name) {
            return Ok(());
        }
        let rel = path
            .strip_prefix(&root.record.path)
            .map_err(|_| AdoptionError {
                reason: AdoptionReason::PathUnsafe,
            })?
            .to_string_lossy()
            .replace('\\', "/");
        let source = root.source_optional(&rel)?.ok_or(AdoptionError {
            reason: AdoptionReason::CoverageIncomplete,
        })?;
        let remaining = byte_limit.saturating_sub(budget.bytes);
        let bytes = crate::storage::read_file(&source.file, remaining.min(MAX_FILE_BYTES))?;
        budget.bytes = budget
            .bytes
            .checked_add(bytes.len() as u64)
            .ok_or(AdoptionError {
                reason: AdoptionReason::LimitExceeded,
            })?;
        if budget.bytes > byte_limit {
            budget.exhausted = true;
            return Err(AdoptionError {
                reason: AdoptionReason::LimitExceeded,
            });
        }
        result.insert(format!("discovered/{scope}/{kind}/{rel}"), bytes);
        return Ok(());
    }
    for child in std::fs::read_dir(path).map_err(|_| AdoptionError {
        reason: AdoptionReason::CoverageIncomplete,
    })? {
        if budget.entries >= MAX_DISCOVERY_ENTRIES {
            budget.exhausted = true;
            return Err(AdoptionError {
                reason: AdoptionReason::LimitExceeded,
            });
        }
        budget.entries += 1;
        collect_manifest_files(
            &child
                .map_err(|_| AdoptionError {
                    reason: AdoptionReason::CoverageIncomplete,
                })?
                .path(),
            root,
            scope,
            kind,
            depth + 1,
            byte_limit,
            budget,
            result,
        )?;
    }
    Ok(())
}

fn inspect_host(host: &str, project: &Path, home: &Path) -> Result<Value, AdoptionError> {
    let mut items = Vec::new();
    let mut coverage = "complete";
    let mut coverage_details = vec![
        json!({"scope":"host-api","state":"incomplete","reason":"host_api_unavailable"}),
        json!({"scope":"plugin-browser","state":"incomplete","reason":"plugin_browser_unavailable"}),
    ];
    for surface in crate::surfaces::for_host(host) {
        let root = if surface.scope == "project" {
            project
        } else {
            home
        };
        let relative = surface.path;
        let bytes = match read_candidate(root, relative) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => {
                items.push(json!({
                    "id": surface.id, "root": surface.scope, "path": relative,
                    "host": host, "scope": semantic_scope(surface.scope), "kind": relative,
                    "surfaceKind": surface.kind, "status": "absent", "installed": false,
                    "enabled": Value::Null, "loaded": Value::Null, "digest": Value::Null,
                    "origin": "local", "protected": surface.kind == "instructions" || surface.kind == "mcp",
                    "duplicate": false, "managed": surface.kind == "managed_settings"
                }));
                continue;
            }
            Err(error) => {
                coverage = "inaccessible";
                coverage_details.push(json!({
                    "scope": surface.id, "state": "incomplete", "reason": format!("{:?}", error.reason)
                }));
                items.push(json!({
                    "id": surface.id, "root": surface.scope, "path": relative,
                    "host": host, "scope": semantic_scope(surface.scope), "kind": relative,
                    "surfaceKind": surface.kind, "status": "inaccessible", "installed": Value::Null,
                    "enabled": Value::Null, "loaded": Value::Null, "digest": Value::Null,
                    "origin": "local", "protected": surface.kind == "instructions" || surface.kind == "mcp",
                    "duplicate": false, "managed": surface.kind == "managed_settings"
                }));
                continue;
            }
        };
        if bytes.len() as u64 > MAX_FILE_BYTES {
            coverage = "truncated";
            items.push(json!({
                "id": surface.id, "root": surface.scope, "path": relative,
                "host": host, "scope": semantic_scope(surface.scope), "kind": relative,
                "surfaceKind": surface.kind,
                "status": "truncated", "installed": true, "enabled": Value::Null,
                "loaded": Value::Null, "origin": "local", "protected": false,
                "duplicate": false, "managed": false
            }));
            continue;
        }
        let parse_status = parse_status(relative, &bytes);
        let mut item = json!({
            "id": surface.id,
            "root": surface.scope,
            "path": relative,
            "host": host,
            "scope": semantic_scope(surface.scope),
            "kind": relative,
            "surfaceKind": surface.kind,
            "status": parse_status,
            "installed": true,
            "enabled": Value::Null,
            "loaded": Value::Null,
            "digest": format!("sha256:{}", hex::encode(Sha256::digest(&bytes))),
            "origin": "local",
            "protected": surface.kind == "instructions" || surface.kind == "mcp" || surface.kind == "managed_settings",
            "duplicate": false,
            "managed": surface.kind == "managed_settings"
        });
        if serde_json::from_slice::<Value>(&bytes)
            .ok()
            .is_some_and(|v| v.get("hooks").is_some())
        {
            item["hookEffects"] = json!("unknown_not_executed");
        }
        items.push(item);
        append_configured_entries(&mut items, host, surface.scope, relative, &bytes);
    }
    if host == "claude" {
        let (managed_item, managed_detail) = managed_policy_observation(host);
        items.push(managed_item);
        coverage_details.push(managed_detail);
    }
    if coverage == "complete" {
        coverage = "incomplete";
    }
    Ok(
        json!({"host": host, "coverage": coverage, "coverageDetails": coverage_details, "items": items}),
    )
}

/// The platform directory Claude Code reads managed policy from
/// (code.claude.com/docs/en/managed-settings, checked 2026-09-22). It lies outside both roots,
/// so it is observed read-only and is never a backup, apply or restore surface. The registry and
/// MDM channels the same page documents are not read; the coverage detail says so.
fn managed_policy_directory() -> std::path::PathBuf {
    if cfg!(windows) {
        std::path::PathBuf::from(
            std::env::var_os("ProgramFiles").unwrap_or_else(|| "C:\\Program Files".into()),
        )
        .join("ClaudeCode")
    } else if cfg!(target_os = "macos") {
        std::path::PathBuf::from("/Library/Application Support/ClaudeCode")
    } else {
        std::path::PathBuf::from("/etc/claude-code")
    }
}

fn managed_policy_observation(host: &str) -> (Value, Value) {
    const FILE: &str = "managed-settings.json";
    let directory = managed_policy_directory();
    let location = format!("{}/{FILE}", directory.display()).replace('\\', "/");
    let mut item = json!({
        "id": "managed/managed-settings.json", "root": "managed", "path": location,
        "host": host, "scope": "managed", "kind": FILE, "surfaceKind": "managed_settings",
        "status": "absent", "installed": false, "enabled": Value::Null, "loaded": Value::Null,
        "digest": Value::Null, "origin": "platform", "protected": true, "duplicate": false,
        "managed": true
    });
    let detail = json!({"scope":"managed-policy","state":"incomplete","reason":"registry_mdm_and_managed_settings_d_not_inspected"});
    if std::fs::symlink_metadata(&directory).is_err() {
        return (item, detail);
    }
    match read_candidate(&directory, FILE) {
        Ok(None) => {}
        Ok(Some(bytes)) if bytes.len() as u64 > MAX_FILE_BYTES => {
            item["status"] = json!("truncated");
            item["installed"] = json!(true);
        }
        Ok(Some(bytes)) => {
            item["status"] = json!(parse_status(FILE, &bytes));
            item["installed"] = json!(true);
            item["digest"] = json!(format!("sha256:{}", hex::encode(Sha256::digest(&bytes))));
        }
        Err(_) => {
            item["status"] = json!("inaccessible");
            item["installed"] = Value::Null;
        }
    }
    (item, detail)
}

fn append_configured_entries(
    items: &mut Vec<Value>,
    host: &str,
    scope: &str,
    path: &str,
    bytes: &[u8],
) {
    let item_scope = semantic_scope(scope);
    if path.ends_with(".json") {
        let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
            return;
        };
        if let Some(plugins) = value.get("enabledPlugins").and_then(Value::as_object) {
            for (name, enabled) in plugins {
                let enabled = enabled.as_bool().map_or(Value::Null, |value| json!(value));
                items.push(json!({"id":format!("{scope}/{path}/plugin/{name}"),"name":name,"root":scope,"path":path,"host":host,"scope":item_scope,"kind":"plugin","installed":Value::Null,"enabled":enabled,"loaded":Value::Null,"digest":Value::Null,"origin":"configured","protected":false,"duplicate":false,"managed":false}));
            }
        }
        if let Some(servers) = value.get("mcpServers").and_then(Value::as_object) {
            for (name, config) in servers {
                let enabled = config.get("enabled").and_then(Value::as_bool);
                items.push(json!({"id":format!("{scope}/{path}/mcp/{name}"),"name":name,"root":scope,"path":path,"host":host,"scope":item_scope,"kind":"mcp_server","installed":Value::Null,"enabled":enabled,"loaded":Value::Null,"digest":Value::Null,"origin":"configured","protected":true,"duplicate":false,"managed":false}));
            }
        }
    } else if path.ends_with("config.toml") {
        let Ok(value) = toml::from_str::<toml::Value>(&String::from_utf8_lossy(bytes)) else {
            return;
        };
        if let Some(config) = value
            .get("skills")
            .and_then(|v| v.get("config"))
            .and_then(toml::Value::as_array)
        {
            for entry in config {
                if let Some(name) = entry.get("path").and_then(toml::Value::as_str) {
                    let enabled = entry.get("enabled").and_then(toml::Value::as_bool);
                    let logical = name
                        .rsplit(['/', '\\'])
                        .next()
                        .filter(|v| !v.is_empty())
                        .unwrap_or("configured-skill");
                    let source_id = hex::encode(Sha256::digest(name.as_bytes()));
                    items.push(json!({"id":format!("{scope}/{path}/skill/{source_id}"),"name":logical,"root":scope,"path":path,"host":host,"scope":item_scope,"kind":"skill","installed":Value::Null,"enabled":enabled,"loaded":Value::Null,"digest":Value::Null,"origin":"configured","protected":false,"duplicate":false,"managed":false}));
                }
            }
        }
        if let Some(servers) = value.get("mcp_servers").and_then(toml::Value::as_table) {
            for (name, config) in servers {
                let enabled = config.get("enabled").and_then(toml::Value::as_bool);
                items.push(json!({"id":format!("{scope}/{path}/mcp/{name}"),"name":name,"root":scope,"path":path,"host":host,"scope":item_scope,"kind":"mcp_server","installed":Value::Null,"enabled":enabled,"loaded":Value::Null,"digest":Value::Null,"origin":"configured","protected":true,"duplicate":false,"managed":false}));
            }
        }
    }
}

fn semantic_scope(root: &str) -> &'static str {
    if root == "project" { "project" } else { "user" }
}

fn validate_root(root: &Path) -> Result<(), AdoptionError> {
    let metadata = std::fs::symlink_metadata(root).map_err(|_| AdoptionError {
        reason: AdoptionReason::CoverageIncomplete,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(AdoptionError {
            reason: AdoptionReason::PathUnsafe,
        });
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(AdoptionError {
                reason: AdoptionReason::PathUnsafe,
            });
        }
    }
    open_root(root).map(|_| ()).map_err(|_| AdoptionError {
        reason: AdoptionReason::CoverageIncomplete,
    })
}

fn read_candidate(root: &Path, relative: &str) -> Result<Option<Vec<u8>>, AdoptionError> {
    let file = match open_beneath(root, relative) {
        Ok(Some(file)) => file,
        Ok(None) => return Ok(None),
        Err(reason) => return Err(AdoptionError { reason }),
    };
    let metadata = file.metadata().map_err(|_| AdoptionError {
        reason: AdoptionReason::CoverageIncomplete,
    })?;
    if !metadata.is_file() {
        return Err(AdoptionError {
            reason: AdoptionReason::PathUnsafe,
        });
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| AdoptionError {
            reason: AdoptionReason::CoverageIncomplete,
        })?;
    Ok(Some(bytes))
}

#[cfg(unix)]
fn open_root(root: &Path) -> std::io::Result<std::fs::File> {
    open_unix_root(root).map(std::fs::File::from)
}

#[cfg(all(not(unix), not(windows)))]
fn open_root(root: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(root)
}

#[cfg(windows)]
fn open_root(root: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(root)
}

#[cfg(unix)]
fn open_unix_root(root: &Path) -> std::io::Result<std::os::fd::OwnedFd> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::path::Component;

    if !root.is_absolute() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "root must be absolute",
        ));
    }
    let fd = unsafe {
        libc::open(
            c"/".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut current = unsafe { OwnedFd::from_raw_fd(fd) };
    for component in root.components() {
        let Component::Normal(component) = component else {
            if matches!(component, Component::RootDir) {
                continue;
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "unsafe root component",
            ));
        };
        let name = CString::new(component.as_encoded_bytes()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "NUL in root component")
        })?;
        let next = unsafe {
            libc::openat(
                current.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if next < 0 {
            return Err(std::io::Error::last_os_error());
        }
        current = unsafe { OwnedFd::from_raw_fd(next) };
    }
    Ok(current)
}

#[cfg(windows)]
fn open_windows_root_chain(root: &Path) -> Result<Vec<std::fs::File>, AdoptionReason> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::path::Component;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    if !root.is_absolute() {
        return Err(AdoptionReason::PathUnsafe);
    }
    let mut current = std::path::PathBuf::new();
    let mut retained = Vec::new();
    for component in root.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                current.push(component.as_os_str());
                if !matches!(component, Component::RootDir) {
                    continue;
                }
            }
            Component::Normal(_) => current.push(component.as_os_str()),
            Component::CurDir | Component::ParentDir => return Err(AdoptionReason::PathUnsafe),
        }
        let handle = std::fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&current)
            .map_err(|_| AdoptionReason::CoverageIncomplete)?;
        let metadata = handle
            .metadata()
            .map_err(|_| AdoptionReason::CoverageIncomplete)?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || !metadata.is_dir() {
            return Err(AdoptionReason::PathUnsafe);
        }
        retained.push(handle);
    }
    Ok(retained)
}

#[cfg(unix)]
fn open_beneath(root: &Path, relative: &str) -> Result<Option<std::fs::File>, AdoptionReason> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    let mut current = open_unix_root(root).map_err(|error| match error.raw_os_error() {
        Some(libc::ELOOP) | Some(libc::ENOTDIR) => AdoptionReason::PathUnsafe,
        _ => AdoptionReason::CoverageIncomplete,
    })?;
    let components: Vec<&str> = relative.split('/').collect();
    for (index, component) in components.iter().enumerate() {
        let name = CString::new(*component).map_err(|_| AdoptionReason::PathUnsafe)?;
        let flags = if index + 1 == components.len() {
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC
        } else {
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC
        };
        let fd = unsafe { libc::openat(current.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::ENOENT) => Ok(None),
                Some(libc::ELOOP) | Some(libc::ENOTDIR) => Err(AdoptionReason::PathUnsafe),
                _ => Err(AdoptionReason::CoverageIncomplete),
            };
        }
        current = unsafe { OwnedFd::from_raw_fd(fd) };
    }
    Ok(Some(std::fs::File::from(current)))
}

#[cfg(windows)]
fn open_beneath(root: &Path, relative: &str) -> Result<Option<std::fs::File>, AdoptionReason> {
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::MetadataExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT,
        FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
    };
    use windows_sys::Win32::Foundation::{HANDLE, UNICODE_STRING};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_READ_DATA,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, READ_CONTROL,
        SYNCHRONIZE,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    enum ChildOpenError {
        Missing,
        Reason(AdoptionReason),
    }

    fn open_child(
        parent: &std::fs::File,
        name: &std::ffi::OsStr,
        directory: bool,
    ) -> Result<std::fs::File, ChildOpenError> {
        let mut wide = name.encode_wide().collect::<Vec<_>>();
        let byte_length = wide
            .len()
            .checked_mul(2)
            .and_then(|length| u16::try_from(length).ok())
            .ok_or(ChildOpenError::Reason(AdoptionReason::PathUnsafe))?;
        let unicode = UNICODE_STRING {
            Length: byte_length,
            MaximumLength: byte_length,
            Buffer: wide.as_mut_ptr(),
        };
        let attributes = OBJECT_ATTRIBUTES {
            Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: parent.as_raw_handle(),
            ObjectName: &unicode,
            Attributes: 0,
            SecurityDescriptor: std::ptr::null(),
            SecurityQualityOfService: std::ptr::null(),
        };
        let mut handle: HANDLE = std::ptr::null_mut();
        let mut status: IO_STATUS_BLOCK = unsafe { zeroed() };
        let access = if directory {
            FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE
        } else {
            FILE_READ_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE
        };
        let result = unsafe {
            NtCreateFile(
                &mut handle,
                access,
                &attributes,
                &mut status,
                std::ptr::null(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                FILE_OPEN,
                FILE_OPEN_REPARSE_POINT
                    | FILE_SYNCHRONOUS_IO_NONALERT
                    | if directory {
                        FILE_DIRECTORY_FILE
                    } else {
                        FILE_NON_DIRECTORY_FILE
                    },
                std::ptr::null(),
                0,
            )
        };
        if result < 0 || handle.is_null() {
            return if result == 0xC000_0034_u32 as i32
                || result == 0xC000_003A_u32 as i32
                || result == 0xC000_000F_u32 as i32
            {
                Err(ChildOpenError::Missing)
            } else {
                Err(ChildOpenError::Reason(AdoptionReason::CoverageIncomplete))
            };
        }
        let file = unsafe { std::fs::File::from_raw_handle(handle) };
        let metadata = file
            .metadata()
            .map_err(|_| ChildOpenError::Reason(AdoptionReason::CoverageIncomplete))?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || metadata.is_dir() != directory
            || (!directory && !metadata.is_file())
        {
            return Err(ChildOpenError::Reason(AdoptionReason::PathUnsafe));
        }
        Ok(file)
    }

    let mut current = open_windows_root_chain(root)?
        .pop()
        .ok_or(AdoptionReason::CoverageIncomplete)?;
    let components: Vec<&str> = relative.split('/').collect();
    for (index, component) in components.iter().enumerate() {
        let child = match open_child(
            &current,
            std::ffi::OsStr::new(component),
            index + 1 != components.len(),
        ) {
            Ok(child) => child,
            Err(ChildOpenError::Missing) => return Ok(None),
            Err(ChildOpenError::Reason(reason)) => return Err(reason),
        };
        current = child;
    }
    Ok(Some(current))
}

fn parse_status(surface: &str, bytes: &[u8]) -> &'static str {
    if surface.ends_with(".json") {
        if serde_json::from_slice::<Value>(bytes).is_ok() {
            "parsed"
        } else {
            "invalid"
        }
    } else if surface.ends_with(".toml") {
        if std::str::from_utf8(bytes)
            .ok()
            .and_then(|text| toml::from_str::<toml::Value>(text).ok())
            .is_some()
        {
            "parsed"
        } else {
            "invalid"
        }
    } else {
        "observed"
    }
}
