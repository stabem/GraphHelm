"""Measure the wall time the #1476 Effect Contract adds per PR (slice d).

Three measurements, pre-registered in docs/keel/EFFECT_CONTRACT.md before any number was taken:

1. `check` time: `python evidence_key.py check --rev R` as a fresh process, for REUSE_REVS revisions
   that answer reuse and INVALIDATE_REVS that answer invalidate (found by walking first-parent
   history back from `--head`, at most WALK_LIMIT commits). The first run on a revision is `cold`,
   the next WARM_RUNS are `warm`. Median and spread (min, max) per case.
2. Observer rerun cost: seconds passed in with `--observer-seconds` (each a separate slot run of
   `cargo +1.97.1 test --locked -p graphhelm-events --test append_effect_contract`) and the
   reached-plan seconds with `--reached-seconds`, for the same diff. Run by hand in the build slot:
   this script never starts cargo.
3. Reuse fraction: over the last WINDOW first-parent commits of `--head` (each a squash-merged PR),
   the closure key at the commit against the key at its first parent. Equal key = `reuse`, else
   `invalidate`. Local git only; no network.

Everything except the seconds is deterministic for a given history.

Usage:
  python tools/keel-effect/measure_cost.py --repo . --head origin/main --out <file.json>
      [--observer-seconds 12.1,11.8,12.0] [--reached-seconds 301.5]
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import statistics
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("evidence_key", HERE / "evidence_key.py")
ek = importlib.util.module_from_spec(SPEC)
sys.modules["evidence_key"] = ek
SPEC.loader.exec_module(ek)

WINDOW = 50
REUSE_REVS = 5
INVALIDATE_REVS = 5
WARM_RUNS = 5
WALK_LIMIT = 400
NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)


def first_parent(repo: Path, head: str, limit: int) -> list[str]:
    out = ek.git(repo, "rev-list", "--first-parent", f"--max-count={limit}", head)
    return out.split()


def key_at(repo: Path, rev: str) -> str:
    return ek.state(repo, rev)["key"]


def decision_at(repo: Path, record_key: str, rev: str) -> str:
    return "reuse" if key_at(repo, rev) == record_key else "invalidate"


def timed_check(repo: Path, rev: str) -> tuple[float, str]:
    started = time.perf_counter()
    done = subprocess.run([sys.executable, str(HERE / "evidence_key.py"), "check", "--repo", str(repo),
                           "--rev", rev], capture_output=True, text=True, encoding="utf-8",
                          creationflags=NO_WINDOW)
    elapsed = time.perf_counter() - started
    if done.returncode not in (0, 1):
        raise RuntimeError(f"check failed at {rev}: {done.stderr.strip()[:200]}")
    return elapsed, json.loads(done.stdout)["decision"]


def spread(values: list[float]) -> dict:
    return {"n": len(values), "median_s": round(statistics.median(values), 3),
            "min_s": round(min(values), 3), "max_s": round(max(values), 3)} if values else {"n": 0}


def check_times(repo: Path, head: str) -> dict:
    record = json.loads((repo / ek.RECORD).read_text(encoding="utf-8"))
    chosen: dict[str, list[str]] = {"reuse": [], "invalidate": []}
    want = {"reuse": REUSE_REVS, "invalidate": INVALIDATE_REVS}
    for rev in first_parent(repo, head, WALK_LIMIT):
        case = decision_at(repo, record["key"], rev)
        if len(chosen[case]) < want[case]:
            chosen[case].append(rev)
        if all(len(chosen[c]) >= want[c] for c in want):
            break
    result = {"record_commit": record["commit"], "revs": chosen, "cases": {}}
    for case, revs in chosen.items():
        cold, warm, per_rev = [], [], {}
        for rev in revs:
            runs = []
            for _ in range(1 + WARM_RUNS):
                elapsed, got = timed_check(repo, rev)
                if got != case:
                    raise RuntimeError(f"{rev}: expected {case}, check answered {got}")
                runs.append(elapsed)
            cold.append(runs[0])
            warm.extend(runs[1:])
            per_rev[rev] = [round(r, 3) for r in runs]
        result["cases"][case] = {"cold": spread(cold), "warm": spread(warm), "runs_s_by_rev": per_rev}
    return result


def reuse_fraction(repo: Path, head: str) -> dict:
    commits = first_parent(repo, head, WINDOW)
    rows, counts = [], {"reuse": 0, "invalidate": 0, "error": 0}
    for commit in commits:
        parents = ek.git(repo, "rev-list", "--parents", "-n", "1", commit).split()[1:]
        try:
            case = "reuse" if key_at(repo, commit) == key_at(repo, parents[0]) else "invalidate"
        except Exception as exc:  # a commit whose manifests cannot be read counts as error, never reuse
            case = "error"
            rows.append({"commit": commit, "decision": case, "why": str(exc)[:120]})
            counts[case] += 1
            continue
        counts[case] += 1
        rows.append({"commit": commit, "decision": case})
    total = len(commits)
    return {"window": total, "counts": counts,
            "reuse_fraction": round(counts["reuse"] / total, 3) if total else None, "prs": rows}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--repo", default=".")
    parser.add_argument("--head", default="origin/main")
    parser.add_argument("--out", required=True)
    parser.add_argument("--observer-seconds", default="")
    parser.add_argument("--reached-seconds", default="")
    args = parser.parse_args(argv)
    repo = Path(args.repo)
    head = ek.git(repo, "rev-parse", f"{args.head}^{{commit}}").strip()
    observer = [float(v) for v in args.observer_seconds.split(",") if v]
    reached = [float(v) for v in args.reached_seconds.split(",") if v]
    report = {"head": head,
              "check_time": check_times(repo, head),
              "observer_rerun": {"command": ek.OBSERVER, "runs_s": observer, **spread(observer)},
              "reached_plan": {"runs_s": reached, **spread(reached)},
              "reuse_fraction": reuse_fraction(repo, head)}
    Path(args.out).write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8", newline="\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
