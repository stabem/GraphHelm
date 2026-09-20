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
    // A GIT THAT RAN AND REFUSED IS NOT A GIT THAT ANSWERED "nothing is tracked". `output()`
    // returns `Ok` for a process that exited non-zero, so the guard above catches only a git that
    // never launched. Measured: outside a work tree `git -C <dir> ls-files` exits 128 and prints
    // nothing, `tracked` collects to the EMPTY SET, and the loop below then turns every file on
    // disk into "on disk but not tracked by git (gitignored?)" — the gate goes red accusing the
    // repository of gitignoring its own committed evidence, when the real cause is that git could
    // not answer the question. `git` refusing is not hypothetical here: a work tree whose owner
    // differs from the account the gate runs as gets "detected dubious ownership", exit 128.
    //
    // `core/quality/tests/freeze.rs` already asserts `status.success()` on this same call, because
    // "an unanswerable population reported as empty would fail the assertions below with an
    // accusation about the wrong thing". This reader now says the same thing the same way.
    if !output.status.success() {
        return vec![format!(
            "git ls-files failed for {directory} (exit {}), so the tracked set is UNDEFINED rather \
             than empty: {}",
            output
                .status
                .code()
                .map_or_else(|| "signal".to_string(), |code| code.to_string()),
            String::from_utf8_lossy(&output.stderr).trim()
        )];
    }
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

/// The CLI suite set `gate.ps1` actually invokes: every `apps/cli/tests/*.rs` base name.
///
/// #98's property -- a new suite is gated the day it is born, never by a hand-maintained list --
/// is satisfied more strongly since #1053 than it was by the discovery loop this used to mirror.
/// The gate runs `cargo nextest run -p graphhelm-cli` once, so the enumeration is CARGO'S: a new
/// integration target is compiled and run because it exists, and there is no list anywhere that
/// could be edited to under-gate it.
///
/// THE EXCLUSION SURFACE IS GONE WITH THE LOOP. `gate.ps1` used to carry an `$excludedSuites`
/// hashtable that this module parsed; one stage over the whole package has nowhere to put such a
/// map, and nothing consults one. `gate_excluded_suites` and its two cells were retired with it
/// rather than left parsing a block that no longer exists -- a parser whose subject is absent
/// returns "nothing excluded" forever, which reads exactly like a working check. If an exclusion
/// is ever needed again it belongs in a `.config/nextest.toml` filter, and this function must be
/// taught to read THAT rather than resurrected against the old shape.
#[must_use]
pub fn gate_cli_suites(root: &Path) -> Vec<String> {
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
    use std::io::Write;

    /// A directory that is NOT inside a git work tree, holding one file under `directory`.
    fn scratch_outside_git(directory: &str, file: &str) -> std::path::PathBuf {
        let unique = format!(
            "acceptance-map-nongit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let root = std::env::temp_dir().join(unique);
        let dir = root.join(directory);
        std::fs::create_dir_all(&dir).expect("scratch directory");
        let mut handle = std::fs::File::create(dir.join(file)).expect("scratch file");
        handle.write_all(b"evidence\n").expect("scratch file body");
        root
    }

    /// A git that REFUSES must not read back as "nothing is tracked here".
    ///
    /// `Command::output()` returns `Ok` for a process that ran and FAILED, so checking only for
    /// `Ok(..)` leaves an exit-128 git indistinguishable from a git that answered with an empty
    /// list — and an empty tracked set turns every file on disk into an accusation about the
    /// repository. `core/quality/tests/freeze.rs` already asserts `status.success()` on this same
    /// call, under the comment "an unanswerable population reported as empty would fail the
    /// assertions below with an accusation about the wrong thing". This is the site that did not.
    #[test]
    fn a_git_that_refuses_is_a_failed_read_not_a_list_of_untracked_files() {
        let root = scratch_outside_git("evidence", "kept.txt");

        // ARRANGEMENT FIRST: git must actually refuse here. If some ancestor of the temp directory
        // were a work tree, git would answer and this test would be about a repository instead of
        // about a refusal — passing for a reason that has nothing to do with the property.
        let probe = std::process::Command::new("git")
            .args(["-C", &root.to_string_lossy(), "ls-files", "--", "evidence"])
            .output()
            .expect("git runs");
        assert!(
            !probe.status.success(),
            "ARRANGEMENT: git answered successfully in {}, so this test is not about a refusal",
            root.display()
        );

        let problems = verify_tracked(&root, "evidence");
        let cleanup = std::fs::remove_dir_all(&root);

        assert!(
            problems.iter().any(|p| p.contains("git ls-files failed")),
            "a git that exited {:?} must be reported as a failed read; got {problems:?}",
            probe.status.code()
        );
        assert!(
            !problems.iter().any(|p| p.contains("not tracked by git")),
            "a file on disk must not be accused of being untracked when git never answered; \
             got {problems:?}"
        );
        cleanup.expect("scratch directory removed");
    }

    /// CONTROL: inside a real work tree the reader still answers, so the refusal above is a
    /// discrimination and not a blanket failure that would redden every caller.
    #[test]
    fn inside_a_work_tree_the_reader_still_answers() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("tools/acceptance-map sits two levels below the repository root");

        let problems = verify_tracked(root, "tools/acceptance-map/src");

        assert!(
            !problems.iter().any(|p| p.contains("git ls-files failed")),
            "git answers inside this repository; got {problems:?}"
        );
        assert!(
            !problems.iter().any(|p| p.contains("lib.rs")),
            "lib.rs is tracked, so the reader must not accuse it; got {problems:?}"
        );
    }
}
