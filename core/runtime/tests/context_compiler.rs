//! Guards for Context Capsule compilation (#222).
//!
//! The constraint that forces this design was measured, not chosen: `schemas/context-capsule.schema.json`
//! declares `sections` as six arrays of bare strings with `additionalProperties: false` at every
//! level, and the capsule must stay byte-identical to its pinned release copy. **The capsule has no
//! per-item identity and cannot gain one.** Yet #222 requires every relied-on result to cite stable
//! capsule item IDs. So identity is derived from content and lives here, outside the capsule.

use graphhelm_runtime::context_compiler::item_id;

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before writing it: **deriving an item's ID
/// from its position** (`sections.evidence[3]`) instead of from its content.
///
/// A positional ID is stable only while nothing is inserted, removed, or reordered above it. The
/// capsule is recompiled deterministically but its *content* changes between versions, so a
/// citation recorded against position 3 silently comes to mean a different item. **A citation whose
/// target can shift does not dangle — it retargets, and nothing reports it.** That is worse than a
/// broken link: the accounting still resolves, and it resolves to the wrong evidence.
#[test]
fn an_item_id_follows_its_content_not_its_position() {
    let capsule = "cap-1";
    let version = 1;

    let at_position_zero = item_id(
        capsule,
        version,
        "evidence",
        0,
        "the wake path burns the lease",
    );
    let same_text_moved = item_id(
        capsule,
        version,
        "evidence",
        3,
        "the wake path burns the lease",
    );

    assert_eq!(
        at_position_zero, same_text_moved,
        "an item's ID changed when only its POSITION changed (#222).\n\
         The ID is derived from where the item sits rather than from what it says, so inserting a \
         line above it renames it. Every citation recorded against the old ID then points at \
         whatever moved into that slot -- it resolves, and it resolves to the wrong evidence.\n\
         Derive the ID from the content."
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: **deriving the ID from the item text alone**,
/// dropping the section or the capsule identity.
///
/// The same sentence can legitimately appear in two sections and mean two different things — an
/// evidence line and a task line are cited for different reasons. Collapsing them would let a
/// citation of one satisfy a required-citation check for the other.
#[test]
fn the_same_text_in_two_sections_gets_two_ids() {
    let text = "the wake path burns the lease";

    let as_evidence = item_id("cap-1", 1, "evidence", 0, text);
    let as_task = item_id("cap-1", 1, "task", 0, text);
    let other_capsule = item_id("cap-2", 1, "evidence", 0, text);

    assert_ne!(
        as_evidence, as_task,
        "the same text in two different sections shares one ID (#222).\n\
         An evidence line and a task line are cited for different reasons; if they share an ID, a \
         citation of one satisfies a required-citation check for the other."
    );
    assert_ne!(
        as_evidence, other_capsule,
        "the same text in two different capsules shares one ID (#222) -- citations would cross \
         capsule boundaries"
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: **letting the ID be re-derived differently from the
/// same inputs** — the property every citation check depends on.
///
/// Stated separately from the two above because it is the one a reader would assume rather than
/// verify: an ID that is content-derived and section-scoped is still useless if the derivation is
/// not a function.
#[test]
fn re_deriving_an_item_id_from_the_same_inputs_gives_the_same_id() {
    let first = item_id("cap-1", 1, "evidence", 0, "the wake path burns the lease");
    let again = item_id("cap-1", 1, "evidence", 0, "the wake path burns the lease");
    assert_eq!(
        first, again,
        "item ID derivation must be a function of its inputs"
    );
    assert!(!first.is_empty(), "an item ID must not be empty");
}

use graphhelm_runtime::context_compiler::compile_capsule;

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before writing it: **emitting sections in the
/// order the caller happened to supply them.**
///
/// The acceptance criterion is "same inputs produce byte-identical capsules", and the trap is that
/// "same inputs" for a keyed collection does not mean "same insertion order". A compiler that
/// preserves caller order is deterministic per-call and non-deterministic across callers — two
/// components assembling the same logical capsule produce different bytes, so the digest that binds
/// it differs, so the cache misses, so a second compilation is paid for evidence already held.
///
/// This is the BASE-capsule half of determinism. The delta half is a different guard: the criterion
/// says "capsules", not "delta capsules", and `context_compiler.rs` is where the from-scratch
/// compilation happens.
#[test]
fn the_same_sections_in_a_different_order_compile_to_the_same_bytes() {
    let one_order = vec![
        ("evidence".to_owned(), vec!["e1".to_owned()]),
        ("task".to_owned(), vec!["t1".to_owned()]),
    ];
    let other_order = vec![
        ("task".to_owned(), vec!["t1".to_owned()]),
        ("evidence".to_owned(), vec!["e1".to_owned()]),
    ];

    assert_ne!(
        one_order, other_order,
        "arrangement check: the two inputs must differ in ORDER, or this proves nothing"
    );

    let first = compile_capsule("cap-1", 1, &one_order);
    let second = compile_capsule("cap-1", 1, &other_order);

    assert_eq!(
        first, second,
        "the same capsule compiled to different BYTES because its sections arrived in a different \
         order (#222).\n\
         Determinism here is not per-call reproducibility -- it is that two components assembling \
         the same logical capsule get the same bytes. Otherwise the binding digest differs, the \
         cache misses, and the second caller pays again for evidence already compiled.\n\
         Emit sections in a fixed order, not the caller's."
    );

    // And the property everyone assumes without checking: repeating the same call is stable.
    assert_eq!(
        compile_capsule("cap-1", 1, &one_order),
        first,
        "recompiling identical inputs must be byte-stable"
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before writing it: **joining sections and
/// items with a bare separator instead of length-prefixing them.**
///
/// Found in review, and the sting is that this PR already prevents exactly this twice — `item_id`
/// and `push_segment` both length-prefix, each with the reason written above it. The rule was held
/// and did not fire on the third site.
///
/// Capsule sections are arrays of free text: evidence excerpts, task descriptions. A multi-line
/// item is ordinary, not exotic. With a bare `\n` separator, **one item containing a newline is
/// indistinguishable from two items split at that point.**
///
/// It matters here more than in the key: these bytes are what gets digested and what the binding
/// carries, so two different capsules share one identity. A cache hit can then return content
/// assembled from different evidence — the cache-poisoning threat this task's own threat model
/// names, arriving through the compiler instead of through the key. `capsules_identical` compares
/// these bytes and would call the two capsules the same, correctly, because by then they are.
#[test]
fn an_item_containing_a_newline_is_not_two_items() {
    let one_multiline_item = vec![("task".to_owned(), vec!["a\nb".to_owned()])];
    let two_separate_items = vec![("task".to_owned(), vec!["a".to_owned(), "b".to_owned()])];

    assert_ne!(
        one_multiline_item, two_separate_items,
        "arrangement check: the two inputs must actually differ, or this proves nothing"
    );

    assert_ne!(
        compile_capsule("cap-1", 1, &one_multiline_item),
        compile_capsule("cap-1", 1, &two_separate_items),
        "one item containing a newline compiled to the same BYTES as two items split there \
         (#222).\n\
         Sections hold free text, so multi-line items are ordinary. These bytes are what gets \
         digested and what the binding carries, so two different capsules would share one \
         identity and a cache hit could return content assembled from different evidence.\n\
         Length-prefix each item and section name, exactly as `item_id` and `push_segment` \
         already do in this same crate."
    );
}

// ---------------------------------------------------------------------------------------------
// Budget refusal. #222: "Required evidence may never be removed to satisfy a token target", and
// "insufficient required context yields CONTEXT_BUDGET_INSUFFICIENT plus an expansion request".
// The code was allocated in the first PR of this lane; this is its emitter.
// ---------------------------------------------------------------------------------------------

use graphhelm_protocols::DevelopmentRefusalCode;
use graphhelm_runtime::context_compiler::{BudgetOutcome, fit_within_budget};

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before writing it: **dropping required items
/// to make the total fit.**
///
/// That implementation succeeds. It returns a capsule, under budget, with every number looking
/// healthy — and the evidence the caller was required to see is simply not in it. **A refusal is
/// visible and a silent trim is not**, which is why the failure direction matters more than the
/// failure rate: the trimmed capsule flatters every metric this task reports.
///
/// The three assertions are separate on purpose. Asserting only "it refused" passes for an
/// implementation that refuses without saying how much more it needs, and asserting only the code
/// passes for one that never emits the expansion request. Each is a thing that can be missing on
/// its own.
#[test]
fn required_context_over_budget_refuses_instead_of_dropping_evidence() {
    let required = vec!["r1".repeat(20), "r2".repeat(20)];
    let optional = vec!["o1".repeat(20)];
    let budget = 10; // far below what required alone needs

    let outcome = fit_within_budget(&required, &optional, budget);

    let BudgetOutcome::Refused { code, expansion } = outcome else {
        panic!(
            "required context over budget produced a SUCCESS (#222).\n\
             Required evidence may never be removed to satisfy a token target. An implementation \
             that trims to fit returns a capsule, under budget, with every number healthy -- and \
             without the evidence the caller was required to see. A refusal is visible; a silent \
             trim is not."
        );
    };

    assert_eq!(
        code,
        DevelopmentRefusalCode::ContextBudgetInsufficient,
        "the refusal must carry the allocated code, not an adjacent one -- nothing here is \
         malformed, so `cardinality_violation` would fold two causes with opposite operator \
         responses into one"
    );

    assert!(
        expansion.required_budget > budget,
        "the refusal must carry an expansion request naming a budget that would actually fit \
         (#222). Refusing without saying how much more is needed leaves the operator with no move \
         -- and the acceptance criterion asks for the refusal AND the request, which are two \
         things that can go missing separately."
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: **refusing whenever anything does not fit**, rather
/// than only when REQUIRED context does not fit.
///
/// Written as the pair to the test above, because a refusal that fires for optional context too is
/// indistinguishable from the correct one on the failing case alone. Optional items are droppable
/// by design and dropping them is recorded, not silent.
#[test]
fn optional_context_over_budget_is_dropped_rather_than_refused() {
    let required = vec!["r".to_owned()];
    let optional = vec!["o".repeat(100)];
    let budget = 10; // fits required, not optional

    match fit_within_budget(&required, &optional, budget) {
        BudgetOutcome::Fits {
            included,
            dropped_optional,
        } => {
            assert!(
                included.contains(&"r".to_owned()),
                "required context must survive: it is the half that may never be trimmed"
            );
            assert_eq!(
                dropped_optional, 1,
                "dropping optional context is legitimate and must be COUNTED -- a drop nobody \
                 records is the same silence the required-item rule exists to prevent"
            );
        }
        BudgetOutcome::Refused { .. } => panic!(
            "optional context over budget produced a REFUSAL (#222).\n\
             Only required context is undroppable. Refusing here makes the refusal \
             indistinguishable from the required case and blocks work that should proceed with \
             optional context recorded as dropped."
        ),
    }
}

// ---------------------------------------------------------------------------------------------
// Citations. #222: "Every relied-on result cites stable capsule item IDs; missing required
// citations refuse." Threat assessment names citation spoofing alongside it.
// ---------------------------------------------------------------------------------------------

use graphhelm_runtime::context_compiler::{CitationVerdict, verify_citations};

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before writing it: **accepting a result that
/// cites nothing for a required item.**
///
/// An uncited result is not a result with a missing footnote. It is a claim whose evidence cannot
/// be located, and the accounting downstream will still count the capsule as used — so the
/// utilization number rises while nothing connects the answer to the evidence. The failure runs in
/// the direction of the metric's incentive, again.
#[test]
fn a_required_item_that_is_not_cited_refuses() {
    let capsule_ids = vec!["item-aaa".to_owned(), "item-bbb".to_owned()];
    let required = vec!["item-aaa".to_owned()];
    let cited: Vec<String> = vec!["item-bbb".to_owned()]; // cites something, just not the required one

    assert!(
        !cited.is_empty(),
        "arrangement check: the result must cite SOMETHING, or this only tests the empty case"
    );

    match verify_citations(&capsule_ids, &required, &cited) {
        CitationVerdict::Refused { missing, unknown } => {
            assert_eq!(
                missing,
                vec!["item-aaa".to_owned()],
                "the refusal must NAME the required item that went uncited -- a refusal that does \
                 not say which one leaves the caller to guess, and a guess is how the wrong item \
                 gets cited next"
            );
            assert!(
                unknown.is_empty(),
                "nothing was cited that the capsule does not contain, so `unknown` must be empty; \
                 folding the two lists would make one cause read as the other"
            );
        }
        CitationVerdict::Accepted => panic!(
            "a result that never cited a REQUIRED item was accepted (#222).\n\
             Downstream accounting still counts the capsule as used, so utilization rises while \
             nothing connects the answer to the evidence it was required to rely on."
        ),
    }
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: **accepting a citation whose ID is not in the
/// capsule.**
///
/// This is citation spoofing, which the issue's own threat assessment names. Item IDs are derived
/// from content, so an ID that no capsule item hashes to is either a typo or a fabrication — and
/// both must be refused for the same reason: a citation that resolves to nothing still *looks*
/// like provenance in every report that counts citations rather than checking them.
///
/// Kept separate from the missing-citation test because the two are different causes with
/// different fixes: one means evidence was not used, the other means the reference is invented.
#[test]
fn a_citation_naming_an_item_the_capsule_does_not_contain_refuses() {
    let capsule_ids = vec!["item-aaa".to_owned()];
    let required = vec!["item-aaa".to_owned()];
    let cited = vec!["item-aaa".to_owned(), "item-zzz".to_owned()];

    match verify_citations(&capsule_ids, &required, &cited) {
        CitationVerdict::Refused { missing, unknown } => {
            assert!(
                missing.is_empty(),
                "the required item WAS cited, so `missing` must be empty -- otherwise the verdict \
                 blames the wrong cause"
            );
            assert_eq!(
                unknown,
                vec!["item-zzz".to_owned()],
                "the refusal must name the citation that resolves to nothing"
            );
        }
        CitationVerdict::Accepted => panic!(
            "a citation naming an item the capsule does not contain was accepted (#222).\n\
             Item IDs are content-derived, so an ID nothing hashes to is a typo or a fabrication. \
             A citation that resolves to nothing still reads as provenance in every report that \
             counts citations instead of checking them."
        ),
    }
}

// ---------------------------------------------------------------------------------------------
// The compiler's section order against the schema that is supposed to be its authority.
//
// Raised by J against this lane's own code: `DECLARED_SECTION_ORDER` and the capsule schema's
// sections agree today with NOTHING linking them, and the const's doc comment claimed an
// authority nothing verified. A comment is a claim about the code; this is the guard.
//
// Measuring the schema first changed the remedy. It names the six sections in TWO places --
// `sections.required` (a JSON array) and `sections.properties` (a JSON object) -- and JSON Schema
// reads BOTH as sets: order carries no meaning to a validator. So there is no ordered authority in
// the schema to bind to, and binding emitted bytes to a file's key order would turn a
// semantically-no-op reorder into a different digest for every capsule ever compiled. The set is
// the schema's to own; the order is this compiler's, declared here and verified by the determinism
// guard above. Two sites also means the two can diverge FROM EACH OTHER, which a guard that reads
// only one of them passes straight through.
// ---------------------------------------------------------------------------------------------

fn capsule_schema() -> serde_json::Value {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/context-capsule.schema.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "arrangement check: cannot read the capsule schema at {} ({e}). If this fires the \
             test proves nothing about the schema -- fix the path, do not weaken the assertion.",
            path.display()
        )
    });
    serde_json::from_str(&text).expect("the capsule schema must be valid JSON")
}

/// The section names the schema declares, from each of its two declarations.
fn declared_sections(schema: &serde_json::Value) -> (Vec<String>, Vec<String>) {
    let sections = &schema["properties"]["sections"];
    let required: Vec<String> = sections["required"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let properties: Vec<String> = sections["properties"]
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    (required, properties)
}

/// Report the members present on one side and absent on the other, in both directions.
///
/// Returns `None` only when the two sides hold the same members. Used against the real schema and
/// against a deliberately divergent one, so that the green reading has a red twin.
fn set_divergence(
    left_label: &str,
    left: &[String],
    right_label: &str,
    right: &[String],
) -> Option<String> {
    let only_left: Vec<&String> = left.iter().filter(|n| !right.contains(n)).collect();
    let only_right: Vec<&String> = right.iter().filter(|n| !left.contains(n)).collect();
    if only_left.is_empty() && only_right.is_empty() {
        return None;
    }
    Some(format!(
        "only in {left_label}: {only_left:?}; only in {right_label}: {only_right:?}"
    ))
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **adding,
/// removing or renaming a section in `schemas/context-capsule.schema.json` without making the same
/// change to `DECLARED_SECTION_ORDER`** -- in either direction.
///
/// The failure is silent in the direction that matters. A section the schema declares and the
/// compiler does not know is not dropped: it falls into the tail bucket and is emitted after the
/// declared ones, ordered by name. The capsule still holds every item, still validates, and still
/// compiles to stable bytes -- so nothing is red, and two components that disagree about which
/// sections are declared now produce different bytes for the same logical capsule, which is
/// exactly the cross-caller determinism this compiler exists to provide.
#[test]
fn the_compilers_section_set_is_the_capsule_schemas_section_set() {
    let schema = capsule_schema();
    let (required, _properties) = declared_sections(&schema);

    assert!(
        required.contains(&"evidence".to_owned()),
        "positive control: the extraction must actually find section names in the schema. It \
         returned {required:?}, which does not hold the landmark `evidence` -- an extraction that \
         silently returns nothing makes every set comparison below trivially true."
    );

    let declared: Vec<String> = graphhelm_runtime::context_compiler::declared_section_order()
        .iter()
        .map(|s| (*s).to_owned())
        .collect();

    if let Some(diff) = set_divergence("the compiler", &declared, "the schema", &required) {
        panic!(
            "the compiler's declared sections and the capsule schema's sections are no longer the \
             same set.\n\n  {diff}\n\n\
             The schema owns the SET; this compiler owns the ORDER. If the schema gained a \
             section, add it to DECLARED_SECTION_ORDER at the position the document should emit \
             it in. If the compiler names one the schema does not, delete it -- it is dead weight \
             claiming to be a contract."
        );
    }
    assert_eq!(
        declared.len(),
        required.len(),
        "set agreement held but the counts differ, which means one side repeats a name: compiler \
         {declared:?} against schema {required:?}"
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **editing one of
/// the schema's two section declarations without the other** -- adding a key under
/// `sections.properties` and not to `sections.required`, or the reverse.
///
/// Both are legal JSON Schema and the document keeps validating. A name in `properties` but not
/// `required` is a silently optional section; a name in `required` but not `properties` is a
/// section with no declared shape that `additionalProperties: false` then forbids, making the
/// document unsatisfiable. Neither is visible to a reader that consults only one of the two lists
/// -- and the test above consults only `required`.
#[test]
fn the_capsule_schemas_two_section_declarations_agree_with_each_other() {
    let schema = capsule_schema();
    let (required, properties) = declared_sections(&schema);

    assert!(
        !required.is_empty() && !properties.is_empty(),
        "positive control: both declarations must be found before they can be compared. \
         required={required:?} properties={properties:?}"
    );

    if let Some(diff) = set_divergence("required", &required, "properties", &properties) {
        panic!(
            "the capsule schema declares its sections in two places and they no longer agree.\n\n\
             \u{20}\u{20}{diff}\n\n\
             Both lists must name the same sections. A name in `properties` alone is silently \
             optional; a name in `required` alone makes the document unsatisfiable against \
             `additionalProperties: false`."
        );
    }
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **weakening
/// `set_divergence` so that it reports agreement it did not find** -- returning `None`
/// unconditionally, comparing a single direction, or comparing lengths instead of members.
///
/// This is the red twin of the two greens above. Those run against the real schema, which agrees
/// with itself today, so a comparison that always says "agreed" passes both and reads exactly like
/// a working guard. The schema is byte-pinned to its `releases/1.0.0` copy and must not be
/// perturbed to find that out, so the divergence is built here instead: a synthetic schema whose
/// two declarations disagree in **both** directions at once, since a one-directional comparison
/// catches one of those and not the other.
#[test]
fn the_agreement_check_reports_a_divergence_when_one_is_present() {
    let divergent: serde_json::Value = serde_json::json!({
        "properties": {
            "sections": {
                "required": ["projectKernel", "task", "onlyInRequired"],
                "properties": {
                    "projectKernel": {},
                    "task": {},
                    "onlyInProperties": {}
                }
            }
        }
    });
    let (required, properties) = declared_sections(&divergent);

    let diff = set_divergence("required", &required, "properties", &properties).expect(
        "the agreement check reported NO divergence for a schema whose two declarations disagree \
         in both directions. Every green it produces elsewhere is therefore worthless.",
    );
    assert!(
        diff.contains("onlyInRequired"),
        "the divergence report must name the member present only in `required`; it said: {diff}"
    );
    assert!(
        diff.contains("onlyInProperties"),
        "the divergence report must name the member present only in `properties` too -- a \
         comparison that checks one direction only passes half of every real drift. It said: \
         {diff}"
    );
}

// ---------------------------------------------------------------------------------------------
// The codes are allocated; these are the guards that make them EMITTED.
//
// A refusal code that exists in the vocabulary and in no emitter is the shape L named on #95: the
// set-equality guard is green, the schema and the type agree, and no run can ever produce the
// refusal. Allocation is cheap and reads like delivery. Amendment 4 allocated two codes for the
// citation half, so the same trap is open twice here.
// ---------------------------------------------------------------------------------------------

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **reporting the
/// first refusal found and stopping** -- an `if missing { .. } else if unknown { .. }` in
/// `refusal_codes`, or a signature returning one code instead of a list.
///
/// The two failures co-occur constantly and for one reason: an answer that cites something the
/// capsule does not contain has usually also failed to cite the item it was given. Reporting one
/// hands the operator half a cure, they apply it, and the verdict refuses again for the half they
/// were never shown. Worse, the half that survives is the one the reader will attribute to the fix
/// they just made.
#[test]
fn a_verdict_failing_both_ways_emits_both_codes() {
    let verdict = graphhelm_runtime::context_compiler::verify_citations(
        &["item-a".to_owned(), "item-b".to_owned()],
        &["item-a".to_owned()],
        &["item-ghost".to_owned()],
    );

    let codes = verdict.refusal_codes();
    assert!(
        codes.contains(&DevelopmentRefusalCode::RequiredCitationMissing),
        "the required item `item-a` was never cited and the verdict did not emit \
         RequiredCitationMissing. It emitted {codes:?}"
    );
    assert!(
        codes.contains(&DevelopmentRefusalCode::CitationUnresolved),
        "the citation `item-ghost` resolves to no capsule item and the verdict did not emit \
         CitationUnresolved. It emitted {codes:?} -- a verdict that reports the first failure and \
         stops hands the operator half a cure."
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **mapping both
/// lists onto one code**, or reading the wrong list -- returning `RequiredCitationMissing`
/// whenever the verdict is refused at all.
///
/// This is the discriminating half of the pair above. A `refusal_codes` that answers with both
/// codes on every refusal passes that test perfectly and says nothing: the operator is told to
/// cite evidence that is already cited, and to stop citing something that does exist. The two
/// remedies are opposite, so a code emitted on the wrong cause is not noise, it is an instruction
/// to undo correct work.
#[test]
fn an_unresolved_citation_alone_does_not_accuse_the_answer_of_missing_one() {
    let verdict = graphhelm_runtime::context_compiler::verify_citations(
        &["item-a".to_owned()],
        &["item-a".to_owned()],
        &["item-a".to_owned(), "item-ghost".to_owned()],
    );

    let codes = verdict.refusal_codes();
    assert!(
        codes.contains(&DevelopmentRefusalCode::CitationUnresolved),
        "arrangement check: this verdict must be refused for an unresolved citation, or the \
         assertion below proves nothing. It emitted {codes:?}"
    );
    assert!(
        !codes.contains(&DevelopmentRefusalCode::RequiredCitationMissing),
        "every required item WAS cited, and the verdict still accused the answer of missing one. \
         It emitted {codes:?}. The remedies are opposite: this tells the operator to cite \
         evidence that is already cited."
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **emitting a
/// code on an accepted verdict** -- deriving the codes from the input lists rather than from the
/// verdict, so that a clean result still carries a refusal.
///
/// Without this the pair above is satisfied by a function that always returns both codes, and a
/// downstream consumer counting refusals would report every successful run as two failures.
#[test]
fn an_accepted_verdict_emits_no_code() {
    let verdict = graphhelm_runtime::context_compiler::verify_citations(
        &["item-a".to_owned()],
        &["item-a".to_owned()],
        &["item-a".to_owned()],
    );

    assert_eq!(
        verdict,
        graphhelm_runtime::context_compiler::CitationVerdict::Accepted,
        "arrangement check: this input must be accepted, or the assertion below is about the \
         wrong verdict"
    );
    assert!(
        verdict.refusal_codes().is_empty(),
        "an accepted verdict carried refusal codes: {:?}",
        verdict.refusal_codes()
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **dropping the
/// capsule identity from the compiled header** -- emitting only the sections, on the reasoning that
/// the ID is already known to whoever asked for the capsule.
///
/// These bytes are what gets digested and what the binding carries, so two capsules compiling alike
/// share one identity. Everything downstream then treats them as the same document: a cache warmed
/// by one is hit by the other, and a binding pinning one verifies against the other. The content
/// that comes back is real, well-formed, and assembled for a different capsule — which is the
/// scope-bleed shape this task's threat assessment names, arriving through the compiler rather than
/// through a citation.
///
/// Version is asserted separately from ID because they can be lost separately: a header carrying
/// the ID and not the version makes every revision of one capsule identical, which is the same
/// failure confined to a single lineage and therefore the harder one to notice.
#[test]
fn capsules_differing_only_in_identity_do_not_compile_to_the_same_bytes() {
    let sections = vec![(
        "evidence".to_owned(),
        vec!["The retry budget is consumed by the caller.".to_owned()],
    )];

    let alpha_v1 =
        graphhelm_runtime::context_compiler::compile_capsule("capsule-alpha", 1, &sections);
    let beta_v1 =
        graphhelm_runtime::context_compiler::compile_capsule("capsule-beta", 1, &sections);
    let alpha_v2 =
        graphhelm_runtime::context_compiler::compile_capsule("capsule-alpha", 2, &sections);

    assert!(
        !alpha_v1.is_empty(),
        "arrangement check: the compiler produced nothing, so the inequalities below would hold \
         between two empty vectors and prove nothing"
    );
    assert_ne!(
        alpha_v1, beta_v1,
        "two DIFFERENT capsules holding identical sections compiled to identical bytes. They now \
         share a digest, so a cache warmed by one is hit by the other and a binding pinning one \
         verifies against the other."
    );
    assert_ne!(
        alpha_v1, alpha_v2,
        "two VERSIONS of the same capsule compiled to identical bytes. Every revision of this \
         capsule is now one document to everything downstream, and the stale one is indis- \
         tinguishable from the current one."
    );
}

// ---------------------------------------------------------------------------------------------
// Delta provenance: the base a delta was computed against, and the three ways it can be wrong.
// ---------------------------------------------------------------------------------------------

use graphhelm_runtime::context_compiler::{
    DeltaBaseRefusal, DeltaProvenance, base_digest, verify_delta_base,
};

fn base_sections() -> Vec<(String, Vec<String>)> {
    vec![(
        "evidence".to_owned(),
        vec!["The retry budget is consumed by the caller.".to_owned()],
    )]
}

fn provenance_for(id: &str, version: u32, bytes: &[u8]) -> DeltaProvenance {
    DeltaProvenance {
        base_capsule_id: id.to_owned(),
        base_version: version,
        base_digest: base_digest(bytes),
    }
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **dropping the
/// digest check** — verifying the base by identity alone.
///
/// This is the cause identity cannot see. The capsule ID matches, the version matches, and the
/// bytes are different: the base was rewritten underneath the delta. Applying the delta then
/// succeeds and produces a capsule that is well-formed, under budget, and assembled against
/// evidence the delta was never computed from. Nothing downstream can tell, because every identity
/// it checks agrees.
///
/// It must also refuse under its OWN variant. Folded into "different capsule", the operator is sent
/// to look for a base they already have, and the actual event — that the base moved — is never
/// reported to anyone.
#[test]
fn a_base_rewritten_under_the_delta_is_refused_under_its_own_cause() {
    let original =
        graphhelm_runtime::context_compiler::compile_capsule("capsule-alpha", 1, &base_sections());
    let delta = provenance_for("capsule-alpha", 1, &original);

    let rewritten = graphhelm_runtime::context_compiler::compile_capsule(
        "capsule-alpha",
        1,
        &[(
            "evidence".to_owned(),
            vec!["The retry budget is consumed by the transport.".to_owned()],
        )],
    );
    assert_ne!(
        original, rewritten,
        "arrangement check: the two bases must actually differ in bytes, or this test is about \
         nothing"
    );

    match verify_delta_base(&delta, "capsule-alpha", 1, &rewritten) {
        Ok(()) => panic!(
            "the delta was accepted against a base with the same identity and different bytes. \
             The base was rewritten underneath it, and every identity check agrees, so nothing \
             downstream can tell that the applied result was assembled from evidence this delta \
             was never computed against."
        ),
        Err(DeltaBaseRefusal::BaseRewritten { expected, actual }) => {
            assert_eq!(expected, base_digest(&original));
            assert_eq!(actual, base_digest(&rewritten));
        }
        Err(other) => panic!(
            "the rewritten base was refused under the wrong cause: {other:?}. Folded into a \
             routing mistake, the operator is sent to look for a base they already have, and the \
             actual event -- that the base moved -- is reported to nobody."
        ),
    }
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **checking the
/// digest first**.
///
/// A digest mismatch is also what a completely different capsule produces. Check it first and every
/// routing mistake is reported as a rewritten base — the two remedies are opposite (find the right
/// base, versus recompute the delta) and the distinction dies quietly, with the guard above still
/// green.
#[test]
fn a_different_capsule_is_refused_as_a_different_capsule_not_as_a_rewrite() {
    let base =
        graphhelm_runtime::context_compiler::compile_capsule("capsule-alpha", 1, &base_sections());
    let delta = provenance_for("capsule-alpha", 1, &base);
    let other =
        graphhelm_runtime::context_compiler::compile_capsule("capsule-beta", 1, &base_sections());

    match verify_delta_base(&delta, "capsule-beta", 1, &other) {
        Err(DeltaBaseRefusal::DifferentCapsule { expected, offered }) => {
            assert_eq!(expected, "capsule-alpha");
            assert_eq!(offered, "capsule-beta");
        }
        other => panic!(
            "a delta offered a different capsule entirely must say so, and said {other:?} instead"
        ),
    }
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **collapsing
/// version into identity** — treating any revision of the named capsule as the base.
///
/// The remedy differs from both neighbours: the right capsule is in hand and the delta may still be
/// recomputable against this revision, which is neither "go find another base" nor "the base moved
/// under you".
#[test]
fn a_different_version_of_the_right_capsule_is_its_own_refusal() {
    let base =
        graphhelm_runtime::context_compiler::compile_capsule("capsule-alpha", 1, &base_sections());
    let delta = provenance_for("capsule-alpha", 1, &base);
    let newer =
        graphhelm_runtime::context_compiler::compile_capsule("capsule-alpha", 2, &base_sections());

    match verify_delta_base(&delta, "capsule-alpha", 2, &newer) {
        Err(DeltaBaseRefusal::DifferentVersion { expected, offered }) => {
            assert_eq!(expected, 1);
            assert_eq!(offered, 2);
        }
        other => panic!("the wrong revision must be named as such, and it said {other:?}"),
    }
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **refusing
/// everything** — the shape that satisfies all three tests above perfectly.
///
/// Three negative guards and no positive one is the classic vacuous set: a verifier that returns an
/// error unconditionally passes every one of them, and the feature is dead rather than strict.
#[test]
fn the_base_a_delta_was_actually_computed_against_verifies() {
    let base =
        graphhelm_runtime::context_compiler::compile_capsule("capsule-alpha", 1, &base_sections());
    let delta = provenance_for("capsule-alpha", 1, &base);

    assert_eq!(
        verify_delta_base(&delta, "capsule-alpha", 1, &base),
        Ok(()),
        "the base this delta was computed against must verify, or the three refusals above are \
         satisfied by a verifier that refuses everything"
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **counting
/// `String::chars().count()` instead of `String::len()`** in `fit_within_budget`. Any change of
/// unit fells the `required_budget` assertion below, which is why that assertion names the NUMBER
/// rather than only the refusal.
///
/// **What this pins, and what it deliberately does NOT claim.** `fit_within_budget` measures
/// BYTES. Nothing in #222's contract declares the unit, the suite never varied the axis -- every
/// existing budget fixture uses single-byte ASCII, where bytes, characters and any plausible token
/// count are the same number -- and `BudgetOutcome`'s own doc comment calls it "a token budget".
/// So the unit was simultaneously unstated, untested, and described as something it is not.
///
/// This test states it. It is a characterization, NOT an endorsement: it does not say bytes is the
/// right unit, only that bytes is the unit, so that a future change to tokens is a deliberate act
/// that turns this red rather than a silent redefinition of what a budget means.
///
/// **Why it matters beyond tidiness (#222 `compiled_input_tokens`).** That counter is an
/// `unavailableField` today and the honest source for it does not exist: no dependency in the
/// workspace tokenizes, and the only production caller of `compile_capsule` compiles the
/// degenerate empty capsule by a sealed decision. The nearest number that LOOKS like a candidate
/// is this byte total. Filling a token counter from it would publish bytes wearing the name of
/// tokens -- an instrument speaking about a quantity it never measured.
#[test]
fn the_budget_is_measured_in_bytes_not_characters() {
    // Five characters, ten bytes: the smallest fixture where the two units disagree. Written as
    // escapes so the assertion cannot be broken by a tool that re-encodes this file.
    let multi_byte = "\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}".to_owned();
    assert_eq!(
        (multi_byte.chars().count(), multi_byte.len()),
        (5, 10),
        "HARNESS-BROKE: the fixture must be one where characters and bytes disagree, or this test \
         cannot tell the two units apart"
    );

    let required = vec![multi_byte];
    let budget = 5; // fits the CHARACTER count exactly; half of the BYTE count

    match fit_within_budget(&required, &[], budget) {
        BudgetOutcome::Refused { code, expansion } => {
            assert_eq!(
                expansion.required_budget, 10,
                "the budget is spent in BYTES: the refusal must report the byte total, not the \
                 character count a token-shaped reading would produce"
            );
            assert_eq!(code, DevelopmentRefusalCode::ContextBudgetInsufficient);
        }
        BudgetOutcome::Fits { .. } => panic!(
            "a required item of 10 bytes fitted a budget of 5, so the budget is NOT counting \
             bytes. If this unit changed on purpose, `compiled_input_tokens` and \
             `BudgetOutcome`'s doc comment both depend on the answer -- change them together."
        ),
    }
}
