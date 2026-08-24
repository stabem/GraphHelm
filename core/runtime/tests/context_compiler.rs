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

    let at_position_zero = item_id(capsule, version, "evidence", 0, "the wake path burns the lease");
    let same_text_moved = item_id(capsule, version, "evidence", 3, "the wake path burns the lease");

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
    assert_eq!(first, again, "item ID derivation must be a function of its inputs");
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
