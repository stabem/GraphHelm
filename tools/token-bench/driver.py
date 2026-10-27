"""Drael driver for the do-less benchmark: runs `opencode run --auto -m drael/drael-v1`
in fresh do-less checkouts and scores each run with the repository's own doless.py
functions (compose_prompt, make_worktree, score_checkout, do_less_verdict).

Usage:
    python driver.py --smoke                 # one arm-a run of docs-config-risk
    python driver.py --plan PLAN.json        # run a plan file [{"task","arm","run"}, ...]
    python driver.py --task T --arm A [-n N] # quick ad-hoc

Environment (required): TOKEN_BENCH_SCRATCH, TOKEN_BENCH_GRAPHHELM_CLI
Rows are appended to tools/token-bench/doless/results.jsonl in the cmd_run row shape.
"""
from __future__ import annotations

import argparse
import json
import os
import sqlite3
import subprocess
import sys
import time
import uuid
from pathlib import Path

REPO = Path(os.environ.get("TOKEN_BENCH_REPO", str(Path(__file__).resolve().parents[2])))
TB = REPO / "tools" / "token-bench"
sys.path.insert(0, str(TB))

import doless  # noqa: E402  (reads TOKEN_BENCH_SCRATCH / TOKEN_BENCH_GRAPHHELM_CLI on import)
import run as runner  # noqa: E402

DB = Path(os.environ.get("OPENCODE_DB", r"C:\Users\gabri\.local\share\opencode\opencode.db"))
MODEL = "drael/drael-v1"
# The npm shim is a .ps1, invisible to subprocess; point at the CLI binary directly.
OPENCODE = os.environ.get(
    "OPENCODE_BIN", r"C:\Users\gabri\AppData\Roaming\npm\node_modules\@opencode\cli\bin\opencode.exe")
NO_WINDOW = subprocess.CREATE_NO_WINDOW
TIMEOUT_S = int(os.environ.get("BENCH_TIMEOUT_S", str(25 * 60)))
WORK = Path(os.environ["TOKEN_BENCH_SCRATCH"]) / "drael"

# Estimated list prices, USD per 1M tokens (cache reads billed at input, writes at output).
# Drael is subscription-metered here (observed cost 0.0); USD figures are estimates only.
PRICE_IN, PRICE_OUT = 0.50, 1.50


def log(msg: str) -> None:
    print(f"[driver] {msg}", flush=True)


def opencode_version() -> str:
    p = subprocess.run([OPENCODE, "--version"], capture_output=True, text=True, creationflags=NO_WINDOW, timeout=30)
    return p.stdout.strip() or f"exit {p.returncode}"


def parse_event(line: str, texts: list[str], session_ids: set[str]) -> None:
    line = line.strip()
    if not line.startswith("{"):
        return
    try:
        ev = json.loads(line)
    except json.JSONDecodeError:
        return
    sid = ev.get("sessionID")
    if sid:
        session_ids.add(sid)
    part = ev.get("part") or {}
    if part.get("type") == "text" and isinstance(part.get("text"), str):
        texts.append(part["text"])


def run_agent(wt: Path, prompt: str, title: str, env: dict, run_dir: Path) -> dict:
    """One Drael turn in `wt`. Prompt goes on the command line; events land in run_dir."""
    cmd = [OPENCODE, "run", "--auto", "-m", MODEL, "--format", "json", "--title", title, prompt]
    t0 = time.monotonic()
    stdout_path = run_dir / "events.jsonl"
    texts: list[str] = []
    session_ids: set[str] = set()
    timed_out = False
    return_code: int | None = None
    out = ""
    stderr_text = ""
    try:
        proc = subprocess.run(cmd, cwd=wt, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              text=True, encoding="utf-8", errors="replace", timeout=TIMEOUT_S,
                              creationflags=NO_WINDOW, env=env)
        out = proc.stdout or ""
        return_code = proc.returncode
        stderr_text = (proc.stderr or "")[-500:]
    except subprocess.TimeoutExpired as exc:
        timed_out = True
        out = exc.stdout if isinstance(exc.stdout, str) else (exc.stdout or b"").decode("utf-8", "replace")
        serr = exc.stderr if isinstance(exc.stderr, str) else (exc.stderr or b"").decode("utf-8", "replace")
        stderr_text = (serr or "")[-500:]
    wall = time.monotonic() - t0
    for line in out.splitlines():
        parse_event(line, texts, session_ids)
    with open(stdout_path, "w", encoding="utf-8") as f:
        f.write(out)
    root = resolve_root_session(session_ids) or (sorted(session_ids)[0] if session_ids else None)
    return {"sessionID": root, "sessionCount": len(session_ids), "texts": texts, "eventsFile": str(stdout_path),
            "stderrTail": stderr_text, "wall": round(wall, 1), "timedOut": timed_out,
            "returnCode": return_code}


def resolve_root_session(session_ids: set[str]) -> str | None:
    """The session whose parent_id is NULL among those observed in the stream."""
    if len(session_ids) == 1:
        return next(iter(session_ids))
    try:
        con = sqlite3.connect(f"file:{DB.as_posix()}?mode=ro", uri=True, timeout=5.0)
        marks = ",".join("?" for _ in session_ids)
        row = con.execute(f"SELECT id FROM session_v2 WHERE id IN ({marks}) AND parent_id IS NULL",
                          tuple(sorted(session_ids))).fetchone()
        con.close()
        return row[0] if row else sorted(session_ids)[0]
    except sqlite3.OperationalError:
        return sorted(session_ids)[0] if session_ids else None


def subtree_tokens(session_id: str | None) -> dict:
    """Sum session_v2 token columns over the session and every descendant (subagents)."""
    if not session_id:
        return {"sessions": 0, "tokensInput": 0, "tokensOutput": 0, "tokensReasoning": 0,
                "tokensCacheRead": 0, "tokensCacheWrite": 0}
    rows = None
    for attempt in range(6):
        try:
            con = sqlite3.connect(f"file:{DB.as_posix()}?mode=ro", uri=True, timeout=5.0)
            rows = con.execute("""
                WITH RECURSIVE tree(id) AS (
                    SELECT id FROM session_v2 WHERE id = ?
                    UNION
                    SELECT s.id FROM session_v2 s JOIN tree t ON s.parent_id = t.id
                )
                SELECT COUNT(*),
                       COALESCE(SUM(tokens_input),0), COALESCE(SUM(tokens_output),0),
                       COALESCE(SUM(tokens_reasoning),0), COALESCE(SUM(tokens_cache_read),0),
                       COALESCE(SUM(tokens_cache_write),0)
                FROM session_v2 WHERE id IN (SELECT id FROM tree)
            """, (session_id,)).fetchone()
            con.close()
            break
        except sqlite3.OperationalError:
            if attempt == 5:
                raise
            time.sleep(1.5)
    usage = {"sessions": int(rows[0]), "tokensInput": int(rows[1]), "tokensOutput": int(rows[2]),
             "tokensReasoning": int(rows[3]), "tokensCacheRead": int(rows[4]),
             "tokensCacheWrite": int(rows[5])}
    # The service may flush usage slightly after exit; retry once while flat.
    if usage["tokensInput"] + usage["tokensOutput"] == 0:
        time.sleep(3.0)
        try:
            con = sqlite3.connect(f"file:{DB.as_posix()}?mode=ro", uri=True, timeout=5.0)
            rows = con.execute("""
                WITH RECURSIVE tree(id) AS (
                    SELECT id FROM session_v2 WHERE id = ?
                    UNION
                    SELECT s.id FROM session_v2 s JOIN tree t ON s.parent_id = t.id
                )
                SELECT COUNT(*), COALESCE(SUM(tokens_input),0), COALESCE(SUM(tokens_output),0),
                       COALESCE(SUM(tokens_reasoning),0), COALESCE(SUM(tokens_cache_read),0),
                       COALESCE(SUM(tokens_cache_write),0)
                FROM session_v2 WHERE id IN (SELECT id FROM tree)
            """, (session_id,)).fetchone()
            con.close()
            usage = {"sessions": int(rows[0]), "tokensInput": int(rows[1]), "tokensOutput": int(rows[2]),
                     "tokensReasoning": int(rows[3]), "tokensCacheRead": int(rows[4]),
                     "tokensCacheWrite": int(rows[5])}
        except sqlite3.OperationalError:
            pass
    return usage


def save_diff(wt: Path, run_dir: Path) -> None:
    """Persist the scored diff (base commit -> snapshot HEAD) for later blind rubric review."""
    if not wt.exists():
        return
    try:
        base = subprocess.run(["git", "rev-list", "--max-parents=0", "HEAD"], cwd=wt,
                              capture_output=True, text=True, creationflags=NO_WINDOW).stdout.strip()
        diff = subprocess.run(["git", "diff", base], cwd=wt, capture_output=True, text=True,
                              encoding="utf-8", errors="replace", creationflags=NO_WINDOW).stdout
        (run_dir / "diff.txt").write_text(diff, encoding="utf-8")
        stat = subprocess.run(["git", "diff", "--stat", base], cwd=wt, capture_output=True, text=True,
                              errors="replace", creationflags=NO_WINDOW).stdout
        (run_dir / "diff.stat").write_text(stat, encoding="utf-8")
    except Exception as exc:
        log(f"save_diff: {type(exc).__name__}: {exc}")


def estimate_usd(usage: dict) -> float:
    inp = usage.get("tokensInput", 0) + usage.get("tokensCacheRead", 0)
    out = usage.get("tokensOutput", 0) + usage.get("tokensReasoning", 0) + usage.get("tokensCacheWrite", 0)
    return round((inp * PRICE_IN + out * PRICE_OUT) / 1_000_000, 6)


def perform(task_id: str, arm: str, run_index: int, *, keep: bool = False) -> dict:
    task = doless.load_tasks()[task_id]
    label = f"dl-{task_id}"
    title = f"tb-{task_id}-{arm}-{run_index}-{uuid.uuid4().hex[:6]}"
    wt = runner.make_worktree(label, arm, task["parentSha"])
    run_dir = WORK / title
    run_dir.mkdir(parents=True, exist_ok=True)
    prompt, surface_digests = doless.compose_prompt(task, arm, doless.REPO, "explicit")
    (run_dir / "prompt.txt").write_text(prompt, encoding="utf-8")
    result: dict = {"agentError": "agent_not_started"}
    usage: dict = {}
    agent_error: str | None = None
    row: dict = {}
    try:
        result = run_agent(wt, prompt, title, runner.bench_env(task), run_dir)
        answer = "\n\n".join(t for t in result["texts"] if t.strip())
        (run_dir / "answer.txt").write_text(answer, encoding="utf-8")
        usage = subtree_tokens(result.get("sessionID"))
        if result.get("timedOut"):
            agent_error = "agent_timeout"
        elif not result.get("sessionID"):
            agent_error = "no_session_observed"
        elif not result["texts"]:
            agent_error = f"agent_no_text exit={result.get('returnCode')}"
        try:
            row = doless.score_checkout(wt, task, answer, False)
        except Exception as exc:
            row = dict(row)
            row["evaluatorError"] = f"{type(exc).__name__}: {exc}"[:500]
    except Exception as exc:
        import traceback
        traceback.print_exc()
        if not row:
            row = {"evaluatorError": f"{type(exc).__name__}: {exc}"[:500]}
    finally:
        pass
    if row.get("evaluatorError"):
        log(f"evaluatorError: {row['evaluatorError']}")
    final_text = "\n\n".join(t for t in result.get("texts", []) if t.strip())
    tokens_total = sum(usage.get(k, 0) for k in ("tokensInput", "tokensOutput", "tokensReasoning",
                                                  "tokensCacheRead", "tokensCacheWrite"))
    base = {
        "at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "dolessVersion": doless.DOLESS_VERSION, "executor": "drael", "task": task_id, "split": task["split"],
        "category": task["category"], "arm": arm, "promptStyle": "explicit", "run": run_index,
        "requestedModel": MODEL, "models": [MODEL], "inferenceConfig": None,
        "effortObservation": "unobserved",
        "costProvenance": "unavailable",  # subscription: observed metered cost is 0.0
        "opencodeCli": opencode_version(), "surfaceDigests": surface_digests,
        "promptDigest": runner.digest_bytes(prompt.encode("utf-8")),
        "taskDigest": runner.digest_bytes(b"\0".join(
            (doless.DOLESS / task[k]).read_bytes() for k in (doless.PROMPT_KEYS["explicit"], "oracle", "card"))),
        "agentError": agent_error, "sessionId": result.get("sessionID"),
        "costUsd": None, "costUsdEstimated": estimate_usd(usage),
        "tokens": usage, "tokensTotal": tokens_total,
        "wallSeconds": round(result.get("wall", 0.0), 1), "transcript": result.get("eventsFile"),
        "answerTail": final_text[-400:],
    }
    base.update(row)
    if base.get("evaluatorError"):
        base["verdict"], base["verdictReasons"] = "INCOMPLETE", ["evaluator_error"]
    elif agent_error:
        base["verdict"], base["verdictReasons"] = "INCOMPLETE", [agent_error]
    else:
        base["verdict"], base["verdictReasons"] = doless.do_less_verdict(base)
    with doless.RESULTS.open("a", encoding="utf-8") as f:
        f.write(json.dumps(base, ensure_ascii=False) + "\n")
    log(json.dumps({k: base.get(k) for k in ("task", "arm", "run", "verdict", "verdictReasons", "oracle",
                                             "regression", "changedLines", "tokensTotal", "costUsdEstimated",
                                             "wallSeconds", "sessionId")}, ensure_ascii=False))
    if row.get("evaluatorError"):
        log(f"evaluatorError: {row['evaluatorError']}")
    if base["verdict"] != "PASS":
        log(f"oracleTail: {(base.get('oracleTail') or '')[-300:]}")
        log(f"regressionTail: {(base.get('regressionTail') or '')[-300:]}")
    save_diff(wt, run_dir)
    if not (keep or base.get("evaluatorError")):
        doless.remove_checkout(wt)
    else:
        log(f"kept checkout: {wt}")
    return base


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--smoke", action="store_true")
    ap.add_argument("--plan", type=Path)
    ap.add_argument("--task")
    ap.add_argument("--arm", choices=["a", "k", "d"])
    ap.add_argument("-n", type=int, default=1)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    if not os.environ.get("TOKEN_BENCH_SCRATCH") or not os.environ.get("TOKEN_BENCH_GRAPHHELM_CLI"):
        raise SystemExit("TOKEN_BENCH_SCRATCH and TOKEN_BENCH_GRAPHHELM_CLI must be set")
    plan: list[tuple[str, str, int]] = []
    if args.smoke:
        plan = [("docs-config-risk", "a", 1)]
    elif args.plan:
        raw = json.loads(args.plan.read_text(encoding="utf-8"))
        plan = [(e["task"], e["arm"], e["run"]) for e in raw]
    elif args.task and args.arm:
        plan = [(args.task, args.arm, i) for i in range(1, args.n + 1)]
    else:
        raise SystemExit("give --smoke, --plan FILE, or --task T --arm A")
    for task_id, arm, index in plan:
        perform(task_id, arm, index, keep=args.keep)


if __name__ == "__main__":
    main()
