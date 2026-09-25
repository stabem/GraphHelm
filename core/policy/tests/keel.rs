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
    assert_eq!(policy.version, "1.1.0");
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
    assert_eq!(c.policy_version, "1.1.0");
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
