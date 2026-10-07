//! `graphhelm skills sync --host codex` (#355): installs the bundled GraphHelm skills into a
//! Codex home and keeps them current. The bundle is located through the same pinned release file
//! `setup` installs packages from (`release_packages`), so the skills synced are the skills of the
//! packages that release pins, plus the two plugin skill roots shipped beside it.
//!
//! Ownership is a manifest at `<home>/skills/.graphhelm-skills.json` naming every skill this
//! command wrote and the digest of the bytes it wrote. A directory the manifest does not name is
//! someone else's and is never written, except when it already holds exactly the bundled bytes
//! (then it is adopted: recording it changes nothing on disk). An installed skill whose bytes no
//! longer match the recorded digest was edited by hand and is left alone.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use graphhelm_protocols::Diagnostic;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::output::Outcome;

const COMMAND: &str = "skills.sync";
const SOURCE: &str = "skills-cli";
const MANIFEST: &str = ".graphhelm-skills.json";
const SCHEMA: &str = "graphhelm.skills-manifest/1";
/// The plugin skill roots shipped beside the extension packages, relative to the bundle root.
const PLUGIN_ROOTS: [&str; 2] = [
    "plugins/graphhelm/skills",
    "examples/chat-surface/claude-code-plugin/skills",
];

pub(super) fn sync(home: Option<&Path>, dry_run: bool) -> Outcome {
    match run(home, dry_run) {
        Ok(data) => Outcome::success(COMMAND, data),
        Err(message) => Outcome::application(
            COMMAND,
            Diagnostic::error(
                crate::error_codes::GHCLI035_SKILLS_SYNC_REFUSED,
                message,
                "/",
                SOURCE,
            ),
        ),
    }
}

fn codex_home(home: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(home) = home {
        return Ok(home.to_path_buf());
    }
    if let Some(home) = std::env::var_os("CODEX_HOME").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(home));
    }
    let user = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .ok_or("no --home, CODEX_HOME or home directory to find the Codex home in")?;
    Ok(PathBuf::from(user).join(".codex"))
}

/// Every bundled skill by name: a directory holding `SKILL.md` under a pinned package's
/// `skills/` or under one of the plugin skill roots. Two skills with one name are refused: Codex
/// reads one directory per name, so a silent winner would hide the other.
fn bundled() -> Result<BTreeMap<String, PathBuf>, String> {
    let packages = graphhelm_host_adoption::hosts::release_packages()
        .map_err(|_| "the pinned release bundle could not be read".to_owned())?;
    let mut roots = Vec::new();
    let mut bundle_root = None;
    for package in packages {
        let path = std::fs::canonicalize(&package.path)
            .map_err(|e| format!("pinned package {} is not readable: {e}", package.id))?;
        // <root>/extensions/builtin/<package>
        bundle_root = path.ancestors().nth(3).map(Path::to_path_buf);
        roots.push(path.join("skills"));
    }
    let bundle_root = bundle_root.ok_or("the release bundle pins no package")?;
    roots.extend(PLUGIN_ROOTS.iter().map(|r| bundle_root.join(r)));
    let mut skills = BTreeMap::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries {
            let path = entry.map_err(|e| e.to_string())?.path();
            if !path.join("SKILL.md").is_file() {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or("a skill directory name is not UTF-8")?
                .to_owned();
            if let Some(other) = skills.insert(name.clone(), path.clone()) {
                return Err(format!(
                    "skill {name} is bundled twice: {} and {}",
                    other.display(),
                    path.display()
                ));
            }
        }
    }
    Ok(skills)
}

/// The relative paths (forward slashes) and bytes of every file under `dir`, sorted.
fn files(dir: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) -> Result<(), String> {
        for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            let path = entry.path();
            if kind.is_symlink() {
                return Err(format!("{} is a link; skills hold files", path.display()));
            }
            if kind.is_dir() {
                walk(base, &path, out)?;
            } else {
                let relative = path
                    .strip_prefix(base)
                    .map_err(|e| e.to_string())?
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                out.push((relative, bytes));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// `sha256:` over each file's path and bytes, both length-prefixed so no two trees share a digest.
fn digest(files: &[(String, Vec<u8>)]) -> String {
    let mut hasher = Sha256::new();
    for (path, bytes) in files {
        hasher.update((path.len() as u64).to_be_bytes());
        hasher.update(path.as_bytes());
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(bytes);
    }
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

fn on_disk_digest(dir: &Path) -> Result<Option<String>, String> {
    if !dir.exists() {
        return Ok(None);
    }
    Ok(Some(digest(&files(dir)?)))
}

/// Writes the skill to a sibling temporary directory, then swaps it into place.
fn install(skills: &Path, name: &str, files: &[(String, Vec<u8>)]) -> Result<(), String> {
    let staging = skills.join(format!(".{name}.graphhelm-staging"));
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
    }
    for (relative, bytes) in files {
        let path = staging.join(relative);
        std::fs::create_dir_all(path.parent().unwrap_or(&staging)).map_err(|e| e.to_string())?;
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let target = skills.join(name);
    if target.exists() {
        std::fs::remove_dir_all(&target).map_err(|e| format!("{}: {e}", target.display()))?;
    }
    std::fs::rename(&staging, &target).map_err(|e| format!("{}: {e}", target.display()))
}

fn run(home: Option<&Path>, dry_run: bool) -> Result<Value, String> {
    let home = codex_home(home)?;
    let skills = home.join("skills");
    let manifest_path = skills.join(MANIFEST);
    let mut owned: BTreeMap<String, String> = match std::fs::read(&manifest_path) {
        Ok(bytes) => {
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|e| format!("{} is not JSON: {e}", manifest_path.display()))?;
            if value["schema"] != SCHEMA {
                return Err(format!("{} is not a {SCHEMA}", manifest_path.display()));
            }
            value["skills"]
                .as_object()
                .ok_or("the manifest has no skills object")?
                .iter()
                .map(|(name, entry)| {
                    entry["digest"]
                        .as_str()
                        .map(|d| (name.clone(), d.to_owned()))
                        .ok_or_else(|| format!("manifest entry {name} has no digest"))
                })
                .collect::<Result<_, _>>()?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(e) => return Err(format!("{}: {e}", manifest_path.display())),
    };

    let mut report: BTreeMap<&str, Vec<String>> = [
        "added",
        "updated",
        "unchanged",
        "adopted",
        "removed",
        "modified",
        "foreign",
    ]
    .into_iter()
    .map(|k| (k, Vec::new()))
    .collect();
    let mut push = |key: &'static str, name: &str| report.get_mut(key).unwrap().push(name.into());

    let bundle = bundled()?;
    if !dry_run {
        std::fs::create_dir_all(&skills).map_err(|e| format!("{}: {e}", skills.display()))?;
    }
    for (name, source) in &bundle {
        let wanted = files(source)?;
        let wanted_digest = digest(&wanted);
        let target = skills.join(name);
        let present = on_disk_digest(&target)?;
        match (owned.get(name), present) {
            (_, None) => {
                push("added", name);
                if !dry_run {
                    install(&skills, name, &wanted)?;
                }
                owned.insert(name.clone(), wanted_digest);
            }
            (_, Some(current)) if current == wanted_digest => {
                if owned.contains_key(name) {
                    push("unchanged", name);
                } else {
                    push("adopted", name);
                }
                owned.insert(name.clone(), wanted_digest);
            }
            (Some(recorded), Some(current)) if *recorded == current => {
                push("updated", name);
                if !dry_run {
                    install(&skills, name, &wanted)?;
                }
                owned.insert(name.clone(), wanted_digest);
            }
            (Some(_), Some(_)) => push("modified", name),
            (None, Some(_)) => push("foreign", name),
        }
    }
    for name in owned.keys().cloned().collect::<Vec<_>>() {
        if bundle.contains_key(&name) {
            continue;
        }
        let target = skills.join(&name);
        match on_disk_digest(&target)? {
            None => {
                owned.remove(&name);
            }
            Some(current) if current == owned[&name] => {
                push("removed", &name);
                if !dry_run {
                    std::fs::remove_dir_all(&target)
                        .map_err(|e| format!("{}: {e}", target.display()))?;
                }
                owned.remove(&name);
            }
            Some(_) => push("modified", &name),
        }
    }

    if !dry_run {
        let manifest = json!({
            "schema": SCHEMA,
            "host": "codex",
            "skills": owned
                .iter()
                .map(|(name, digest)| (name.clone(), json!({ "digest": digest })))
                .collect::<serde_json::Map<_, _>>(),
        });
        let text = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())? + "\n";
        std::fs::write(&manifest_path, text)
            .map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    }

    let mut data = json!({
        "host": "codex",
        "skillsDir": skills.display().to_string(),
        "dryRun": dry_run,
    });
    for (key, names) in report {
        data[key] = json!(names);
    }
    Ok(data)
}
