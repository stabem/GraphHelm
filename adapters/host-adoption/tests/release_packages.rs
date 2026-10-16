//! The bundled release manifest names each builtin package. In a source checkout it pins no
//! digest (#407) and no version (#1459): the loader derives both from the package, so an edit
//! under `extensions/builtin/` needs no re-pin and two PRs never collide on the file. A packaged
//! bundle still pins both (`hosts/packages.rs`, refused without them).

#[test]
fn the_source_release_file_pins_no_digest_and_the_loader_derives_the_current_one() {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../extensions/releases/adoption-0.1.1.json");
    let release: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    assert!(
        release["packages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p.get("digest").is_none() && p.get("version").is_none()),
        "the source release file must not pin digests (#407) or versions (#1459): {release}"
    );
    let packages = graphhelm_host_adoption::hosts::release_packages().unwrap();
    assert!(!packages.is_empty());
    let stale: Vec<String> = packages
        .iter()
        .filter_map(|package| {
            let current = graphhelm_schema::validate_extension_package(&package.path).unwrap();
            (package.id != current.id
                || package.version != current.version
                || package.digest != current.package_digest)
                .then(|| format!("{} -> {}", package.id, current.package_digest))
        })
        .collect();
    assert!(
        stale.is_empty(),
        "the loader derived a digest the package does not have: {stale:?}"
    );
}

/// #407: two PRs that edit bundled packages must not collide in the release file. Under per-PR
/// digest pins each branch rewrites its package's `digest` line, and the lines sit next to each
/// other, so git refuses the second merge (#394, #395 and #399 hit it on 2026-10-07). The test
/// takes the real checked-in release file into a temp repository, makes on two branches the edit
/// a change to each package forces in that file, and merges both. Cost: one temp git repository,
/// a handful of git commands.
#[test]
fn two_package_edits_do_not_collide_in_the_release_file() {
    let release = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../extensions/releases/adoption-0.1.1.json");
    let text = std::fs::read_to_string(&release).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let out = graphhelm_process::command("git")
            .arg("-C")
            .arg(dir.path())
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        (
            out.status.success(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    assert!(git(&["init", "-q", "-b", "main"]).0);
    std::fs::write(dir.path().join("release.json"), &text).unwrap();
    assert!(git(&["add", "-A"]).0);
    assert!(git(&["commit", "-qm", "base"]).0);
    // What editing one package forces in the release file: a new digest (#407) and a version
    // bump (#1459) on that package's line when the file pins them, nothing when the loader
    // derives them.
    let forced = |id: &str, text: &str| -> String {
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
        let package = value["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|package| package["id"] == id)
            .unwrap();
        let mut out = text.to_owned();
        if let Some(digest) = package["digest"].as_str() {
            out = out.replace(digest, &format!("sha256:{}", "e".repeat(64)));
        }
        if let Some(version) = package["version"].as_str() {
            let line = format!("\"id\":\"{id}\",\"version\":\"{version}\"");
            assert!(out.contains(&line), "unexpected release line for {id}");
            out = out.replace(&line, &format!("\"id\":\"{id}\",\"version\":\"9.9.9\""));
        }
        out
    };
    for (branch, id) in [
        ("pr-a", "graphhelm-jpd"),
        ("pr-b", "graphhelm-development-contracts"),
    ] {
        assert!(git(&["checkout", "-q", "-b", branch, "main"]).0);
        std::fs::write(dir.path().join("release.json"), forced(id, &text)).unwrap();
        std::fs::write(dir.path().join(format!("{id}.txt")), "edited package\n").unwrap();
        assert!(git(&["add", "-A"]).0);
        assert!(git(&["commit", "-qm", branch]).0);
    }
    assert!(git(&["checkout", "-q", "main"]).0);
    assert!(git(&["merge", "-q", "--no-edit", "pr-a"]).0);
    let (merged, stderr) = git(&["merge", "-q", "--no-edit", "pr-b"]);
    assert!(
        merged,
        "two package edits collided in extensions/releases/adoption-0.1.1.json: {stderr}"
    );
}
