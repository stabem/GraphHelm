//! The self-verifying Milestone 05 acceptance map (05f Task 6): `m05-clauses.toml` binds
//! every runtime-design §8 clause to the tests that prove it, this crate generates the
//! human document from those bindings, and the grounding test refuses a map that has
//! rusted — a renamed fn, a suite off the gate, a gutted assertion, a decayed D-citation,
//! or a stale committed document.

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Clauses {
    pub clause: Vec<Clause>,
    #[serde(default)]
    pub refused: Vec<Refused>,
}

#[derive(Debug, Deserialize)]
pub struct Clause {
    pub id: String,
    pub text: String,
    pub rationale: String,
    #[serde(default)]
    pub prover: Vec<Prover>,
    /// True for the one clause proven by the gate run itself rather than a test fn.
    #[serde(default)]
    pub gate: bool,
    /// Artifact provers (the manual acceptance run's committed evidence).
    #[serde(default)]
    pub artifact: Vec<ArtifactProver>,
    /// Demonstration provers (M06 Task 6): recorded journeys replayed against the
    /// current build.
    #[serde(default)]
    pub demonstration: Vec<Demonstration>,
}

/// The third binding: a recorded journey artifact — a committed event store, a frozen
/// traversal seed sampled at recording, and the projection digest the current build must
/// reproduce on replay.
#[derive(Debug, Deserialize)]
pub struct Demonstration {
    /// Repo-relative directory holding `demo.json`, the `events/` store and `SHA256SUMS`.
    pub directory: String,
    /// What the journey demonstrates — rendered into the map beside the directory.
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct Prover {
    #[serde(rename = "fn")]
    pub function: String,
    pub suite: String,
    pub assert_fingerprint: String,
}

/// The artifact prover (05f Task 7): a clause proven by a MANUAL run whose artifacts are
/// committed and checksummed — the gate never re-runs the paid call, it verifies the
/// evidence still exists and still hashes to what the run recorded.
#[derive(Debug, Deserialize)]
pub struct ArtifactProver {
    /// Repo-relative directory holding the run's artifacts and its `SHA256SUMS`.
    pub directory: String,
    /// What the run was — rendered into the map beside the directory.
    pub description: String,
}

/// Re-hashes every file `SHA256SUMS` names and compares; returns the mismatches (empty =
/// the evidence is intact). Files present in the directory but absent from the sums file
/// are also mismatches — evidence cannot be quietly swapped in either direction.
pub fn verify_artifacts(root: &Path, directory: &str) -> Vec<String> {
    use sha2::{Digest, Sha256};
    let base = root.join(directory);
    let mut problems = Vec::new();
    let Ok(sums) = std::fs::read_to_string(base.join("SHA256SUMS")) else {
        return vec![format!("{directory}/SHA256SUMS is missing")];
    };
    let mut named = std::collections::BTreeSet::new();
    for line in sums.lines().filter(|line| !line.trim().is_empty()) {
        let Some((expected, relative)) = line.split_once("  ") else {
            problems.push(format!("malformed SHA256SUMS line: {line}"));
            continue;
        };
        named.insert(relative.to_owned());
        match std::fs::read(base.join(relative)) {
            Ok(bytes) => {
                let actual = hex::encode(Sha256::digest(&bytes));
                if actual != expected {
                    problems.push(format!("{relative}: hash mismatch"));
                }
            }
            Err(_) => problems.push(format!("{relative}: named but missing")),
        }
    }
    let mut on_disk: Vec<String> = Vec::new();
    walk_all(&base, &base, &mut on_disk);
    for relative in on_disk {
        if relative != "SHA256SUMS" && !named.contains(&relative) {
            problems.push(format!("{relative}: on disk but not in SHA256SUMS"));
        }
    }
    problems
}

fn walk_all(dir: &Path, base: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_all(&path, base, out);
        } else if let Ok(relative) = path.strip_prefix(base) {
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// What `demo.json` freezes at recording time: the seed sampled from injected entropy,
/// the seed-derived node-check traversal, and the projection digest the current build
/// must reproduce.
#[derive(Debug, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DemoManifest {
    /// The traversal seed — sampled AT RECORDING from entropy the recorder was HANDED
    /// (never generated here: this crate stays deterministic), frozen forever after.
    pub seed: u64,
    /// The stream the committed store holds.
    pub stream_id: String,
    /// Node terminal states, in the SEED-DERIVED traversal order.
    pub node_checks: Vec<DemoNodeCheck>,
    /// `sha256:<hex>` over the canonical JSON of the replayed projection.
    pub expected_projection_digest: String,
}

#[derive(Debug, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DemoNodeCheck {
    pub node: String,
    /// The node's terminal state as its wire string (e.g. `succeeded`).
    pub state: String,
}

/// The seed-derived traversal: a deterministic xorshift walk over the SORTED name list.
/// The same derivation runs at recording and at replay — a recorded order that does not
/// derive from the frozen seed is refused, so the seed can never be decorative.
#[must_use]
pub fn seed_traversal(seed: u64, names: &[String]) -> Vec<String> {
    let mut pool: Vec<String> = names.to_vec();
    pool.sort();
    let mut state = seed | 1;
    let mut order = Vec::with_capacity(pool.len());
    while !pool.is_empty() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let index = usize::try_from(state % pool.len() as u64).expect("bounded index");
        order.push(pool.remove(index));
    }
    order
}

/// Digest of a replayed projection: sha256 over its canonical JSON, `sha256:`-prefixed —
/// one derivation shared by the recorder and the verifier, so drift is impossible.
#[must_use]
pub fn projection_digest(projection: &graphhelm_events::ExecutionProjection) -> String {
    use sha2::{Digest, Sha256};
    let canonical = serde_json::to_vec(projection).expect("a projection serializes");
    format!("sha256:{}", hex::encode(Sha256::digest(canonical)))
}

/// Opens a committed demonstration store and replays its stream with the CURRENT build.
///
/// # Errors
/// A human-readable problem string when the store cannot be opened or the stream refuses
/// to replay.
pub fn replay_demonstration_store(
    events: &Path,
    stream_id: &str,
) -> Result<graphhelm_events::ExecutionProjection, String> {
    struct WallClock;
    impl graphhelm_protocols::Clock for WallClock {
        fn now(&self) -> chrono::DateTime<chrono::Utc> {
            chrono::Utc::now()
        }
    }
    #[derive(Default)]
    struct CountingIds(std::sync::atomic::AtomicU64);
    impl graphhelm_protocols::IdGenerator for CountingIds {
        fn next_id(&self, prefix: &'static str) -> String {
            let next = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            format!("{prefix}-verify-{next}")
        }
    }
    // A committed store loses its EMPTY child directories on checkout (git tracks no
    // empty dirs), and the repository's anchor validation requires them (#58, found on
    // the first post-merge gate of main — the branch was green only because the
    // recording worktree still carried them untracked). They are content-empty by
    // construction for a fixture journey — any real blob is a tracked FILE and survives
    // checkout — so the replayer restores the shape before opening.
    for child in ["blobs", ".tmp", "active"] {
        std::fs::create_dir_all(events.join(child))
            .map_err(|error| format!("the demonstration store shape: {error}"))?;
    }
    let store = graphhelm_events::LocalEventRepository::open(
        events,
        std::sync::Arc::new(WallClock),
        std::sync::Arc::new(CountingIds::default()),
    )
    .map_err(|error| format!("the demonstration store does not open: {error:?}"))?;
    let streams = store
        .list_streams()
        .map_err(|error| format!("the store lists no streams: {error:?}"))?;
    let stream = streams
        .into_iter()
        .find(|stream| stream.stream_id == stream_id)
        .ok_or_else(|| format!("stream {stream_id:?} is not in the store"))?;
    let history = store
        .read_replay_stream(&stream.scope, &stream.stream_id)
        .map_err(|error| format!("the stream does not read: {error:?}"))?;
    graphhelm_events::replay(&stream.scope, &stream.stream_id, &history)
        .map_err(|error| format!("the current build refuses the recorded history: {error:?}"))
}

/// Replays a demonstration against the CURRENT build: the recorded traversal must derive
/// from the frozen seed, every node check must hold, and the replayed projection must
/// digest to exactly what the recording froze.
pub fn verify_demonstration(root: &Path, directory: &str) -> Vec<String> {
    let base = root.join(directory);
    let mut problems = Vec::new();
    let manifest: DemoManifest = match std::fs::read_to_string(base.join("demo.json"))
        .map_err(|error| format!("{directory}/demo.json is missing: {error}"))
        .and_then(|text| {
            serde_json::from_str(&text)
                .map_err(|error| format!("{directory}/demo.json does not parse: {error}"))
        }) {
        Ok(manifest) => manifest,
        Err(problem) => return vec![problem],
    };

    // The frozen seed must actually derive the recorded traversal — the seed chose the
    // path at recording, and a hand-ordered (or tampered) record is refused.
    let names: Vec<String> = manifest
        .node_checks
        .iter()
        .map(|check| check.node.clone())
        .collect();
    let derived = seed_traversal(manifest.seed, &names);
    if derived != names {
        problems.push(format!(
            "the recorded traversal does not derive from the frozen seed (derived {derived:?})"
        ));
    }

    let projection = match replay_demonstration_store(&base.join("events"), &manifest.stream_id) {
        Ok(projection) => projection,
        Err(problem) => {
            problems.push(problem);
            return problems;
        }
    };
    for check in &manifest.node_checks {
        let actual = projection
            .node_states
            .get(&check.node)
            .and_then(|state| serde_json::to_value(state).ok())
            .and_then(|value| value.as_str().map(str::to_owned));
        if actual.as_deref() != Some(check.state.as_str()) {
            problems.push(format!(
                "node {:?} replays as {actual:?}, the recording froze {:?}",
                check.node, check.state
            ));
        }
    }
    let digest = projection_digest(&projection);
    if digest != manifest.expected_projection_digest {
        problems.push(format!(
            "the replayed projection digests to {digest}, the recording froze {}",
            manifest.expected_projection_digest
        ));
    }
    problems
}

/// Every file a committed artifact directory holds must be TRACKED by git — a named but
/// gitignored artifact silently vanishes from fresh clones (the 05f journal lesson).
pub fn verify_tracked(root: &Path, directory: &str) -> Vec<String> {
    let output = std::process::Command::new("git")
        .args(["-C", &root.to_string_lossy(), "ls-files", "--", directory])
        .output();
    let Ok(output) = output else {
        return vec![format!("git ls-files failed for {directory}")];
    };
    let tracked: std::collections::BTreeSet<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().replace('\\', "/"))
        .filter(|line| !line.is_empty())
        .collect();
    let base = root.join(directory);
    let mut on_disk = Vec::new();
    walk_all(&base, &base, &mut on_disk);
    let mut problems = Vec::new();
    for relative in on_disk {
        let repo_relative = format!("{}/{relative}", directory.trim_end_matches('/'));
        if !tracked.contains(&repo_relative) {
            problems.push(format!(
                "{repo_relative}: on disk but not tracked by git (gitignored?) — it will \
                 vanish from a fresh clone"
            ));
        }
    }
    problems
}

/// Suite names `gate.ps1`'s `$excludedSuites` hashtable names, parsed from the script's own
/// text (#104: the map's grounding check must read the gate's suite set the way the gate
/// NOW produces it — discovery, not a literal list — or the two drift apart silently).
/// Only non-comment lines are matched: the block's own illustrative comment
/// (`# (none today - add 'suite_name' = 'reason' ...)`) contains the exact `'x' = 'y'`
/// shape it is describing, and a naive whole-block regex would misread that prose as a
/// real exclusion.
#[must_use]
pub fn gate_excluded_suites(gate_text: &str) -> Vec<String> {
    let marker = "$excludedSuites = @{";
    let Some(marker_at) = gate_text.find(marker) else {
        return Vec::new();
    };
    let body_start = marker_at + marker.len();
    let mut depth = 1_i32;
    let mut body_end = gate_text.len();
    for (offset, character) in gate_text[body_start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    body_end = body_start + offset;
                    break;
                }
            }
            _ => {}
        }
    }
    let mut excluded = Vec::new();
    for line in gate_text[body_start..body_end].lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('\'')
            && let Some(end) = rest.find('\'')
        {
            excluded.push(rest[..end].to_owned());
        }
    }
    excluded
}

/// The CLI suite set `gate.ps1` actually invokes, computed the same way the gate itself
/// does since #98: every `apps/cli/tests/*.rs` base name, minus any name the gate's own
/// `$excludedSuites` map excludes. `gate.ps1` carries no more literal suite names for a
/// grounding test to string-match against — the surface is now structural, so the check
/// must be too, or the map and the gate can drift apart with nothing to notice (#104).
#[must_use]
pub fn gate_cli_suites(root: &Path, gate_text: &str) -> Vec<String> {
    let excluded = gate_excluded_suites(gate_text);
    let tests_dir = root.join("apps/cli/tests");
    let mut suites: Vec<String> = std::fs::read_dir(&tests_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().is_some_and(|ext| ext == "rs"))
                .then(|| {
                    path.file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned())
                })
                .flatten()
        })
        .filter(|name| !excluded.contains(name))
        .collect();
    suites.sort();
    suites
}

#[derive(Debug, Deserialize)]
pub struct Refused {
    pub affordance: String,
    pub citation: String,
}

/// The repository root, from this crate's own manifest location.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository root resolves")
}

pub fn load_clauses(root: &Path) -> Clauses {
    let text = std::fs::read_to_string(root.join("docs/acceptance/m05-clauses.toml"))
        .expect("m05-clauses.toml is readable");
    toml::from_str(&text).expect("m05-clauses.toml parses")
}

/// Every `.rs` file under the source trees a prover may live in.
pub fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for top in ["core", "adapters", "apps", "tools"] {
        walk(&root.join(top), &mut files);
    }
    files
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if name != "target" && name != ".git" {
                walk(&path, out);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// The body of `fn {name}` inside `source`, extracted by brace counting from the fn's
/// opening brace. `None` when the fn is not defined in this source.
pub fn fn_body<'a>(source: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("fn {name}(");
    let start = source.find(&needle)?;
    let open = start + source[start..].find('{')?;
    let mut depth = 0_i32;
    for (offset, character) in source[open..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&source[open..=open + offset]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Renders the acceptance map document — the exact bytes the committed file must carry.
pub fn generate(clauses: &Clauses) -> String {
    let mut out = String::new();
    out.push_str("# Milestone 05 acceptance map\n\n");
    out.push_str(
        "Generated from `m05-clauses.toml` by `tools/acceptance-map` — do not edit by \
         hand; regenerate with `cargo run -p acceptance-map`. The `acceptance_map_is_grounded` \
         test (gate: workspace tests) verifies every binding below against the real tree, so \
         this document cannot rust silently.\n\n",
    );
    out.push_str("## The §8 clauses and their provers\n\n");
    for clause in &clauses.clause {
        out.push_str(&format!(
            "### {}\n\n> {}\n\n{}\n\n",
            clause.id, clause.text, clause.rationale
        ));
        if clause.gate {
            out.push_str(
                "**Proven by the gate run itself**: `./ci/gate.ps1` must end in its GREEN \
                 verdict with both PostgreSQL locale passes on the surface.\n\n",
            );
        }
        for prover in &clause.prover {
            out.push_str(&format!(
                "- `{}` (suite: {}) — fingerprint: `{}`\n",
                prover.function, prover.suite, prover.assert_fingerprint
            ));
        }
        for artifact in &clause.artifact {
            out.push_str(&format!(
                "- **committed run evidence** `{}` — {} (every file checksummed in its \
                 `SHA256SUMS`; the grounding test re-hashes it AND opens the archived store, \
                 replaying it against the current build. The store is archived as bytes: git \
                 cannot carry its empty `.tmp/` and `active/` directories, so the shape is \
                 restored before opening — see the directory's `README.md`)\n",
                artifact.directory, artifact.description
            ));
        }
        for demonstration in &clause.demonstration {
            out.push_str(&format!(
                "- **recorded demonstration** `{}` — {} (frozen seed, seed-derived \
                 traversal, and a projection digest the current build must reproduce on \
                 replay; every file tracked and checksummed)\n",
                demonstration.directory, demonstration.description
            ));
        }
        if !clause.prover.is_empty()
            || !clause.artifact.is_empty()
            || !clause.demonstration.is_empty()
        {
            out.push('\n');
        }
    }
    out.push_str("## Refused scope (D-040)\n\n");
    out.push_str(
        "Every affordance below is banned from the monitor; the citation is the decision \
         register's own sentence, and the grounding test verifies it still appears there.\n\n",
    );
    for refused in &clauses.refused {
        out.push_str(&format!(
            "- **{}** — \"{}\"\n",
            refused.affordance, refused.citation
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pinned against the REAL file, not a synthetic stand-in: `gate.ps1`'s own
    // `$excludedSuites` comment (`# (none today - add 'suite_name' = 'reason' ...)`) is
    // exactly the shape a naive parser would misread as a real exclusion. If a future edit
    // ever makes `gate_excluded_suites` comment-blind, this is the line that would first
    // start lying — pin it directly rather than trusting a synthetic fixture to still
    // resemble the file it is supposed to stand in for.
    #[test]
    fn gate_excluded_suites_ignores_its_own_illustrative_comment() {
        let root = repo_root();
        let gate = std::fs::read_to_string(root.join("ci/gate.ps1")).expect("gate.ps1 readable");
        assert!(
            gate.contains("# (none today"),
            "this test pins against the exact illustrative comment gate.ps1 carries today — \
             update the pinned text alongside any edit to that comment"
        );
        let excluded = gate_excluded_suites(&gate);
        assert!(
            !excluded.iter().any(|name| name == "suite_name"),
            "the comment's illustrative 'suite_name' = 'reason' must not be read as a real \
             exclusion: {excluded:?}"
        );
    }

    #[test]
    fn gate_excluded_suites_parses_a_real_entry_past_the_comment() {
        let synthetic = "\
$excludedSuites = @{
    # (none today - add 'suite_name' = 'reason' here if one is ever needed)
    'flaky_suite' = 'quarantined pending #999'
}
";
        let excluded = gate_excluded_suites(synthetic);
        assert_eq!(excluded, vec!["flaky_suite".to_owned()]);
    }
}
