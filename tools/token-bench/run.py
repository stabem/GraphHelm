#!/usr/bin/env python3
"""token-bench: how many tokens does one task cost, with and without GraphHelm?

One run = one task x one arm. The task is a closed issue whose fix is hidden (the
worktree is cut at the fix's parent). A fresh headless `claude -p` session gets the
issue text and works in that worktree. Afterwards the oracle - the test file that
landed with the real fix, which the agent never saw - is copied in and run: PASS or FAIL.
The token numbers come from the session's own JSON result, not from an estimate.

Arms:
  a  the agent alone in the worktree
  b  the agent told to work through the GraphHelm MCP (start -> briefing ->
     compile_context -> evidence -> signal); the MCP config is the repository's .mcp.json

Usage:
  python tools/token-bench/run.py run  --task 1145 --arm a [--model <id>] [--timeout-min 30]
  python tools/token-bench/run.py table

Results append to tools/token-bench/results.jsonl - one JSON object per run, never rewritten.
This is a hand-run instrument. It has no test suite on purpose.
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import shutil
import subprocess
import sys
import time
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
TASKS = json.loads((HERE / "tasks.json").read_text(encoding="utf-8"))["tasks"]
RESULTS = HERE / "results.jsonl"
# TOKEN_BENCH_MCP_CONFIG points arm b at a different MCP config - e.g. one whose command is a freshly
# built graphhelm.exe - without touching the repository's .mcp.json or the binary the Runtime holds open.
MCP_CONFIG = Path(os.environ.get("TOKEN_BENCH_MCP_CONFIG") or r"F:\github\GraphHelm\.mcp.json")

BENCH_VERSION = 6  # v6: signal example complete (evidence, emittedAt). v5: arm b against 32d0306e (every refusal carries the minimal valid call; start observes after a timeout). v4: arm b may run against a candidate MCP binary (mcpConfig recorded per row). v3: the prompt forbids running the gate inside the checkout (1044 arm b spent ~25 min on it). v1 rows used a shared worktree with a remote: the agent could see the fix. Invalid.

COMMON_PREFIX = """This issue is OPEN and UNFIXED. This checkout is the only source of truth: do not consult
GitHub, any remote, or any other clone - they do not exist for this task. Fix the defect in this
checkout and add the test that proves it. Do not stop at analysis: the task is done when the change
is on disk. Verify with the targeted tests of the crate or script you touched only: do NOT run
`ci/gate.ps1`, the full workspace test suite, or any other repository-wide gate - the bench runs
its own oracle afterwards, and a gate run inside this checkout measures the gate, not you.

"""

ARM_B_PREFIX = """You are working INSIDE the GraphHelm methodology. Before touching code:
1. call the `graphhelm` MCP tool `start` with the objective below and remember the executionId;
2. call `briefing` and `compile_context` on it and use what they return;
3. record what you find and what you change with `evidence` on that execution;
4. when done, `signal` completion on it.
Every code decision must be preceded by the context you got from GraphHelm, not from a raw search.

Objective / issue:

"""


def sh(args: list[str], cwd: Path | None = None, check: bool = True, **kw) -> subprocess.CompletedProcess:
    return subprocess.run(args, cwd=cwd, check=check, text=True, capture_output=True, **kw)


def git_show(sha: str, path: str) -> str:
    return sh(["git", "show", f"{sha}:{path}"], cwd=REPO).stdout


def make_worktree(task_id: str, arm: str, parent_sha: str) -> Path:
    # NOT a git worktree: a worktree shares the repository's remote and every ref, so the
    # agent can `git fetch origin main` and find the fix already landed (bench v1 did exactly
    # that: "Already fixed. Nothing to do.", $1.14). The bench checkout is an archive of the
    # parent commit inside a fresh repository with ONE commit and NO remote - it has no future.
    name = f"tb-{task_id}-{arm}-{dt.datetime.utcnow():%Y%m%dT%H%M%S}-{uuid.uuid4().hex[:6]}"
    base = REPO.parent if REPO.parent.name == "worktrees" else REPO / ".claude" / "worktrees"
    wt = base / name
    wt.mkdir(parents=True)
    archive = subprocess.run(["git", "archive", "--format=tar", parent_sha], cwd=REPO, check=True, capture_output=True).stdout
    import io, tarfile
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:") as tar:  # not the shell's tar: msys mangles Windows paths
        tar.extractall(wt)
    sh(["git", "init", "-q", "-b", "main"], cwd=wt)
    sh(["git", "-c", "user.name=bench", "-c", "user.email=bench@invalid", "-c", "commit.gpgsign=false",
        "add", "-A"], cwd=wt)
    sh(["git", "-c", "user.name=bench", "-c", "user.email=bench@invalid", "-c", "commit.gpgsign=false",
        "commit", "-qm", f"baseline: {parent_sha[:8]} (bench task {task_id})"], cwd=wt)
    return wt


def remove_worktree(wt: Path) -> None:
    shutil.rmtree(wt, ignore_errors=True)


def bench_env(task: dict) -> dict:
    # a Rust task shares ONE warm target dir across its arms (arms run one at a time), so neither
    # the agent nor the oracle pays a cold workspace build; tokens are what is measured, not cargo
    env = dict(os.environ)
    if task.get("sharedTargetDir"):
        base = REPO.parent if REPO.parent.name == "worktrees" else REPO / ".claude" / "worktrees"
        env["CARGO_TARGET_DIR"] = str(base / task["sharedTargetDir"])
    return env


def run_agent(wt: Path, prompt: str, arm: str, model: str | None, timeout_min: int, task: dict) -> tuple[dict, str, float]:
    cmd = ["claude", "-p", "--output-format", "json", "--dangerously-skip-permissions",
           "--disallowedTools", "Bash(gh *)", "WebFetch", "WebSearch"]
    if model:
        cmd += ["--model", model]
    if arm == "b":
        if not MCP_CONFIG.exists():
            sys.exit(f"arm b needs {MCP_CONFIG}")
        cmd += ["--mcp-config", str(MCP_CONFIG), "--allowedTools", "mcp__graphhelm"]
    t0 = time.monotonic()
    proc = subprocess.run(cmd, cwd=wt, input=prompt, text=True, capture_output=True,
                          timeout=timeout_min * 60, shell=(os.name == "nt"), env=bench_env(task))
    wall = time.monotonic() - t0
    raw = proc.stdout.strip()
    try:
        result = json.loads(raw)
    except json.JSONDecodeError:
        result = {"unparseable_stdout": raw[-4000:], "stderr": proc.stderr[-4000:], "exit": proc.returncode}
    return result, proc.stderr, wall


def transcript_usage(session_id: str | None) -> dict:
    """Sum the session's own transcript. The `usage` block in `claude -p`'s JSON result is NOT the
    session total - on task 1044 arm b it reported 3 turns / 297 output tokens for a 113-step,
    $8.68 session - so the transcript is the instrument and the JSON is kept only as a cross-check."""
    if not session_id:
        return {}
    hits = list((Path.home() / ".claude" / "projects").glob(f"*/{session_id}.jsonl"))
    if not hits:
        return {"missing": True}
    tot = {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "assistantMessages": 0}
    for line in hits[0].read_text(encoding="utf-8", errors="replace").splitlines():
        try:
            d = json.loads(line)
        except json.JSONDecodeError:
            continue
        if d.get("type") != "assistant":
            continue
        u = (d.get("message") or {}).get("usage") or {}
        tot["input"] += u.get("input_tokens", 0); tot["output"] += u.get("output_tokens", 0)
        tot["cacheRead"] += u.get("cache_read_input_tokens", 0); tot["cacheWrite"] += u.get("cache_creation_input_tokens", 0)
        tot["assistantMessages"] += 1
    tot["path"] = str(hits[0])
    return tot


def run_oracle(wt: Path, task: dict) -> tuple[str, int, str]:
    oracle = task["oracle"]
    for entry in oracle.get("files") or [oracle]:
        target = wt / entry["file"]
        target.parent.mkdir(parents=True, exist_ok=True)
        # the bench's own oracle file when the task has one (the fix's suite restricted to the
        # issue's ask), else the test file exactly as the real fix landed it
        if entry.get("source"):
            target.write_bytes((HERE / entry["source"]).read_bytes())
        else:
            target.write_text(git_show(task["fixSha"], entry["file"]), encoding="utf-8")
    proc = subprocess.run(oracle["command"], cwd=wt, text=True, capture_output=True, env=bench_env(task))
    verdict = "PASS" if proc.returncode == 0 else "FAIL"
    # keep the lines that say WHICH assertion failed; the raw tail was CLIXML progress noise from stderr
    lines = [l for l in proc.stdout.splitlines() if "FAIL" in l or "assertions" in l or "HARNESS" in l]
    tail = "\n".join(lines)[-2000:] or proc.stdout[-800:]
    return verdict, proc.returncode, tail


def cmd_run(a: argparse.Namespace) -> None:
    task = TASKS[a.task]
    prompt = COMMON_PREFIX + (HERE / task["prompt"]).read_text(encoding="utf-8")
    if a.arm == "b":
        prompt = ARM_B_PREFIX + prompt
    verdict = "FAIL"
    wt = make_worktree(a.task, a.arm, task["parentSha"])
    print(f"[bench] task {a.task} arm {a.arm} worktree {wt}", flush=True)
    try:
        result, stderr, wall = run_agent(wt, prompt, a.arm, a.model, a.timeout_min, task)
        # everything the agent left behind - committed, staged, unstaged or untracked - against the baseline
        root = sh(["git", "rev-list", "--max-parents=0", "HEAD"], cwd=wt).stdout.strip()
        sh(["git", "add", "-A"], cwd=wt, check=False)
        diff = sh(["git", "diff", "--cached", "--stat", root], cwd=wt, check=False).stdout
        verdict, code, oracle_tail = run_oracle(wt, task)
    finally:
        # a failed run keeps its checkout: the diff IS the evidence, and rmtree was silently
        # half-deleting it (open handles) while reporting nothing
        if not a.keep and verdict == "PASS":
            remove_worktree(wt)
        else:
            print(f"[bench] checkout kept at {wt}", flush=True)
    usage = result.get("usage", {})
    row = {
        "at": dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds"),
        "benchVersion": BENCH_VERSION, "task": a.task, "arm": a.arm,
        "mcpConfig": str(MCP_CONFIG) if a.arm == "b" else None,
        "model": a.model or ",".join((result.get("modelUsage") or {}).keys()) or None,
        "verdict": verdict, "oracleExit": code,
        "wallSeconds": round(wall, 1),
        "agentDurationMs": result.get("duration_ms"), "turns": result.get("num_turns"),
        "costUsd": result.get("total_cost_usd"),
        "inputTokens": usage.get("input_tokens"), "outputTokens": usage.get("output_tokens"),
        "cacheReadTokens": usage.get("cache_read_input_tokens"),
        "cacheWriteTokens": usage.get("cache_creation_input_tokens"),
        "sessionId": result.get("session_id"),
        "transcript": transcript_usage(result.get("session_id")),
        "diffStat": diff.strip()[-800:],
        "oracleTail": oracle_tail,
        "agentError": result.get("unparseable_stdout") and {"stdout": result["unparseable_stdout"], "stderr": result.get("stderr"), "exit": result.get("exit")},
    }
    with RESULTS.open("a", encoding="utf-8") as f:
        f.write(json.dumps(row, ensure_ascii=False) + "\n")
    print(json.dumps({k: row[k] for k in ("task", "arm", "model", "verdict", "wallSeconds", "turns", "costUsd",
                                          "inputTokens", "outputTokens", "cacheReadTokens", "cacheWriteTokens")}, indent=2))


def cmd_table(_: argparse.Namespace) -> None:
    if not RESULTS.exists():
        print("no results yet"); return
    rows = [json.loads(l) for l in RESULTS.read_text(encoding="utf-8").splitlines() if l.strip()]
    hdr = ("at", "v", "task", "arm", "model", "verdict", "issue-scoped", "wall s", "turns", "USD", "in", "out", "cache rd", "cache wr")
    print(" | ".join(hdr))
    for r in rows:
        t = r.get("transcript") or {}
        if t.get("assistantMessages"):
            r = {**r, "inputTokens": t["input"], "outputTokens": t["output"], "cacheReadTokens": t["cacheRead"], "cacheWriteTokens": t["cacheWrite"]}
        tot = sum(x or 0 for x in (r.get("inputTokens"), r.get("outputTokens"), r.get("cacheReadTokens"), r.get("cacheWriteTokens")))
        print(" | ".join(str(x) for x in (r["at"][:16], r.get("benchVersion", 1), r["task"], r["arm"], r.get("model"), r["verdict"], r.get("verdictIssueScoped", "-"), r["wallSeconds"],
                                          r.get("turns"), r.get("costUsd"), r.get("inputTokens"), r.get("outputTokens"),
                                          r.get("cacheReadTokens"), r.get("cacheWriteTokens"))) + f" | total {tot:,}")


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("--task", required=True, choices=sorted(TASKS))
    r.add_argument("--arm", required=True, choices=["a", "b"])
    r.add_argument("--model")
    r.add_argument("--timeout-min", type=int, default=30)
    r.add_argument("--keep", action="store_true", help="keep the worktree for inspection")
    r.set_defaults(fn=cmd_run)
    t = sub.add_parser("table"); t.set_defaults(fn=cmd_table)
    a = p.parse_args(); a.fn(a)


if __name__ == "__main__":
    main()
