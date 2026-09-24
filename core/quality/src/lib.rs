//! The deterministic quality evaluators (M06 Task 3): a spec-derived content manifest and
//! a layout grammar over rendered HTML — geometry, never prose (binding decision 4).
//!
//! Two rules shape everything here:
//! - **Builder-authored text is stripped before anything is SCORED** ([`strip_authored_text`]):
//!   free text can never persuade an evaluator — the sentinel test pins that a page stuffed
//!   with praise scores identically to one without. Manifest checks LOOK UP spec-provided
//!   strings (trusted input, exact match); they never grade authored prose.
//! - **Geometry alone must not gate**: the layout grammar cannot see a gutted test or an
//!   empty diff, so certification (the thymus, M06 Task 2) is earned by the COMPOSED
//!   evaluator over the whole delivered surface — pinned by a test that certifies the
//!   composition and REFUSES the grammar alone.
//!
//! The crate is pure: no clock, no entropy, no IO — inputs arrive whole, findings come out.

use std::collections::BTreeSet;

use graphhelm_protocols::{GateFinding, SignalSeverity};

/// Everything the evaluators may inspect about a delivered feature — the same information
/// surface the pathogen suite models, owned here so `core` never depends on `tools`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Delivered {
    pub claims: Vec<DeliveredClaim>,
    pub html: String,
    /// Element ids reachable by navigation from the root.
    pub reachable_ids: BTreeSet<String>,
    pub journey: Vec<JourneyStep>,
    pub tests: Vec<TestCheck>,
    pub diff: DiffShape,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeliveredClaim {
    /// What the spec says exists — also the label the element renders.
    pub feature: String,
    pub element_id: Option<String>,
    pub artifact: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JourneyStep {
    pub action: String,
    pub assertion: Option<String>,
    pub exercises_error_path: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TestCheck {
    pub name: String,
    pub passed: bool,
    pub assertions: u32,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiffShape {
    pub files_touched: u32,
    pub behavior_lines: u32,
}

/// The spec-derived content manifest: what MUST be present and reachable. Markers are
/// literal substrings of the rendered HTML (spec-trusted, exact lookup).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContentManifest {
    pub required: Vec<RequiredElement>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequiredElement {
    /// A literal marker that must appear in the HTML (an `id="..."` attribute or a
    /// structural tag sequence).
    pub marker: String,
    /// The spec's name for it — findings cite this, never page prose.
    pub label: String,
}

impl ContentManifest {
    /// The manifest a claim list implies: every claim naming an element requires that
    /// element's id marker.
    #[must_use]
    pub fn from_claims(claims: &[DeliveredClaim]) -> Self {
        Self {
            required: claims
                .iter()
                .filter_map(|claim| {
                    claim.element_id.as_ref().map(|id| RequiredElement {
                        marker: format!("id=\"{id}\""),
                        label: claim.feature.clone(),
                    })
                })
                .collect(),
        }
    }
}

/// The layout grammar's budgets — thresholds are configuration, the checks are the grammar.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct LayoutBudget {
    /// Most list/row items one section may hold before it is noise, not information.
    pub max_items_per_section: usize,
    /// Minimum WCAG contrast ratio for every declared foreground against the page ground.
    pub min_contrast: f64,
}

impl Default for LayoutBudget {
    fn default() -> Self {
        Self {
            max_items_per_section: 120,
            min_contrast: 3.0,
        }
    }
}

fn finding(severity: SignalSeverity, claim: String, remediation: String) -> GateFinding {
    GateFinding {
        severity,
        claim,
        evidence: Vec::new(),
        remediation,
    }
}

/// Strips every authored text node, keeping tags and attributes — what the layout grammar
/// scores. Persuasion dies here; geometry survives.
#[must_use]
pub fn strip_authored_text(html: &str) -> String {
    let mut stripped = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_style = false;
    for character in html.chars() {
        match character {
            '<' => {
                in_tag = true;
                stripped.push(character);
            }
            '>' => {
                in_tag = false;
                stripped.push(character);
                // A stylesheet is geometry declaration, not prose: keep its body.
                if stripped.ends_with("</style>") {
                    in_style = false;
                } else if let Some(open) = stripped.rfind("<style")
                    && !stripped[open..].contains("</")
                {
                    in_style = true;
                }
            }
            _ if in_tag || in_style => stripped.push(character),
            _ => {}
        }
    }
    stripped
}

/// The content manifest check: presence, reachability, label binding, claim backing,
/// journey grounding, test honesty, and diff substance — the delivery-coherence half.
#[must_use]
pub fn check_content(delivered: &Delivered, manifest: &ContentManifest) -> Vec<GateFinding> {
    let mut findings = Vec::new();

    // A rendered surface with no content at all is its own finding, before any lookup.
    let stripped = strip_authored_text(&delivered.html);
    if !stripped.contains("<table") && !stripped.contains("<li") && !stripped.contains("<section") {
        findings.push(finding(
            SignalSeverity::Critical,
            "the rendered surface carries no content structure at all".to_owned(),
            "render the delivered feature; a blank screen delivers nothing".to_owned(),
        ));
    }

    for required in &manifest.required {
        if !delivered.html.contains(&required.marker) {
            findings.push(finding(
                SignalSeverity::High,
                format!(
                    "required element {:?} ({}) never renders",
                    required.label, required.marker
                ),
                "render the element the spec names, where a user can see it".to_owned(),
            ));
        }
    }

    for claim in &delivered.claims {
        if claim.artifact.is_none() {
            findings.push(finding(
                SignalSeverity::High,
                format!("claim {:?} is backed by no artifact", claim.feature),
                "attach the artifact that proves the claim or drop the claim".to_owned(),
            ));
        }
        if let Some(id) = &claim.element_id {
            let marker = format!("id=\"{id}\"");
            if delivered.html.contains(&marker) && !delivered.reachable_ids.contains(id) {
                findings.push(finding(
                    SignalSeverity::High,
                    format!("element {id:?} renders but is unreachable from the root"),
                    "link the element into navigation; unreachable is undelivered".to_owned(),
                ));
            }
            // Label binding: the element's region must carry its OWN spec label, not a
            // sibling's (spec-provided strings, exact lookup — never scored prose).
            if let Some(region) = element_region(&delivered.html, id) {
                let carries_own = region.contains(&claim.feature);
                let carries_other = delivered
                    .claims
                    .iter()
                    .any(|other| other.feature != claim.feature && region.contains(&other.feature));
                if !carries_own && carries_other {
                    findings.push(finding(
                        SignalSeverity::High,
                        format!("element {id:?} carries another feature's label"),
                        "bind each element to its own spec label".to_owned(),
                    ));
                }
            }
        }
    }

    // Journey grounding: every assertion must reference something delivered.
    for step in &delivered.journey {
        if let Some(assertion) = &step.assertion {
            let grounded = delivered
                .claims
                .iter()
                .any(|claim| assertion.contains(&claim.feature))
                || delivered
                    .reachable_ids
                    .iter()
                    .any(|id| assertion.contains(id.as_str()));
            if !grounded {
                findings.push(finding(
                    SignalSeverity::Medium,
                    format!(
                        "journey step {:?} asserts nothing the delivery contains",
                        step.action
                    ),
                    "assert against a delivered element or claim".to_owned(),
                ));
            }
        }
    }
    if !delivered.journey.is_empty()
        && !delivered
            .journey
            .iter()
            .any(|step| step.exercises_error_path)
    {
        findings.push(finding(
            SignalSeverity::Medium,
            "the journey never leaves the happy path".to_owned(),
            "exercise at least one error or refusal path".to_owned(),
        ));
    }

    for test in &delivered.tests {
        if test.passed && test.assertions == 0 {
            findings.push(finding(
                SignalSeverity::Critical,
                format!("test {:?} passes while asserting nothing", test.name),
                "restore the assertions; a green test with no teeth is a pathogen".to_owned(),
            ));
        }
    }
    if delivered.diff.files_touched > 0 && delivered.diff.behavior_lines == 0 {
        findings.push(finding(
            SignalSeverity::High,
            "the change touches files without changing behavior".to_owned(),
            "deliver behavior or deliver nothing".to_owned(),
        ));
    }

    findings
}

/// The layout grammar over STRIPPED html: empty populated sections, density budgets,
/// declared-contrast pairs, section link-orphans, and row-shape variance. Geometry only —
/// certified solely as part of the composed evaluator (see the crate doc's second rule).
#[must_use]
pub fn check_layout(html: &str, budget: &LayoutBudget) -> Vec<GateFinding> {
    let stripped = strip_authored_text(html);
    let mut findings = Vec::new();

    // Sections: <section id=...> blocks and <h2>-delimited runs both count.
    for (name, body) in sections(&stripped) {
        let items = body.matches("<li").count() + body.matches("<tr").count();
        if items == 0 && !body.contains("<table") && !body.contains("<ul") {
            findings.push(finding(
                SignalSeverity::High,
                format!("section {name:?} renders empty"),
                "populate the section or do not render it".to_owned(),
            ));
        }
        if items > budget.max_items_per_section {
            findings.push(finding(
                SignalSeverity::Medium,
                format!(
                    "section {name:?} holds {items} items, over the {} budget",
                    budget.max_items_per_section
                ),
                "page or summarize; density past the budget is noise".to_owned(),
            ));
        }
        if let Some(id) = name.strip_prefix("section:")
            && !stripped.contains(&format!("href=\"#{id}\""))
        {
            findings.push(finding(
                SignalSeverity::Medium,
                format!("view {id:?} renders but nothing links to it"),
                "link the view from navigation or retire it".to_owned(),
            ));
        }
    }

    // Row-shape variance: every row of one table carries the same cell count.
    for table in stripped.split("<table").skip(1) {
        let table = table.split("</table>").next().unwrap_or("");
        let mut widths = BTreeSet::new();
        for row in table.split("<tr").skip(1) {
            let row = row.split("</tr>").next().unwrap_or("");
            widths.insert(row.matches("<td").count() + row.matches("<th").count());
        }
        if widths.len() > 1 {
            findings.push(finding(
                SignalSeverity::Medium,
                format!("table rows disagree on cell count ({widths:?})"),
                "align every row to one column shape".to_owned(),
            ));
        }
    }

    // Declared contrast: every foreground color in the style block against the page
    // ground (declared background or white). Only hex forms — the grammar reads what the
    // page declares, it does not compute a render.
    let ground = declared_colors(&stripped, "background")
        .into_iter()
        .next()
        .unwrap_or((255.0, 255.0, 255.0));
    for foreground in declared_colors(&stripped, "color") {
        let ratio = contrast_ratio(foreground, ground);
        if ratio < budget.min_contrast {
            findings.push(finding(
                SignalSeverity::High,
                format!(
                    "a declared foreground reaches contrast {ratio:.2}, under the {:.1} floor",
                    budget.min_contrast
                ),
                "raise the contrast of the declared pair".to_owned(),
            ));
        }
    }

    findings
}

/// The composed evaluator — the ONLY shape the thymus certifies: delivery coherence AND
/// geometry together, over the whole delivered surface.
#[must_use]
pub fn evaluate_geometry(
    delivered: &Delivered,
    manifest: &ContentManifest,
    budget: &LayoutBudget,
) -> Vec<GateFinding> {
    let mut findings = check_content(delivered, manifest);
    findings.extend(check_layout(&delivered.html, budget));
    findings
}

/// `(name, body)` per section: `<section id="X">` blocks (`section:X`) and `<h2>`-runs
/// (`h2:N`, body until the next h2/section).
fn sections(stripped: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (index, chunk) in stripped.split("<h2>").skip(1).enumerate() {
        let body = chunk
            .split("<h2>")
            .next()
            .unwrap_or("")
            .split("<section")
            .next()
            .unwrap_or("");
        out.push((format!("h2:{index}"), body.to_owned()));
    }
    for chunk in stripped.split("<section id=\"").skip(1) {
        let id = chunk.split('"').next().unwrap_or("").to_owned();
        let body = chunk.split("</section>").next().unwrap_or("").to_owned();
        out.push((format!("section:{id}"), body));
    }
    out
}

fn element_region<'a>(html: &'a str, id: &str) -> Option<&'a str> {
    let start = html.find(&format!("id=\"{id}\""))?;
    Some(&html[start..html.len().min(start + 400)])
}

/// Every `property: #hex` declaration in the style block, as linear-ish RGB.
fn declared_colors(stripped: &str, property: &str) -> Vec<(f64, f64, f64)> {
    let mut colors = Vec::new();
    for chunk in stripped.split(&format!("{property}:")).skip(1) {
        let value = chunk.trim_start();
        if let Some(hex) = value.strip_prefix('#') {
            let hex: String = hex
                .chars()
                .take_while(|character| character.is_ascii_hexdigit())
                .collect();
            let rgb = match hex.len() {
                3 => hex
                    .chars()
                    .map(|c| u8::from_str_radix(&format!("{c}{c}"), 16).ok())
                    .collect::<Option<Vec<_>>>(),
                6 => (0..3)
                    .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok())
                    .collect::<Option<Vec<_>>>(),
                _ => None,
            };
            if let Some(rgb) = rgb
                && rgb.len() == 3
            {
                colors.push((f64::from(rgb[0]), f64::from(rgb[1]), f64::from(rgb[2])));
            }
        }
    }
    colors
}

/// WCAG relative-luminance contrast between two sRGB colors.
fn contrast_ratio(a: (f64, f64, f64), b: (f64, f64, f64)) -> f64 {
    fn luminance((r, g, b): (f64, f64, f64)) -> f64 {
        fn channel(value: f64) -> f64 {
            let value = value / 255.0;
            if value <= 0.040_45 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
    }
    let (light, dark) = {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb), la.min(lb))
    };
    (light + 0.05) / (dark + 0.05)
}

/// The gate-freeze rule (M06 binding decision 5) as a pure check over a changed-path
/// list: a change touching the GATE MACHINERY (this crate, the pathogen suite, the freeze
/// charter under `docs/gates/`, or the shared source-invariant predicate, which IS a gate
/// rather than an input to one) together with anything OUTSIDE it is a hard violation — the
/// judge and the judged never move in one PR. Returns the offending pair for the refusal
/// message; `None` is a clean diff.
///
/// This doc used to describe the `docs/gates/` entry as "a stream's gate stamps". Measured
/// (#282): no stamp was ever written there — certification is a `GateCertified` EVENT in the
/// stream, not a file — and the directory was empty until the charter moved in. A description
/// of contents that never existed is how an empty prefix survives review looking deliberate.
#[must_use]
pub fn freeze_violation(changed_paths: &[&str]) -> Option<(String, String)> {
    const GATE_MACHINERY: [&str; 4] = [
        "core/quality/",
        "tools/pathogens/",
        "docs/gates/",
        "tools/source-invariants/",
    ];
    // THE GATE'S OWN RECEIPT IS NEITHER SIDE (#898). #674(a) made every authoritative run commit
    // its manifest under this prefix onto the branch it judged, so from a branch's second run on
    // the store was always in its diff. Read as gated code, that receipt paired with any gate
    // path -- every `core/quality/` or `tools/pathogens/` branch went RED at the freeze cell
    // from its second run, and no gate-machinery change could carry the GREEN manifest
    // `merge-proof` (retired 2026-09-24) required. The rule was condemning its own receipt.
    // Receipts are no longer committed, but `ci/gate.ps1` still writes its manifests under this
    // prefix, so the exemption stays. The exemption is this prefix and nothing wider: a
    // non-manifest file under `.factory/` still counts as code.
    const RUN_MANIFEST_STORE: &str = ".factory/gate-runs/";
    let is_gate = |path: &str| GATE_MACHINERY.iter().any(|prefix| path.starts_with(prefix));
    let is_receipt = |path: &str| path.starts_with(RUN_MANIFEST_STORE);
    let gate_side = changed_paths.iter().find(|path| is_gate(path))?;
    let code_side = changed_paths
        .iter()
        .find(|path| !is_gate(path) && !is_receipt(path))?;
    Some(((*gate_side).to_owned(), (*code_side).to_owned()))
}

/// The evidence needed to decide whether one gate manifest explains a lockfile delta.
/// The caller must provide the exact base and current bytes; an absent or unparsable side is
/// deliberately a refusal rather than an implicit clean result.
#[derive(Clone, Copy, Debug)]
pub struct GateManifestChange<'a> {
    pub path: &'a str,
    pub base: &'a str,
    pub current: &'a str,
}

/// Applies the freeze rule with the one narrow lockfile exception.
///
/// `Cargo.lock` is neutral only when it is the sole outside path (run receipts are still
/// exempt), both lockfiles are structurally parseable, package identity and all non-gate
/// records are unchanged, and every changed gate package dependency list is explained by a
/// same-diff gate manifest dependency change. This keeps the public path-only rule fail-closed
/// for callers that do not have exact base/current evidence.
#[must_use]
pub fn freeze_violation_with_lockfile(
    changed_paths: &[&str],
    base_lockfile: &str,
    current_lockfile: &str,
    manifests: &[GateManifestChange<'_>],
) -> Option<(String, String)> {
    let violation = freeze_violation(changed_paths)?;
    if violation.1 != "Cargo.lock"
        || changed_paths
            .iter()
            .filter(|path| !is_gate_path(path) && !is_run_receipt(path))
            .count()
            != 1
    {
        return Some(violation);
    }

    if !lockfile_delta_is_attributable(base_lockfile, current_lockfile, manifests, changed_paths) {
        return Some(violation);
    }
    None
}

fn is_gate_path(path: &str) -> bool {
    freeze_violation(&[path, "README.md"]).is_some()
}

fn is_run_receipt(path: &str) -> bool {
    path.starts_with(".factory/gate-runs/")
}

#[derive(Debug, PartialEq, Eq)]
struct LockPackage {
    fields: std::collections::BTreeMap<String, String>,
    dependencies: BTreeSet<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct Lockfile {
    metadata: String,
    packages: std::collections::BTreeMap<String, LockPackage>,
}

fn lockfile_delta_is_attributable(
    base: &str,
    current: &str,
    manifests: &[GateManifestChange<'_>],
    changed_paths: &[&str],
) -> bool {
    let Some(base_lock) = parse_lockfile(base) else {
        return false;
    };
    let Some(current_lock) = parse_lockfile(current) else {
        return false;
    };
    if base_lock.metadata != current_lock.metadata
        || base_lock.packages.keys().ne(current_lock.packages.keys())
    {
        return false;
    }

    let manifest_paths: BTreeSet<&str> = manifests.iter().map(|manifest| manifest.path).collect();
    let changed_manifest_paths: BTreeSet<&str> = changed_paths
        .iter()
        .copied()
        .filter(|path| is_gate_path(path) && path.ends_with("Cargo.toml"))
        .collect();
    if manifest_paths != changed_manifest_paths || manifests.is_empty() {
        return false;
    }

    let mut allowed_dependency_changes = std::collections::BTreeMap::new();
    for manifest in manifests {
        let Some(base_manifest) = parse_manifest(manifest.base) else {
            return false;
        };
        let Some(current_manifest) = parse_manifest(manifest.current) else {
            return false;
        };
        if base_manifest.name != current_manifest.name {
            return false;
        }
        let added: BTreeSet<String> = current_manifest
            .dependencies
            .difference(&base_manifest.dependencies)
            .cloned()
            .collect();
        let removed: BTreeSet<String> = base_manifest
            .dependencies
            .difference(&current_manifest.dependencies)
            .cloned()
            .collect();
        allowed_dependency_changes.insert(current_manifest.name, (added, removed));
    }

    for (name, base_package) in &base_lock.packages {
        let Some(current_package) = current_lock.packages.get(name) else {
            return false;
        };
        let Some(package_name) = base_package.fields.get("name").and_then(|value| {
            value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
        }) else {
            return false;
        };
        let Some((allowed_added, allowed_removed)) = allowed_dependency_changes.get(package_name)
        else {
            if base_package != current_package {
                return false;
            }
            continue;
        };
        if base_package.fields != current_package.fields {
            return false;
        }
        let actual_added: BTreeSet<String> = current_package
            .dependencies
            .difference(&base_package.dependencies)
            .cloned()
            .collect();
        let actual_removed: BTreeSet<String> = base_package
            .dependencies
            .difference(&current_package.dependencies)
            .cloned()
            .collect();
        if actual_added != *allowed_added || actual_removed != *allowed_removed {
            return false;
        }
    }
    true
}

fn parse_lockfile(text: &str) -> Option<Lockfile> {
    let lines: Vec<&str> = text.lines().collect();
    let first_package = lines.iter().position(|line| line.trim() == "[[package]]")?;
    let metadata = lines[..first_package]
        .iter()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n");
    if metadata
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .any(|line| !line.contains('='))
    {
        return None;
    }

    let mut packages = std::collections::BTreeMap::new();
    let mut index = first_package;
    while index < lines.len() {
        if lines[index].trim() != "[[package]]" {
            return None;
        }
        index += 1;
        let mut fields = std::collections::BTreeMap::new();
        let mut dependencies = BTreeSet::new();
        let mut saw_dependencies = false;
        let mut name = None;
        while index < lines.len() && lines[index].trim() != "[[package]]" {
            let line = lines[index].trim();
            index += 1;
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line.split_once('=')?;
            let key = key.trim().to_owned();
            let value = value.trim();
            if key == "dependencies" {
                if saw_dependencies {
                    return None;
                }
                saw_dependencies = true;
                let mut raw = value.to_owned();
                if !raw.starts_with('[') {
                    return None;
                }
                while !raw.contains(']') {
                    let next = lines.get(index)?.trim();
                    index += 1;
                    raw.push_str(next);
                }
                let end = raw.rfind(']')?;
                if !raw[end + 1..].trim().is_empty() {
                    return None;
                }
                let items: Vec<&str> = raw[1..end].split(',').collect();
                for (position, item) in items.iter().enumerate() {
                    let item = item.trim();
                    if item.is_empty() {
                        if position == items.len() - 1 {
                            continue;
                        }
                        return None;
                    }
                    let item = item.strip_prefix('"')?.strip_suffix('"')?;
                    if item.is_empty() {
                        return None;
                    }
                    if !dependencies.insert(item.to_owned()) {
                        return None;
                    }
                }
                continue;
            }
            if key.is_empty() || fields.insert(key.clone(), value.to_owned()).is_some() {
                return None;
            }
            if key == "name" {
                name = Some(value.strip_prefix('"')?.strip_suffix('"')?.to_owned());
            }
        }
        let name = name?;
        let version = fields.get("version")?;
        let package_key = format!("{name}\0{version}");
        if packages
            .insert(
                package_key,
                LockPackage {
                    fields,
                    dependencies,
                },
            )
            .is_some()
        {
            return None;
        }
    }
    Some(Lockfile { metadata, packages })
}

#[derive(Debug)]
struct Manifest {
    name: String,
    dependencies: BTreeSet<String>,
}

fn parse_manifest(text: &str) -> Option<Manifest> {
    let mut package = false;
    let mut dependency_section = false;
    let mut name = None;
    let mut dependencies = BTreeSet::new();
    for line in text.lines() {
        let line = line.split('#').next()?.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            let header = line.strip_prefix('[')?.strip_suffix(']')?;
            package = header == "package";
            dependency_section = header == "dependencies"
                || header == "dev-dependencies"
                || header == "build-dependencies"
                || header.ends_with(".dependencies");
            continue;
        }
        let (key, value) = line.split_once('=')?;
        let key = key.trim();
        let value = value.trim();
        if package && key == "name" {
            name = Some(value.strip_prefix('"')?.strip_suffix('"')?.to_owned());
        } else if dependency_section {
            let dep_name = key.trim_matches('"').to_owned();
            if dep_name.is_empty() {
                return None;
            }
            let canonical = value
                .split("package = \"")
                .nth(1)
                .and_then(|tail| tail.split('"').next())
                .unwrap_or(&dep_name);
            dependencies.insert(canonical.to_owned());
        }
    }
    Some(Manifest {
        name: name?,
        dependencies,
    })
}
