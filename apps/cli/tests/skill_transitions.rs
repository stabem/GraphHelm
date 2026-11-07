//! Conformance for the delivery skills: every step skill carries the canonical
//! "If in doubt" index byte-for-byte, the index stays under 40 lines, and every
//! skill it names exists. Part 1 of #2582; transition tables come in part 2.

use std::fs;
use std::path::{Path, PathBuf};

const CANONICAL: &str = "plugins/graphhelm/skills/graphhelm-work/references/if-in-doubt.md";
const MAX_INDEX_LINES: usize = 40;

/// Step skills: each one must carry the index. Keep in step with plan.skill_list_for_implementation.
const STEP_SKILLS: &[&str] = &[
    "plugins/keel/skills/keel-exec/SKILL.md",
    "plugins/keel/skills/keel-plan/SKILL.md",
    "plugins/keel/skills/keel-review/SKILL.md",
    "plugins/keel/skills/keel-merge/SKILL.md",
    "plugins/keel/skills/keel-journey-qa/SKILL.md",
    "extensions/builtin/graphhelm-development-contracts/skills/task-plan/SKILL.md",
    "extensions/builtin/graphhelm-development-contracts/skills/implement/SKILL.md",
    "extensions/builtin/graphhelm-development-contracts/skills/blind-review/SKILL.md",
    "extensions/builtin/graphhelm-development-contracts/skills/merge/SKILL.md",
    "extensions/builtin/graphhelm-jpd/skills/journey-prove/SKILL.md",
];

fn repo_root() -> PathBuf {
    // apps/cli is two levels below the repo root.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn index_block(text: &str) -> Option<&str> {
    let start = text.find("## If in doubt")?;
    Some(&text[start..])
}

#[test]
fn canonical_index_is_short_enough() {
    let canonical = read(CANONICAL);
    let lines = canonical.lines().count();
    assert!(
        lines <= MAX_INDEX_LINES,
        "canonical index has {lines} lines, limit is {MAX_INDEX_LINES}"
    );
}

#[test]
fn every_step_skill_carries_the_canonical_index() {
    let canonical = read(CANONICAL);
    for rel in STEP_SKILLS {
        let text = read(rel);
        let block =
            index_block(&text).unwrap_or_else(|| panic!("{rel} has no '## If in doubt' section"));
        assert!(
            block.trim_end() == canonical.trim_end(),
            "{rel}: 'If in doubt' section differs from the canonical index in {CANONICAL}"
        );
    }
}

#[test]
fn every_skill_named_in_the_index_exists() {
    let canonical = read(CANONICAL);
    let known: &[&str] = &[
        "task-plan",
        "implement",
        "blind-review",
        "merge",
        "journey-prove",
        "graphhelm-work",
        "graphhelm-guide",
    ];
    for line in canonical.lines().filter(|l| l.starts_with('|')) {
        for cell in line.split('|').map(str::trim) {
            let name = cell.split_whitespace().next().unwrap_or("");
            if name.is_empty() || name == "Situation" || name == "Skill" || name.starts_with('-') {
                continue;
            }
            let candidate = name.trim_matches(|c| c == '(' || c == ')' || c == ',');
            if candidate
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '-')
                && candidate.contains(|c: char| c.is_ascii_alphabetic())
            {
                assert!(
                    known.contains(&candidate),
                    "index names unknown skill '{candidate}'"
                );
            }
        }
    }
}

/// Every SKILL.md under the owned trees (plugins/graphhelm, plugins/keel, and the two builtin
/// extensions) carries the canonical index, not only the step skills (owner order, #2592).
const OWNED_TREES: &[&str] = &[
    "plugins/graphhelm/skills",
    "plugins/keel/skills",
    "extensions/builtin/graphhelm-development-contracts/skills",
    "extensions/builtin/graphhelm-jpd/skills",
];

fn collect_skill_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("readable directory entry").path();
        if path.is_dir() {
            collect_skill_files(&path, out);
        } else if path.file_name().is_some_and(|n| n == "SKILL.md") {
            out.push(path);
        }
    }
}

#[test]
fn every_owned_skill_carries_the_canonical_index() {
    let canonical = read(CANONICAL);
    let mut found = Vec::new();
    for tree in OWNED_TREES {
        collect_skill_files(&repo_root().join(tree), &mut found);
    }
    assert!(!found.is_empty(), "no SKILL.md found under the owned trees");
    for path in &found {
        let text = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let block = index_block(&text)
            .unwrap_or_else(|| panic!("{} has no '## If in doubt' section", path.display()));
        assert!(
            block.trim_end() == canonical.trim_end(),
            "{}: 'If in doubt' section differs from the canonical index in {CANONICAL}",
            path.display()
        );
    }
}
