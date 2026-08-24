//! Hostile-package guards for the development-contracts bundle (#224, task-008).
//!
//! **Why the hostile packages are built in a `TempDir` rather than shipped.** A deliberately broken
//! package living inside the real one is a package the inventory guard must then be taught to
//! ignore, and a guard with an exception list is a guard with a hole shaped like its exception.
//! Each case below copies the shipped package, changes **one** thing, and asserts the refusal — one
//! mutation per case, so the refusal is attributable to that field. A fixture that breaks two
//! things is satisfied by a validator that catches either.
//!
//! **Every case asserts its OWN diagnostic code, not merely that something was refused.** A hostile
//! fixture that goes red on a neighbouring rule reads as coverage of a threat nobody guards. That
//! mapping was measured against `core/schema/src/extension.rs` before these tests were written, and
//! the first version of the map was wrong — see the two notes below.
//!
//! **Two of the six shapes #224 names are not here, and neither is silently dropped:**
//!
//! * *Self-publication at the manifest level.* `/spec/contracts/publication` is declared
//!   `governor-only` by every package and **read by nothing** — measured across `core/` with keys
//!   the validator demonstrably reads as controls in the same run. A package declaring
//!   `"publication": "self"` validates clean, so that fixture has no emitter. Filed as #285. The
//!   same threat one layer down *is* enforced and is covered here by
//!   `a_skill_that_writes_instead_of_proposing_is_refused`: what a contribution may DO is checked,
//!   what the package SAYS IT IS is not, and treating the first as coverage of the second is the
//!   flattening this file exists to avoid.
//!
//! * *Private import.* `extension://` references are read from graph contributions, at
//!   `/spec/nodes/*/agent/ref` and `/spec/policies`. This package declares no graph contribution --
//!   only fixtures, schemas and policies -- so the shape cannot occur here without inventing one.
//!   It is already guarded and already tested: `apps/cli/tests/extension_cli.rs` drives
//!   `extension://another-package/defect-hunter` against a graph-bearing package. Repeating it here
//!   would duplicate an ORACLE rather than a mechanism, and a duplicated oracle diverges in silence
//!   and quietly changes what "passed" means.

use std::path::{Path, PathBuf};

use graphhelm_protocols::Diagnostic;
use tempfile::TempDir;

fn shipped_package() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../extensions/builtin/graphhelm-development-contracts")
}

/// Copy the shipped package into a temporary directory.
///
/// Every hostile case mutates the copy. Nothing here can perturb the real package, which is what
/// makes it safe to sabotage in a test that runs alongside seven other lanes' work.
fn stage() -> (TempDir, PathBuf) {
    let temp = TempDir::new().expect("a temporary directory");
    let root = temp.path().join("package");
    copy_tree(&shipped_package(), &root);
    (temp, root)
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("create the staged directory");
    for entry in std::fs::read_dir(from).expect("read the source directory") {
        let entry = entry.expect("a readable directory entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("a file type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("copy a package file");
        }
    }
}

fn manifest_of(root: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(root.join("extension.json")).expect("read the manifest");
    serde_json::from_str(&text).expect("the manifest is JSON")
}

fn write_manifest(root: &Path, manifest: &serde_json::Value) {
    let text = serde_json::to_string_pretty(manifest).expect("serialise the manifest") + "\n";
    std::fs::write(root.join("extension.json"), text).expect("write the manifest");
}

fn refusal_codes(root: &Path) -> Vec<String> {
    match graphhelm_schema::validate_extension_package(root) {
        Ok(validated) => panic!(
            "the package validated when it should have been refused: id={} version={} \
             contributions={}",
            validated.id, validated.version, validated.contribution_count
        ),
        Err(diagnostics) => diagnostics
            .iter()
            .map(|d: &Diagnostic| d.code.clone())
            .collect(),
    }
}

fn assert_refused_under(root: &Path, code: &str, what_was_changed: &str) {
    let codes = refusal_codes(root);
    assert!(
        codes.iter().any(|c| c == code),
        "changing {what_was_changed} must be refused under {code}, and the validator answered \
         {codes:?}. Refused for the wrong reason is not coverage: this case exists to prove that \
         THIS threat has a guard, and a red from a neighbouring rule proves only that the package \
         is broken in some way."
    );
}

/// The index of a contribution this bundle's skills own.
///
/// Fails loudly rather than skipping when no skill is declared: before the bundle lands this IS the
/// red, and it names what closes it. A hostile case that silently found nothing to mutate would
/// pass, and a passing case that never ran its mutation is the vacuous green this whole file is
/// arranged against.
fn skill_index(manifest: &serde_json::Value) -> usize {
    let contributions = manifest["spec"]["contracts"]["contributions"]
        .as_array()
        .expect("the manifest declares contributions");
    contributions
        .iter()
        .position(|c| c["kind"] == "skill")
        .unwrap_or_else(|| {
            panic!(
                "no `skill` contribution is declared, so there is nothing for this hostile case to \
                 mutate. This is the RED that #224's bundle closes: three entry skills -- \
                 code-contract, context-retrieval and memory-curator -- declared in extension.json \
                 with their SKILL.md paths and digests."
            )
        })
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **shipping the
/// bundle without one of its three entry skills or one of its three host manifests**, or shipping a
/// package that no longer validates at all.
///
/// This runs first by intent. The six negatives below are each satisfied by a validator that
/// refuses everything, so none of them means anything until the known-good package is shown to
/// pass. A negative suite with no positive control measures nothing.
#[test]
fn the_shipped_bundle_validates_and_declares_what_the_issue_promises() {
    let package = shipped_package();
    let validated = match graphhelm_schema::validate_extension_package(&package) {
        Ok(validated) => validated,
        Err(diagnostics) => panic!(
            "the shipped package does not validate, so every hostile case below proves nothing: \
             {diagnostics:?}"
        ),
    };
    assert!(
        validated.contribution_count > 0,
        "the package validated with zero contributions, which makes every case here vacuous"
    );

    let manifest = manifest_of(&package);
    let contributions = manifest["spec"]["contracts"]["contributions"]
        .as_array()
        .expect("the manifest declares contributions");
    let skills: Vec<&str> = contributions
        .iter()
        .filter(|c| c["kind"] == "skill")
        .filter_map(|c| c["path"].as_str())
        .collect();

    for expected in [
        "skills/code-contract/SKILL.md",
        "skills/context-retrieval/SKILL.md",
        "skills/memory-curator/SKILL.md",
    ] {
        assert!(
            skills.contains(&expected),
            "`{expected}` is not declared as a skill contribution. Declared skills: {skills:?}"
        );
    }

    for host in [
        ".claude-plugin/plugin.json",
        ".codex-plugin/plugin.json",
        ".mcp.json",
    ] {
        assert!(
            package.join(host).is_file(),
            "the host manifest `{host}` is missing. Host views are hand-written and deletable, but \
             this bundle is specified to ship all three."
        );
    }
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **widening the
/// package authority grant**, or letting a skill declare an effect the grant does not carry.
///
/// `/spec/permissions/runtime/mutations` is empty in this package, so `runtime.mutate` is authority
/// this bundle was never granted. A skill that can mutate the runtime is no longer data-only, and
/// the whole premise of the task is that these entries request typed operations rather than perform
/// them.
#[test]
fn an_effect_beyond_the_package_grant_is_refused_as_authority_escalation() {
    let (_temp, root) = stage();
    let mut manifest = manifest_of(&root);
    let index = skill_index(&manifest);
    manifest["spec"]["contracts"]["contributions"][index]["effects"] =
        serde_json::json!(["runtime.mutate"]);
    write_manifest(&root, &manifest);

    assert_refused_under(
        &root,
        "GHEX018_AUTHORITY_ESCALATION",
        "a skill's effects to include `runtime.mutate`, which the package grant does not carry",
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **dropping the
/// requirement that an effect names its matching contribution permission.**
///
/// `host.discover` is inside the package grant, so this case is not about exceeding authority: it is
/// about taking authority without declaring it. The permission list is what a reviewer reads to see
/// what an entry can reach, and an effect that works without its permission makes that list a
/// description rather than a constraint.
#[test]
fn an_effect_without_its_declared_permission_is_refused_as_authority_escalation() {
    let (_temp, root) = stage();
    let mut manifest = manifest_of(&root);
    let index = skill_index(&manifest);
    manifest["spec"]["contracts"]["contributions"][index]["effects"] =
        serde_json::json!(["host.discover"]);
    manifest["spec"]["contracts"]["contributions"][index]["permissions"] = serde_json::json!([]);
    write_manifest(&root, &manifest);

    assert_refused_under(
        &root,
        "GHEX018_AUTHORITY_ESCALATION",
        "a skill to take the `host.discover` effect without declaring `package.read`",
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **granting the
/// package `workspace.artifact.write`**, which would let an entry skill publish instead of propose.
///
/// This is #224's self-publication threat at the layer where it is actually enforced. The grant
/// carries `workspaceArtifacts: "proposal-write"`, so `artifact.local.write` exceeds it. The
/// manifest-level `publication` field expresses the same intent and is enforced by nothing at all --
/// #285 -- so this case must not be read as covering it.
#[test]
fn a_skill_that_writes_instead_of_proposing_is_refused() {
    let (_temp, root) = stage();
    let mut manifest = manifest_of(&root);
    let index = skill_index(&manifest);
    manifest["spec"]["contracts"]["contributions"][index]["effects"] =
        serde_json::json!(["artifact.local.write"]);
    write_manifest(&root, &manifest);

    assert_refused_under(
        &root,
        "GHEX018_AUTHORITY_ESCALATION",
        "a skill to write artifacts directly instead of proposing them",
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **removing the
/// digest check over declared contribution content.**
///
/// The bytes move and the declaration does not. Nothing breaks at runtime, so the package ships
/// describing content it no longer has, and every consumer trusting that digest is trusting a
/// number about different bytes.
#[test]
fn moving_a_declared_files_bytes_is_refused_as_a_digest_mismatch() {
    let (_temp, root) = stage();
    let manifest = manifest_of(&root);
    let first = manifest["spec"]["contracts"]["contributions"][0]["path"]
        .as_str()
        .expect("the first contribution declares a path")
        .to_owned();
    let target = root.join(&first);
    let mut bytes = std::fs::read(&target).expect("read a declared file");
    bytes.push(b'\n');
    std::fs::write(&target, bytes).expect("perturb a declared file");

    assert_refused_under(
        &root,
        "GHEX005_DIGEST",
        &format!("the bytes of `{first}` without moving its declared digest"),
    );
}

/// Activation is out of reach until #212 lands, and this records what will be asserted then.
///
/// Deliberately not `#[ignore]`. A skipped test and a passing test are the same green row in a
/// summary, and the thing most likely to be forgotten is the case nobody sees fail. This one states
/// the claim it cannot yet make and names what unblocks it, so the gap is legible in the same place
/// the guards are.
#[test]
fn activation_is_declared_pending_on_212() {
    let package = shipped_package();
    assert!(
        package.join("extension.json").is_file(),
        "arrangement: the package must exist for this declaration to refer to anything"
    );

    // WHEN #212 LANDS, this case asserts: activating the bundle installs exactly the three entry
    // skills and the three host views, grants no permission beyond the package grant, and can be
    // rolled back atomically to the prior version leaving user-authored artifacts untouched.
    //
    // It cannot assert any of that today: nothing in this repository activates an extension, so the
    // assertion would be written against a mechanism that does not exist and would pass for that
    // reason alone.
    let activation = manifest_of(&package)["spec"]["contracts"]["activation"].clone();
    assert_eq!(
        activation,
        serde_json::json!("explicit"),
        "until #212 lands, the only activation claim this bundle can support is that it declares \
         itself explicit-activation and nothing here activates it"
    );
}

// ---------------------------------------------------------------------------------------------
// The host cross-match is a property of MANY facts, and one mutation exercises one of them.
//
// Raised by L on #290: the first version of this file changed `name` in the Claude manifest and
// called the cross-match covered. A regression that stopped comparing `version` in the Codex
// manifest would have reddened nothing, and the suite would have reported coverage of a property
// it tested a sixth of. It is this file's own rule — each case red under its OWN code — one floor
// up: each FACT gets its own cell, or the coverage is declared with what is missing named.
//
// Measured from `valid_mcp_registration` and the two identity comparisons rather than counted from
// the review note: five identity facts, and a closed MCP shape whose load-bearing conditions are
// the server name, the command, and the loopback URL.
//
// WHAT IS NOT DECOMPOSED, so the declared coverage is honest: the MCP argument vector's arity and
// the exact order of its seven elements, and the "exactly one key" conditions at the root and the
// server object. Those are shape checks that fail as a unit; a cell per permutation would be a
// table of the validator's implementation rather than of the property.
// ---------------------------------------------------------------------------------------------

/// Stage the package, mutate one field of one host manifest, move that file's declared digest so
/// identity is the only thing wrong, and require the refusal under the host code.
///
/// The digest move is what makes each of these a ONE-fact case. Without it the validator answers
/// `GHEX005_DIGEST` and never reaches the cross-match — a refusal, a green test, and no coverage,
/// which is exactly how the first version of this suite passed.
fn assert_host_fact(host_path: &str, mutate: impl Fn(&mut serde_json::Value), what: &str) {
    let (_temp, root) = stage();
    let host = root.join(host_path);
    assert!(
        host.is_file(),
        "`{host_path}` does not exist, so there is no host manifest to perturb"
    );

    let text = std::fs::read_to_string(&host).expect("read the host manifest");
    let mut value: serde_json::Value = serde_json::from_str(&text).expect("host manifest is JSON");
    mutate(&mut value);
    let rewritten = serde_json::to_string_pretty(&value).expect("serialise") + "\n";
    std::fs::write(&host, &rewritten).expect("write the host manifest");

    let mut manifest = manifest_of(&root);
    let digest = format!(
        "sha256:{}",
        hex::encode(<sha2::Sha256 as sha2::Digest>::digest(rewritten.as_bytes()))
    );
    let entry = manifest["spec"]["contracts"]["contributions"]
        .as_array_mut()
        .expect("the manifest declares contributions")
        .iter_mut()
        .find(|c| c["path"] == host_path)
        .unwrap_or_else(|| panic!("`{host_path}` is not a declared contribution"));
    entry["sha256"] = serde_json::json!(digest);
    write_manifest(&root, &manifest);

    assert_refused_under(&root, "GHEX013_HOST", what);
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: **the Claude manifest stops being compared by name.**
#[test]
fn the_claude_host_name_is_cross_matched() {
    assert_host_fact(
        ".claude-plugin/plugin.json",
        |v| v["name"] = serde_json::json!("graphhelm-somebody-else"),
        "the Claude host manifest's package name",
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: **the Claude manifest stops being compared by
/// version** — the regression L named, which the single-mutation suite could not see.
#[test]
fn the_claude_host_version_is_cross_matched() {
    assert_host_fact(
        ".claude-plugin/plugin.json",
        |v| v["version"] = serde_json::json!("9.9.9"),
        "the Claude host manifest's version",
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: **the Codex manifest stops being compared by id.**
///
/// `id` and `name` are separate facts in that file and are compared separately. A host reading only
/// one of them would install a package whose other half names something else.
#[test]
fn the_codex_host_id_is_cross_matched() {
    assert_host_fact(
        ".codex-plugin/plugin.json",
        |v| v["id"] = serde_json::json!("graphhelm-somebody-else"),
        "the Codex host manifest's package id",
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: **the Codex manifest stops being compared by name.**
#[test]
fn the_codex_host_name_is_cross_matched() {
    assert_host_fact(
        ".codex-plugin/plugin.json",
        |v| v["name"] = serde_json::json!("graphhelm-somebody-else"),
        "the Codex host manifest's package name",
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: **the Codex manifest stops being compared by
/// version.**
#[test]
fn the_codex_host_version_is_cross_matched() {
    assert_host_fact(
        ".codex-plugin/plugin.json",
        |v| v["version"] = serde_json::json!("9.9.9"),
        "the Codex host manifest's version",
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, and the one that matters most here: **the MCP
/// registration stops requiring a loopback URL.**
///
/// This registration hands the Runtime a token by file path. Point it at a host that is not
/// loopback and the token file is read and presented to whatever answers there — the package's own
/// grant says `network.external: false`, and this is the one file that could contradict it without
/// declaring a single extra permission.
#[test]
fn the_mcp_registration_must_address_loopback() {
    assert_host_fact(
        ".mcp.json",
        |v| v["mcpServers"]["graphhelm"]["args"][2] = serde_json::json!("http://evil.example:8080"),
        "the MCP registration's URL to a host that is not loopback",
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: **the MCP registration stops pinning the command.**
///
/// The command is the binary the host will run. Substituting it is arbitrary execution wearing the
/// package's name, and nothing else in the bundle would look different.
#[test]
fn the_mcp_registration_must_pin_the_graphhelm_command() {
    assert_host_fact(
        ".mcp.json",
        |v| v["mcpServers"]["graphhelm"]["command"] = serde_json::json!("/usr/bin/whatever"),
        "the MCP registration's command to something other than the GraphHelm CLI",
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: **the MCP registration stops requiring exactly the
/// `graphhelm` server.**
#[test]
fn the_mcp_registration_must_declare_the_graphhelm_server() {
    assert_host_fact(
        ".mcp.json",
        |v| {
            let server = v["mcpServers"]["graphhelm"].clone();
            v["mcpServers"] = serde_json::json!({ "something-else": server });
        },
        "the MCP registration's server name",
    );
}
