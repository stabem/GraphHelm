//! The suite's size lives in the suite, not in prose that describes it (#272).
//!
//! Doc comments in gate machinery stated how many specimens the pathogen suite holds, and some of
//! them were wrong: they said **ten** where `suite()`'s own `vec![]` holds twelve. The shipped
//! guards already assert `certification.specimens` against the real suite, so nothing was blind
//! except the prose, and a reader who trusted a comment over the code got the wrong population.
//!
//! **This header states no count either, including of its own sites** — an earlier draft said "two
//! doc comments" and "both sites", and a third site was found before it merged. A file that exists
//! to remove hand-maintained counts is the last place one should survive.
//!
//! **The fix is to REMOVE the number, not to correct it, and that distinction is the whole point
//! of #272.** Writing "twelve" would put a fresh hand-maintained integer in the one place nothing
//! can check, which is the defect this ticket is about, restated one value later. This repository
//! has already settled the question in the same terms — `apps/cli/src/commands/mcp/tools.rs`'s own
//! header says:
//!
//! > **No count is stated here on purpose.** This prose said "fourteen" while `TOOLS` held
//! > eighteen ... Prose cannot be guarded, so it no longer carries a number to be wrong about.
//!
//! That last clause is what this file makes false in the useful direction: prose cannot be guarded
//! in general, but *this* claim can be, because "a count of specimens" has a recognisable shape.
//! Removing the number without a guard leaves nothing stopping the next writer from adding one
//! back — a note where a mechanism belongs.
//!
//! **Every site is checked from here, on purpose.** They span two crates (`tools/pathogens/` and
//! `core/quality/`), both gate machinery, so a change touching only these is legal under the M06
//! freeze while a change mixing them with anything outside is not. A guard per crate would let one
//! site drift while the others stayed clean, which is the same two-producers-of-one-number problem
//! in miniature -- the count is ONE population and one guard should see all of it.

use std::path::{Path, PathBuf};

/// Number words a stated count could plausibly use, plus bare digits, checked separately.
///
/// Closed and small on purpose: this is not a natural-language parser, and a longer list would
/// invite the belief that it is. Anything past twenty in a sentence about this suite is a
/// different problem than the one measured.
const NUMBER_WORDS: [&str; 21] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
];

/// The nouns a count of this population would attach to.
///
/// **`way` and `mode` are here because the first version missed a live instance, and the way it
/// was missed is the reusable part.** I chose the vocabulary from the defect I had already seen —
/// "ten specimens", "ten pathogens" — and shipped a guard whose population was narrower than its
/// own subject's. Twelve lines above `suite()`, `UselessnessMode`'s doc said "The ten ways a
/// deliverable can be green", over an enum with twelve variants: the same population, the same
/// wrong number, a noun I had not thought to look for.
///
/// One specimen exists per mode, so "ways", "modes", "specimens" and "pathogens" are names for
/// one count. Measured before widening rather than after: across the named sites these catch
/// statements of the count and nothing else, while the singular forms carrying the other roles
/// ("one specimen per mode", "at least one pathogen") stay untouched by the plurality rule below.
///
/// The sentence above deliberately counts nothing. An earlier draft of it read "these four catch
/// exactly the three real lines and nothing else in either file", and by the time a third site
/// and a third file had been found, every number in it was wrong -- inside the doc comment of the
/// guard against exactly that.
const COUNTED_NOUNS: [&str; 4] = ["specimen", "pathogen", "way", "mode"];

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository root resolves from this crate")
}

/// The prose sites that describe this population, all inside the M06 frozen prefixes.
///
/// **A NAMED LIST, not a walk, and the difference was measured rather than preferred.** Walking
/// the two frozen prefixes and applying the rule below marks prose that is correct as written:
///
/// * `tools/pathogens/src/jpd.rs:188` — *"Two specimens is a FLOOR, not a coverage claim"*. A
///   plural lower bound, immediately adjacent, so no window setting excludes it. It is the
///   counterexample that kills the walk on its own.
/// * `core/quality/tests/geometry.rs:87` — *"Break it three ways"*. A manner, not a population,
///   and it is `way` — one of the nouns added here after a miss — biting back.
/// * **this file's own explanations**, whose prose quotes the defect in order to describe it. A
///   guard that walks its own prefix marks its own examples, exactly as `source_invariants.rs`
///   records for the crate that scans itself.
///
/// **No total is stated, and the omission is the rule of this file applied to itself.** An earlier
/// draft said "SEVEN lines across 19 files". The 19 was wrong — 18, at both refs — and the seven
/// was right only under a convention the sentence never declared. Worse, the self-match count is a
/// MOVING TARGET BY CONSTRUCTION: every edit to the paragraphs above changes how many lines a walk
/// would mark, so any number here is stale the next time someone improves the explanation. The two
/// addresses above carry the argument on their own and do not move. (Found by L, applying
/// cite-against-a-named-base to a seal I wrote in a file about numbers that only look checked.)
///
/// **So the justification written above `is_counted_noun_plural` needs narrowing, and this is
/// where.** It says the singular carries the other roles. That is TRUE across these named files
/// and FALSE across the crate — `jpd.rs:188` is a lower bound in the plural. It was a property of
/// the corpus I had measured, stated as a property of the language. Widening this list therefore
/// costs two things at once: a role exemption for this file's own explanatory prose, and a
/// discriminator that survives a plural lower bound. Neither exists yet, and inventing one to
/// justify a walk would be a guard guessing at meaning.
///
/// The list is the population, so a fourth prose site added elsewhere in gate machinery is NOT
/// covered — which is why the test below is named for what it reads rather than for the prefix
/// it sits in.
fn prose_sites() -> Vec<(String, String)> {
    let root = repository_root();
    [
        "tools/pathogens/src/lib.rs",
        "tools/pathogens/tests/thymus.rs",
        "core/quality/tests/thymus.rs",
    ]
    .iter()
    .map(|relative| {
        let path = root.join(relative);
        let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("HARNESS-BROKE: cannot read {}: {error}", path.display())
        });
        ((*relative).to_owned(), text)
    })
    .collect()
}

/// Whether `word` is a stated quantity: a number word, or a bare run of digits.
fn is_quantity(word: &str) -> bool {
    let word = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    if word.is_empty() {
        return false;
    }
    word.chars().all(|c| c.is_ascii_digit())
        || NUMBER_WORDS
            .iter()
            .any(|known| word.eq_ignore_ascii_case(known))
}

/// Whether `word` is the PLURAL of a noun a count of this population attaches to.
///
/// **Plurality is the discriminator, and it was found by measurement rather than chosen.** The
/// first version accepted the singular too, and every extra line it reported was real prose doing
/// a different job with the same shape. Measured **at `202aca1`**, before any fix in this change,
/// with the line numbers of that base:
///
/// ```text
/// lib.rs:4    A gate that passes even one pathogen is itself fake
/// lib.rs:122  One specimen per mode; the mode names the axis the specimen defeats.
/// lib.rs:154  One pathogen: a deliverable that is green by correctness measures ...
/// lib.rs:247  Certification refused: the gate passed at least one pathogen.
/// lib.rs:264  The gate passed at least one specimen; `fooled_by` names them.
/// ```
///
/// The addresses are the evidence and they are bound to that base; a total is not restated here,
/// because the earlier one ("SEVEN ... where there are two") counted defects that this change has
/// since removed and would read as false to anyone re-running it at head.
///
/// Every one is a rate, a definition, or a lower bound -- never a claim about how big the suite
/// is. A stated population count says "ten specimenS"; a rate says "one specimen". The singular
/// carries the other roles, so requiring the plural separates them without needing to know what
/// the sentence means.
///
/// **SEALED:** a suite of exactly one, described as "exactly one specimen", would slip past. That
/// is accepted rather than patched around -- distinguishing it from "one specimen per mode" needs
/// the sentence's meaning, and a guard that guesses at meaning is one the next reader edits until
/// it stops complaining.
fn is_counted_noun_plural(word: &str) -> bool {
    let word = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    COUNTED_NOUNS
        .iter()
        .any(|noun| word.eq_ignore_ascii_case(&format!("{noun}s")))
}

/// How far a plural counted noun may sit from its quantity and still be that quantity's noun.
///
/// **Two, and it was MEASURED rather than chosen — which is the correction that produced it.**
/// The first version required immediate adjacency, and adjacency was picked because both defects
/// I had looked at happened to have it. It missed `tools/pathogens/tests/thymus.rs:1`, *"twelve
/// green-but-useless specimens"*, where one adjective stands between. That is the same mistake as
/// the narrow vocabulary one function above, in the other dimension: the vocabulary was corrected
/// by measurement and the SHAPE was still a guess.
///
/// Measured over the three named sites **as they stood at `78b59db`** — before the fixes in this
/// change, while the defects were still there to be found. The base is named because these numbers
/// do not re-derive at head: the sites are clean now, so every setting reads near zero, and a
/// reader who re-ran this table without the base would conclude the measurement was invented.
///
/// | window | marks at `78b59db` | verdict |
/// |---|---|---|
/// | 1 | 0 | misses `thymus.rs:1` entirely |
/// | 2 | 2 | both real: `thymus.rs:1` and `lib.rs:89` |
/// | 3 | 3 | adds `lib.rs:578`, where the quantity is **"no one** checked the specimens" |
///
/// So three is where the rule stops describing counts and starts matching pronouns. Two is not a
/// safe default that happens to work; it is the last setting whose every mark is real.
///
/// The numbers stay here, unlike the walk total above, because they ARE the argument: strike them
/// and nothing explains why the window is two. A count that carries a reason gets a base; a count
/// that decorates one gets deleted.
const NOUN_DISTANCE: usize = 2;

/// Comment lines that state a count OF this population: a quantity, then the plural noun within
/// [`NOUN_DISTANCE`] words, with no sentence end between them.
///
/// **A window rather than no rule at all is what keeps this from being too blunt to live with.**
/// The very line this was written against reads "exactly ten specimens, one per uselessness mode"
/// -- it contains TWO number words, and only one of them is a count of the population. A rule that
/// banned number words outright would fire on "one per uselessness mode", which is correct prose,
/// and the next person would weaken the guard rather than the sentence.
///
/// The sentence boundary matters for the same reason: a quantity ending one sentence has nothing
/// to do with a noun opening the next, and without this a two-word window would reach across the
/// full stop and invent a claim neither sentence makes.
fn lines_stating_a_count(text: &str) -> Vec<String> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| line.trim_start().starts_with("//"))
        .filter(|(_, line)| {
            let words: Vec<&str> = line.split_whitespace().collect();
            words.iter().enumerate().any(|(index, word)| {
                if !is_quantity(word) {
                    return false;
                }
                (1..=NOUN_DISTANCE).any(|step| {
                    words
                        .get(index + step)
                        .is_some_and(|candidate| is_counted_noun_plural(candidate))
                        && !words[index..index + step]
                            .iter()
                            .any(|passed| passed.ends_with('.'))
                })
            })
        })
        .map(|(number, line)| format!("{}: {}", number + 1, line.trim()))
        .collect()
}

/// The detector must fire on the shape it exists to catch, and must not fire on correct prose.
///
/// Without this the check below is a rule nobody has watched work: if `is_quantity` or the
/// adjacency walk were wrong, it would report zero offenders on a file full of them and read
/// exactly like a clean tree. The negative cases are the half that actually earns the guard the
/// right to be blunt -- "one per uselessness mode" is the sentence that would have been sacrificed
/// by a cruder rule.
#[test]
fn the_detector_fires_on_a_stated_count_and_not_on_ordinary_prose() {
    for fires in [
        "/// The bred suite: exactly ten specimens, one per uselessness mode, deterministic.",
        "//! receipt by rejecting all ten pathogens - and the layout grammar ALONE is REFUSED,",
        "/// twelve specimens",
        "// 12 pathogens",
        // The instance the first version of this guard shipped without catching.
        "/// The ten ways a deliverable can be green by correctness measures and useless by",
        "/// the twelve modes",
        // The instance the SECOND version shipped without catching: one adjective of distance,
        // which is what turned an adjacency rule into a measured window.
        "//! The thymus harness proofs: twelve green-but-useless specimens (the original ten plus",
    ] {
        assert!(
            !lines_stating_a_count(fires).is_empty(),
            "the detector missed a stated count: {fires}"
        );
    }
    // The five below are not invented. They are the REAL lines the first version of this detector
    // reported as offenders, taken verbatim from the corpus it was run against. Hand-written
    // negative cases had missed every one of them: I wrote "one per uselessness mode", where the
    // number is followed by "per", and the corpus answered with "One specimen per mode", where it
    // is followed by the noun. Fixtures drawn from the measurement cover the shapes imagination
    // does not.
    for silent in [
        "//! useless-but-green deliverables. A gate that passes even one pathogen is itself fake",
        "/// construction. One specimen per mode; the mode names the axis the specimen defeats.",
        "/// One pathogen: a deliverable that is green by correctness measures and useless by",
        "/// Certification refused: the gate passed at least one pathogen.",
        "/// The gate passed at least one specimen; `fooled_by` names them.",
        "/// The bred suite: one per uselessness mode, deterministic.",
        "/// Every specimen is deterministic.",
        "/// ten reasons to read the code instead",
        "let specimens = 12;",
        "/// Right, in six calls, where one would do",
        // The window's own boundary: at a distance of three this matches, and the quantity is the
        // "one" of "no one". It is why NOUN_DISTANCE is two rather than a rounder number.
        "/// still named what slipped, the suite still certified, and no one checked the specimens",
    ] {
        assert!(
            lines_stating_a_count(silent).is_empty(),
            "the detector fired on prose that states no count of this population: {silent}"
        );
    }

    // These two ARE marked, and that is the point: they are the measured argument for reading a
    // named list instead of walking the frozen prefixes. Both are correct prose that the widened
    // population would flag -- a plural LOWER BOUND and a manner -- so the walk cannot be adopted
    // without a discriminator that does not exist.
    //
    // I first put them in the silent list above, which claimed the detector stays quiet on them.
    // It does not, and the cell said so. Asserting the opposite of what a rule does, inside the
    // file that documents the rule, is the same class as the prose this guard exists to remove --
    // so the claim now lives where it can fail.
    for marked_but_correct in [
        "/// Two specimens is a FLOOR, not a coverage claim. Growing it changes `suite_digest`,",
        "// Break it three ways: a washed-out declared color, a ragged table row, a cramped gap",
    ] {
        assert!(
            !lines_stating_a_count(marked_but_correct).is_empty(),
            "this line no longer trips the detector, so the standing argument against walking the \
             frozen prefixes has lost its evidence. Re-measure the walk before widening \
             `prose_sites`: {marked_but_correct}"
        );
    }
}

/// None of the NAMED prose sites states how many specimens the suite holds.
///
/// The production change this catches is the one that looks like a fix: replacing "ten" with
/// "twelve". That is a hand-maintained integer in unguarded prose, which is what was wrong the
/// first time -- the value being right today is not the property that failed.
///
/// **Named for what it reads, not for the prefix it lives in.** It used to be called
/// `no_prose_in_the_gate_machinery_states_the_specimen_count`, which claimed the whole of gate
/// machinery while reading two files -- and a third file inside that machinery was stating the
/// count at the time. The assertion was honest and the name was not, and the name is the half a
/// reader believes. `prose_sites` documents what the population is and why widening it is a trap.
#[test]
fn no_named_prose_site_states_the_specimen_count() {
    let sites = prose_sites();
    // NO `sites.len()` ASSERTION HERE, deliberately, and its absence is the point in a change
    // about numbers that only look checked. The list is a literal and `prose_sites` PANICS on a
    // path it cannot read, so a length check could never observe a short return: it would read
    // as a population control and control nothing. The real one is below -- each site must
    // mention the population it is supposed to describe, which fails if a path silently starts
    // pointing at the wrong file.
    for (path, text) in &sites {
        assert!(
            text.contains("pathogen") || text.contains("specimen"),
            "HARNESS-BROKE: {path} mentions neither specimens nor pathogens, so this guard is \
             reading the wrong file and would pass whatever it contained"
        );
    }

    let offenders: Vec<String> = sites
        .iter()
        .flat_map(|(path, text)| {
            lines_stating_a_count(text)
                .into_iter()
                .map(move |line| format!("{path}:{line}"))
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "prose states how many specimens the suite holds. The population is the `vec![]` in \
         `suite()`, and `certification.specimens` is already asserted against it -- a number \
         repeated in a comment is a second producer of one count, and the comment is the one \
         nothing checks. Delete the number rather than correcting it:\n{}",
        offenders.join("\n")
    );
}
