#!/bin/sh
# #161 sabotage runner. Every row of .factory/j-agent-161-sabotage.md, one pass, isolated dir.
#
# Why an isolated CARGO_TARGET_DIR and not the shared one: the shared dir is what makes
# `cargo clean -p` mandatory at slot start, because artifacts fingerprinted by name+version can
# link code from another worktree. A dir nobody else writes to removes that failure mode by
# construction, so the slot here is about machine contention, never about correctness.
set -e
cd "$(dirname "$0")/../.."
# Per-LANE, never per-agent: a dir shared across this agent's branches is the same cross-tree
# contamination the isolation exists to prevent. This default was `D:/gh-check/j` and was wrong on
# both counts - the check-dir convention used for tests, and no per-branch segment. It never had a
# chance to complain: the matrix was only ever correct because J_TARGET was passed on the command
# line. A wrong default is silent by construction. The header below records the dir into the
# RESULTS file, which is why a reader can tell where a run built instead of trusting this line.
DIR=${J_TARGET:-D:/graphhelm-target-j161}
T=core/events/tests/execution_projection.rs
P=core/events/src/projection.rs
OUT=.factory/j-161-sabotage-results.txt
NAME=the_identity_registry_folds_deterministically_and_revocation_removes

run() {
  echo "=== $1 ==="
  # `|| true`: a sabotage that does NOT fail is a result, not a script error.
  CARGO_TARGET_DIR="$DIR" cargo test -p graphhelm-events --test execution_projection "$NAME" \
    2>&1 | grep -E "^(test |thread |assertion|  left|  right|test result|error| *Blocking| *Compiling| *Finished)" || true
  echo "--- end $1"
}

: > "$OUT"
{
  echo "base: $(git rev-parse HEAD)   dir: $DIR"
  git checkout -- "$P"
  run S0-control

  for S in S1 S2 S3; do
    git checkout -- "$P"
    python .factory/bin/j-161-sabotage-mutate.py "$S"
    run "$S"
  done

  # S1' / S2': the SAME mutations against the assertions as they stood BEFORE the two identities
  # were added. These are the rows that can report that the strengthening bought nothing.
  python .factory/bin/j-161-strip-new-assertions.py
  for S in S1 S2; do
    git checkout -- "$P"
    python .factory/bin/j-161-sabotage-mutate.py "$S"
    run "$S-prime-old-assertions"
  done

  git checkout -- "$P" "$T"
  run S0-control-restored
} 2>&1 | tee -a "$OUT"
echo "WROTE $OUT"
