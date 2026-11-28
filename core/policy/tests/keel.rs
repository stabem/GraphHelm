//! Keel classifier (#1212): every bound tested at N and N+1, every language on its own diff, and
//! the ladder folded from a history rather than asserted from a number.
use graphhelm_policy::keel::{
    Classification, Debit, Enforcement, KeelPolicy, Ladder, Mode, NodeRecord, SurfaceBudget,
    check_card, classify_write, effective_mode, ladder,
};

fn package_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../extensions/builtin/graphhelm-development-contracts")
}

fn shipped_policy() -> KeelPolicy {
    let text = std::fs::read_to_string(package_root().join("policies/keel.yaml")).unwrap();
    serde_yaml_ng::from_str(&text).unwrap()
}

/// The shipped rules with count findings set to refuse, for the cells that test the shapes a
/// refusal takes. The shipped default is `signal`, tested on its own below.
fn blocking_policy() -> KeelPolicy {
    KeelPolicy {
        surface_enforcement: Enforcement::Block,
        ..shipped_policy()
    }
}

fn diff(path: &str, is_new: bool, body: &str) -> String {
    let mut out = format!("diff --git a/{path} b/{path}\n");
    if is_new {
        out.push_str("new file mode 100644\n--- /dev/null\n");
    } else {
        out.push_str(&format!("--- a/{path}\n"));
    }
    out.push_str(&format!(
        "+++ b/{path}\n@@ -0,0 +1,{} @@\n",
        body.lines().count()
    ));
    for line in body.lines() {
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Builds an indented Python body without a run of spaces inside any source literal (the
/// workspace's collapsed-run guard refuses three or more), so the test can carry real indentation.
fn python_body(lines: &[(usize, &str)]) -> String {
    let mut out = String::new();
    for (indent, text) in lines {
        out.push_str(&" ".repeat(*indent));
        out.push_str(text);
        out.push('\n');
    }
    out
}

/// A unified diff over an existing file with context and added lines, indentation built at
/// runtime for the same reason as [`python_body`].
fn hunk(path: &str, lines: &[(char, usize, &str)]) -> String {
    let mut out =
        format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1,1 +1,1 @@\n");
    for (marker, indent, text) in lines {
        out.push(*marker);
        out.push_str(&" ".repeat(*indent));
        out.push_str(text);
        out.push('\n');
    }
    out
}

fn total(c: &Classification, debit: Debit) -> u32 {
    c.totals.get(&debit).copied().unwrap_or(0)
}

fn rules(c: &Classification) -> Vec<&str> {
    c.findings.iter().map(|f| f.rule.as_str()).collect()
}

#[test]
fn the_shipped_policy_loads_into_the_struct_and_its_fixtures_agree_with_it() {
    let policy = shipped_policy();
    assert_eq!(policy.version, "1.6.0");
    assert_eq!(
        policy.surface_enforcement,
        Enforcement::Signal,
        "counts ship as signals"
    );
    assert!(!policy.ladder.enabled, "the ladder ships off");
    let fixture: serde_json::Value = serde_json::from_slice(
        &std::fs::read(package_root().join("fixtures/keel/valid/policy-shipped-shape.json"))
            .unwrap(),
    )
    .unwrap();
    let from_fixture: KeelPolicy = serde_json::from_value(fixture).unwrap();
    assert_eq!(
        from_fixture, policy,
        "the valid fixture is the shipped policy, byte for meaning"
    );
    for invalid in [
        "ladder-strikes-per-rung-zero.json",
        "surface-unknown-kind.json",
    ] {
        let value: serde_json::Value = serde_json::from_slice(
            &std::fs::read(package_root().join("fixtures/keel/invalid").join(invalid)).unwrap(),
        )
        .unwrap();
        let parsed = serde_json::from_value::<KeelPolicy>(value);
        if invalid.starts_with("surface-unknown") {
            assert!(
                parsed.is_err(),
                "{invalid}: an unknown surface kind must not deserialize"
            );
        } else {
            // The struct accepts a zero divisor; the schema refuses it and the fold treats it as
            // a ladder that never demotes. Both halves are asserted: here and in the ladder test.
            assert_eq!(parsed.unwrap().ladder.strikes_per_rung, 0);
        }
    }
}

#[test]
fn a_body_only_edit_in_an_existing_file_charges_nothing() {
    let policy = shipped_policy();
    let d = diff(
        "core/graph/src/hash.rs",
        false,
        &format!(
            "{i}let digest = Sha256::digest(bytes);\n{i}hex::encode(digest)\n",
            i = " ".repeat(4)
        ),
    );
    let c = classify_write(&d, &policy, None, Mode::Full);
    assert!(c.charges.is_empty(), "{c:?}");
    assert!(!c.refused);
    assert_eq!(c.policy_version, "1.6.0");
}

#[test]
fn rust_declarations_are_charged_by_kind_and_private_ones_are_free() {
    let policy = shipped_policy();
    let d = diff(
        "core/graph/src/lint.rs",
        false,
        "pub struct Finding;\npub enum Severity { A }\npub trait Lint {}\npub type Id = u64;\n\
         pub fn lint() {}\npub async fn lint_async() {}\npub(crate) fn hidden() {}\nfn private() {}\n\
         struct Private;\n#[test]\nfn a_test() {}\n",
    );
    let c = classify_write(&d, &policy, None, Mode::Full);
    assert_eq!(total(&c, Debit::NewType), 4, "{c:?}");
    assert_eq!(total(&c, Debit::NewPublicFn), 2, "{c:?}");
    assert_eq!(total(&c, Debit::NewTest), 1, "{c:?}");
    assert_eq!(total(&c, Debit::NewModule), 0);
    let symbols: Vec<&str> = c.charges.iter().map(|ch| ch.symbol.as_str()).collect();
    assert!(
        symbols.contains(&"Finding") && symbols.contains(&"lint_async"),
        "{symbols:?}"
    );
    assert!(!symbols.contains(&"hidden") && !symbols.contains(&"private"));
    assert_eq!(
        rules(&c),
        vec!["keel.surface.new_type_over_budget"],
        "4 types against 2"
    );
}

#[test]
fn typescript_exports_are_charged_and_a_non_function_const_is_not_a_public_fn() {
    let policy = shipped_policy();
    let d = diff(
        "studio/src/lib/routes.ts",
        false,
        "export function load() {}\nexport async function save() {}\n\
         export const pick = (x: number) => x;\nexport const run = async () => {};\n\
         export const LIMIT = 4;\nexport const typed: Handler = (e) => e;\n\
         export interface Route {}\nexport type Id = string;\nexport class Store {}\nexport enum Kind { A }\n\
         export default function main() {}\nconst local = () => 1;\nfunction inner() {}\n",
    );
    let c = classify_write(&d, &policy, None, Mode::Full);
    assert_eq!(total(&c, Debit::NewPublicFn), 6, "{c:?}");
    assert_eq!(total(&c, Debit::NewType), 4, "{c:?}");
    assert!(
        !c.charges
            .iter()
            .any(|ch| ch.symbol == "LIMIT" || ch.symbol == "local")
    );
}

#[test]
fn python_top_level_public_defs_and_classes_count_and_underscore_or_nested_do_not() {
    let policy = shipped_policy();
    let d = diff(
        "tools/bench/run.py",
        false,
        &python_body(&[
            (0, "def public_one():"),
            (4, "pass"),
            (0, "def _private():"),
            (4, "pass"),
            (0, "async def public_two():"),
            (4, "pass"),
            (0, "class Runner:"),
            (4, "def method(self):"),
            (8, "pass"),
            (0, "class _Hidden:"),
            (4, "pass"),
            (0, "def test_inside_source():"),
            (4, "pass"),
        ]),
    );
    let c = classify_write(&d, &policy, None, Mode::Full);
    assert_eq!(total(&c, Debit::NewPublicFn), 2, "{c:?}");
    assert_eq!(total(&c, Debit::NewType), 1, "{c:?}");
    assert_eq!(total(&c, Debit::NewTest), 1, "{c:?}");
}

#[test]
fn a_new_source_file_is_a_module_and_a_new_test_file_is_not() {
    let policy = shipped_policy();
    let source = diff("core/graph/src/keelcheck.rs", true, "pub fn check() {}\n");
    let c = classify_write(&source, &policy, None, Mode::Full);
    assert_eq!(total(&c, Debit::NewModule), 1, "{c:?}");
    assert_eq!(total(&c, Debit::NewPublicFn), 1);
    assert!(!c.refused, "one module and one fn are within budget: {c:?}");

    let tests = diff(
        "core/graph/tests/keelcheck.rs",
        true,
        "#[test]\nfn it_checks() {}\n#[test]\nfn it_checks_again() {}\npub fn helper() {}\n",
    );
    let c = classify_write(&tests, &policy, None, Mode::Full);
    assert_eq!(
        total(&c, Debit::NewModule),
        0,
        "a test file is not public surface: {c:?}"
    );
    assert_eq!(total(&c, Debit::NewTest), 2);
    assert_eq!(
        total(&c, Debit::NewPublicFn),
        0,
        "a helper in a test file is free"
    );
    assert!(!c.refused);

    let third = diff(
        "studio/src/lib/routes.test.ts",
        true,
        "it('a', () => {});\ntest('b', () => {});\nit.each([1])('c', () => {});\n",
    );
    let c = classify_write(&third, &policy, None, Mode::Full);
    assert_eq!(total(&c, Debit::NewTest), 3);
    assert_eq!(
        rules(&c),
        vec!["keel.surface.new_test_over_budget"],
        "3 tests against 2"
    );
}

#[test]
fn dependencies_are_charged_only_inside_dependency_sections_of_each_manifest() {
    let policy = shipped_policy();
    let cargo = "diff --git a/core/graph/Cargo.toml b/core/graph/Cargo.toml\n--- a/core/graph/Cargo.toml\n+++ b/core/graph/Cargo.toml\n@@ -1,8 +1,10 @@\n [package]\n name = \"graphhelm-graph\"\n+edition = \"2024\"\n \n [dependencies]\n serde.workspace = true\n+regex = \"1\"\n+\n [dev-dependencies]\n+proptest = \"1\"\n";
    let c = classify_write(cargo, &policy, None, Mode::Full);
    assert_eq!(total(&c, Debit::NewDependency), 2, "{c:?}");
    let names: Vec<&str> = c.charges.iter().map(|ch| ch.symbol.as_str()).collect();
    assert_eq!(names, vec!["regex", "proptest"]);
    assert_eq!(
        rules(&c),
        vec!["keel.surface.new_dependency_over_budget"],
        "budget is zero"
    );

    let package = hunk(
        "studio/package.json",
        &[
            (' ', 0, "{"),
            (' ', 2, "\"name\": \"studio\","),
            ('+', 2, "\"version\": \"1.0.0\","),
            (' ', 2, "\"dependencies\": {"),
            (' ', 4, "\"react\": \"18\","),
            ('+', 4, "\"zod\": \"3\""),
            (' ', 2, "},"),
            (' ', 2, "\"scripts\": {"),
            ('+', 4, "\"lint\": \"eslint\""),
            (' ', 2, "}"),
            (' ', 0, "}"),
        ],
    );
    let c = classify_write(&package, &policy, None, Mode::Full);
    assert_eq!(total(&c, Debit::NewDependency), 1, "{c:?}");
    assert_eq!(c.charges[0].symbol, "zod");

    let requirements = diff(
        "requirements.txt",
        false,
        "# tooling\nhttpx==0.27\n\npytest\n",
    );
    let c = classify_write(&requirements, &policy, None, Mode::Full);
    assert_eq!(total(&c, Debit::NewDependency), 2, "{c:?}");
}

#[test]
fn every_surface_budget_is_refused_at_n_plus_one_and_accepted_at_n() {
    let policy = shipped_policy();
    // newPublicFn budget is 4.
    let four = diff(
        "a/src/x.rs",
        false,
        "pub fn a(){}\npub fn b(){}\npub fn c(){}\npub fn d(){}\n",
    );
    let five = diff(
        "a/src/x.rs",
        false,
        "pub fn a(){}\npub fn b(){}\npub fn c(){}\npub fn d(){}\npub fn e(){}\n",
    );
    assert!(!classify_write(&four, &policy, None, Mode::Full).refused);
    let c = classify_write(&five, &policy, None, Mode::Full);
    assert_eq!(rules(&c), vec!["keel.surface.new_public_fn_over_budget"]);
    assert_eq!(c.findings[0].detail, "charged 5, budget 4");
    // An allowance of one lifts it; an allowance above the ceiling is capped to the ceiling.
    let lifted = classify_write(
        &five,
        &policy,
        Some(SurfaceBudget {
            new_public_fn: 1,
            ..SurfaceBudget::default()
        }),
        Mode::Full,
    );
    assert!(!lifted.refused, "{lifted:?}");
    let seventeen = diff(
        "a/src/x.rs",
        false,
        &(0..17)
            .map(|i| format!("pub fn f{i}(){{}}\n"))
            .collect::<String>(),
    );
    let capped = classify_write(
        &seventeen,
        &policy,
        Some(SurfaceBudget {
            new_public_fn: 999,
            ..SurfaceBudget::default()
        }),
        Mode::Full,
    );
    assert_eq!(
        capped.budget.new_public_fn,
        4 + 12,
        "surface plus maxAllowance"
    );
    assert_eq!(
        rules(&capped),
        vec!["keel.surface.new_public_fn_over_budget"]
    );
}

#[test]
fn the_body_cap_refuses_at_n_plus_one_lines_and_not_at_n() {
    let policy = shipped_policy();
    let n = policy.body.max_file_lines_delta as usize;
    let at = diff("a/src/x.rs", false, &"let _ = 1;\n".repeat(n));
    let over = diff("a/src/x.rs", false, &"let _ = 1;\n".repeat(n + 1));
    assert!(!classify_write(&at, &policy, None, Mode::Full).refused);
    let c = classify_write(&over, &policy, None, Mode::Full);
    assert_eq!(rules(&c), vec!["keel.body.oversized_change"]);
    assert_eq!(c.findings[0].path.as_deref(), Some("a/src/x.rs"));
}

#[test]
fn modes_shrink_the_surface_by_shape_not_by_number() {
    let policy = blocking_policy();
    let new_type = diff("a/src/x.rs", false, "pub struct S;\n");
    let new_fn = diff("a/src/x.rs", false, "pub fn f(){}\n");
    let body = diff("a/src/x.rs", false, &format!("{}x += 1;\n", " ".repeat(4)));
    assert!(!classify_write(&new_type, &policy, None, Mode::Full).refused);
    assert!(classify_write(&new_type, &policy, None, Mode::ContractOnly).refused);
    assert!(!classify_write(&new_fn, &policy, None, Mode::ContractOnly).refused);
    assert!(classify_write(&new_fn, &policy, None, Mode::PatchOnly).refused);
    assert!(!classify_write(&body, &policy, None, Mode::PatchOnly).refused);
    // An allowance does not reopen a rung: contract_only stays closed to new types.
    let c = classify_write(
        &new_type,
        &policy,
        Some(SurfaceBudget {
            new_type: 6,
            ..SurfaceBudget::default()
        }),
        Mode::ContractOnly,
    );
    assert!(c.refused, "{c:?}");
    assert_eq!(c.budget.new_type, 0);
}

#[test]
fn the_ladder_folds_strikes_minus_credit_over_the_window_and_never_banks_credit() {
    let policy = Ladder {
        enabled: true,
        strikes_per_rung: 2,
        credit_per_proven_promise: 1,
        window_nodes: 4,
    };
    let drift = NodeRecord {
        drifted: true,
        proven: false,
    };
    let proof = NodeRecord {
        drifted: false,
        proven: true,
    };
    let quiet = NodeRecord {
        drifted: false,
        proven: false,
    };
    assert_eq!(ladder(&[], &policy), Mode::Full);
    assert_eq!(
        ladder(&[drift], &policy),
        Mode::Full,
        "one strike is under the rung"
    );
    assert_eq!(ladder(&[drift, drift], &policy), Mode::ContractOnly);
    assert_eq!(
        ladder(&[drift, drift, drift, drift], &policy),
        Mode::PatchOnly
    );
    assert_eq!(
        ladder(&[drift; 6], &policy),
        Mode::PatchOnly,
        "window is 4: only 4 of the 6 strikes count"
    );
    let wide = Ladder {
        window_nodes: 6,
        ..policy
    };
    assert_eq!(
        ladder(&[drift; 6], &wide),
        Mode::ProposeOnly,
        "window 6: all 6 count"
    );
    assert_eq!(
        ladder(&[drift, drift, proof], &policy),
        Mode::Full,
        "a proof pays one back"
    );
    assert_eq!(
        ladder(&[proof, proof, proof, drift, drift], &policy),
        Mode::ContractOnly,
        "proofs BEFORE the drifts pay nothing back: 2 strikes stand, credit is never banked"
    );
    assert_eq!(
        ladder(&[drift, drift, proof, proof], &policy),
        Mode::Full,
        "the same records with the proofs AFTER the drifts pay both back"
    );
    assert_eq!(
        ladder(&[proof, drift, drift, drift], &policy),
        Mode::ContractOnly,
        "credit before drift is not banked: [proof, drift, drift, drift] folds to 3 strikes"
    );
    assert_eq!(
        ladder(&[quiet, quiet, drift, drift], &policy),
        Mode::ContractOnly
    );
    let never = Ladder {
        enabled: true,
        strikes_per_rung: 0,
        credit_per_proven_promise: 0,
        window_nodes: 4,
    };
    assert_eq!(
        ladder(&[drift; 4], &never),
        Mode::Full,
        "a zero divisor never demotes"
    );
}

#[test]
fn a_card_is_bounded_on_every_side() {
    let policy = shipped_policy().card;
    let ok = check_card(&["a/b.rs".into()], &["x".into()], 100, &policy);
    assert!(ok.is_empty(), "{ok:?}");
    let empty = check_card(&[], &[], 10, &policy);
    assert_eq!(empty[0].rule, "keel.card.scope_empty");
    let wide: Vec<String> = (0..13).map(|i| format!("p{i}.rs")).collect();
    assert!(check_card(&wide[..12], &[], 10, &policy).is_empty());
    assert_eq!(
        check_card(&wide, &[], 10, &policy)[0].rule,
        "keel.card.scope_too_wide"
    );
    let glob = check_card(&["src/**/*.rs".into()], &[], 10, &policy);
    assert_eq!(glob[0].rule, "keel.card.scope_not_a_path");
    let escape = check_card(&["../other/x.rs".into()], &[], 10, &policy);
    assert_eq!(escape[0].rule, "keel.card.scope_not_a_path");
    let symbols: Vec<String> = (0..9).map(|i| format!("s{i}")).collect();
    assert!(check_card(&["a.rs".into()], &symbols[..8], 10, &policy).is_empty());
    assert_eq!(
        check_card(&["a.rs".into()], &symbols, 10, &policy)[0].rule,
        "keel.card.too_many_symbols"
    );
    assert!(check_card(&["a.rs".into()], &[], 4096, &policy).is_empty());
    assert_eq!(
        check_card(&["a.rs".into()], &[], 4097, &policy)[0].rule,
        "keel.card.too_large"
    );
}

#[test]
fn a_diff_without_a_file_header_is_a_finding_not_a_silent_zero() {
    let policy = shipped_policy();
    let c = classify_write("+pub fn orphan() {}\n", &policy, None, Mode::Full);
    assert_eq!(rules(&c), vec!["keel.diff.unparseable"]);
    assert!(c.charges.is_empty());
    let empty = classify_write("", &policy, None, Mode::Full);
    assert!(
        !empty.refused && empty.findings.is_empty(),
        "an empty diff spends nothing"
    );
}

#[test]
fn shipped_counts_report_and_only_objective_contracts_refuse() {
    let policy = shipped_policy();
    let five = diff(
        "a/src/x.rs",
        false,
        "pub fn a(){}
pub fn b(){}
pub fn c(){}
pub fn d(){}
pub fn e(){}
",
    );
    let c = classify_write(&five, &policy, None, Mode::Full);
    assert_eq!(rules(&c), vec!["keel.surface.new_public_fn_over_budget"]);
    assert!(!c.findings[0].blocking, "a count is a signal: {c:?}");
    assert!(!c.refused, "signals never refuse: {c:?}");
    // The same diff under `block` refuses: the switch is what decides, not the count.
    let blocked = classify_write(&five, &blocking_policy(), None, Mode::Full);
    assert!(
        blocked.refused && blocked.findings[0].blocking,
        "{blocked:?}"
    );
    // Objective contracts refuse under the shipped `signal` policy.
    let unparseable = classify_write(
        "+pub fn orphan() {}
",
        &policy,
        None,
        Mode::Full,
    );
    assert!(unparseable.refused && unparseable.findings[0].blocking);
    let card = check_card(&["src/**/*.rs".into()], &[], 10, &policy.card);
    assert!(card[0].blocking, "a glob is not a path: {card:?}");
    let wide: Vec<String> = (0..13).map(|i| format!("p{i}.rs")).collect();
    let wide = check_card(&wide, &[], 10, &policy.card);
    assert_eq!(wide[0].rule, "keel.card.scope_too_wide");
    assert!(
        !wide[0].blocking,
        "a wide card is a signal to split, not a refusal"
    );
}

#[test]
fn a_disabled_ladder_measures_the_rung_and_applies_none() {
    let drift = NodeRecord {
        drifted: true,
        proven: false,
    };
    let off = shipped_policy().ladder;
    assert!(!off.enabled);
    assert_eq!(
        ladder(&[drift; 4], &off),
        Mode::PatchOnly,
        "the fold still measures"
    );
    assert_eq!(
        effective_mode(&[drift; 4], &off),
        Mode::Full,
        "and applies nothing while off"
    );
    let on = Ladder {
        enabled: true,
        ..off
    };
    assert_eq!(effective_mode(&[drift; 4], &on), Mode::PatchOnly);
}

fn card(scope: &[&str]) -> graphhelm_policy::keel::Card {
    graphhelm_policy::keel::Card {
        promise: "p".into(),
        scope_paths: scope.iter().map(|path| (*path).to_owned()).collect(),
        proof: "cargo test".into(),
        exported_symbols: vec!["added".into()],
        allowance: None,
        journeys: Vec::new(),
    }
}

fn scope_rules(report: &graphhelm_policy::keel::KeelCheck) -> Vec<(String, bool)> {
    report
        .findings
        .iter()
        .filter(|finding| finding.rule == "keel.scope.path_outside_card")
        .map(|finding| (finding.path.clone().unwrap_or_default(), finding.blocking))
        .collect()
}

#[test]
fn changed_paths_reads_new_deleted_renamed_and_binary_headers() {
    let text = "diff --git a/src/a.rs b/src/a.rs\nnew file mode 100644\n--- /dev/null\n+++ b/src/a.rs\n@@ -0,0 +1 @@\n+x\n\
diff --git a/old.rs b/old.rs\ndeleted file mode 100644\n--- a/old.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-x\n\
diff --git a/from.rs b/to.rs\nsimilarity index 100%\nrename from from.rs\nrename to to.rs\n\
diff --git a/img.png b/img.png\nBinary files a/img.png and b/img.png differ\n";
    let paths = graphhelm_policy::keel::changed_paths(text);
    let names: Vec<&str> = paths.iter().map(|entry| entry.path.as_str()).collect();
    assert_eq!(names, ["src/a.rs", "old.rs", "from.rs", "to.rs", "img.png"]);
    assert!(paths[0].is_new && !paths[0].is_deleted);
    assert!(paths[1].is_deleted && !paths[1].is_new);
    assert!(paths.iter().all(|entry| entry.plain));
}

#[test]
fn scope_is_a_path_or_a_directory_prefix_never_a_string_prefix() {
    let text = format!(
        "{}{}{}",
        diff("src/lib.rs", false, "pub fn added() {}"),
        diff("src-other/x.rs", false, "let a = 1;"),
        diff("docs/notes.md", true, "note"),
    );
    let policy = shipped_policy();
    let bound = card(&["./src/", "docs/notes.md"]);
    let report = graphhelm_policy::keel::check(&text, Some((&bound, 100)), &policy);
    assert_eq!(scope_rules(&report), [("src-other/x.rs".to_owned(), true)]);
    assert!(report.refused);
    assert_eq!(report.surface.changed_files, 3);
    assert_eq!(report.surface.new_files, 1);
}

#[test]
fn a_rename_out_of_scope_and_a_deletion_out_of_scope_both_block() {
    let text = "diff --git a/src/a.rs b/lib/a.rs\nrename from src/a.rs\nrename to lib/a.rs\n\
diff --git a/gone.rs b/gone.rs\ndeleted file mode 100644\n--- a/gone.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-x\n";
    let report =
        graphhelm_policy::keel::check(text, Some((&card(&["src"]), 100)), &shipped_policy());
    assert_eq!(
        scope_rules(&report),
        [("lib/a.rs".to_owned(), true), ("gone.rs".to_owned(), true)]
    );
}

#[test]
fn a_quoted_path_is_a_signal_and_no_card_means_no_scope_rule() {
    // git quotes a non-ASCII path and escapes its bytes in octal; this reader does not decode it.
    let quoted = r#""a/sp\303\251c.rs""#;
    let text = [
        format!("diff --git {quoted} {quoted}"),
        format!("--- {quoted}"),
        format!("+++ {quoted}"),
        "@@ -1 +1 @@".to_owned(),
        "+x".to_owned(),
        String::new(),
    ]
    .join("\n");
    let text = text.as_str();
    let report =
        graphhelm_policy::keel::check(text, Some((&card(&["src"]), 100)), &shipped_policy());
    let rules = scope_rules(&report);
    assert!(!rules.is_empty());
    assert!(rules.iter().all(|(_, blocking)| !blocking), "{rules:?}");
    assert!(!report.refused);

    let loose = diff("anywhere/x.rs", false, "let a = 1;");
    let report = graphhelm_policy::keel::check(&loose, None, &shipped_policy());
    assert!(!report.card_declared);
    assert!(scope_rules(&report).is_empty());
    assert!(!report.refused);
}

#[test]
fn the_report_sets_counts_beside_the_card() {
    let text = format!(
        "{}{}",
        diff(
            "src/lib.rs",
            false,
            "pub fn added() {}\npub struct Sneaked;"
        ),
        diff("tests/extra.rs", true, "#[test]\nfn a() {}"),
    );
    let report = graphhelm_policy::keel::check(
        &text,
        Some((&card(&["src", "tests"]), 100)),
        &shipped_policy(),
    );
    assert!(!report.refused, "{:?}", report.findings);
    assert_eq!(report.surface.new_public_symbols, 2);
    assert_eq!(report.surface.new_tests, 1);
    assert_eq!(report.surface.new_test_files, 1);
    assert_eq!(report.surface.card_exported_symbols, Some(1));
    assert_eq!(report.surface.undeclared_public_symbols, ["Sneaked"]);
}

#[test]
fn the_shipped_card_fixture_is_a_card_and_an_unknown_field_is_not() {
    let bytes =
        std::fs::read(package_root().join("fixtures/keel-card/valid/card-full.json")).unwrap();
    let card: graphhelm_policy::keel::Card = serde_json::from_slice(&bytes).unwrap();
    assert!(!card.scope_paths.is_empty());
    assert!(card.allowance.is_some());
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    value["scope"] = serde_json::json!(["src"]);
    assert!(serde_json::from_value::<graphhelm_policy::keel::Card>(value).is_err());
}

#[test]
fn a_card_may_name_its_journeys_and_an_old_card_still_reads() {
    let old =
        std::fs::read(package_root().join("fixtures/keel-card/valid/card-full.json")).unwrap();
    let old: graphhelm_policy::keel::Card = serde_json::from_slice(&old).unwrap();
    assert!(old.journeys.is_empty());
    let new =
        std::fs::read(package_root().join("fixtures/keel-card/valid/card-journeys.json")).unwrap();
    let new: graphhelm_policy::keel::Card = serde_json::from_slice(&new).unwrap();
    assert_eq!(new.journeys, ["checkout"]);
}

#[test]
fn a_touched_screen_without_a_fresh_capture_is_a_warning_and_never_blocks() {
    use graphhelm_policy::keel::{JourneyScreen, check_journeys};
    let screen = |step: &str, scope: &str, not_fresh: Option<&str>| JourneyScreen {
        contract_id: "checkout".into(),
        step_id: step.into(),
        scope_paths: vec![scope.into()],
        not_fresh: not_fresh.map(str::to_owned),
    };
    let screens = [
        screen(
            "cart",
            "web/cart/",
            Some("no capture of this step was read"),
        ),
        screen("pay", "web/pay/", Some("stale")),
        screen("home", "web/home/", None),
    ];
    let findings = check_journeys(&["web/cart/Line.tsx".into(), "web/home".into()], &screens);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].rule, "keel.journey.no_fresh_capture");
    assert!(!findings[0].blocking);
    assert_eq!(findings[0].path.as_deref(), Some("web/cart/Line.tsx"));
    assert_eq!(
        findings[0].detail,
        "checkout/cart: no capture of this step was read"
    );
    assert_eq!(check_journeys(&["web".into()], &screens).len(), 2);
}

#[test]
fn a_body_without_a_filled_identity_line_blocks_at_the_card_path() {
    use graphhelm_policy::keel::{BodySource, check_identity_line};
    let filled = "Session: Author [2ba4e9] (keel-author subagent) \u{b7} Head: 0123abcd\n";
    assert!(check_identity_line(filled, BodySource::PullRequestBody).is_empty());
    assert!(
        check_identity_line(
            &format!("- **{}**", filled.trim()),
            BodySource::PullRequestBody
        )
        .is_empty()
    );
    for body in [
        "Promise: x\nCloses #1\n",
        "Session: <session name> [ref] \u{b7} Head: <sha8>\n",
        "Session: <session name> \u{b7} Head: 0123abcd\n",
        "Session: Author [2ba4e9] \u{b7} Head: <sha8 of your head>\n",
        "Session: Author \u{b7} Head: 0123abc\n",
    ] {
        let findings = check_identity_line(body, BodySource::PullRequestBody);
        assert_eq!(findings.len(), 1, "{body}");
        assert_eq!(findings[0].rule, "keel.card.identity_line_missing");
        assert_eq!(findings[0].path.as_deref(), Some("/card"));
        assert!(findings[0].blocking);
        // A markdown card might not be a body: the same text only warns.
        let findings = check_identity_line(body, BodySource::MarkdownCard);
        assert!(!findings[0].blocking);
    }
}

#[test]
fn a_body_without_a_filled_skills_line_warns_at_the_card_path() {
    for body in [
        "Skills: keel:keel, keel:keel-exec\n",
        "- **Skills:** keel:keel\n",
        "\u{feff}Skills: keel:keel\n",
        "## Skills usadas\n\n- keel:keel\n",
        "### Skills used\nkeel:keel\n",
        "Skills usadas: keel:keel\n",
        "Skills:\n- keel:keel\n",
        "Skills: keel:keel -> keel:keel-exec\n",
    ] {
        assert!(
            graphhelm_policy::keel::check_skills_line(body).is_empty(),
            "{body}"
        );
    }
    for body in [
        "Promise: x\nCloses #1\n",
        "Skills:\n",
        "Skills: <the skills loaded, in order>\n",
        "## Skills usadas\n\n## Next\n",
        "## Skills usadas\n",
        "Session: A [2ba4e9] · Head: 0123abcd\nSkills:\nAdvisor: not available\n",
        "Skills used by authors vary.\n",
    ] {
        let findings = graphhelm_policy::keel::check_skills_line(body);
        assert_eq!(findings.len(), 1, "{body}");
        assert_eq!(findings[0].rule, "keel.card.skills_line_missing");
        assert_eq!(findings[0].path.as_deref(), Some("/card"));
        assert!(!findings[0].blocking);
    }
}

#[test]
fn a_declared_skill_without_a_loaded_record_warns_with_its_name_in_the_path() {
    use graphhelm_policy::keel::{check_skills_loaded, declared_skills};
    let declared = declared_skills(
        "Session: A [2ba4e9] · Head: 0123abcd\nSkills: `keel:keel`, implement > keel-exec (via the Skill tool)\nAdvisor: not available\n",
    );
    assert_eq!(declared, ["keel:keel", "implement", "keel-exec"]);
    assert_eq!(
        declared_skills("## Skills usadas\n\n- keel:keel\n- keel:keel-exec\n\nRollback: x\n"),
        ["keel:keel", "keel:keel-exec"]
    );
    assert!(declared_skills("Skills: none\n").is_empty());

    let loaded = history_of(
        &[("2ba4e9", "keel:keel", 1), ("2ba4e9", "keel:implement", 2)],
        &[],
        &[],
    );
    let lane = Some("2ba4e9");
    // Declared and loaded: exact, or bare against a namespaced record.
    assert!(check_skills_loaded(&declared[..2], &loaded, lane, NO_TASK).is_empty());
    // Declared and not loaded: one warning per skill, the name in the path.
    let findings = check_skills_loaded(&declared, &loaded, lane, NO_TASK);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, "keel.card.skill_not_loaded");
    assert_eq!(findings[0].path.as_deref(), Some("/card/skills/keel-exec"));
    assert!(!findings[0].blocking);
    // A namespaced declaration is not satisfied by another namespace.
    let other = ["graphhelm:keel"].map(String::from).to_vec();
    assert_eq!(check_skills_loaded(&other, &loaded, lane, NO_TASK).len(), 1);
}

/// A history of `(actor, skill, sequence)` records, `(actor, sequence, issue)` claims and
/// `(actor, sequence, head sha)` pull request openings; each record's session is the actor's name
/// and the hundred its sequence falls in, so two lanes' sessions never coincide.
fn history_of(
    records: &[(&str, &str, u64)],
    claims: &[(&str, u64, Option<u64>)],
    opens: &[(&str, u64, &str)],
) -> graphhelm_policy::keel::SkillHistory {
    use graphhelm_policy::keel::{PrOpened, SkillHistory, SkillRecord, TaskClaim};
    SkillHistory {
        records: records
            .iter()
            .map(|(actor, name, sequence)| SkillRecord {
                name: (*name).into(),
                actor: (*actor).into(),
                session: Some(format!("session-of-{actor}-{}", sequence / 100)),
                agent: None,
                sequence: *sequence,
            })
            .collect(),
        claims: claims
            .iter()
            .map(|(actor, sequence, issue)| TaskClaim {
                actor: (*actor).into(),
                sequence: *sequence,
                issue: *issue,
            })
            .collect(),
        pr_opens: opens
            .iter()
            .map(|(actor, sequence, head_sha)| PrOpened {
                actor: (*actor).into(),
                sequence: *sequence,
                head_sha: (*head_sha).into(),
            })
            .collect(),
        actors: records
            .iter()
            .map(|r| r.0)
            .chain(claims.iter().map(|c| c.0))
            .chain(opens.iter().map(|o| o.0))
            .map(String::from)
            .collect(),
    }
}

const NO_TASK: graphhelm_policy::keel::TaskRef<'static> = graphhelm_policy::keel::TaskRef {
    head: None,
    issues: &[],
};

/// Contract (#3037): a declared skill counts only when the lane the identity line names loaded it
/// during this task. Regression: the union over every lane and day passed any skill anyone ever
/// loaded.
#[test]
fn a_declared_skill_counts_only_when_the_named_lane_loaded_it_during_this_task() {
    use graphhelm_policy::keel::{check_skills_loaded, declared_skills};
    let declared = declared_skills("Skills: keel, keel-exec\n");
    // Another lane loaded both; the named lane has records of its own for something else.
    let other_lane = history_of(
        &[
            ("author", "keel:keel", 10),
            ("reviewer", "keel:keel", 11),
            ("reviewer", "keel:keel-exec", 12),
        ],
        &[],
        &[],
    );
    let findings = check_skills_loaded(&declared, &other_lane, Some("author"), NO_TASK);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].path.as_deref(), Some("/card/skills/keel-exec"));
    // Loaded after the claim: no finding.
    let during = history_of(
        &[
            ("author", "keel:keel", 51),
            ("author", "keel:keel-exec", 52),
        ],
        &[("author", 50, None)],
        &[],
    );
    assert!(check_skills_loaded(&declared, &during, Some("author"), NO_TASK).is_empty());
    // A claim by another lane does not move this lane's start.
    let foreign_claim = history_of(
        &[
            ("author", "keel:keel", 10),
            ("author", "keel:keel-exec", 11),
        ],
        &[("reviewer", 50, None)],
        &[],
    );
    assert!(check_skills_loaded(&declared, &foreign_claim, Some("author"), NO_TASK).is_empty());
}

/// Contract (#3039): skills a lane loads in one session just before it claims the task count for
/// that task. Regression: the start was the claim itself, so every one of them was reported.
#[test]
fn skills_loaded_just_before_the_claim_count_for_the_task() {
    use graphhelm_policy::keel::{check_skills_loaded, declared_skills};
    let declared = declared_skills("Skills: keel, keel-exec\n");
    let history = history_of(
        &[
            ("author", "keel:keel", 10),
            ("author", "keel:keel-exec", 11),
        ],
        &[("author", 50, None)],
        &[],
    );
    assert!(check_skills_loaded(&declared, &history, Some("author"), NO_TASK).is_empty());
}

/// Contract (#3039): the task is named by the pull request's head sha and the issues the body
/// closes, so a later claim of the same lane does not take this task's loads away. Regression:
/// the newest claim of the lane was the start.
#[test]
fn a_later_claim_of_the_same_lane_does_not_hide_this_tasks_loads() {
    use graphhelm_policy::keel::{TaskRef, check_skills_loaded, declared_skills};
    let declared = declared_skills("Skills: keel, keel-exec\n");
    // The measured shape B: loads, claim, pull request opened, then another task in a new session.
    let history = history_of(
        &[
            ("author", "keel:keel", 10),
            ("author", "keel:keel-exec", 11),
            ("author", "keel:keel", 210),
        ],
        &[("author", 50, Some(7)), ("author", 300, Some(8))],
        &[("author", 60, "0123abcd")],
    );
    let by_head = TaskRef {
        head: Some("0123abcd99999999999999999999999999999999"),
        issues: &[],
    };
    assert!(check_skills_loaded(&declared, &history, Some("author"), by_head).is_empty());
    // With no pull request record, the issue the body closes names the claim.
    let by_issue = TaskRef {
        head: Some("ffffffff"),
        issues: &[7],
    };
    assert!(check_skills_loaded(&declared, &history, Some("author"), by_issue).is_empty());
}

/// Contract (#3039): the loads of one session before the claim all count, however long they took
/// and whatever time the branch's base commit has. Regression: a commit-time bound cut the session
/// in two (measured shape C).
#[test]
fn a_session_loading_in_two_stretches_before_the_claim_counts_whole() {
    use graphhelm_policy::keel::{TaskRef, check_skills_loaded, declared_skills};
    let declared = declared_skills("Skills: keel, keel-exec, graphhelm-work\n");
    let history = history_of(
        &[
            ("author", "keel:keel", 10),
            ("author", "keel:keel-exec", 20),
            ("author", "graphhelm:graphhelm-work", 30),
        ],
        &[("author", 50, Some(7))],
        &[],
    );
    let task = TaskRef {
        head: Some("0123abcd"),
        issues: &[7],
    };
    assert!(check_skills_loaded(&declared, &history, Some("author"), task).is_empty());
}

/// Contract (#3037): a lane whose only records belong to another task is told the skill was not
/// loaded: its claims name other issues, or the pull request opened is another head.
#[test]
fn a_lane_with_only_another_tasks_records_is_flagged() {
    use graphhelm_policy::keel::{TaskRef, check_skills_loaded, declared_skills};
    let declared = declared_skills("Skills: keel\n");
    let history = history_of(
        &[("author", "keel:keel", 10)],
        &[("author", 50, Some(7))],
        &[("author", 60, "aaaaaaaa")],
    );
    let task = TaskRef {
        head: Some("bbbbbbbb"),
        issues: &[8],
    };
    let findings = check_skills_loaded(&declared, &history, Some("author"), task);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].rule, "keel.card.skill_not_loaded");
    // The same lane, the same records, and the body closes the issue it claimed: it counts.
    let own = TaskRef {
        head: Some("bbbbbbbb"),
        issues: &[7],
    };
    assert!(check_skills_loaded(&declared, &history, Some("author"), own).is_empty());
}

/// Contract (#3039): a pull request opened by the lane does not make the lane's newest earlier
/// claim this task's claim when that claim names another issue. Regression (measured shape A): the
/// head sha match took an old claim of another issue, so loads made days later for other work
/// counted and the finding went away.
#[test]
fn a_matching_head_does_not_adopt_a_claim_of_another_issue() {
    use graphhelm_policy::keel::{TaskRef, check_skills_loaded, declared_skills};
    let declared = declared_skills("Skills: keel, keel-exec\n");
    let history = history_of(
        &[
            ("author", "keel:keel", 310),
            ("author", "keel:keel-exec", 315),
        ],
        &[("author", 5, Some(2389))],
        &[
            ("author", 312, "11111111"),
            ("author", 320, "22222222"),
            ("author", 330, "33333333"),
            ("author", 340, "0123abcd"),
        ],
    );
    let task = TaskRef {
        head: Some("0123abcd99999999999999999999999999999999"),
        issues: &[2557],
    };
    let findings = check_skills_loaded(&declared, &history, Some("author"), task);
    assert_eq!(findings.len(), 2, "{findings:?}");
    // The body closes the issue the lane claimed: the claim is this task's, and the loads count.
    let own = TaskRef {
        head: task.head,
        issues: &[2389],
    };
    assert!(check_skills_loaded(&declared, &history, Some("author"), own).is_empty());
}

/// A body's closing keywords name the issues; other numbers do not.
#[test]
fn closing_issues_reads_the_closing_keywords_only() {
    use graphhelm_policy::keel::closing_issues;
    assert_eq!(
        closing_issues("Fixes #12\nSee #99\ncloses: #3, resolved #4"),
        vec![12, 3, 4]
    );
    assert!(closing_issues("Promise: see #5").is_empty());
}

/// Contract (#3037): with no `task.claimed` record the task starts with the lane's newest session.
#[test]
fn without_a_claim_the_task_starts_with_the_lanes_newest_session() {
    use graphhelm_policy::keel::{check_skills_loaded, declared_skills};
    let declared = declared_skills("Skills: keel, keel-exec\n");
    // `history_of` gives sequences 10 and 250 different sessions: only the newest session counts.
    let history = history_of(
        &[
            ("author", "keel:keel", 10),
            ("author", "keel:keel-exec", 250),
        ],
        &[],
        &[],
    );
    let findings = check_skills_loaded(&declared, &history, Some("author"), NO_TASK);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].path.as_deref(), Some("/card/skills/keel"));
}

/// Contract (#3037): an identity ref no event carries, or no ref at all, is reported once under
/// its own code and no skill is called unloaded; a ref is read as the last `[...]` before `Head:`.
#[test]
fn an_identity_ref_with_no_record_is_reported_not_passed() {
    use graphhelm_policy::keel::{check_skills_loaded, declared_skills, identity_ref};
    let history = history_of(&[("author", "keel:keel", 10)], &[], &[]);
    let declared = declared_skills("Skills: keel\n");
    for lane in [Some("ghost"), None] {
        let findings = check_skills_loaded(&declared, &history, lane, NO_TASK);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, "keel.card.identity_ref_unknown");
        assert_eq!(findings[0].path.as_deref(), Some("/card/identity"));
        assert!(!findings[0].blocking);
    }
    assert_eq!(
        identity_ref(
            "Session: Ronda (zuvemo) [local_143fe79a] (keel-author subagent) · Head: 0123abcd\n"
        )
        .as_deref(),
        Some("local_143fe79a")
    );
    assert_eq!(identity_ref("Session: Author · Head: 0123abcd\n"), None);
}

/// Contract: only the names the author declares as loaded are returned. Regression: prose after
/// the list on the same line, or a "not used" line under the heading, became skill names.
#[test]
fn declared_skills_skips_prose_after_the_list_and_a_not_used_line() {
    use graphhelm_policy::keel::{check_skills_loaded, declared_skills};
    assert_eq!(
        declared_skills(
            "## Skills usadas\nkeel, graphhelm-implement, keel-exec, journey-prove, graphhelm-work (all loaded with the Skill tool in this task). Sem replay: o fluxo é rascunho.\n"
        ),
        [
            "keel",
            "graphhelm-implement",
            "keel-exec",
            "journey-prove",
            "graphhelm-work"
        ]
    );
    let body_two = "## Skills usadas\n`keel`, `graphhelm-implement`, `keel-exec`, `test-audit`, `modern-web-guidance`, `graphhelm-work`\nNão usadas nesta tarefa: `journey-prove` (OBSERVER_MISSING: jornada), `task-plan` (plano na issue).\n";
    let declared = declared_skills(body_two);
    assert_eq!(
        declared,
        [
            "keel",
            "graphhelm-implement",
            "keel-exec",
            "test-audit",
            "modern-web-guidance",
            "graphhelm-work"
        ]
    );
    assert_eq!(
        declared_skills("## Skills used\nkeel\nNot used: task-plan\n"),
        ["keel"]
    );
    assert_eq!(
        declared_skills("## Skills usadas\nkeel\nNão carregadas: task-plan\n"),
        ["keel"]
    );
    // A sentence ends only its own line: a later list item is still declared.
    assert_eq!(
        declared_skills("## Skills usadas\n- keel (first).\n- made-up-skill (second).\n"),
        ["keel", "made-up-skill"]
    );
    // A list marker or an abbreviation is not a sentence end: the unloaded name is still declared.
    for body in [
        "## Skills used\n1. keel\n2. made-up-skill\n",
        "Skills: e.g. keel, made-up-skill\n",
    ] {
        let names = declared_skills(body);
        assert!(names.iter().any(|name| name == "keel"), "{body}");
        assert!(names.iter().any(|name| name == "made-up-skill"), "{body}");
    }
    // A listed name with no record keeps firing, known or not.
    let loaded = history_of(&[("2ba4e9", "keel:keel", 1)], &[], &[]);
    let findings = check_skills_loaded(
        &declared_skills("Skills: keel, made-up-skill\n"),
        &loaded,
        Some("2ba4e9"),
        NO_TASK,
    );
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, "keel.card.skill_not_loaded");
    assert_eq!(
        findings[0].path.as_deref(),
        Some("/card/skills/made-up-skill")
    );
}
