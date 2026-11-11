//! Conformance for the delivery skills: every step skill carries the canonical
//! "If in doubt" index byte-for-byte, the index stays under 40 lines, and every
//! skill it names exists (part 1 of #2582). Part 2: every step skill ends with a
//! transition table whose targets exist, the guide's flowchart and those tables name
//! one chain (the rest are declared detours), and the AGENTS.md reading map names only
//! skills that exist.

use std::collections::{HashMap, HashSet};
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

/// A skill name resolves when an owned tree holds `<name>/SKILL.md` on disk, so renaming or
/// deleting a skill fails here instead of staying green against a hard-coded list.
fn skill_exists_on_disk(name: &str) -> bool {
    OWNED_TREES
        .iter()
        .any(|tree| repo_root().join(tree).join(name).join("SKILL.md").is_file())
}

#[test]
fn every_skill_named_in_the_index_exists() {
    let canonical = read(CANONICAL);
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
                    skill_exists_on_disk(candidate),
                    "index names unknown skill '{candidate}': no <tree>/{candidate}/SKILL.md under the owned trees"
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

// ---- Part 2: transition tables, flowchart and reading map ----

const GUIDE: &str = "plugins/graphhelm/skills/graphhelm-guide/SKILL.md";
const TRANSITION_HEADER: [&str; 3] = ["After this", "Next skill", "Who acts"];
/// Outcomes a "Next skill" cell may start with instead of a skill.
const NON_SKILL_OUTCOMES: &[&str] = &["done", "report", "wait", "author"];
/// The skills that carry the task flow the guide draws.
const FLOW_SKILLS: &[&str] = &[
    "extensions/builtin/graphhelm-development-contracts/skills/task-plan/SKILL.md",
    "extensions/builtin/graphhelm-development-contracts/skills/implement/SKILL.md",
    "extensions/builtin/graphhelm-development-contracts/skills/blind-review/SKILL.md",
    "extensions/builtin/graphhelm-development-contracts/skills/merge/SKILL.md",
    "extensions/builtin/graphhelm-jpd/skills/journey-prove/SKILL.md",
];

fn skill_name_of(rel: &str) -> String {
    let parent = Path::new(rel).parent().and_then(Path::file_name);
    parent
        .expect("skill path has a folder")
        .to_string_lossy()
        .into_owned()
}

fn cells_of(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|c| c.trim().to_string())
        .collect()
}

/// Data rows of the table whose header line is `| a | b | c |`, or None when `text` has no such
/// header. The table ends at the first line that is not a table row.
fn table_with_header(text: &str, header: &[&str]) -> Option<Vec<Vec<String>>> {
    let mut lines = text.lines().skip_while(|l| cells_of(l) != header);
    lines.next()?;
    let rows = lines
        .take_while(|l| l.trim_start().starts_with('|'))
        .map(cells_of)
        .filter(|cells| !cells.iter().all(|c| c.starts_with('-')))
        .collect();
    Some(rows)
}

/// Text from `heading` to the next `## ` heading (or the end).
fn section<'a>(text: &'a str, heading: &str) -> Option<&'a str> {
    let start = text.find(heading)?;
    let rest = &text[start + heading.len()..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    Some(&rest[..end])
}

fn transition_rows(rel: &str) -> Vec<Vec<String>> {
    let text = read(rel);
    let body = section(&text, "## What comes next")
        .unwrap_or_else(|| panic!("{rel} has no '## What comes next' section"));
    table_with_header(body, &TRANSITION_HEADER).unwrap_or_else(|| {
        panic!("{rel}: 'What comes next' has no '| After this | Next skill | Who acts |' table")
    })
}

/// First word of a "Next skill" cell, without trailing punctuation.
fn first_word(cell: &str) -> String {
    cell.split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .to_string()
}

/// Every `[a-z]+(-[a-z]+)+` token in `cell`.
fn hyphenated_tokens(cell: &str) -> Vec<String> {
    cell.split(|c: char| !(c.is_ascii_lowercase() || c == '-'))
        .map(|t| t.trim_matches('-'))
        .filter(|t| t.contains('-') && t.split('-').all(|p| !p.is_empty()))
        .map(str::to_string)
        .collect()
}

#[test]
fn every_step_skill_ends_with_a_transition_table() {
    for rel in STEP_SKILLS {
        let rows = transition_rows(rel);
        assert!(!rows.is_empty(), "{rel}: transition table has no data row");
        for row in &rows {
            assert!(
                row.len() == 3 && row.iter().all(|c| !c.is_empty()),
                "{rel}: transition row is not 3 filled cells: {row:?}"
            );
        }
    }
}

#[test]
fn every_transition_names_a_skill_that_exists() {
    for rel in STEP_SKILLS {
        for row in transition_rows(rel) {
            let cell = &row[1];
            let first = first_word(cell);
            assert!(
                NON_SKILL_OUTCOMES.contains(&first.as_str()) || skill_exists_on_disk(&first),
                "{rel}: 'Next skill' cell '{cell}' starts with '{first}', neither a skill nor one of {NON_SKILL_OUTCOMES:?}"
            );
            for token in hyphenated_tokens(cell) {
                assert!(
                    skill_exists_on_disk(&token),
                    "{rel}: 'Next skill' cell '{cell}' names unknown skill '{token}'"
                );
            }
        }
    }
}

/// `A --> B` edges of the guide's mermaid diagram, without the start and end markers.
fn mermaid_edges(guide: &str) -> Vec<(String, String)> {
    let block =
        section(guide, "```mermaid\nstateDiagram-v2").expect("guide has a stateDiagram-v2 block");
    let block = &block[..block.find("```").expect("mermaid block is closed")];
    block
        .lines()
        .filter_map(|l| l.split_once("-->"))
        .map(|(a, b)| {
            let target = b.split(':').next().unwrap_or("");
            (a.trim().to_string(), target.trim().to_string())
        })
        .filter(|(a, b)| a != "[*]" && b != "[*]")
        .collect()
}

fn unticked(cell: &str) -> String {
    cell.trim_matches('`').to_string()
}

#[test]
fn the_flowchart_and_the_transition_tables_agree() {
    let guide = read(GUIDE);
    let edges = mermaid_edges(&guide);
    assert!(
        !edges.is_empty(),
        "no edges parsed from the guide's diagram"
    );

    let state_rows = table_with_header(&guide, &["State", "Acts", "Skill"])
        .expect("guide has a '| State | Acts | Skill |' table");
    // state -> skill, for the states that have one (the cell is a backticked name).
    let state_skill: HashMap<String, String> = state_rows
        .iter()
        .filter(|r| r[2].starts_with('`'))
        .map(|r| (unticked(&r[0]), unticked(&r[2])))
        .collect();

    let tables: HashMap<String, Vec<Vec<String>>> = FLOW_SKILLS
        .iter()
        .map(|rel| (skill_name_of(rel), transition_rows(rel)))
        .collect();

    // Forward: every arrow out of a state with a skill has a row in that skill's table.
    let mut skipped = Vec::new();
    for (from, to) in &edges {
        let Some(skill) = state_skill.get(from) else {
            continue;
        };
        let rows = tables.get(skill).unwrap_or_else(|| {
            panic!("state '{from}' maps to '{skill}', which is not a flow skill")
        });
        let carried = if to == "cleanup" || to == "done" {
            rows.iter().any(|r| first_word(&r[1]) == "done")
        } else if let Some(target_skill) = state_skill.get(to) {
            rows.iter().any(|r| first_word(&r[1]) == *target_skill)
        } else {
            skipped.push(format!("{from}->{to}"));
            continue;
        };
        assert!(
            carried,
            "diagram edge {from} --> {to}: the '{skill}' transition table has no row for it"
        );
    }
    assert_eq!(
        skipped,
        vec!["review->diagnose".to_string()],
        "edges into a state with no skill must be exactly review->diagnose"
    );

    // Backward: every skill-to-skill row is an arrow of the diagram or a declared detour.
    let detour_rows = table_with_header(&guide, &["From skill", "To", "When"])
        .expect("guide has a '| From skill | To | When |' table (Declared detours)");
    let detours: HashSet<(String, String)> = detour_rows
        .iter()
        .map(|r| (unticked(&r[0]), unticked(&r[1])))
        .collect();
    let drawn: HashSet<(String, String)> = edges
        .iter()
        .filter_map(|(a, b)| Some((state_skill.get(a)?.clone(), state_skill.get(b)?.clone())))
        .collect();
    let mut rows_seen = HashSet::new();
    for (skill, rows) in &tables {
        for row in rows {
            let to = first_word(&row[1]);
            if !skill_exists_on_disk(&to) {
                continue;
            }
            let pair = (skill.clone(), to.clone());
            assert!(
                drawn.contains(&pair) || detours.contains(&pair),
                "'{skill}' transition row '{}' -> '{to}' is neither a diagram edge nor a row of 'Declared detours'",
                row[0]
            );
            rows_seen.insert(pair);
        }
    }
    for pair in &detours {
        assert!(
            rows_seen.contains(pair),
            "'Declared detours' lists {} -> {}, which no transition table carries",
            pair.0,
            pair.1
        );
    }
}

#[test]
fn the_reading_map_names_only_skills_that_exist() {
    let agents = read("AGENTS.md");
    let body = section(&agents, "## Reading map by step").expect("AGENTS.md has the reading map");
    let rows = table_with_header(body, &["Step", "Read", "Skill"])
        .expect("reading map header is '| Step | Read | Skill |'");
    assert!(!rows.is_empty(), "reading map has no rows");
    let mut by_step: HashMap<&str, Vec<String>> = HashMap::new();
    for row in &rows {
        let names: Vec<String> = row[2]
            .split('`')
            .skip(1)
            .step_by(2)
            .map(str::to_string)
            .collect();
        for name in &names {
            assert!(
                skill_exists_on_disk(name),
                "reading map row '{}' names unknown skill '{name}'",
                row[0]
            );
        }
        by_step.insert(row[0].as_str(), names);
    }
    for (step, skill) in [
        ("Review", "blind-review"),
        ("Merge and cleanup", "merge"),
        ("Write and prove", "implement"),
    ] {
        assert!(
            by_step
                .get(step)
                .is_some_and(|n| n.iter().any(|s| s == skill)),
            "reading map row '{step}' must name `{skill}`"
        );
    }
}
