//! The pair invariant #170 names: a node's `completion` block has TWO readers in
//! `externalize.rs` — `collect_completion_content` (registers content slots) and
//! `build_completion_control` (validates and builds the control) — and they disagree about
//! unknown keys. The builder refuses them; the collector walks the keys it knows and ignores
//! the rest.
//!
//! Measured in #170, and the reason this guard exists rather than a doc line: the divergence
//! is ACCIDENTAL. Both functions arrived in the same commit; every sibling nested block
//! (`agent`, `policy`) IS validated in both its collector and its builder, so double
//! validation is this module's pattern rather than redundancy it avoids; and no commit in the
//! repository states an unknown-key policy.
//!
//! **The failure mode this test blocks:** teaching a key to the BUILDER and not to the
//! COLLECTOR. The graph then publishes — the builder knows the key — while any free-form
//! content under it is never registered as a content slot. No error, no diagnostic, content
//! silently absent from the sealed record. #160 is the first change that can trip it: it must
//! add a `customs` arm to the builder.
//!
//! This test does NOT decide #170's open question (share one vocabulary vs. declare-and-point).
//! It only makes silent divergence impossible: whoever adds a builder key must either teach
//! the collector or name the key here as content-free, and both are deliberate acts.
//!
//! **SCOPE, so the file name does not oversell it.** This guards the `completion` block ONLY. The
//! sibling authoring blocks — `agent`, `tool`, and the edge blocks — were NOT examined for the same
//! divergence; #170 says so explicitly. A file called `source_invariants.rs` described as holding
//! "the pair invariant" invites being read as covering the class, and it does not.
//!
//! **DIRECTION, and only one of the two is silent.** This catches a key taught to the BUILDER and
//! not the collector. The opposite — taught to the COLLECTOR and not the builder — does NOT fire
//! here, and that is correct rather than a gap: that direction is loud, because publication refuses
//! the graph. The guarded direction is the one that publishes and then loses content in silence.

const EXTERNALIZE: &str = include_str!("../src/externalize.rs");

/// Keys the builder accepts that carry NO free-form content, so the collector has nothing to
/// register for them. Every entry needs a reason — an unexplained entry here is how the guard
/// would be silenced.
const CONTENT_FREE_KEYS: &[(&str, &str)] = &[
    (
        "contractRef",
        "a reference to a contract, not authored prose: the collector has nothing to seal",
    ),
    (
        "customs",
        // M11 #160, decided against B's own criterion rather than to quiet this guard. The arm
        // emits exactly two things and neither is authored prose: `proofKinds` is a closed
        // vocabulary encoded through `token_array_unique` (kind names, not text), and `budgets`
        // does not pass through the control at all — it crosses to the sealed form as
        // `PersistedNode::customs`, because two spellings of one declaration drift and a reader
        // of the second copy cannot tell which produced a deadline.
        //
        // THE CONDITION THAT ENDS THIS EXEMPTION, written here because whoever adds the next
        // field under `customs` reads this before the guard fires at them: if free-form prose
        // ever lands under `customs` — a description, an expression, a message — this entry is
        // WRONG and the collector must learn to read the key instead. Otherwise the graph
        // publishes and that string is never registered: content lost with no error and no
        // diagnostic, which is the exact failure this guard exists to make impossible.
        "closed-vocabulary tokens and integers, no authored prose: proofKinds is a kind list and \
         budgets cross as PersistedNode::customs, not as content",
    ),
];

/// The body of `fn <name>(`, from its signature to the line that closes it at column 0.
fn function_body<'a>(source: &'a str, name: &str) -> &'a str {
    let signature = format!("\nfn {name}(");
    let start = source
        .find(&signature)
        .unwrap_or_else(|| panic!("{name} must exist in externalize.rs"));
    let rest = &source[start + 1..];
    let end = rest
        .find("\n}\n")
        .unwrap_or_else(|| panic!("{name} must be closed at column 0"));
    &rest[..end]
}

/// The top-level `match key.as_str()` arm literals inside `body`, counted by brace depth so
/// nested matches (the `evidence { type, min }` sub-match, for instance) never leak in.
///
/// Depth is tracked from the match's own opening brace: an arm literal is one that appears at
/// depth 1 immediately before its `=>`.
fn match_arm_keys(body: &str, matched_expression: &str) -> Vec<String> {
    let anchor = format!("match {matched_expression} {{");
    let Some(start) = body.find(&anchor) else {
        return Vec::new();
    };
    let after = &body[start + anchor.len()..];

    let mut keys = Vec::new();
    let mut depth = 1_i32;
    let mut chars = after.char_indices().peekable();
    let mut pending: Vec<String> = Vec::new();

    while let Some((index, character)) = chars.next() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            '"' => {
                // Consume the whole literal at EVERY depth, not just at arm level. A brace inside
                // a string would otherwise be counted as structure: `format!("{{")` is two `{`
                // characters with no matching close, and the depth walk would never come back to
                // arm level. Measured at d0b3f04: externalize.rs has no doubled braces and the
                // function has no escaped quotes, so today's source parses either way — this is
                // here so the guard does not depend on that staying true.
                //
                // The failure it prevents is the WORSE of the two directions. Under-extraction
                // yields an empty set and the landmark below fires ("extraction is broken").
                // Over-extraction — depth stuck at 1, so literals from nested match arms are
                // collected as if they were top-level keys — passes the landmark and produces a
                // RED that blames the invariant for a parser fault. A spurious red costs the same
                // as a vacuous green: both point the next reader at the wrong thing.
                let mut literal = String::new();
                for (_, next) in chars.by_ref() {
                    if next == '"' {
                        break;
                    }
                    literal.push(next);
                }
                if depth == 1 {
                    // Collect only at arm level; decide at `=>` whether it was an arm pattern
                    // (possibly one of several joined by `|`) or something else.
                    pending.push(literal);
                }
            }
            '=' if depth == 1 && after[index..].starts_with("=>") => {
                keys.append(&mut pending);
            }
            ';' | ',' if depth == 1 => pending.clear(),
            _ => {}
        }
    }
    keys
}

/// The node-completion half of `build_completion_control` — everything after the
/// `graph_completion` branch returns, so the graph vocabulary never contaminates the node one.
fn node_completion_arm(body: &str) -> &str {
    let marker = "return builder.finish(control_type);";
    let end_of_graph_arm = body
        .find(marker)
        .expect("build_completion_control must keep its graph_completion early return");
    &body[end_of_graph_arm + marker.len()..]
}

#[test]
fn the_completion_builders_key_vocabulary_is_known_to_its_collector() {
    let builder = function_body(EXTERNALIZE, "build_completion_control");
    let collector = function_body(EXTERNALIZE, "collect_completion_content");
    let builder_keys = match_arm_keys(node_completion_arm(builder), "key.as_str()");

    // LANDMARK, and it is load-bearing: a parsing failure would return an empty set and make
    // the subset assertion below pass vacuously — the exact shape a guard must not have. If
    // this fires, the extraction broke, not the invariant.
    assert!(
        builder_keys.len() >= 2 && builder_keys.iter().any(|key| key == "requires"),
        "extraction is broken, not the invariant: expected the node arm to yield at least two \
         keys including \"requires\", got {builder_keys:?}"
    );

    for key in &builder_keys {
        let known_to_collector = collector.contains(&format!("\"{key}\""));
        let content_free = CONTENT_FREE_KEYS.iter().any(|(name, _)| name == key);
        assert!(
            known_to_collector || content_free,
            "`build_completion_control` accepts the completion key \"{key}\" and \
             `collect_completion_content` never reads it (#170).\n\
             A graph declaring it PUBLISHES — the builder knows the key — while any free-form \
             content under it is never registered as a content slot: no error, no diagnostic, \
             content silently missing from the sealed record.\n\
             Choose deliberately: teach the collector to read \"{key}\", or add it to \
             CONTENT_FREE_KEYS in this test WITH the reason it carries no authored prose."
        );
    }
}

#[test]
fn every_content_free_key_is_still_a_key_the_builder_accepts() {
    let builder = function_body(EXTERNALIZE, "build_completion_control");
    let builder_keys = match_arm_keys(node_completion_arm(builder), "key.as_str()");

    // The exemption list may not outlive what it exempts. Without this, a key removed from the
    // builder leaves a stale entry that would silently exempt it again if it ever returned.
    for (key, reason) in CONTENT_FREE_KEYS {
        assert!(
            builder_keys.iter().any(|accepted| accepted == key),
            "CONTENT_FREE_KEYS still exempts \"{key}\" ({reason}), but \
             `build_completion_control` no longer accepts that key — remove the exemption"
        );
    }
}

// SEALED BEFORE THE SLOT (B, 2026-08-20), so the guard proves itself rather than waiting for
// someone else's PR to prove it:
//
//   P1  Unsabotaged at d0b3f04: BOTH tests GREEN. The builder accepts
//       `contractRef | requires | forbids`; the collector reads requires/forbids; contractRef
//       is the one declared exemption.
//   P2  Sabotage = add one arm to build_completion_control's NODE match that the collector
//       never reads (e.g. `"customs" => builder.count("customsCount", 0)?`). Expect RED in
//       `the_completion_builders_key_vocabulary_is_known_to_its_collector`, AT THE SUBSET
//       ASSERTION, naming the key.
//   P3  The red must NOT land on the landmark assertion. A landmark failure means the
//       extraction broke — that is a sabotage of the instrument, not of the invariant, and
//       counts as vacuous red, not as evidence.
//   P4  Revert restores P1 by identity (git diff --exit-code), not by re-measurement.
//
// UNINFORMATIVE cell: if P1 comes back RED today, my reading of the pair is wrong — the
// builder accepts a key the collector does not read ALREADY — and the design is re-derived
// before any fix is written.

// PROTOCOL ADDENDUM (B, 2026-08-20, after the orchestrator amended the slot law). The four
// predictions above are UNCHANGED — this fixes how they get measured, not what they claim.
//
// The amended law: per-package `cargo clean -p` is NECESSARY BUT NOT SUFFICIENT. It can leave
// cargo's freshness ledger believing a removed rlib is still current; the dependent links the old
// artifact, `check` (which reads `.rmeta`) passes and `build` explodes. Signature: check green,
// build red. So a gate whose verdict gets CITED runs a FULL `cargo clean` first.
//
// This test needs that more than most, and the reason is its own construction: it reads its
// subject through `include_str!("../src/externalize.rs")`, so the source text is baked into the
// binary AT COMPILE TIME. Under a corrupted freshness ledger a reused test binary would carry an
// OLD copy of externalize.rs and assert against source that no longer exists. The failure is
// invisible from the outside — the assertions still pass, on the wrong bytes.
//
// That is exactly the vacuous shape P3 exists to reject, arriving through the build system rather
// than through the parser. So P1's green is only evidence when all three hold:
//   - the run followed a FULL clean (enumerate with `cargo metadata`, never by hand);
//   - the wall clock is consistent with a cold tree (a suspiciously fast pass is the tell that
//     unmasked this in the first place: clippy "passing" in 53s against 21 cold packages);
//   - CARGO_TARGET_DIR=D:/graphhelm-target-b170 was set explicitly ON THE SAME LINE as cargo.
// A green missing any of the three does not distinguish "the invariant holds" from "the binary
// was linked against someone else's tree", and P1 is precisely the cell that cannot be vacuous.
//
// TREE-IDENTITY PRE-FLIGHT, added after measuring that 12 of the 23 directories under
// .claude/worktrees/ are NOT registered worktrees — they are empty dirs nested inside the main
// checkout, so a bare git command answers about that checkout (branch issue-m09-arming-the-alarm,
// pre-#100) and cargo walks up to ITS Cargo.toml. This file does not exist in that tree, and a
// test that does not exist cannot fail: "0 tests" reads as a pass. Before the clean and before
// cargo, three checks that turn "green" into "green about my code":
//   - `git worktree list` shows the directory;
//   - `rev-parse --show-toplevel` returns that directory ITSELF, not its parent;
//   - this file exists under that toplevel.
//
// CLOCK-ORACLE PRECONDITION, added after the slot protocol moved (2026-08-20). The wall-clock is
// the third of the three conditions above, and it is the one another process can corrupt without
// touching this tree: a concurrent `cargo check` competes for CPU and inflates the duration, so a
// cold-tree run can be made to look warm-tree slow, or a warm one hidden in the noise. Before
// citing a duration, read D:/graphhelm-slot/check-activity.log for START/END entries overlapping
// the run window, and record the window boundaries alongside the number. A duration measured with
// competing activity is not WRONG, it is WITHOUT DEFINITION -- carry that fact rather than
// averaging it away. The lock answers "who holds it now"; only the append-only log answers "what
// else was running", and the clock is worthless without the second question.
//
// HAND-VERIFICATION OF P1'S KEY SET, done without cargo while the slot was held by another agent
// (B, 2026-08-20). Read directly from core/governor/src/externalize.rs at d0b3f04, by a method
// independent of this file's parser (awk over the function body, listing `=>` lines):
//
//   graph arm  (before the early return): "terminalNodes", "requires", "allowWaivers", "statuses"
//   node arm   (after  the early return): "contractRef", "requires" | "forbids", then `_ =>`
//
// So P1's expectation — the node arm yields contractRef/requires/forbids — is confirmed against
// the SOURCE. It is NOT confirmed against the TEST: nothing here has been compiled or executed,
// and the whole point of the parser is that reading and extracting can disagree. If the run
// reports a different set, the discrepancy is in this file's extraction, not in externalize.rs,
// and that is the landmark's job to say. Recorded so the two halves stay separable.
//
// PRE-FLIGHT, AMENDED — three questions, three commands, none answering for the others. The earlier
// wording ("the file under test exists under that toplevel") named no command, and an adoption
// filled that hole from memory with `git ls-files --error-unmatch`, which answers a DIFFERENT
// question (tracked?) and blocks on any untracked file — i.e. on every test under development,
// including this one. A rule that needs a command must carry the command:
//
//   exists?      test -e core/governor/tests/source_invariants.rs
//   participates? core/governor/Cargo.toml declares no [[test]] and no required-features, and no
//                 package in the tree sets autotests=false (measured at origin/main), so this file
//                 is auto-discovered and really is built. Presence is not participation: elsewhere
//                 in this repo `[[test]]` targets carry required-features and silently do not run
//                 without them, and "0 tests" reads as a pass.
//   committed?   git ls-files --error-unmatch <path> -- ONLY for a citable verdict, never as a
//                precondition for running. Today this file is untracked and that is correct.
//
// SABOTAGE PRE-VERIFIED BY READING (B, before the slot). Every step below is established from
// source, NOT from execution — nothing here has been run, and none of it discharges P1-P4.
//
//   the edit        `"customs" => builder.count("customsCount", 0)?,` in the NODE match.
//   it compiles     `fn count(&mut self, key: &str, value: usize) -> Result<(), GovernorError>`
//                   (externalize.rs:1746); four existing arms call it in this exact shape. A
//                   sabotage that fails to COMPILE would land the red on rustc, not on the
//                   assertion -- vacuous by P3.
//   parser handles it   `"customs"` sits at depth 1 before `=>` and is collected; `"customsCount"`
//                   also sits at depth 1 AFTER the `=>`, but the comma inside the call clears
//                   `pending` before the arm ends, so it is not mistaken for a key.
//   it must fire    `collect_completion_content` reads exactly "expression", "forbids",
//                   "requires" -- "customs" appears in it ZERO times -- and "customs" is not in
//                   CONTENT_FREE_KEYS. So the subset assertion has no way to pass.
//   landmark holds  keys become {contractRef, requires, forbids, customs}: 4 >= 2 and "requires"
//                   present, so the landmark does NOT fire and the red lands where P2 says.
//
// Also read, and it is why P1 is expected green: the builder's node arm accepts exactly
// contractRef / requires / forbids. Two are read by the collector; contractRef is the one declared
// exemption. Read, not run: if the run disagrees with any line above, the run wins and the
// UNINFORMATIVE cell applies.
//
// SABOTAGE KEY CHANGED, and P2 is NOT moved. P2 names the property -- "add one arm to the node
// match that the collector never reads" -- and offers `"customs"` only as an example ("e.g."). That
// example has since become a REAL key: A decided `customs` becomes a declared CONTENT_FREE_KEYS
// entry, because its node arm emits only closed-vocabulary tokens (`proof_kinds`) and integers
// (`budgets`), neither of which is authored prose. Once that lands, sabotaging with `customs` would
// hit the exemption and NOT fire -- the sabotage would silently prove nothing.
//
// So the probe key is `"zzzUnreadProbe"`: chosen precisely because no lane will ever make it
// legitimate, which keeps this measurement independent of another agent's landing order. The
// sealed property is unchanged, and every pre-verified step above holds for it: it is absent from
// `collect_completion_content` (which reads only expression/forbids/requires), absent from
// CONTENT_FREE_KEYS, and sits at depth 1 before its `=>` so the parser collects it.
//
//   sabotage line:  `"zzzUnreadProbe" => builder.count("zzzUnreadProbeCount", 0)?,`
//
// A sabotage whose outcome depends on WHEN another lane merges is not a controlled experiment.
//
// TARGET-DIR CORRECTION (B, after H's ED-10 answer). The line above previously named
// `D:/graphhelm-target-m10`. That directory is RETIRED for gate runs: the shared target dir let
// artifacts fingerprinted by name+version arrive from another worktree, so each lane now uses its
// own `D:/graphhelm-target-<lane>`. Isolation by construction removes the contamination that the
// full `cargo clean` was compensating for -- the clean is still required for a verdict I cite, but
// it is no longer the only thing standing between this measurement and another agent's artifacts.
//
// Also corrected, against myself: the isolated type-check earlier today ran in `D:/gh-check/b`.
// The convention is `D:/gh-check/<agent>/<issue>` -- per agent AND per branch. Mine was per-agent
// only, so a second branch of mine would have shared that directory. The isolation property I
// claimed held (no other agent could reach it), but the claim was narrower than the convention,
// and the next run uses `D:/gh-check/b/170`.
