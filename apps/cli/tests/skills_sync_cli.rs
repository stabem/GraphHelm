//! `graphhelm skills sync --host codex` (#355): the bundled skills reach a Codex home and stay
//! current there. A second run changes nothing, a directory the command did not install is never
//! touched, a hand edit to an installed entry is left alone, an entry dropped from the bundle is
//! removed only when it is still the bytes we wrote, and `--dry-run` writes nothing. Pinned
//! packages land whole, so a skill's `../../schemas/...` citation resolves; a manifest name that
//! escapes `skills/` and a leftover staging directory are refused, never acted on.
//!
//! Cost: one debug `graphhelm` binary, a temp home per test, no network; about a second each.

use std::path::{Path, PathBuf};

use serde_json::Value;

const REFUSED: &str = "GHCLI035_SKILLS_SYNC_REFUSED";

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

fn run(home: &Path, extra: &[&str]) -> (bool, Value) {
    let output = command()
        .args(["--json", "skills", "sync", "--host", "codex", "--home"])
        .arg(home)
        .args(extra)
        .output()
        .unwrap();
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!("{e}: {}", String::from_utf8_lossy(&output.stdout));
    });
    (output.status.success(), value)
}

fn sync(home: &Path, extra: &[&str]) -> Value {
    let (ok, value) = run(home, extra);
    assert!(ok, "sync failed: {value}");
    value
}

fn refused(home: &Path) -> String {
    let (ok, value) = run(home, &[]);
    assert!(!ok, "sync must refuse: {value}");
    assert!(value.to_string().contains(REFUSED), "{value}");
    value.to_string()
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

/// The pinned package ids plus every plugin skill directory holding a SKILL.md.
fn bundled() -> Vec<String> {
    let root = repo();
    let release: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("extensions/releases/adoption-0.1.1.json")).unwrap(),
    )
    .unwrap();
    let mut names: Vec<String> = release["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_owned())
        .collect();
    for dir in [
        "plugins/graphhelm/skills",
        "examples/chat-surface/claude-code-plugin/skills",
    ] {
        for entry in std::fs::read_dir(root.join(dir)).unwrap() {
            let path = entry.unwrap().path();
            if path.join("SKILL.md").is_file() {
                names.push(path.file_name().unwrap().to_string_lossy().into_owned());
            }
        }
    }
    names.sort();
    names
}

fn copy_tree(from: &Path, to: &Path) {
    for entry in walk(from) {
        let target = to.join(entry.strip_prefix(from).unwrap());
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::copy(&entry, target).unwrap();
    }
}

#[test]
fn a_fresh_home_receives_every_bundled_entry_and_a_second_run_changes_nothing() {
    let home = tempfile::tempdir().unwrap();
    let expected = bundled();
    assert!(
        expected.contains(&"graphhelm-jpd".to_owned()),
        "{expected:?}"
    );
    assert!(
        expected.contains(&"graphhelm-guide".to_owned()),
        "{expected:?}"
    );

    let dry = sync(home.path(), &["--dry-run"]);
    assert_eq!(names(&dry, "added"), expected);
    assert!(
        !home.path().join("skills").exists(),
        "--dry-run must write nothing"
    );

    let first = sync(home.path(), &[]);
    assert_eq!(names(&first, "added"), expected);
    let keel = "graphhelm-development-contracts/skills/keel/SKILL.md";
    assert_eq!(
        std::fs::read(home.path().join("skills").join(keel)).unwrap(),
        std::fs::read(repo().join("extensions/builtin").join(keel)).unwrap()
    );
    assert!(home.path().join("skills/.graphhelm-skills.json").is_file());

    let second = sync(home.path(), &[]);
    assert_eq!(names(&second, "unchanged"), expected);
    assert!(names(&second, "added").is_empty());
    assert!(names(&second, "updated").is_empty());
}

#[test]
fn a_packaged_skill_finds_the_package_files_it_cites() {
    let home = tempfile::tempdir().unwrap();
    sync(home.path(), &[]);
    let skill = home
        .path()
        .join("skills/graphhelm-jpd/skills/journey-contract");
    let text = std::fs::read_to_string(skill.join("SKILL.md")).unwrap();
    let cited = "../../schemas/journey-contract.schema.json";
    assert!(text.contains(cited), "the skill no longer cites {cited}");
    assert!(
        skill.join(cited).is_file(),
        "{cited} dangles in the install"
    );
}

#[test]
fn a_directory_it_did_not_install_is_never_touched_unless_it_is_already_identical() {
    let home = tempfile::tempdir().unwrap();
    let skills = home.path().join("skills");
    // A hand-copied, stale `graphhelm-guide` and a hand-copied, current `graphhelm-resume`.
    std::fs::create_dir_all(skills.join("graphhelm-guide")).unwrap();
    std::fs::write(skills.join("graphhelm-guide/SKILL.md"), "old hand copy").unwrap();
    copy_tree(
        &repo().join("plugins/graphhelm/skills/graphhelm-resume"),
        &skills.join("graphhelm-resume"),
    );

    let report = sync(home.path(), &[]);
    assert_eq!(names(&report, "foreign"), vec!["graphhelm-guide"]);
    assert_eq!(names(&report, "adopted"), vec!["graphhelm-resume"]);
    assert_eq!(
        std::fs::read_to_string(skills.join("graphhelm-guide/SKILL.md")).unwrap(),
        "old hand copy"
    );
    // Adopted means owned: the next run sees it as ours.
    let again = sync(home.path(), &[]);
    assert!(names(&again, "unchanged").contains(&"graphhelm-resume".to_owned()));
    assert_eq!(names(&again, "foreign"), vec!["graphhelm-guide"]);
}

#[test]
fn a_hand_edit_to_an_installed_entry_is_kept_and_reported() {
    let home = tempfile::tempdir().unwrap();
    sync(home.path(), &[]);
    let edited = home
        .path()
        .join("skills/graphhelm-development-contracts/skills/keel/SKILL.md");
    std::fs::write(&edited, "my local notes").unwrap();

    let report = sync(home.path(), &[]);
    assert_eq!(
        names(&report, "modified"),
        vec!["graphhelm-development-contracts"]
    );
    assert_eq!(std::fs::read_to_string(&edited).unwrap(), "my local notes");
}

#[test]
fn an_entry_dropped_from_the_bundle_is_removed_only_when_still_ours() {
    let home = tempfile::tempdir().unwrap();
    sync(home.path(), &[]);
    let skills = home.path().join("skills");
    let manifest_path = skills.join(".graphhelm-skills.json");
    let mut manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    // Two entries a previous bundle installed and this one no longer ships: `ghost` is still the
    // bytes we wrote, `edited-ghost` was changed by hand afterwards.
    let guide = manifest["skills"]["graphhelm-guide"].clone();
    for ghost in ["ghost", "edited-ghost"] {
        copy_tree(&skills.join("graphhelm-guide"), &skills.join(ghost));
        manifest["skills"][ghost] = guide.clone();
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
fn a_manifest_name_that_leaves_the_skills_directory_is_refused() {
    let home = tempfile::tempdir().unwrap();
    sync(home.path(), &[]);
    let skills = home.path().join("skills");
    // A victim beside `skills/` holding the exact bytes of an entry we installed: digest-equal,
    // so only the name check stands between it and `remove_dir_all`.
    let victim = home.path().join("victim");
    copy_tree(&skills.join("graphhelm-guide"), &victim);
    let manifest_path = skills.join(".graphhelm-skills.json");
    let mut manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["skills"]["../victim"] = manifest["skills"]["graphhelm-guide"].clone();
    std::fs::write(&manifest_path, manifest.to_string()).unwrap();

    refused(home.path());
    assert!(victim.join("SKILL.md").is_file(), "the victim was removed");
}

#[test]
fn a_leftover_staging_directory_is_refused_not_deleted() {
    let home = tempfile::tempdir().unwrap();
    let staging = home
        .path()
        .join("skills/.graphhelm-guide.graphhelm-staging");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("keep.txt"), "not yours").unwrap();

    refused(home.path());
    assert_eq!(
        std::fs::read_to_string(staging.join("keep.txt")).unwrap(),
        "not yours"
    );
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
    assert!(
        home.path()
            .join("skills/graphhelm-guide/SKILL.md")
            .is_file()
    );
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
