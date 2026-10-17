"""Plant the five #1476 defects into the event store append and see whether its contract tests catch them.

Each defect is one exact-text substitution in `core/events/src/local.rs`. For each one the file is
mutated in place, the append effect-contract tests run through the build slot, and the original
bytes are restored before the next defect, whatever happened. The verdict per defect is:

- `caught`: the tests compiled, ran, and at least one failed;
- `missed`: the tests compiled, ran, and all passed with the defect in place;
- `UNOBSERVED`: the anchor did not match exactly once, the mutated tree did not compile, or the
  unmutated baseline was not green. An unobserved defect is never counted as caught.

The issue's bar: pass when all five are caught, kill when fewer than four are.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

TARGET = "core/events/src/local.rs"
TESTS = ("append_effect_contract", "local_atomicity")
NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)


@dataclass(frozen=True)
class Defect:
    id: str
    what: str
    anchor: str
    replacement: str


DEFECTS: tuple[Defect, ...] = (
    Defect(
        "drop-dedup",
        "an append never looks for an earlier batch with the same idempotency keys",
        "            if !requested_keys.is_disjoint(&batch_keys) {",
        "            if false && !requested_keys.is_disjoint(&batch_keys) {",
    ),
    Defect(
        "reuse-key",
        "a divergent reuse of an idempotency key is accepted instead of refused",
        "                return Err(EventRepositoryError::IdempotencyConflict);\n"
        "            }\n"
        "        }\n"
        "\n"
        "        validate_graph_successors(request, &state)?;",
        "                continue;\n"
        "            }\n"
        "        }\n"
        "\n"
        "        validate_graph_successors(request, &state)?;",
    ),
    Defect(
        "lose-ack",
        "a resume after a lost ack does not return the batch the first attempt committed",
        "                if batch.request_digest == request_digest && batch_keys == requested_keys {",
        "                if false && batch.request_digest == request_digest && batch_keys == requested_keys {",
    ),
    Defect(
        "concurrent-writers",
        "the append path takes the shared lock, so two writers can append at once",
        "    ) -> Result<T, EventRepositoryError> {\n"
        "        self.with_lock(Exclusivity::Exclusive, operation)\n"
        "    }",
        "    ) -> Result<T, EventRepositoryError> {\n"
        "        self.with_lock(Exclusivity::Shared, operation)\n"
        "    }",
    ),
    Defect(
        "kill-mid-write",
        "an unterminated journal tail is read on instead of refused",
        "        if !bytes.is_empty() && bytes.last() != Some(&b'\\n') {\n"
        "            return Err(EventRepositoryError::IntegrityAt(\n"
        "                \"load_state:journal-tail-unterminated\",",
        "        if false && !bytes.is_empty() && bytes.last() != Some(&b'\\n') {\n"
        "            return Err(EventRepositoryError::IntegrityAt(\n"
        "                \"load_state:journal-tail-unterminated\",",
    ),
)


def apply(source: str, defect: Defect) -> str | None:
    """The source with exactly one anchor replaced, or None when the anchor is not there exactly once."""
    if source.count(defect.anchor) != 1:
        return None
    return source.replace(defect.anchor, defect.replacement, 1)


def classify(exit_code: int, output: str) -> str:
    """Turn one cargo test run into ran-green, ran-red or not-run (did not compile or did not start)."""
    if exit_code != 0 and "test result: FAILED" in output:
        return "red"
    if exit_code == 0 and "test result: ok" in output:
        return "green"
    return "not-run"


Runner = Callable[[Path], "tuple[int, str]"]


def slot_runner(root: str, lane: str, jobs: int, graphhelm: str) -> Runner:
    def run(repo: Path) -> tuple[int, str]:
        cargo = ["cargo", "+1.97.1", "test", "--locked", "-p", "graphhelm-events"]
        for test in TESTS:
            cargo += ["--test", test]
        cargo += ["--", "--test-threads=2"]
        command = [graphhelm, "workspace", "slot", "--root", root, "--lane", lane,
                   "--jobs", str(jobs), "--label", "keel-planted-defects", "--", *cargo]
        done = subprocess.run(command, cwd=repo, capture_output=True, text=True,
                              encoding="utf-8", errors="replace", creationflags=NO_WINDOW)
        return done.returncode, done.stdout + done.stderr
    return run


def plant(repo: Path, runner: Runner, defects: tuple[Defect, ...] = DEFECTS) -> dict:
    """Run the baseline and every defect; the target file is byte-identical afterwards."""
    target = repo / TARGET
    original = target.read_bytes()
    digest = hashlib.sha256(original).hexdigest()
    source = original.decode("utf-8")
    started = time.monotonic()
    code, output = runner(repo)
    baseline = classify(code, output)
    rows = []
    try:
        for defect in defects:
            row = {"id": defect.id, "what": defect.what}
            mutated = apply(source, defect)
            if baseline != "green":
                row.update(verdict="UNOBSERVED", reason=f"baseline was {baseline}, not green")
            elif mutated is None:
                row.update(verdict="UNOBSERVED", reason="anchor did not match exactly once")
            else:
                target.write_bytes(mutated.encode("utf-8"))
                try:
                    code, output = runner(repo)
                finally:
                    target.write_bytes(original)
                result = classify(code, output)
                row.update(verdict={"red": "caught", "green": "missed"}.get(result, "UNOBSERVED"),
                           exit=code)
                if result == "not-run":
                    row["reason"] = "the mutated tree did not compile or the tests did not run"
            rows.append(row)
    finally:
        target.write_bytes(original)
    if hashlib.sha256(target.read_bytes()).hexdigest() != digest:
        raise RuntimeError(f"{TARGET} was not restored byte-identically")
    caught = sum(row["verdict"] == "caught" for row in rows)
    observed = all(row["verdict"] != "UNOBSERVED" for row in rows)
    status = ("pass" if caught == len(rows) else "kill" if observed and caught < 4 else
              "partial" if observed else "UNOBSERVED")
    return {"baseline": baseline, "defects": rows, "caught": caught, "total": len(rows),
            "status": status, "seconds": round(time.monotonic() - started, 1),
            "targetSha256": digest}


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--repo", default=".")
    ap.add_argument("--root", required=True, help="build slot root")
    ap.add_argument("--lane", required=True)
    ap.add_argument("--jobs", type=int, default=2)
    ap.add_argument("--graphhelm", default=os.environ.get("GRAPHHELM_CLI", "graphhelm"))
    ap.add_argument("--output", help="write the JSON report here as well")
    args = ap.parse_args(argv)
    repo = Path(args.repo).resolve()
    report = plant(repo, slot_runner(args.root, args.lane, args.jobs, args.graphhelm))
    text = json.dumps(report, indent=2)
    if args.output:
        Path(args.output).write_text(text + "\n", encoding="utf-8")
    print(text)
    return 0 if report["status"] in ("pass", "partial") else 1


if __name__ == "__main__":
    sys.exit(main())
