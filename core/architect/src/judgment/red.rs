//! Shadow classification of a RED gate run (#1138, edge 1). Pure: a bounded excerpt of a gate
//! log, one typed request over it, and a reading of the reply. Nothing here changes a verdict,
//! selects a stage, counts a pass or re-queues anything: the classification is RECORDED beside
//! the manifest so a confusion table can be built before any edge gets the power to act
//! (`AGENTS.md`: deterministic policy and evidence decide gates; a judge classifies and proposes).
//!
//! The excerpt reads four shapes the runner log has (`ci/gate.ps1`, `cargo test`):
//! `[gate] RED - failed stages: a, b`, `[gate] FAILED: <stage> (exit N)`,
//! `test <name> ... FAILED`, the first `panicked at` line (joined with the next line when the
//! location wrapped onto it, which the PowerShell transcript does), and the first
//! `error:`/`error[` line. Everything is capped so the state handed to the judge is bounded
//! whatever the log's size.
//!
//! **The first two answer the same question and disagree, so the banner DECIDES whenever it is
//! present** (#1149). `[gate] FAILED:` is a line the gate prints; the banner is the answer the
//! gate computed. The gate's own self-test fixtures fail on purpose and print the gate's own
//! vocabulary into the gate's own log, so the printed lines count the gate testing itself:
//! measured on the first live System One call, eight of them for one failed stage. The printed
//! lines remain the evidence for the one shape that never reaches a banner — a run that dies
//! before any verdict line, the canary abort included.

use std::collections::BTreeMap;

use graphhelm_gateway::judgment::{
    Answer, JEV_LATEST, JudgeReply, JudgeRequest, NoulCriteria, Question,
};
use sha2::{Digest, Sha256};

use super::policy::{acts, noul_is_yes};

/// The closed vocabulary of the `class` question, in the order the judge sees it. Disjoint from
/// `ci/classify-run.ps1`'s human classes on purpose: a shadow class is never written into
/// `runClass`.
pub const RED_CLASSES: [&str; 4] = [
    "known_flake",
    "environment_void",
    "real_defect",
    "harness_broke",
];

/// At most this many lines of the log's tail travel in the excerpt.
pub const MAX_TAIL_LINES: usize = 40;
/// At most this many `FAILED` test names travel in the excerpt.
pub const MAX_FAILED_TESTS: usize = 20;
/// Every string in the excerpt is cut to this many characters.
pub const MAX_LINE_CHARS: usize = 400;

const RED_BANNER: &str = "[gate] RED - failed stages:";
const STAGE_FAILED: &str = "[gate] FAILED:";
const PANIC_MARK: &str = "panicked at";

/// The bounded state a judge is shown for one red run.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedExcerpt {
    /// The stages the gate named as failed, in log order, without repeats.
    pub failed_stages: Vec<String>,
    /// The `test <name> ... FAILED` names, in log order, at most [`MAX_FAILED_TESTS`].
    pub failed_tests: Vec<String>,
    /// The first `panicked at` line, with its location when the transcript wrapped it.
    pub first_panic: Option<String>,
    /// The first line beginning `error:` or `error[`.
    pub first_error_line: Option<String>,
    /// The last [`MAX_TAIL_LINES`] non-blank lines.
    pub tail: Vec<String>,
}

/// One intermittent test a lane already knows about, with the issue that tracks it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KnownFlake {
    pub issue: u64,
    pub test: String,
    pub summary: String,
}

/// What the reply says, read under the same named thresholds every other site uses.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedClassification {
    /// The judge's word, verbatim: one of [`RED_CLASSES`] when resolved; otherwise whatever was
    /// answered (or empty when nothing was), so the record shows what the judge said.
    pub class: String,
    /// The `class` answer's confidence; `0.0` when unanswered.
    pub confidence: f64,
    /// The known-flake issues whose `same_as` read as "yes" (`NOUL_YES_THRESHOLD`).
    pub same_as: Vec<u64>,
    /// Known flakes the judge answered yes about while the excerpt does not name their test. The
    /// answer is not counted as a match, and the disagreement is reported rather than dropped
    /// (#1140 review).
    pub contradicted: Vec<u64>,
    /// The class is in the vocabulary AND at or above `ACT_THRESHOLD`. In shadow mode nothing
    /// reads this to act; it is the column the confusion table needs.
    pub acts: bool,
    /// The answer was missing, mistyped, outside the vocabulary, or under the acting threshold.
    pub unresolved: bool,
}

fn cap(line: &str) -> String {
    line.chars().take(MAX_LINE_CHARS).collect()
}

fn push_unique(stages: &mut Vec<String>, stage: &str) {
    let stage = cap(stage.trim());
    if !stage.is_empty() && !stages.contains(&stage) {
        stages.push(stage);
    }
}

/// The bounded excerpt of one gate log.
#[must_use]
pub fn excerpt(log: &str) -> RedExcerpt {
    let lines: Vec<&str> = log
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .collect();
    let mut out = RedExcerpt::default();
    // TWO SOURCES FOR ONE FIELD, kept apart until the end (#1149).
    //
    // `[gate] FAILED: <stage>` is a line the gate PRINTS; `[gate] RED - failed stages:` is the
    // answer the gate COMPUTED. They disagree, and measurably: the gate's own self-test
    // fixtures (`ci/gate-*.tests.ps1`) fail on purpose, to prove the gate reports failures, and
    // they print the gate's own vocabulary into the gate's own log. Measured on the first live
    // System One call, `1128-20260917T005738.log` carried EIGHT such lines for ONE failed stage,
    // so the judge was shown "eight unrelated stages failed" for a single flaky test.
    //
    // The banner wins whenever it EXISTS -- tracked as its own flag rather than by "did the
    // banner yield anything", so a banner that names nothing reports nothing instead of silently
    // falling back to the noise this exists to exclude.
    let mut banner_stages: Vec<String> = Vec::new();
    let mut banner_seen = false;
    let mut printed_stages: Vec<String> = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(RED_BANNER) {
            // TWO BANNERS POOL, they do not supersede -- and the reason is NOT that the rest of
            // this struct pools. It does not. MEASURED on a two-run log
            // (`two_banners_pool_and_a_two_run_log_has_no_coherent_answer`, which asserts each
            // of these rather than describing them):
            //
            //     failed_stages  pools       failed_tests  pools
            //     first_panic    FIRST-wins  tail          LAST-wins beyond MAX_TAIL_LINES
            //
            // So under two runs this excerpt is ALREADY a mixture, and no choice for this field
            // makes it describe one run. An earlier version of this comment claimed the
            // neighbours all pooled and argued from consistency; that premise was false, and a
            // reviewer had withdrawn a live objection on the strength of it (`eloquent-jones`
            // measured it, #1153).
            //
            // What actually decides it: a second banner means the FILE holds two runs, which is
            // a malformed input this type has no correct answer for. Between the two available
            // behaviours, pooling discards nothing and last-wins silently drops a stage that
            // really did fail. The cost is the honest one to state -- pooling can show the judge
            // more failed stages than any single run had, which is the very contamination the
            // rest of this change removes. It is preferred anyway because its failure is VISIBLE
            // (a stage list that does not match any one run) where last-wins produces a
            // plausible subset that reads as a clean single run.
            //
            // Not settled: whether a two-run log should be REFUSED rather than excerpted. That is
            // the answer neither behaviour gives and it is out of this change's scope.
            banner_seen = true;
            for stage in rest.split(',') {
                push_unique(&mut banner_stages, stage);
            }
        } else if let Some(rest) = trimmed.strip_prefix(STAGE_FAILED) {
            // `[gate] FAILED: contamination canary (ci-canary) (exit 101)`: the exit suffix is
            // the process's, not the stage's name.
            let stage = match rest.rfind(" (exit ") {
                Some(at) if rest.ends_with(')') => &rest[..at],
                _ => rest,
            };
            push_unique(&mut printed_stages, stage);
        } else if let Some(rest) = trimmed.strip_prefix("test ") {
            if let Some(name) = rest.strip_suffix(" ... FAILED")
                && out.failed_tests.len() < MAX_FAILED_TESTS
                && !name.starts_with("result:")
            {
                out.failed_tests.push(cap(name.trim()));
            }
        } else if out.first_panic.is_none() && trimmed.contains(PANIC_MARK) {
            let mut panic = trimmed.to_owned();
            let after = trimmed.rsplit(PANIC_MARK).next().map_or("", str::trim);
            if after.is_empty() {
                // The transcript wrapped the location onto the next line.
                if let Some(next) = lines[index + 1..]
                    .iter()
                    .map(|line| line.trim())
                    .find(|line| !line.is_empty())
                {
                    panic.push(' ');
                    panic.push_str(next);
                }
            }
            out.first_panic = Some(cap(&panic));
        } else if out.first_error_line.is_none()
            && (trimmed.starts_with("error:") || trimmed.starts_with("error["))
        {
            out.first_error_line = Some(cap(trimmed));
        }
        index += 1;
    }
    // The canary abort is the shape that never reaches a verdict line, so the printed lines are
    // the only evidence a run of that shape leaves (#1140 deviation 2). Everything else has the
    // banner, and the banner is what the gate itself concluded.
    out.failed_stages = if banner_seen {
        banner_stages
    } else {
        printed_stages
    };
    let mut tail: Vec<String> = lines
        .iter()
        .rev()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .take(MAX_TAIL_LINES)
        .map(cap)
        .collect();
    tail.reverse();
    out.tail = tail;
    out
}

/// The lowercase hex sha256 of the excerpt's canonical JSON: the key a recorded classification
/// is filed under beside the manifest, so two runs with the same excerpt are visibly the same.
#[must_use]
pub fn excerpt_sha256(excerpt: &RedExcerpt) -> String {
    let bytes =
        serde_json::to_vec(excerpt).expect("RedExcerpt is plain data and always serializes");
    hex::encode(Sha256::digest(bytes))
}

fn same_as_key(issue: u64) -> String {
    format!("same_as:{issue}")
}

impl RedExcerpt {
    /// Does the excerpt's own evidence name `test` among the tests that failed? The comparison is
    /// on the test's PATH-FREE name as `cargo` prints it, so `module::name` and `name` both match
    /// a `failed_tests` entry ending in that name (#1140 review).
    #[must_use]
    pub fn names_test(&self, test: &str) -> bool {
        let wanted = Self::segments(test);
        if wanted.is_empty() {
            return false;
        }
        self.failed_tests
            .iter()
            .any(|failed| Self::same_test(&wanted, &Self::segments(failed)))
    }

    /// A test path split into its segments, leaf FIRST, blanks dropped.
    fn segments(path: &str) -> Vec<&str> {
        path.trim()
            .rsplit("::")
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .collect()
    }

    /// Are these the same test? Every segment BOTH sides carry, from the leaf backwards, must
    /// agree. Leaf-only admitted a cross-module collision — `totally::other::my_cell` matched a
    /// failure of `outer::inner::my_cell` (#1140 re-read) — and requiring the full path would
    /// refuse a legitimate pair, because the two sides are written by different producers: a
    /// known-flake entry is authored by hand, `cargo` prints what it prints, and one may be
    /// qualified where the other is bare. So the comparison is the common suffix: it uses every
    /// segment the evidence actually offers and invents none. When one side is bare the evidence
    /// does not distinguish the modules, and matching on the leaf is the most it supports.
    fn same_test(left: &[&str], right: &[&str]) -> bool {
        if left.is_empty() || right.is_empty() {
            return false;
        }
        left.iter().zip(right.iter()).all(|(a, b)| a == b)
    }
}

/// The request: state = the excerpt and the known-flake list; questions = `class` (Choice over
/// [`RED_CLASSES`], one rubric each) and one `same_as:<issue>` Noul per known flake.
#[must_use]
pub fn request(excerpt: &RedExcerpt, known: &[KnownFlake]) -> JudgeRequest {
    let classes: BTreeMap<String, Option<String>> = [
        (
            "known_flake",
            "the failing cell is one of the known intermittent tests and nothing else failed",
        ),
        (
            "environment_void",
            "disk, memory, network, a killed process or a tool missing on the host; the code \
             was not measured",
        ),
        (
            "real_defect",
            "a test or build failure the diff can explain",
        ),
        (
            "harness_broke",
            "the gate's own machinery failed (canary, manifest, slot) before or after the stages",
        ),
    ]
    .into_iter()
    .map(|(class, rubric)| (class.to_owned(), Some(rubric.to_owned())))
    .collect();
    debug_assert!(RED_CLASSES.iter().all(|class| classes.contains_key(*class)));
    let mut questions = BTreeMap::new();
    questions.insert(
        "class".to_owned(),
        Question::Choice {
            instructions: "Why did this gate run go RED? Judge from `excerpt` (the failed \
                           stages, the FAILED tests, the first panic, the first error line and \
                           the tail of the log) and `knownFlakes` (tests a lane already knows \
                           are intermittent). Pick the one class whose rubric fits."
                .to_owned(),
            criteria: classes,
        },
    );
    for flake in known {
        questions.insert(
            same_as_key(flake.issue),
            Question::Noul {
                instructions: format!(
                    "Is this red the same failure as the known flake tracked by issue {} (see \
                     `knownFlakes`: test `{}`, {})?",
                    flake.issue, flake.test, flake.summary
                ),
                criteria: Some(NoulCriteria {
                    r#true: "the same test failed for the reason the summary describes and \
                             nothing else failed"
                        .to_owned(),
                    r#false: "a different test failed, or the same test failed for a reason the \
                              summary does not describe, or something else failed too"
                        .to_owned(),
                }),
            },
        );
    }
    JudgeRequest {
        state: serde_json::json!({ "excerpt": excerpt, "knownFlakes": known }),
        model: JEV_LATEST.to_owned(),
        questions,
    }
}

/// Reads the reply. A missing or mistyped `class`, a word outside [`RED_CLASSES`], or a
/// confidence under `ACT_THRESHOLD` is `unresolved`, never a class acted on; `same_as` holds
/// the issues whose Noul is in `[0.0, 1.0]` and reads as "yes".
#[must_use]
pub fn read(reply: &JudgeReply, excerpt: &RedExcerpt, known: &[KnownFlake]) -> RedClassification {
    let (class, confidence) = match reply.answers.get("class") {
        Some(Answer::Choice {
            choice, confidence, ..
        }) => (choice.clone(), *confidence),
        _ => (String::new(), 0.0),
    };
    let in_vocabulary = RED_CLASSES.contains(&class.as_str());
    let in_range = (0.0..=1.0).contains(&confidence);
    // THE CLAIM AND ITS CHECK IN ONE PLACE (#1140 review). A `same_as` says "this red is that
    // known flake". The judge's yes is grounded only in the prompt: the flake's test name reaches
    // it as a string and nothing compared it to what actually failed, so a confident yes about a
    // test the log never names would have been recorded as a match. The excerpt is the log's own
    // evidence, so the answer is admitted only when the flake's test IS one of the failed tests.
    // A judge that says yes about a test that did not fail is reported as `contradicted`, never
    // silently dropped: a disagreement between the answer and the evidence is a finding.
    let mut contradicted = Vec::new();
    let mut same_as = Vec::new();
    for flake in known {
        let said_yes = matches!(
            reply.answers.get(&same_as_key(flake.issue)),
            Some(Answer::Noul { noul }) if (0.0..=1.0).contains(noul) && noul_is_yes(*noul)
        );
        if !said_yes {
            continue;
        }
        if excerpt.names_test(&flake.test) {
            same_as.push(flake.issue);
        } else {
            contradicted.push(flake.issue);
        }
    }
    // A `known_flake` MUST NAME A FLAKE THE EVIDENCE SUPPORTS (#1140 re-read, twice). The first
    // round left a contradiction sitting beside the class instead of denying it: the row read
    // "known_flake, acting" while the log refuted every flake named. The second round found the
    // hole the first comment claimed to cover and did not — the judge can reach the same place by
    // SILENCE, answering the class and naming no flake at all, which is the same zero evidence
    // arrived at differently. So the condition is `same_as` empty while there WAS something to
    // name: an operator who supplied no known flakes has asked no such question, and denying
    // there would make the class unreachable rather than grounded. The other three classes are
    // not claims about a flake and are untouched.
    let flake_unsupported = class == "known_flake" && same_as.is_empty() && !known.is_empty();
    let acts = in_vocabulary && in_range && acts(confidence) && !flake_unsupported;
    RedClassification {
        class,
        confidence,
        same_as,
        contradicted,
        acts,
        unresolved: !acts,
    }
}

#[cfg(test)]
mod tests {
    use graphhelm_gateway::call::Usage;

    use super::*;
    use crate::judgment::policy::{ACT_THRESHOLD, NOUL_YES_THRESHOLD};

    const FLAKE_LOG: &str = "\
[gate] scope: FULL: the selector escalated (Cargo.toml)
[gate] workspace tests
test a_declared_timeout_reaches_the_verdict_over_the_real_surface ... ok
test eof_arriving_after_the_deadline_is_not_silently_accepted ... FAILED
test harness_broke_error_leaves_other_kinds_untouched ... ok
failures:

---- eof_arriving_after_the_deadline_is_not_silently_accepted stdout ----

thread 'eof_arriving_after_the_deadline_is_not_silently_accepted' (40564) panicked at
apps\\cli\\tests\\api_http.rs:5858:9:
neither the EOF arm nor the top-of-loop check produced this (#738).
test result: FAILED. 126 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
[gate] run manifest committed: gate: run manifest for 5210831837fc (#1128)
[gate] RED - failed stages: workspace tests
";

    const DISK_FULL_LOG: &str = "\
[gate] scope: SCOPED: 8 crate(s) selected
[gate] contamination canary (ci-canary)
error: failed to write to `E:\\t\\full.rmeta`: There is not enough space on the disk. (os error 112)
error: could not compile `cpufeatures` (lib) due to 1 previous error
[gate] FAILED: contamination canary (ci-canary) (exit 101)
[gate] ABORTING: the contamination canary failed.
[gate] run manifest committed: gate: run manifest for 3b375369bf0e (#1116)
";

    /// A banner that names NOTHING, beside decoys — the case `banner_seen` exists for.
    ///
    /// Without this fixture the flag is indistinguishable from `!banner_stages.is_empty()`:
    /// every other banner fixture here names at least one stage, so both rules agree on all of
    /// them and a one-token edit removes the distinction in silence
    /// (`unruffled-babbage-df857c-1d` on #1153, who measured the substitution at 16 passed,
    /// 0 failed). "The gate concluded: no failed stages" is an ASSERTION, and falling back to
    /// the printed lines here would re-import the self-test noise in exactly the case where the
    /// banner is most informative.
    const EMPTY_BANNER_LOG: &str = "\
[gate] ps suites
[gate] FAILED: background stdout evidence (exit 2)
[gate] FAILED: stderr evidence (exit 101)
[gate] RED - failed stages:
";

    /// TWO banners, which means the LOG holds two runs (the banner has one producer per run).
    const TWO_BANNER_LOG: &str = "\
[gate] FAILED: stdout evidence (exit 101)
[gate] RED - failed stages: workspace tests
[gate] RED - failed stages: clippy (deny warnings)
";

    /// The shape #1149 measured on the FIRST REAL JEV CALL, reduced to its essentials.
    ///
    /// `D:\graphhelm-slot\runner\1128-20260917T005738.log` carries EIGHT
    /// `[gate] FAILED: <stage>` lines and exactly ONE stage failed. Seven of them are
    /// `ci/gate-*.tests.ps1` cells that fail ON PURPOSE, to prove the gate reports failures --
    /// the gate's own self-test fixtures, printing the gate's own vocabulary into the gate's own
    /// log. The excerpt read all eight and handed the judge "eight unrelated stages failed",
    /// which does not read like one flaky test; the live classification hedged and `same_as`
    /// came back below the yes threshold.
    ///
    /// The authoritative line was in the log the whole time and the excerpt already read it.
    const SELF_TEST_NOISE_LOG: &str = "\
[gate] workspace tests
test eof_arriving_after_the_deadline_is_not_silently_accepted ... FAILED
[gate] FAILED: workspace tests (exit 101)
[gate] ps suites
[gate] FAILED: background stdout evidence (exit 2)
[gate] FAILED: background stderr evidence (exit 2)
[gate] FAILED: background silent failure (exit 2)
[gate] FAILED: stdout evidence (exit 101)
[gate] FAILED: information stream evidence (exit 101)
[gate] FAILED: stderr evidence (exit 101)
[gate] FAILED: no output at all (exit 101)
[gate] RED - failed stages: workspace tests
";

    const REAL_DEFECT_LOG: &str = "\
[gate] clippy (deny warnings)
[gate] workspace tests
test a_tree_diff_is_silent_about_provenance ... FAILED
test the_view_erases ... FAILED
thread 'the_view_erases' panicked at core/graph/src/lint.rs:12:5:
assertion `left == right` failed
[gate] RED - failed stages: clippy (deny warnings), workspace tests
";

    fn known() -> Vec<KnownFlake> {
        vec![KnownFlake {
            issue: 886,
            test: "eof_arriving_after_the_deadline_is_not_silently_accepted".to_owned(),
            summary: "20 ms deadline race in api_http.rs".to_owned(),
        }]
    }

    fn reply(answers: BTreeMap<String, Answer>) -> JudgeReply {
        JudgeReply {
            model: JEV_LATEST.to_owned(),
            answers,
            usage: Usage::default(),
        }
    }

    fn choice(class: &str, confidence: f64) -> Answer {
        Answer::Choice {
            choice: class.to_owned(),
            probabilities: BTreeMap::new(),
            confidence,
        }
    }

    #[test]
    fn a_flake_log_yields_its_stage_test_and_wrapped_panic_location() {
        let excerpt = excerpt(FLAKE_LOG);
        assert_eq!(excerpt.failed_stages, ["workspace tests"]);
        assert_eq!(
            excerpt.failed_tests,
            ["eof_arriving_after_the_deadline_is_not_silently_accepted"]
        );
        let panic = excerpt.first_panic.as_deref().unwrap();
        assert!(panic.starts_with("thread 'eof_arriving"), "{panic}");
        assert!(
            panic.ends_with("panicked at apps\\cli\\tests\\api_http.rs:5858:9:"),
            "the wrapped location was not joined: {panic}"
        );
        assert_eq!(excerpt.first_error_line, None);
        assert_eq!(
            excerpt.tail.last().map(String::as_str),
            Some("[gate] RED - failed stages: workspace tests")
        );
        assert!(!excerpt.tail.iter().any(String::is_empty));
    }

    #[test]
    fn a_disk_full_log_yields_the_aborted_stage_and_the_first_error_line() {
        let excerpt = excerpt(DISK_FULL_LOG);
        assert_eq!(excerpt.failed_stages, ["contamination canary (ci-canary)"]);
        assert!(excerpt.failed_tests.is_empty());
        assert_eq!(excerpt.first_panic, None);
        let error = excerpt.first_error_line.unwrap();
        assert!(error.contains("not enough space on the disk"), "{error}");
    }

    /// #1149: THE BANNER DECIDES when it is there, and the `FAILED:` lines are the fallback for
    /// the one shape that never reaches it.
    ///
    /// The gate's own self-test fixtures print `[gate] FAILED: <stage>` into the gate's own log
    /// on purpose, so counting those lines counts the gate testing itself as stages that failed.
    /// `[gate] RED - failed stages:` is the gate's own answer to the same question, computed
    /// rather than pattern-matched.
    ///
    /// The three arms are one claim each and are not interchangeable:
    ///   - the noise log: a banner beside seven decoys, and only the banner survives;
    ///   - the canary abort: no banner exists, so the fallback must still be live -- without this
    ///     arm the fix could delete the `FAILED:` reading entirely and still pass;
    ///   - the real-defect log: a banner naming TWO stages, so the first arm's `len() == 1` is
    ///     not being satisfied by a rule that simply keeps one.
    #[test]
    fn the_red_banner_decides_the_failed_stages_and_the_failed_lines_are_the_fallback() {
        let noisy = excerpt(SELF_TEST_NOISE_LOG);
        assert_eq!(
            noisy.failed_stages,
            ["workspace tests"],
            "the gate's own self-test fixtures are not stages that failed: {:?}",
            noisy.failed_stages
        );
        // The decoys really are in the text the excerpt read, so the assertion above is a
        // statement about the rule and not about a fixture that never carried them.
        assert!(
            SELF_TEST_NOISE_LOG.contains("[gate] FAILED: stderr evidence (exit 101)"),
            "the fixture must carry the decoys or this cell proves nothing"
        );
        // And the real failure is still reported through its test name, which is what the judge
        // matches a known flake on -- the fix must not cost that.
        assert_eq!(
            noisy.failed_tests,
            ["eof_arriving_after_the_deadline_is_not_silently_accepted"]
        );

        // The canary abort: the run dies before any verdict line, so no banner is ever written
        // and the `FAILED:` reading is the only evidence there is (#1140 deviation 2).
        assert!(!DISK_FULL_LOG.contains(RED_BANNER));
        assert_eq!(
            excerpt(DISK_FULL_LOG).failed_stages,
            ["contamination canary (ci-canary)"]
        );

        // A banner naming two stages still yields two.
        assert_eq!(
            excerpt(REAL_DEFECT_LOG).failed_stages,
            ["clippy (deny warnings)", "workspace tests"]
        );
    }

    /// #1153 review: **the flag is `banner_seen`, not `!banner_stages.is_empty()`, and this is
    /// the only cell that can tell those two apart.**
    ///
    /// Every other banner fixture in this module names at least one stage, so both rules agree
    /// on all of them; `unruffled-babbage-df857c-1d` measured the substitution and the suite
    /// stayed at 16 passed, 0 failed. A one-token edit removed the whole point of the change in
    /// silence, which is the defect class this module was written to close, committed by the
    /// module itself.
    ///
    /// The behaviour asserted here is the position, stated: **"failed stages: " with nothing
    /// after it is the gate ASSERTING that none failed**, not an absence of an answer. Falling
    /// back to the printed lines here would re-import the self-test noise in exactly the case
    /// where the banner is most informative.
    #[test]
    fn a_banner_that_names_nothing_is_an_answer_and_not_an_absence() {
        let excerpt = excerpt(EMPTY_BANNER_LOG);
        assert!(
            excerpt.failed_stages.is_empty(),
            "an empty banner is the gate saying none failed: {:?}",
            excerpt.failed_stages
        );
        // The decoys are in the text, so the emptiness above is a statement about the rule and
        // not about a fixture with nothing in it to find.
        assert!(
            EMPTY_BANNER_LOG.contains("[gate] FAILED: stderr evidence (exit 101)"),
            "the fixture must carry the printed lines the fallback would have taken"
        );
    }

    /// #1153 review: TWO banners POOL, **and a two-run log has no coherent answer** -- which is
    /// the correction of an argument this cell used to carry in its own name.
    ///
    /// It previously read `..._because_every_other_field_of_the_excerpt_pools`, and that premise
    /// is FALSE. `eloquent-jones` measured it on a two-run log and it reproduces here, asserted
    /// below rather than described: stages and tests pool, `first_panic` is FIRST-wins, and the
    /// tail is LAST-wins once a log exceeds `MAX_TAIL_LINES`. So under two runs the excerpt is
    /// already a mixture and no choice for `failed_stages` makes it describe one run. The
    /// consistency argument cannot do the work it was asked to do -- in either direction.
    ///
    /// **This matters beyond the field.** `unruffled-babbage-df857c-1d` argued last-wins, and
    /// withdrew a live objection on the strength of that false premise. A reviewer conceding to
    /// something false is worse than the field being wrong, because it closes the question instead
    /// of deciding it. The question is re-opened by this cell existing in this form.
    ///
    /// What actually decides it is stated at the collecting site in `excerpt`: a malformed input,
    /// pooling discards nothing, last-wins silently drops a stage that really failed, and pooling's
    /// failure is visible where last-wins produces a plausible subset. Whether such a log should be
    /// REFUSED instead is open and out of scope.
    #[test]
    fn two_banners_pool_and_a_two_run_log_has_no_coherent_answer() {
        let two_banners = excerpt(TWO_BANNER_LOG);
        assert_eq!(
            two_banners.failed_stages,
            ["workspace tests", "clippy (deny warnings)"],
            "in log order, both runs, no repeats"
        );

        // THE MEASUREMENT THE OLD NAME GOT WRONG, asserted so the claim cannot rot back into
        // prose. Two runs, each with its own stage, test and panic.
        const TWO_RUNS: &str = "\
[gate] FAILED: printed one (exit 1)
test run_one_test ... FAILED
thread 'run_one_test' panicked at one.rs:1:1:
[gate] RED - failed stages: run one stage
[gate] FAILED: printed two (exit 1)
test run_two_test ... FAILED
thread 'run_two_test' panicked at two.rs:2:2:
[gate] RED - failed stages: run two stage
";
        let mixed = excerpt(TWO_RUNS);
        assert_eq!(
            mixed.failed_stages,
            ["run one stage", "run two stage"],
            "stages pool"
        );
        assert_eq!(
            mixed.failed_tests,
            ["run_one_test", "run_two_test"],
            "tests pool"
        );
        assert!(
            mixed
                .first_panic
                .as_deref()
                .is_some_and(|panic| panic.contains("run_one_test")),
            "first_panic is FIRST-wins, not pooled: {:?}",
            mixed.first_panic
        );
        assert!(
            !mixed
                .first_panic
                .as_deref()
                .is_some_and(|panic| panic.contains("run_two_test")),
            "and run two's panic is absent, which is what makes it a mixture"
        );
    }

    #[test]
    fn a_real_defect_log_yields_every_stage_and_test_once() {
        let excerpt = excerpt(REAL_DEFECT_LOG);
        assert_eq!(
            excerpt.failed_stages,
            ["clippy (deny warnings)", "workspace tests"]
        );
        assert_eq!(
            excerpt.failed_tests,
            ["a_tree_diff_is_silent_about_provenance", "the_view_erases"]
        );
        assert!(
            excerpt
                .first_panic
                .as_deref()
                .unwrap()
                .ends_with("core/graph/src/lint.rs:12:5:")
        );
    }

    #[test]
    fn the_excerpt_is_bounded_whatever_the_log_holds() {
        let long = "x".repeat(MAX_LINE_CHARS * 3);
        let mut log = String::new();
        for index in 0..(MAX_FAILED_TESTS + 5) {
            log.push_str(&format!("test t{index}_{long} ... FAILED\n"));
        }
        for _ in 0..(MAX_TAIL_LINES * 2) {
            log.push_str(&format!("{long}\n"));
        }
        let excerpt = excerpt(&log);
        assert_eq!(excerpt.failed_tests.len(), MAX_FAILED_TESTS);
        assert_eq!(excerpt.tail.len(), MAX_TAIL_LINES);
        assert!(
            excerpt
                .failed_tests
                .iter()
                .chain(excerpt.tail.iter())
                .all(|line| line.chars().count() == MAX_LINE_CHARS)
        );
    }

    #[test]
    fn the_request_asks_one_class_and_one_same_as_per_known_flake() {
        let request = request(&excerpt(FLAKE_LOG), &known());
        assert_eq!(request.model, JEV_LATEST);
        let keys: Vec<&str> = request.questions.keys().map(String::as_str).collect();
        assert_eq!(keys, ["class", "same_as:886"]);
        let Question::Choice { criteria, .. } = &request.questions["class"] else {
            panic!("class is a Choice");
        };
        let offered: Vec<&str> = criteria.keys().map(String::as_str).collect();
        let mut expected = RED_CLASSES.to_vec();
        expected.sort_unstable();
        assert_eq!(offered, expected);
        assert!(
            criteria.values().all(Option::is_some),
            "every class has a rubric"
        );
        assert!(matches!(
            request.questions["same_as:886"],
            Question::Noul { .. }
        ));
        assert_eq!(request.state["knownFlakes"][0]["issue"], 886);
        assert_eq!(
            request.state["excerpt"]["failedStages"],
            serde_json::json!(["workspace tests"])
        );
    }

    #[test]
    fn an_in_class_confident_answer_acts_and_names_the_flake() {
        let reply = reply(BTreeMap::from([
            ("class".to_owned(), choice("known_flake", ACT_THRESHOLD)),
            (
                "same_as:886".to_owned(),
                Answer::Noul {
                    noul: NOUL_YES_THRESHOLD,
                },
            ),
        ]));
        let read = read(&reply, &excerpt(FLAKE_LOG), &known());
        assert_eq!(read.class, "known_flake");
        assert_eq!(read.same_as, [886]);
        assert!(read.acts);
        assert!(!read.unresolved);
    }

    #[test]
    fn a_low_confidence_answer_is_unresolved_and_a_no_names_no_flake() {
        let reply = reply(BTreeMap::from([
            (
                "class".to_owned(),
                choice("real_defect", ACT_THRESHOLD - 1e-9),
            ),
            ("same_as:886".to_owned(), Answer::Noul { noul: 0.1 }),
        ]));
        let read = read(&reply, &excerpt(FLAKE_LOG), &known());
        assert_eq!(read.class, "real_defect");
        assert!(read.same_as.is_empty());
        assert!(!read.acts);
        assert!(read.unresolved);
    }

    /// #1140 review: the two `(0.0..=1.0)` guards had no cell, and removing either left nine
    /// green — a `confidence` of 1.5 is out of range AND above `ACT_THRESHOLD`, so without the
    /// guard the classification ACTS on a number the wire cannot carry.
    #[test]
    fn a_confidence_outside_zero_to_one_never_acts_however_large() {
        for confidence in [1.5_f64, -0.1, f64::INFINITY, f64::NAN] {
            let reply = reply(BTreeMap::from([(
                "class".to_owned(),
                Answer::Choice {
                    choice: "known_flake".to_owned(),
                    probabilities: BTreeMap::new(),
                    confidence,
                },
            )]));
            let out = read(&reply, &excerpt(FLAKE_LOG), &known());
            assert!(!out.acts, "confidence {confidence} must not act");
            assert!(out.unresolved, "confidence {confidence} is unresolved");
        }
        // The control: the same shape inside the range does act, so the cell above is not passing
        // because everything is refused. `environment_void` rather than `known_flake`, because a
        // `known_flake` naming no flake is denied for a DIFFERENT reason (see the silence cell),
        // which would make this control pass for the wrong one.
        let control = reply(BTreeMap::from([(
            "class".to_owned(),
            Answer::Choice {
                choice: "environment_void".to_owned(),
                probabilities: BTreeMap::new(),
                confidence: ACT_THRESHOLD,
            },
        )]));
        assert!(read(&control, &excerpt(FLAKE_LOG), &known()).acts);
    }

    /// The same guard on the `Noul` side: a probability the wire cannot carry never names a flake.
    #[test]
    fn a_noul_outside_zero_to_one_never_names_a_flake() {
        for noul in [1.5_f64, -0.1, f64::INFINITY, f64::NAN] {
            let reply = reply(BTreeMap::from([
                (
                    "class".to_owned(),
                    Answer::Choice {
                        choice: "known_flake".to_owned(),
                        probabilities: BTreeMap::new(),
                        confidence: 0.95,
                    },
                ),
                (same_as_key(886), Answer::Noul { noul }),
            ]));
            let out = read(&reply, &excerpt(FLAKE_LOG), &known());
            assert!(out.same_as.is_empty(), "noul {noul} must name no flake");
            assert!(out.contradicted.is_empty(), "nor contradict one");
        }
        // Control: in range and above the yes threshold, the same answer DOES name it.
        let reply = reply(BTreeMap::from([
            (
                "class".to_owned(),
                Answer::Choice {
                    choice: "known_flake".to_owned(),
                    probabilities: BTreeMap::new(),
                    confidence: 0.95,
                },
            ),
            (
                same_as_key(886),
                Answer::Noul {
                    noul: NOUL_YES_THRESHOLD,
                },
            ),
        ]));
        assert_eq!(
            read(&reply, &excerpt(FLAKE_LOG), &known()).same_as,
            vec![886]
        );
    }

    /// #1140 review, the substantive one: a `same_as` claims the red IS a known flake, and the
    /// judge's yes was grounded only in the prompt. The excerpt is the log's own evidence, so a
    /// yes about a test the log never names is reported as contradicted and counted as no match.
    #[test]
    fn a_same_as_about_a_test_the_log_never_names_is_contradicted_not_matched() {
        let reply = reply(BTreeMap::from([
            (
                "class".to_owned(),
                Answer::Choice {
                    choice: "known_flake".to_owned(),
                    probabilities: BTreeMap::new(),
                    confidence: 0.95,
                },
            ),
            (same_as_key(886), Answer::Noul { noul: 0.99 }),
        ]));
        // DISK_FULL_LOG's failure is a canary abort; it names no test at all, let alone #886's.
        let against_disk_full = read(&reply, &excerpt(DISK_FULL_LOG), &known());
        assert!(
            against_disk_full.same_as.is_empty(),
            "no match against a log that never names the test"
        );
        assert_eq!(
            against_disk_full.contradicted,
            vec![886],
            "and the disagreement is reported"
        );
        // The control: the same reply against the log that DOES name that test matches.
        let against_flake = read(&reply, &excerpt(FLAKE_LOG), &known());
        assert_eq!(against_flake.same_as, vec![886]);
        assert!(against_flake.contradicted.is_empty());
    }

    /// #1140 re-read (second): the judge reaches "acting on zero flake evidence" by SILENCE as
    /// well as by a refuted claim — it answers `known_flake` and names no flake at all. The
    /// previous comment said this was denied and the condition did not implement it. Denied now,
    /// with the boundary the operator controls: when the known-flake list is EMPTY nothing was
    /// asked, so the class is not denied on that ground (denying there would make it unreachable).
    #[test]
    fn a_known_flake_naming_no_flake_does_not_act_unless_none_was_offered() {
        let silent = reply(BTreeMap::from([(
            "class".to_owned(),
            Answer::Choice {
                choice: "known_flake".to_owned(),
                probabilities: BTreeMap::new(),
                confidence: 0.99,
            },
        )]));

        // A flake WAS offered and the judge named none: no evidence, so it does not act.
        let offered = read(&silent, &excerpt(FLAKE_LOG), &known());
        assert!(offered.same_as.is_empty());
        assert!(offered.contradicted.is_empty());
        assert!(
            !offered.acts,
            "silence about an offered flake is not evidence"
        );
        assert!(offered.unresolved);

        // Nothing was offered: the class is not denied on a question nobody asked.
        let unoffered = read(&silent, &excerpt(FLAKE_LOG), &[]);
        assert!(
            unoffered.acts,
            "an empty known-flake list must not make the class unreachable"
        );

        // CONTROL: offered AND named, supported by the log, acts.
        let named = reply(BTreeMap::from([
            (
                "class".to_owned(),
                Answer::Choice {
                    choice: "known_flake".to_owned(),
                    probabilities: BTreeMap::new(),
                    confidence: 0.99,
                },
            ),
            (same_as_key(886), Answer::Noul { noul: 0.99 }),
        ]));
        assert!(read(&named, &excerpt(FLAKE_LOG), &known()).acts);
    }

    /// #1140 re-read: `contradicted` was detected and then ignored, so a `known_flake` whose every
    /// named flake the log refutes still read "acting" on zero flake evidence — the same shape as
    /// the ungrounded `same_as`, one level up. A contradiction now denies the class.
    #[test]
    fn a_known_flake_whose_every_flake_is_contradicted_does_not_act() {
        let flake_reply = reply(BTreeMap::from([
            (
                "class".to_owned(),
                Answer::Choice {
                    choice: "known_flake".to_owned(),
                    probabilities: BTreeMap::new(),
                    confidence: 0.99,
                },
            ),
            (same_as_key(886), Answer::Noul { noul: 0.99 }),
        ]));
        // The disk-full log names no test, so the judge's yes about #886 is contradicted.
        let refuted = read(&flake_reply, &excerpt(DISK_FULL_LOG), &known());
        assert_eq!(refuted.contradicted, vec![886]);
        assert!(refuted.same_as.is_empty());
        assert!(
            !refuted.acts,
            "a claim its own evidence refutes must not act"
        );
        assert!(refuted.unresolved);

        // CONTROL: the same reply against the log that DOES name the test acts.
        let supported = read(&flake_reply, &excerpt(FLAKE_LOG), &known());
        assert_eq!(supported.same_as, vec![886]);
        assert!(
            supported.acts,
            "the control must act, or the cell above proves nothing"
        );

        // AND the denial is scoped to the claim: an `environment_void` says nothing about a
        // flake, so a stray contradiction there does not deny it.
        let other_reply = reply(BTreeMap::from([
            (
                "class".to_owned(),
                Answer::Choice {
                    choice: "environment_void".to_owned(),
                    probabilities: BTreeMap::new(),
                    confidence: 0.99,
                },
            ),
            (same_as_key(886), Answer::Noul { noul: 0.99 }),
        ]));
        let void = read(&other_reply, &excerpt(DISK_FULL_LOG), &known());
        assert_eq!(void.contradicted, vec![886]);
        assert!(void.acts, "the class is not a claim about a flake");
    }

    /// `names_test` compares the path-free name, so `module::name` and `name` are one test.
    #[test]
    fn names_test_compares_the_common_suffix_and_refuses_an_empty_one() {
        let excerpt = excerpt(FLAKE_LOG);
        // The log prints the bare name, so a bare entry and a qualified one both match: the
        // evidence does not distinguish the module, and the leaf is the most it supports.
        assert!(excerpt.names_test("eof_arriving_after_the_deadline_is_not_silently_accepted"));
        assert!(
            excerpt
                .names_test("api_http::eof_arriving_after_the_deadline_is_not_silently_accepted")
        );
        assert!(!excerpt.names_test("a_test_that_did_not_fail"));
        assert!(!excerpt.names_test(""));
        assert!(!excerpt.names_test("   "));

        // #1140 re-read: when BOTH sides are qualified the evidence DOES distinguish, and a
        // cross-module collision must not match. Leaf-only accepted this pair.
        let collision = RedExcerpt {
            failed_stages: Vec::new(),
            failed_tests: vec!["outer::inner::my_flaky_cell".to_owned()],
            first_panic: None,
            first_error_line: None,
            tail: Vec::new(),
        };
        assert!(!collision.names_test("totally::other::my_flaky_cell"));
        assert!(collision.names_test("inner::my_flaky_cell"));
        assert!(collision.names_test("my_flaky_cell"));
    }

    #[test]
    fn a_class_outside_the_vocabulary_is_unresolved_however_confident() {
        let reply = reply(BTreeMap::from([(
            "class".to_owned(),
            choice("green-by-luck", 0.99),
        )]));
        let read = read(&reply, &excerpt(FLAKE_LOG), &known());
        assert_eq!(read.class, "green-by-luck");
        assert!(!read.acts);
        assert!(read.unresolved);
        let empty = super::read(&self::reply(BTreeMap::new()), &excerpt(FLAKE_LOG), &known());
        assert_eq!(empty.class, "");
        assert!(empty.unresolved);
    }

    #[test]
    fn the_excerpt_digest_is_a_pure_function_of_the_excerpt() {
        assert_eq!(
            excerpt_sha256(&excerpt(FLAKE_LOG)),
            excerpt_sha256(&excerpt(FLAKE_LOG))
        );
        assert_ne!(
            excerpt_sha256(&excerpt(FLAKE_LOG)),
            excerpt_sha256(&excerpt(DISK_FULL_LOG))
        );
        assert!(crate::model::is_sha256_hex(&excerpt_sha256(
            &RedExcerpt::default()
        )));
    }
}
