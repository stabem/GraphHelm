//! `graphhelm skills sync --host codex` (#355): the bundled skills reach a Codex home and stay
//! current there. A second run changes nothing, a skill directory the command did not install is
//! never touched, a hand edit to an installed skill is left alone, a skill dropped from the
//! bundle is removed only when it is still the bytes we wrote, and `--dry-run` writes nothing.
//!
//! Cost: one debug `graphhelm` binary, a temp home per test, no network; under a second each.

use std::path::{Path, PathBuf};

use serde_json::Value;

fn command() -> assert_cmd::Command {
    let mut command = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command.env_remove("CODEX_HOME");
    command
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn sync(home: &Path, extra: &[&str]) -> Value {
    let output = command()
        .args(["--json", "skills", "sync", "--host", "codex", "--home"])
        .arg(home)
        .args(extra)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "sync failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn names(report: &Value, key: &str) -> Vec<String> {
    let mut names: Vec<String> = report["data"][key]
        .as_array()
        .unwrap_or_else(|| panic!("no {key} in {report}"))
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    names.sort();
    names
}

/// Every directory holding a SKILL.md under the three bundled skill roots.
fn bundled() -> Vec<String> {
    let root = repo();
    let mut dirs = vec![
        root.join("plugins/graphhelm/skills"),
        root.join("examples/chat-surface/claude-code-plugin/skills"),
    ];
    for package in std::fs::read_dir(root.join("extensions/builtin")).unwrap() {
        dirs.push(package.unwrap().path().join("skills"));
    }
    let mut names = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.join("SKILL.md").is_file() {
                names.push(path.file_name().unwrap().to_string_lossy().into_owned());
            }
        }
    }
    names.sort();
    names
}

fn source_of(name: &str) -> PathBuf {
    let root = repo();
    let mut candidates = vec![
        root.join("plugins/graphhelm/skills").join(name),
        root.join("examples/chat-surface/claude-code-plugin/skills")
            .join(name),
    ];
    for package in std::fs::read_dir(root.join("extensions/builtin")).unwrap() {
        candidates.push(package.unwrap().path().join("skills").join(name));
    }
    candidates.into_iter().find(|p| p.is_dir()).unwrap()
}

#[test]
fn a_fresh_home_receives_every_bundled_skill_and_a_second_run_changes_nothing() {
    let home = tempfile::tempdir().unwrap();
    let expected = bundled();
    assert!(expected.contains(&"keel".to_owned()), "{expected:?}");

    let dry = sync(home.path(), &["--dry-run"]);
    assert_eq!(names(&dry, "added"), expected);
    assert!(
        !home.path().join("skills").exists(),
        "--dry-run must write nothing"
    );

    let first = sync(home.path(), &[]);
    assert_eq!(names(&first, "added"), expected);
    let keel = std::fs::read(home.path().join("skills/keel/SKILL.md")).unwrap();
    assert_eq!(
        keel,
        std::fs::read(source_of("keel").join("SKILL.md")).unwrap()
    );
    assert!(home.path().join("skills/.graphhelm-skills.json").is_file());

    let second = sync(home.path(), &[]);
    assert_eq!(names(&second, "unchanged"), expected);
    assert!(names(&second, "added").is_empty());
    assert!(names(&second, "updated").is_empty());
}

#[test]
fn a_skill_it_did_not_install_is_never_touched_unless_it_is_already_identical() {
    let home = tempfile::tempdir().unwrap();
    let skills = home.path().join("skills");
    // A hand-copied, stale `keel` and a hand-copied, current `test-audit`.
    std::fs::create_dir_all(skills.join("keel")).unwrap();
    std::fs::write(skills.join("keel/SKILL.md"), "old hand copy").unwrap();
    let audit = skills.join("test-audit");
    std::fs::create_dir_all(&audit).unwrap();
    for entry in walk(&source_of("test-audit")) {
        let relative = entry.strip_prefix(source_of("test-audit")).unwrap();
        std::fs::create_dir_all(audit.join(relative).parent().unwrap()).unwrap();
        std::fs::copy(&entry, audit.join(relative)).unwrap();
    }

    let report = sync(home.path(), &[]);
    assert_eq!(names(&report, "foreign"), vec!["keel"]);
    assert_eq!(names(&report, "adopted"), vec!["test-audit"]);
    assert_eq!(
        std::fs::read_to_string(skills.join("keel/SKILL.md")).unwrap(),
        "old hand copy"
    );
    // Adopted means owned: the next run sees it as ours.
    let again = sync(home.path(), &[]);
    assert!(names(&again, "unchanged").contains(&"test-audit".to_owned()));
    assert_eq!(names(&again, "foreign"), vec!["keel"]);
}

#[test]
fn a_hand_edit_to_an_installed_skill_is_kept_and_reported() {
    let home = tempfile::tempdir().unwrap();
    sync(home.path(), &[]);
    let edited = home.path().join("skills/keel/SKILL.md");
    std::fs::write(&edited, "my local notes").unwrap();

    let report = sync(home.path(), &[]);
    assert_eq!(names(&report, "modified"), vec!["keel"]);
    assert_eq!(std::fs::read_to_string(&edited).unwrap(), "my local notes");
}

#[test]
fn a_skill_dropped_from_the_bundle_is_removed_only_when_still_ours() {
    let home = tempfile::tempdir().unwrap();
    sync(home.path(), &[]);
    let skills = home.path().join("skills");
    let manifest_path = skills.join(".graphhelm-skills.json");
    let mut manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    // Two skills a previous bundle installed and this one no longer ships: `ghost` is still the
    // bytes we wrote, `edited-ghost` was changed by hand afterwards.
    let keel_entry = manifest["skills"]["keel"].clone();
    for ghost in ["ghost", "edited-ghost"] {
        std::fs::create_dir_all(skills.join(ghost)).unwrap();
        for entry in walk(&skills.join("keel")) {
            let relative = entry.strip_prefix(skills.join("keel")).unwrap();
            std::fs::create_dir_all(skills.join(ghost).join(relative).parent().unwrap()).unwrap();
            std::fs::copy(&entry, skills.join(ghost).join(relative)).unwrap();
        }
        manifest["skills"][ghost] = keel_entry.clone();
    }
    std::fs::write(skills.join("edited-ghost/SKILL.md"), "edited").unwrap();
    std::fs::write(&manifest_path, manifest.to_string()).unwrap();

    let dry = sync(home.path(), &["--dry-run"]);
    assert_eq!(names(&dry, "removed"), vec!["ghost"]);
    assert!(
        skills.join("ghost").exists(),
        "--dry-run must write nothing"
    );

    let report = sync(home.path(), &[]);
    assert_eq!(names(&report, "removed"), vec!["ghost"]);
    assert_eq!(names(&report, "modified"), vec!["edited-ghost"]);
    assert!(!skills.join("ghost").exists());
    assert!(skills.join("edited-ghost/SKILL.md").is_file());
}

#[test]
fn the_home_defaults_to_codex_home() {
    let home = tempfile::tempdir().unwrap();
    let output = command()
        .env("CODEX_HOME", home.path())
        .args(["--json", "skills", "sync", "--host", "codex"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(home.path().join("skills/keel/SKILL.md").is_file());
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
}
