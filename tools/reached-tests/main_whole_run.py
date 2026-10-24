#!/usr/bin/env python3
"""Main's one whole run after merges, on its own (#1522, KEEL_MERGE.md step 2).

One shot, meant for a hidden scheduled task (for example every 15 minutes). It does nothing unless
origin/main moved since the last run AND the last run is at least `--min-interval` seconds old
(default one hour). When due it checks main out in a dedicated detached worktree, runs
`run_reached.py --keep-going` over everything merged since the last run, records the receipt as a
`main.whole_run` signal on the Runtime (severity `high` and the red steps named when it failed, so the
Lanes board and the needs-you list show it) and saves the state. No GitHub Actions; standard library.

    python tools/reached-tests/main_whole_run.py --token-file .graphhelm/events.token [--dry-run]
"""
import argparse
import hashlib
import importlib.util
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

# Owner rule: children run hidden in the calling terminal, never in a new console window.
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
HERE = Path(__file__).resolve().parent
LOOPBACK = ("127.0.0.1", "localhost", "::1")
SIGNAL_TYPE = "main.whole_run"


def due(state, main_sha, now, min_interval):
    """(run?, reason). A run is due when main moved and the last run is old enough."""
    last = (state or {}).get("sha")
    if last == main_sha:
        return False, "main has not moved since the last whole run"
    waited = now - float((state or {}).get("startedAt", 0))
    if last and waited < min_interval:
        return False, f"last whole run was {int(waited)}s ago; the minimum is {int(min_interval)}s"
    return True, "main moved" if last else "no earlier whole run"


def red_steps(report):
    return [str(step.get("name") or step.get("id") or step.get("command") or "?")[:120]
            for step in (report.get("completed") or []) if step.get("returncode") not in (0, None)]


def build_signal(sha, base, report, report_path, repo, now_iso):
    status = report.get("status", "incomplete")
    reds = red_steps(report)
    words = {"sha": sha, "base": base, "status": status, "report": str(report_path), "red": reds[:20]}
    return {"signal": {
        "id": f"main-whole-run-{sha[:12]}-{hashlib.sha256(status.encode()).hexdigest()[:6]}",
        "type": SIGNAL_TYPE, "source": {"type": "user", "id": "main-whole-run"},
        "severity": "low" if status == "passed" and not reds else "high", "emittedAt": now_iso,
        "evidence": [f"https://github.com/{repo}/commit/{sha}"],
        "description": json.dumps(words, separators=(",", ":"))}}


def child_budget(timeout):
    """Wall budget for run_reached: the hard timeout minus a margin, so it stops itself and writes its report first."""
    return max(1.0, timeout - min(300.0, timeout * 0.1))


def child_command(tree, base, sha, root, report_path, timeout):
    return [sys.executable, str(Path(tree) / "tools" / "reached-tests" / "run_reached.py"), "--repo", str(tree),
            "--base", base, "--head", sha, "--root", str(root), "--lane", "main-whole-run",
            "--output", str(report_path), "--keep-going", "--no-resume",
            "--budget-seconds", repr(child_budget(timeout))]


def git(repo, *args):
    done = subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True, encoding="utf-8",
                          timeout=300, creationflags=NO_WINDOW)
    if done.returncode:
        raise RuntimeError(f"git {' '.join(args)}: {done.stderr.strip()[:200]}")
    return done.stdout.strip()


def prepare_tree(repo, tree, sha, run_git=git):
    """Put the dedicated worktree on `sha`, detached. A dirty tree is refused, never forced or cleaned."""
    if not Path(tree).exists():
        run_git(repo, "worktree", "add", "--detach", str(tree), sha)
        return
    if run_git(tree, "status", "--porcelain"):
        raise RuntimeError(f"{tree} has uncommitted changes; refusing to touch it")
    run_git(tree, "checkout", "--detach", sha)


def newer_copy(tree, own):
    """The copy of this script inside `tree` when it differs from the running one, else None."""
    mine = Path(tree) / "tools" / "reached-tests" / Path(own).name
    try:
        if mine.is_file() and mine.read_bytes() != Path(own).read_bytes():
            return mine
    except OSError:
        pass
    return None


def reexec(copy, argv, env=None):
    """Run main's copy of this script in place of the stale one (hidden child); returns its exit code."""
    env = dict(os.environ if env is None else env, MAIN_WHOLE_RUN_REEXEC="1")
    return subprocess.run([sys.executable, str(copy), *argv], env=env, creationflags=NO_WINDOW).returncode


def post(args, body):
    spec = importlib.util.spec_from_file_location("task_record", HERE.parent / "task-record" / "task_record.py")
    task_record = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(task_record)
    args.lane = "main-whole-run"
    token = task_record.read_credential(args)
    url = f"{args.url.rstrip('/')}/v1/executions/{urllib.parse.quote(args.execution, safe='')}/signal"
    if urllib.parse.urlsplit(url).hostname not in LOOPBACK:
        sys.exit(f"main_whole_run: refusing {args.url}: the token is sent only to a loopback Runtime")
    headers = {"Content-Type": "application/json", "Idempotency-Key": body["signal"]["id"],
               "X-GraphHelm-Actor": args.lane, "X-GraphHelm-Actor-Type": "agent", "Authorization": f"Bearer {token}"}
    request = urllib.request.Request(url, data=json.dumps(body).encode(), method="POST", headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response).get("ok") is True
    except urllib.error.HTTPError as error:
        return error.code == 409  # the same receipt already recorded
    except urllib.error.URLError:
        return False


def resolve_execution(args):
    """Resolve the configured execution before any scheduled work or state write."""
    spec = importlib.util.spec_from_file_location("task_record", HERE.parent / "task-record" / "task_record.py")
    task_record = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(task_record)
    return task_record.resolve_execution(args.execution, args.token_file, args.repo)


def save_then_post(args, state_file, sha, now, report, body, poster=None):
    """State first, then the post: a missing credential (post exits) must not re-run the suite every tick."""
    def save(recorded):
        state_file.write_text(json.dumps({"sha": sha, "startedAt": now, "status": report.get("status"),
                                          "recorded": recorded}) + "\n", encoding="utf-8")

    save(False)
    recorded = (poster or post)(args, body)
    if recorded:
        save(True)
    return recorded


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", default=".", help="a checkout of the repository (fetched; its files are not changed)")
    ap.add_argument("--github", default="stabem/GraphHelm")
    ap.add_argument("--root", default="D:/gh")
    ap.add_argument("--state", help="state file; default <root>/main-whole-run/state.json")
    ap.add_argument("--min-interval", type=float, default=3600)
    ap.add_argument("--timeout", type=float, default=3600, help="hard limit in seconds for the run_reached child")
    ap.add_argument("--url", default="http://127.0.0.1:8793")
    ap.add_argument("--execution", default=None)
    ap.add_argument("--token-file", default=None)
    ap.add_argument("--allow-shared-token", action="store_true")
    ap.add_argument("--dry-run", action="store_true", help="say what would run and print the signal; run nothing")
    args = ap.parse_args(argv)
    raw_argv = list(sys.argv[1:] if argv is None else argv)
    if args.token_file is None:
        args.token_file = str(Path(args.repo) / ".graphhelm" / "events.token")
    args.execution = resolve_execution(args)
    work = Path(args.root) / "main-whole-run"
    state_file = Path(args.state) if args.state else work / "state.json"
    try:
        state = json.loads(state_file.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        state = {}
    git(args.repo, "fetch", "--quiet", "origin", "main")
    sha = git(args.repo, "rev-parse", "origin/main")
    now = time.time()
    if not args.dry_run and not os.environ.get("MAIN_WHOLE_RUN_REEXEC"):
        # A fix merged to main must reach this scheduled copy: run main's own copy of the script (#1523).
        work.mkdir(parents=True, exist_ok=True)
        prepare_tree(args.repo, work / "tree", sha)
        copy = newer_copy(work / "tree", __file__)
        if copy:
            print(f"main_whole_run: running main's copy {copy}")
            return reexec(copy, raw_argv)
    run, reason = due(state, sha, now, args.min_interval)
    print(f"main_whole_run: {'run' if run else 'skip'} ({reason}); main {sha[:8]}")
    if not run:
        return 0
    base = state.get("sha") or git(args.repo, "rev-parse", "origin/main~1")
    tree = work / "tree"
    if args.dry_run:
        print(json.dumps(build_signal(sha, base, {"status": "dry-run"}, work / "report.json", args.github,
                                      time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(now))), indent=1))
        return 0
    work.mkdir(parents=True, exist_ok=True)
    prepare_tree(args.repo, tree, sha)
    report_path = work / f"report-{sha[:8]}.json"
    # The runner of the tree under test; --keep-going reports every red in one pass.
    try:
        subprocess.run(child_command(tree, base, sha, args.root, report_path, args.timeout),
                   capture_output=True, text=True, encoding="utf-8", creationflags=NO_WINDOW,
                   timeout=args.timeout)
    except subprocess.TimeoutExpired:
        pass  # the report below says "incomplete"
    try:
        report = json.loads(report_path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        report = {"status": "incomplete", "error": "no report was written"}
    body = build_signal(sha, base, report, report_path, args.github, time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()))
    recorded = save_then_post(args, state_file, sha, now, report, body)
    print(f"main_whole_run: {report.get('status')} on {sha[:8]}; receipt {'recorded' if recorded else 'NOT recorded'}: {report_path}")
    return 0 if report.get("status") == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
