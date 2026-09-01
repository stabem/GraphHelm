//! The prover's blind spot, pinned in both directions (#632, D-049).
//!
//! `one_of_branches_are_provably_disjoint` decides some unions and not others, and the ones it
//! cannot decide are where an additive change reads as breaking. That set is a real property of the
//! shipped schemas, so it is written down here rather than rediscovered by whoever next adds a
//! branch and finds the gate demanding a major bump.
//!
//! **NONE OF THE BOUNDS IN THIS FILE IS EXERCISED BY A RED CELL, and that is a trade rather than
//! an oversight.** Reddening them would need a catalog past `MAX_RESOURCE_BYTES`, a union of 512+
//! branches, a 64-deep schema, or a corpus charging a million nodes of prover work -- fixtures this
//! repository does not contain and which are not worth manufacturing for a test-only scan. What
//! holds them up is the enumeration of the limits and the measurement of the subject against each:
//! 15 catalog entries of 256, 80 KB of 32 MB, 44 branches of 512, depth 9 of 64, 420,104 weighted units of
//! 4,194,304. Every one is a harness guard sized to a measured subject, and not one has been seen
//! to fire.
//!
//! The seal lives HERE and not only in the commit messages, because the audience for it is whoever
//! opens this file and sees six asserts with nothing saying which of them has ever been observed
//! to work (L, review of #646).
//!
//! **Both directions, because a one-directional pin is half a guard.** A union that becomes
//! unprovable is the gap GROWING in silence. A pinned union that becomes provable is a stale pin --
//! debt claimed that no longer exists, which is how a document starts lying about its own code.

use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use graphhelm_schema_evolution::{
    MAX_FILE_BYTES, MAX_RESOURCE_BYTES, MAX_SCHEMAS, SchemaCatalog,
    one_of_branches_are_provably_disjoint,
};
use serde_json::Value;

const LIVE_CATALOG: &str = "schemas/catalog.json";

/// Every union this prover cannot decide today, each carrying WHY it cannot.
///
/// **Four classes, and they are not interchangeable** — a maintainer reading one rationale for an
/// entry that belongs to another class attempts a widening that cannot work (Codex on #634, twice).
/// Measured at `0fcb8ab0`: **A=7, B=4, C=1, D=1**.
///
/// **The classes are assigned by POSITIVE predicate, and that is the second correction.** The first
/// version of this taxonomy read `else -> B`, so B was a residual bucket: it absorbed distinct
/// causes and reported them as one, which is the exact defect the taxonomy was written to fix. The
/// `node` union sat in it wrongly and was found immediately. A union matching no predicate must be
/// marked `?` and investigated, never folded into the nearest neighbour.
///
/// - **A — `X`-or-`null` with a `$ref` branch.** Genuinely disjoint; the prover sees nothing to
///   compare because a `$ref` carries no `type` of its own, and it deliberately does not resolve
///   references. A `$ref` hop would decide these.
/// - **B — constraints inherited from the enclosing schema.** Genuinely disjoint, but the branches
///   declare no `type` and no `required` of their own: what separates them lives one level up, or
///   behind a `$ref`, or both. `event-envelope`'s top-level union is the case D-049 is written
///   about, and the reopening door named there is the only path that decides these.
/// - **C — DELIBERATELY NOT DISJOINT.** `agent`'s union is
///   `[{"required":["instructions"]},{"required":["instructionsRef"]}]` — the standard "exactly one
///   of" idiom, where an instance carrying BOTH fields satisfies both branches and `oneOf` rejects
///   it on purpose. **No widening will ever move this entry**, because there is nothing true to
///   prove. It is pinned because the prover cannot decide it, not because anything is wrong.
/// - **D — disjoint by EXCLUSION.** `node`'s union: both branches declare their own `type` and
///   `required`, and what separates them is the second branch's `additionalProperties: false`,
///   which forbids the property the first branch requires. Nothing is inherited and no `$ref` is in
///   the way, so neither of the remedies above touches it; the proof it needs is one this prover
///   does not model at all — required-here versus closed-and-absent-there. Arguably the most
///   tractable of the four, and the reason it must not sit under B's rationale.
///
/// The class tag is DOCUMENTATION verified at a named commit, not an assertion: the set equality
/// below compares schema and pointer only. A tag that drifts is a comment that lies, so it is
/// marked as one rather than dressed up as a guard.
const UNPROVABLE_UNIONS: &[(&str, &str, &str)] = &[
    ("agent", "/oneOf", "C"),
    ("event-envelope", "/oneOf", "B"),
    (
        "event-envelope",
        "/$defs/nodeStateChanged/properties/previousState/oneOf",
        "A",
    ),
    (
        "event-envelope",
        "/$defs/executionModeChanged/properties/previousMode/oneOf",
        "A",
    ),
    (
        "execution-accounting-receipt",
        "/$defs/costField/oneOf",
        "B",
    ),
    (
        "execution-accounting-receipt",
        "/$defs/costField/properties/producer/oneOf",
        "A",
    ),
    (
        "execution-accounting-receipt",
        "/$defs/providerReportedInputField/allOf/1/oneOf",
        "B",
    ),
    (
        "execution-accounting-receipt",
        "/$defs/outputField/allOf/1/oneOf",
        "B",
    ),
    ("node", "/properties/agent/oneOf", "D"),
    (
        "persisted-graph-version",
        "/properties/predecessor/oneOf",
        "A",
    ),
    (
        "persisted-graph-version",
        "/$defs/edge/properties/priority/oneOf",
        "A",
    ),
    (
        "persisted-graph-version",
        "/$defs/edge/properties/condition/oneOf",
        "A",
    ),
    ("policy-waiver", "/properties/expiresAt/oneOf", "A"),
];
/// Read a catalog-named file the way the catalog LOADER does, not the way a test usually does.
///
/// The paths below come out of `schemas/catalog.json`, which this test treats as input. `catalog.rs`
/// bounds exactly these reads by [`MAX_FILE_BYTES`] and rejects paths that are not schema paths —
/// and in the authoritative gate the workspace tests run BEFORE the schema-catalog stage, so an
/// entry pointing at an endless device or an enormous file would hang or exhaust the run *before*
/// the stage that exists to reject it ever executes (Codex on #634).
///
/// `take` rather than a metadata length check, because the two failures are different: a large file
/// is caught by size, and an endless one reports length zero and would pass that check while reading
/// forever.
fn read_catalog_file(root: &Path, relative: &str) -> Vec<u8> {
    // The check is on the CATALOG-SUPPLIED half, not on the joined path: `repository_root()` is
    // built as `CARGO_MANIFEST_DIR/../..` and legitimately contains parent hops of its own. The
    // first version of this assertion looked at the join and fired on the test harness rather than
    // on untrusted input -- a guard whose subject was one level too wide.
    let candidate = Path::new(relative);
    assert!(
        candidate.is_relative()
            && !candidate
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir)),
        "catalog path is not a contained relative path: {relative}"
    );
    let path = root.join(relative);
    let file = fs::File::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut bytes = Vec::new();
    std::io::Read::take(file, MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert!(
        bytes.len() <= MAX_FILE_BYTES,
        "{} exceeds MAX_FILE_BYTES; the catalog stage would have refused it, but this test runs \
         first and would have read it",
        path.display()
    );
    bytes
}

/// The sweep's WORK budget, in weighted units, and the only bound here whose completeness does not
/// depend on someone having enumerated the prover's inputs. 10x the measured subject.
const MAX_WORK: usize = 4_194_304;

/// What one branch costs to compare: its nodes PLUS the bytes of its scalars and keys.
///
/// Nodes alone were a proxy -- an 8 KiB string charged as 1, while comparing it costs 8 KiB. Keys
/// count because the prover intersects property names. Chosen because it cannot be made incomplete
/// by the prover learning a new field: whatever it reads is a subset of this.
fn branch_weight(value: &Value) -> usize {
    match value {
        Value::Object(map) => {
            1 + map
                .iter()
                .map(|(key, child)| key.len() + branch_weight(child))
                .sum::<usize>()
        }
        Value::Array(values) => 1 + values.iter().map(branch_weight).sum::<usize>(),
        Value::String(text) => 1 + text.len(),
        _ => 1,
    }
}

/// Recursion depth. The walk recurses in three places and had no depth parameter at all, which is
/// the FOURTH sibling of the bound this file keeps rediscovering -- path containment, file size,
/// count and cardinality, and now the stack (L, review of #646). Deepest path in the shipped
/// catalog is 9; 64 is seven times that and still far under any stack this test could reach.
const MAX_DEPTH: usize = 64;

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Collect `(schema, pointer, verdict)` for every `oneOf` site in the live catalog.
fn union_sites() -> Vec<(String, String, bool)> {
    let root = repository_root();
    let catalog: SchemaCatalog =
        serde_json::from_slice(&read_catalog_file(&root, LIVE_CATALOG)).unwrap();
    // THE PER-FILE BOUND DOES NOT BOUND THE SWEEP. A catalog whose every entry is under
    // `MAX_FILE_BYTES` can still name more entries than `MAX_SCHEMAS` or sum past
    // `MAX_RESOURCE_BYTES`, and this loop would read and walk all of them -- the ceiling defeated by
    // composition, one call at a time. `catalog.rs` applies both limits; this scan runs BEFORE the
    // stage that does, so it applies them itself (Codex on #634).
    assert!(
        catalog.schemas.len() <= MAX_SCHEMAS,
        "catalog names {} schemas, beyond MAX_SCHEMAS ({MAX_SCHEMAS}); the catalog stage would \
         refuse it, and this scan runs first",
        catalog.schemas.len()
    );
    let mut budget = MAX_RESOURCE_BYTES;
    let mut work = 0_usize;
    let mut sites = Vec::new();
    for (name, entry) in &catalog.schemas {
        let bytes = read_catalog_file(&root, &entry.path);
        budget = budget.checked_sub(bytes.len()).unwrap_or_else(|| {
            panic!(
                "{} pushes the catalog's files past MAX_RESOURCE_BYTES ({MAX_RESOURCE_BYTES}); the \
                 per-file bound does not bound the sweep",
                entry.path
            )
        });
        let document: Value = serde_json::from_slice(&bytes).unwrap();
        walk(&document, String::new(), name, &mut sites, &mut work, 0);
    }
    sites.sort();
    sites
}

/// RFC 6901 escaping, `~` before `/` because the order is the whole rule.
///
/// Without it two different locations produce the same pointer -- a `$defs` entry named
/// `x/$defs/y` and a nested `x` -> `y` -- and since the inventory is a SET, one of them hides
/// under the other's pin. A union going unprovable behind an existing entry is the gap growing in
/// silence, which is the one thing this file exists to catch.
fn escape_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

fn walk(
    node: &Value,
    pointer: String,
    schema: &str,
    sites: &mut Vec<(String, String, bool)>,
    work: &mut usize,
    depth: usize,
) {
    assert!(
        depth <= MAX_DEPTH,
        "{schema}{pointer} nests deeper than {MAX_DEPTH}; the walk recurses and nothing else \
         bounds the stack"
    );
    match node {
        Value::Object(map) => {
            if let Some(Value::Array(branches)) = map.get("oneOf") {
                // BOUND THE PAIRS, NOT JUST THE BYTES. The prover is O(n^2) over branches, and a
                // file well under `MAX_FILE_BYTES` holds tens of thousands of compact ones -- so a
                // size cap bounds the input and not the work (Codex on #634).
                //
                // 512 is 11.6x the largest union in the population this walk actually visits: the
                // 15 entries of `schemas/catalog.json`, whose biggest `oneOf` is 44 branches
                // (`event-envelope` at both `/oneOf` and `$defs/eventKind`). ONE order of
                // magnitude, not two -- an earlier version of this comment claimed two and was
                // wrong by 10x (L, review of #646). The population is named because a ratio
                // without one is a number nobody can re-derive.
                // NO LONGER THE BINDING CONSTRAINT, AND THAT IS DELIBERATE. A maximal union at
                // this cap would charge ~53,896,192 against a work budget of 4,194,304, so the
                // budget stops it first and this assert is a local message naming WHICH union was
                // absurd rather than a bound that holds. A bound that no longer holds is exactly
                // what someone simplifies away in six months, so it says so out loud (L, #646).
                const MAX_BRANCHES: usize = 512;
                assert!(
                    branches.len() <= MAX_BRANCHES,
                    "{schema}{pointer}/oneOf has {} branches, beyond {MAX_BRANCHES}; the prover is \
                     quadratic in this number and the file-size bound does not see it",
                    branches.len()
                );
                // AND BOUND THE SWEEP, because a per-union cap leaves the total unbounded: 512
                // branches is 130,816 comparisons, and nothing limited how many SITES could each
                // spend that. The cap turned "unlimited by branches" into "unlimited by sites"
                // (Codex on #646) -- the same composition defeat, one level out, for the fourth
                // time in this file. Measured today: 1,907 comparisons across 15 sites.
                // CHARGE THE WORK, NOT THE FIELDS, AND WEIGH EACH BRANCH ONCE.
                //
                // The seventh axis arrived as a `type` array uncounted by a cap on `required` and
                // `properties`. The prover reads `required`, `properties`, `const` and `type`
                // TODAY (measured); a charge derived from that list is correct today and silently
                // wrong the next time it learns a field, because the bound's completeness would
                // rest on somebody having imagined every input. So the charge is STRUCTURAL: an
                // upper bound on any traversal the prover can perform, whatever it reads.
                //
                // TWO THINGS THE FIRST VERSION GOT WRONG, both found in review and both fixed by
                // the same edit rather than two:
                //
                // 1. THE MEASURER COST MORE THAN WHAT IT MEASURED. Weighing inside the pair loop
                //    re-walks every branch once per PAIR -- 261,632 traversals at the branch cap,
                //    before the assert can fire. A guard whose own cost is unbounded in the input
                //    it guards rebuilds the problem it exists to bound. Weighed once now, and the
                //    pairs charged by arithmetic: every branch appears in exactly `n - 1` pairs,
                //    so the sum over pairs of `w[i] + w[j]` is `(n - 1) * sum(w)`. Exact, and O(n).
                //
                // 2. THE UNIT WAS STILL A PROXY. A node count charges an 8 KiB string as 1. The
                //    weight now carries scalar BYTES and key lengths, which is what a comparison
                //    actually touches.
                //
                // Measured on the 15 catalog entries: worst branch weight 206, and
                // 420,104 charged across the whole sweep against a budget of 4,194,304 -- 10x, and the
                // bound that BINDS FIRST: a maximal union at the branch cap would charge roughly
                // 54,000,000, so the work budget stops it long before the branch count does, which
                // is what makes it the load-bearing one rather than a fourth opinion.
                // NEVER TRAVERSE WHAT YOU WILL NOT CHARGE (L, review of #646 -- the third finding
                // about this measurer, and the one that names the class). With fewer than two
                // branches there are no pairs, so the charge is `0 * sum` = 0 -- and the weighing
                // happened anyway. Nothing in this corpus is a singleton today, but the legal worst
                // case is one at every level of `MAX_DEPTH`, re-weighing subtrees up to
                // `MAX_FILE_BYTES`: 268,435,456 visits, every one billed at zero. A structural
                // budget bounds only the work that passes THROUGH it, so the defect is always the
                // traversal that goes around it.
                if branches.len() >= 2 {
                    let weights: Vec<usize> = branches.iter().map(branch_weight).collect();
                    let pairs_per_branch = weights.len() - 1;
                    *work += weights
                        .iter()
                        .sum::<usize>()
                        .saturating_mul(pairs_per_branch);
                }
                assert!(
                    *work <= MAX_WORK,
                    "the sweep has charged {work} units of prover work, beyond {MAX_WORK}; this is \
                     the one bound whose completeness does not depend on someone having listed the \
                     prover's inputs"
                );
                sites.push((
                    schema.to_owned(),
                    format!("{pointer}/oneOf"),
                    one_of_branches_are_provably_disjoint(branches),
                ));
            }
            for (key, value) in map {
                let child = format!("{pointer}/{}", escape_token(key));
                match key.as_str() {
                    // INSTANCE-VALUED: the payload under these is an example value, not a schema.
                    // A `{"oneOf": [...]}` sitting there is data that happens to share a name.
                    "default" | "examples" | "const" | "enum" => {}
                    // NAME-KEYED SCHEMA MAPS: every value is a schema and every KEY is an
                    // arbitrary name, so no key inside them is a keyword. This is what keeps a
                    // property honestly named `default` from being skipped as one.
                    "properties" | "$defs" | "definitions" | "patternProperties"
                    | "dependentSchemas" => {
                        if let Value::Object(named) = value {
                            for (name, subschema) in named {
                                walk(
                                    subschema,
                                    format!("{child}/{}", escape_token(name)),
                                    schema,
                                    sites,
                                    work,
                                    depth + 1,
                                );
                            }
                        }
                    }
                    _ => walk(value, child, schema, sites, work, depth + 1),
                }
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                walk(
                    value,
                    format!("{pointer}/{index}"),
                    schema,
                    sites,
                    work,
                    depth + 1,
                );
            }
        }
        _ => {}
    }
}

/// The blind spot is exactly the pinned set -- no larger, and no smaller.
#[test]
fn the_unprovable_unions_are_exactly_the_pinned_ones() {
    let sites = union_sites();

    // Arrangement control: a pin read against an empty scan passes vacuously in one direction and
    // fails confusingly in the other, so the scan proves it found something first.
    assert!(
        sites.len() > UNPROVABLE_UNIONS.len(),
        "the scan found {} union sites, not more than the {} pinned -- the walk is not reaching \
         the schemas and neither direction below means anything",
        sites.len(),
        UNPROVABLE_UNIONS.len()
    );

    let found: BTreeSet<(String, String)> = sites
        .iter()
        .filter(|(_, _, provable)| !provable)
        .map(|(schema, pointer, _)| (schema.clone(), pointer.clone()))
        .collect();
    let pinned: BTreeSet<(String, String)> = UNPROVABLE_UNIONS
        .iter()
        .map(|(schema, pointer, _class)| ((*schema).to_owned(), (*pointer).to_owned()))
        .collect();

    let grew: Vec<_> = found.difference(&pinned).collect();
    assert!(
        grew.is_empty(),
        "a union the prover cannot decide is NOT in the pin: {grew:?}. Either reshape the change \
         so the prover can decide it, or grow the pin and D-049 together -- deliberately, with the \
         reason written beside the entry."
    );

    let stale: Vec<_> = pinned.difference(&found).collect();
    assert!(
        stale.is_empty(),
        "a PINNED union is now provable: {stale:?}. The pin claims debt that no longer exists, \
         which makes D-049 a lie about this code. Remove the entry."
    );
}

/// The pin is about the prover, so the prover must be able to say yes.
///
/// Without this, every entry above could be explained by a helper that answers "unprovable" to
/// everything, and the pin would be a list of nothing in particular.
#[test]
fn the_prover_decides_the_unions_that_are_not_pinned() {
    let sites = union_sites();
    let provable: Vec<_> = sites
        .iter()
        .filter(|(_, _, provable)| *provable)
        .map(|(schema, pointer, _)| format!("{schema}{pointer}"))
        .collect();

    assert!(
        !provable.is_empty(),
        "no union in the catalog is provable, so the helper may be answering no to everything and \
         the pin above would mean nothing"
    );
    assert!(
        provable.iter().any(|site| site.contains("eventKind")),
        "the tagged union this prover was built for is not among the ones it decides: {provable:?}"
    );
}

/// Instance-valued payloads are DATA, not schema locations (Codex P2 on #634).
///
/// A schema may legally carry `{"oneOf": [...]}` inside `default`, `examples`, `const` or `enum` --
/// there it is an example value, not a union. Counting it inflates the denominator this pin
/// depends on and demands a D-049 entry for something that is not a union at all.
///
/// **The remedy chosen is "skip the instance-valued keywords", NOT "traverse only the
/// schema-valued ones".** Both were offered in review and they fail in opposite directions: an
/// enumeration of schema keywords that misses one hides a real union SILENTLY, which is the
/// failure this pin exists to prevent, while a list of data keywords that misses one merely counts
/// an extra site loudly and someone goes and looks.
#[test]
fn instance_valued_payloads_are_not_scanned_as_schema_locations() {
    let tagged = serde_json::json!({
        "oneOf": [
            {"type": "object", "required": ["type"], "properties": {"type": {"const": "a"}}},
            {"type": "object", "required": ["type"], "properties": {"type": {"const": "b"}}},
        ]
    });
    let payload = serde_json::json!({"oneOf": [{}, {}]});
    let document = serde_json::json!({
        "type": "object",
        "properties": {"tagged": tagged},
        "default": payload.clone(),
        "examples": [payload.clone()],
        "const": payload.clone(),
        "enum": [payload],
    });

    let mut sites = Vec::new();
    walk(&document, String::new(), "fixture", &mut sites, &mut 0, 0);

    let pointers: Vec<&str> = sites
        .iter()
        .map(|(_, pointer, _)| pointer.as_str())
        .collect();
    assert_eq!(
        pointers,
        vec!["/properties/tagged/oneOf"],
        "the walk counted an instance-valued payload as a union"
    );
}

/// A PROPERTY may be named `default`, and it is a schema wherever it sits under `properties`.
///
/// The trap for the fix above. Skipping the key `default` everywhere is the easy version and it
/// hides a real union the moment someone names a field after a keyword -- the silent direction,
/// and the one the whole pin exists to keep closed. The values under `properties`, `$defs`,
/// `patternProperties` and `dependentSchemas` are keyed by ARBITRARY names, so no key inside them
/// is a keyword at all.
#[test]
fn a_property_named_after_a_keyword_is_still_a_schema() {
    let union = serde_json::json!({
        "oneOf": [
            {"type": "object", "required": ["type"], "properties": {"type": {"const": "a"}}},
            {"type": "object", "required": ["type"], "properties": {"type": {"const": "b"}}},
        ]
    });
    let document = serde_json::json!({
        "type": "object",
        "properties": {"default": union.clone(), "enum": union.clone()},
        "$defs": {"examples": union},
    });

    let mut sites = Vec::new();
    walk(&document, String::new(), "fixture", &mut sites, &mut 0, 0);
    let mut pointers: Vec<&str> = sites
        .iter()
        .map(|(_, pointer, _)| pointer.as_str())
        .collect();
    pointers.sort_unstable();

    assert_eq!(
        pointers,
        vec![
            "/$defs/examples/oneOf",
            "/properties/default/oneOf",
            "/properties/enum/oneOf",
        ],
        "a union under a property named after a keyword went unseen"
    );
}

/// Two different locations must not produce the same pointer (Codex on #634).
///
/// The inventory is a set keyed by `(schema, pointer)`, so two locations that COLLIDE share one
/// entry -- and a union that becomes unprovable at one of them stays silently covered by the
/// other's pin. That is the gap growing unnoticed, which is the single thing this file exists to
/// prevent, so the collision matters far more than the pointers being well-formed.
///
/// RFC 6901 escapes `~` as `~0` and `/` as `~1`, in that order. A `$defs` entry literally named
/// `x/$defs/y` and a nested `x` -> `y` are different places and now read differently.
#[test]
fn map_names_are_escaped_so_distinct_locations_cannot_collide() {
    let union = serde_json::json!({
        "oneOf": [
            {"type": "object", "required": ["type"], "properties": {"type": {"const": "a"}}},
            {"type": "object", "required": ["type"], "properties": {"type": {"const": "b"}}},
        ]
    });
    let document = serde_json::json!({
        "$defs": {
            "x/$defs/y": union.clone(),
            "x": {"$defs": {"y": union.clone()}},
            "tilde~name": union,
        }
    });

    let mut sites = Vec::new();
    walk(&document, String::new(), "fixture", &mut sites, &mut 0, 0);
    let mut pointers: Vec<&str> = sites
        .iter()
        .map(|(_, pointer, _)| pointer.as_str())
        .collect();
    pointers.sort_unstable();

    assert_eq!(
        pointers,
        vec![
            "/$defs/tilde~0name/oneOf",
            "/$defs/x/$defs/y/oneOf",
            "/$defs/x~1$defs~1y/oneOf",
        ],
        "two distinct locations produced the same pointer, so one of them can hide under the \
         other's pin"
    );
    assert_eq!(
        pointers.len(),
        3,
        "three unions went in and fewer came out -- a collision ate one"
    );
}
