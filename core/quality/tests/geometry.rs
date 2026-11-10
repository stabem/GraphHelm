//! M06 Task 3: the evaluators over the REAL monitor page (the committed 05f acceptance
//! snapshot — a genuine render, checksummed since the day it happened), plus the sentinel
//! that pins "geometry, never prose".

use std::collections::BTreeSet;
use std::path::Path;

use graphhelm_quality::{
    ContentManifest, Delivered, DeliveredClaim, DiffShape, LayoutBudget, RequiredElement,
    check_content, check_layout, strip_authored_text,
};

fn real_monitor_page() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/acceptance/m05-run-2026-08-16/monitor-snapshot.html");
    std::fs::read_to_string(path).expect("the committed acceptance snapshot is readable")
}

/// The monitor's spec-derived manifest: the three sections D-040 names, as structural
/// markers (spec-trusted lookups, not scored prose).
fn monitor_manifest() -> ContentManifest {
    ContentManifest {
        required: vec![
            RequiredElement {
                marker: "<h2>nodes</h2>".to_owned(),
                label: "node table".to_owned(),
            },
            RequiredElement {
                marker: "<h2>triage</h2>".to_owned(),
                label: "triage list".to_owned(),
            },
            RequiredElement {
                marker: "<h2>events".to_owned(),
                label: "event tail".to_owned(),
            },
        ],
    }
}

fn monitor_delivered(html: String) -> Delivered {
    Delivered {
        claims: Vec::new(),
        html,
        reachable_ids: BTreeSet::new(),
        journey: Vec::new(),
        tests: Vec::new(),
        diff: DiffShape {
            files_touched: 1,
            behavior_lines: 1,
        },
    }
}

#[test]
fn the_real_monitor_page_satisfies_its_manifest_and_a_blanked_section_fails() {
    let page = real_monitor_page();
    let manifest = monitor_manifest();

    let clean = check_content(&monitor_delivered(page.clone()), &manifest);
    assert!(
        clean.is_empty(),
        "the genuine 05f render satisfies its own spec manifest: {clean:?}"
    );

    // Blank the nodes section: the manifest catches the hole by name.
    let blanked = page.replace("<h2>nodes</h2>", "");
    let findings = check_content(&monitor_delivered(blanked), &manifest);
    assert!(
        findings
            .iter()
            .any(|f| f.claim.contains("node table") && f.claim.contains("never renders")),
        "a blanked section is a named finding: {findings:?}"
    );
}

#[test]
fn the_layout_grammar_passes_the_real_render_and_fails_a_broken_one() {
    let page = real_monitor_page();
    let budget = LayoutBudget::default();

    let clean = check_layout(&page, &budget);
    assert!(
        clean.is_empty(),
        "the genuine render clears the grammar: {clean:?}"
    );

    // Break it three ways: a washed-out declared color, a ragged table row, an empty
    // h2 section — each is its own named finding.
    let broken = page
        .replace(".t{color:#777", ".t{color:#eee")
        .replace("</table>", "<tr><td>x</td></tr></table>")
        + "<h2>ghost section</h2><p></p>";
    let findings = check_layout(&broken, &budget);
    assert!(
        findings.iter().any(|f| f.claim.contains("contrast")),
        "washed-out contrast is caught: {findings:?}"
    );
    assert!(
        findings.iter().any(|f| f.claim.contains("cell count")),
        "row-shape variance is caught: {findings:?}"
    );
    assert!(
        findings.iter().any(|f| f.claim.contains("renders empty")),
        "an empty section is caught: {findings:?}"
    );
}

#[test]
fn background_color_is_the_ground_and_never_a_foreground() {
    // #2745: `color:` also matched inside `background-color:`, and `background:` never matched
    // `background-color:`, so a dark ground was scored as a foreground against white.
    let page = "<style>body{background-color:#111;color:#eee}</style><body></body>";
    let findings = check_layout(page, &LayoutBudget::default());
    assert!(
        findings.iter().all(|f| !f.claim.contains("contrast")),
        "light text on a dark declared ground clears the floor: {findings:?}"
    );

    let washed = "<style>body{background-color:#111;color:#222}</style><body></body>";
    let findings = check_layout(washed, &LayoutBudget::default());
    assert_eq!(
        findings
            .iter()
            .filter(|f| f.claim.contains("contrast"))
            .count(),
        1,
        "exactly the one declared foreground is scored: {findings:?}"
    );
}

#[test]
fn a_multibyte_character_at_the_region_edge_does_not_panic() {
    // #2745: the label region was cut at a fixed byte offset, which can fall inside a character.
    let claim = |feature: &str, id: &str| DeliveredClaim {
        feature: feature.to_owned(),
        element_id: Some(id.to_owned()),
        artifact: None,
    };
    // The region starts at `id="a"` (byte 5) and was cut 400 bytes later, at byte 405; the
    // 2-byte character below occupies bytes 404 and 405.
    let html = format!("<div id=\"a\">{}é</div>", "x".repeat(392));
    let mut delivered = monitor_delivered(html);
    delivered.claims = vec![claim("alpha", "a"), claim("beta", "b")];
    delivered.reachable_ids = ["a".to_owned()].into_iter().collect();
    let _ = check_content(&delivered, &ContentManifest::from_claims(&[]));
}

#[test]
fn praise_stuffing_changes_nothing_the_grammar_scores() {
    let page = real_monitor_page();
    let budget = LayoutBudget::default();
    // The sentinel (binding decision 4): builder-authored prose is stripped before
    // scoring, so a page stuffed with persuasion scores IDENTICALLY.
    let stuffed = page.replace(
        "<body>",
        "<body>excellent beautiful perfect flawless world-class ",
    );
    assert_eq!(
        format!("{:?}", check_layout(&page, &budget)),
        format!("{:?}", check_layout(&stuffed, &budget)),
        "praise must not move a single finding"
    );
    assert!(
        !strip_authored_text(&stuffed).contains("excellent"),
        "the strip removes authored text wholesale"
    );
}
