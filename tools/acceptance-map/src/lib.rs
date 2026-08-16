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
                 `SHA256SUMS`; the grounding test re-hashes it)\n",
                artifact.directory, artifact.description
            ));
        }
        if !clause.prover.is_empty() || !clause.artifact.is_empty() {
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
