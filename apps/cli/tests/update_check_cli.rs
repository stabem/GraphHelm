//! #1244: `graphhelm update --check` builds nothing and fails when this binary was not built from
//! origin/main. A test binary carries no install stamp, so the check must refuse it ("cannot
//! tell"), never pass it. Cost: one local bare repository, one clone, one CLI call; no network.

use std::path::Path;

fn git(dir: &Path, args: &[&str]) {
    let status = graphhelm_process::command("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

#[test]
fn update_check_refuses_a_binary_it_cannot_tie_to_main() {
    let scratch = tempfile::tempdir().unwrap();
    let work = scratch.path().join("work");
    std::fs::create_dir_all(work.join("apps/studio/tools")).unwrap();
    std::fs::write(work.join("apps/studio/tools/studio-up.ps1"), "").unwrap();
    git(&work, &["init", "-q", "-b", "main"]);
    git(&work, &["add", "."]);
    git(
        &work,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "-m",
            "one",
        ],
    );
    let origin = scratch.path().join("origin.git");
    git(
        scratch.path(),
        &[
            "clone",
            "-q",
            "--bare",
            work.to_str().unwrap(),
            origin.to_str().unwrap(),
        ],
    );
    let clone = scratch.path().join("clone");
    git(
        scratch.path(),
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    let output = graphhelm_process::command(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["update", "--check", "--source"])
        .arg(&clone)
        .env_remove("GRAPHHELM_SOURCE")
        .output()
        .unwrap();
    assert!(!output.status.success(), "an unstamped binary never passes");
    let reply: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reply["command"], "update.check", "{reply}");
    let message = reply["diagnostics"][0]["message"].as_str().unwrap();
    assert!(message.contains("cannot tell"), "{message}");
}
