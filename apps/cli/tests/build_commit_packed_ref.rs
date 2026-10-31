//! #2277: the commit `build.rs` records follows HEAD when the branch ref is packed. A real Cargo
//! build of a tiny crate that uses `apps/cli/build.rs`, in a scratch repository: pack the refs,
//! build, commit, build again incrementally and read the commit the binary reports.
//! Cost: two small Cargo builds in a temp dir; no network.

use std::path::Path;

fn run(dir: &Path, program: &str, args: &[&str]) -> String {
    // #2281: a git hook (e.g. pre-push running this suite) exports GIT_DIR and GIT_INDEX_FILE,
    // and the user's global config may set core.hooksPath. Either would aim git (here and in
    // `build.rs`) at another repository or run the user's hooks, so the scratch repo sees none.
    let empty_config = dir.with_extension("gitconfig");
    std::fs::write(&empty_config, "").unwrap();
    let out = graphhelm_process::command(program)
        .args(args)
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_WORK_TREE")
        .env("GIT_CONFIG_GLOBAL", &empty_config)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "{program} {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

#[test]
fn an_incremental_build_reports_the_new_commit_when_the_branch_ref_was_packed() {
    let temp = std::env::temp_dir().join(format!("gh-2277-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(temp.join("src")).unwrap();
    std::fs::write(
        temp.join("Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(temp.join("build.rs"), include_str!("../build.rs")).unwrap();
    std::fs::write(
        temp.join("src/main.rs"),
        "fn main() { print!(\"{}\", env!(\"GRAPHHELM_BUILD_COMMIT\")); }\n",
    )
    .unwrap();
    std::fs::write(temp.join(".gitignore"), "target/\n").unwrap();
    let git = |args: &[&str]| run(&temp, "git", args);
    git(&["init", "-q", "-b", "main"]);
    git(&[
        "-c",
        "user.email=t@example.invalid",
        "-c",
        "user.name=T",
        "add",
        "-A",
    ]);
    git(&[
        "-c",
        "user.email=t@example.invalid",
        "-c",
        "user.name=T",
        "commit",
        "-qm",
        "one",
    ]);
    git(&["pack-refs", "--all"]);
    assert!(
        !temp.join(".git/refs/heads/main").exists(),
        "the branch ref is packed"
    );
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let target = temp.join("target");
    let build_and_read = || {
        run(
            &temp,
            &cargo,
            &["build", "-q", "--target-dir", target.to_str().unwrap()],
        );
        run(&temp, target.join("debug/probe").to_str().unwrap(), &[])
    };
    assert_eq!(build_and_read(), git(&["rev-parse", "HEAD"]));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    git(&[
        "-c",
        "user.email=t@example.invalid",
        "-c",
        "user.name=T",
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "two",
    ]);
    assert_eq!(
        build_and_read(),
        git(&["rev-parse", "HEAD"]),
        "the rebuild names the new commit"
    );
    let _ = std::fs::remove_dir_all(&temp);
    let _ = std::fs::remove_file(temp.with_extension("gitconfig"));
}
