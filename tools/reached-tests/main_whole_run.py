#!/usr/bin/env python3
"""Run one hidden whole-main reached proof at a time (#1522, KEEL_MERGE.md step 2).

The nightly mode is intended for an OpenBot routine. It checks out latest origin/main in a clean
detached tree, refuses duplicate runs for the same UTC night, and keeps a durable plan, report,
logs, and final receipt. A red or incomplete run opens one idempotent P0 issue. The legacy
change-triggered mode remains available without `--nightly`. No Actions or cron; standard library.

    python tools/reached-tests/main_whole_run.py --nightly --repo F:/github/GraphHelm \\
      --receipt-root C:/Users/gabri/OpenBot/Shared/graphhelm-openbot/nightly
"""
import argparse
import datetime
import hashlib
import importlib.util
import json
import os
import re
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


class NightlyLock:
    """Cross-process, non-waiting lock for the single nightly run."""
    def __init__(self, path):
        self.path = Path(path)
        self.stream = None

    def __enter__(self):
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.stream = self.path.open("a+b")
        self.stream.seek(0, os.SEEK_END)
        if self.stream.tell() == 0:
            self.stream.write(b"\0")
            self.stream.flush()
        self.stream.seek(0)
        try:
            if os.name == "nt":
                import msvcrt
                msvcrt.locking(self.stream.fileno(), msvcrt.LK_NBLCK, 1)
            else:
                import fcntl
                fcntl.flock(self.stream.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError as error:
            self.stream.close()
            self.stream = None
            raise RuntimeError("another nightly run holds the lock") from error
        return self

    def __exit__(self, *_):
        if self.stream is None:
            return
        self.stream.seek(0)
        if os.name == "nt":
            import msvcrt
            msvcrt.locking(self.stream.fileno(), msvcrt.LK_UNLCK, 1)
        else:
            import fcntl
            fcntl.flock(self.stream.fileno(), fcntl.LOCK_UN)
        self.stream.close()
        self.stream = None


def atomic_json(path, value):
    """Durably replace one receipt file without exposing a partial JSON document."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f"{path.name}.{os.getpid()}.tmp")
    with temporary.open("w", encoding="utf-8", newline="\n") as stream:
        json.dump(value, stream, ensure_ascii=True, indent=2)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)


def utc_now():
    return datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0)


def night_date(now=None):
    """Night key uses UTC so the scheduled local evening maps to one stable date."""
    return (now or utc_now()).date().isoformat()


def reserved_window(path, now):
    """Require the exact #2510 owner reservation to cover the current UTC instant."""
    try:
        rows = Path(path).read_text(encoding="utf-8").splitlines()
    except OSError as error:
        return False, f"cannot read idle-window file ({type(error).__name__})"
    for row in rows:
        cells = [cell.strip() for cell in row.strip().strip("|").split("|")]
        if (len(cells) < 4 or cells[2] != "#2510: single hidden latest-main full reached run at 22:15Z; 60-minute timeout and 30-minute margin"
                or cells[3] != "openbot-shikaku"):
            continue
        try:
            start = datetime.datetime.strptime(cells[0], "%Y-%m-%d %H:%MZ").replace(tzinfo=datetime.timezone.utc)
            end = datetime.datetime.strptime(cells[1], "%Y-%m-%d %H:%MZ").replace(tzinfo=datetime.timezone.utc)
        except ValueError:
            return False, "#2510 idle-window row has invalid timestamps"
        if start <= now < end and now.date().isoformat() == start.date().isoformat():
            return True, {"start": start.isoformat(), "end": end.isoformat()}
        return False, "current UTC instant is outside the declared #2510 window"
    return False, "no exact #2510 idle-window reservation is declared"


def active_heavy_processes(run=subprocess.run):
    """Read the Windows process table; fail closed if the shared slot cannot be inspected."""
    command = ["powershell.exe", "-NoProfile", "-NonInteractive", "-Command",
               "Get-CimInstance Win32_Process | Select-Object ProcessId,Name,CommandLine | ConvertTo-Json -Compress"]
    try:
        result = run(command, capture_output=True, text=True, encoding="utf-8", timeout=15,
                     stdin=subprocess.DEVNULL, creationflags=NO_WINDOW)
        if result.returncode:
            return None, "process-table query failed"
        rows = json.loads(result.stdout or "[]")
        if isinstance(rows, dict):
            rows = [rows]
    except (OSError, subprocess.SubprocessError, ValueError):
        return None, "process-table query failed"
    pattern = re.compile(
        r"(?:cargo(?:\.exe)?\s+(?:test|build|clippy|check)|rustc(?:\.exe)?\b|"
        r"run_reached\.py|pytest(?:\.exe)?\b|vitest\b|(?:npm|pnpm|yarn)(?:\.cmd)?\s+(?:test|run\s+build))",
        re.IGNORECASE)
    found = []
    for row in rows:
        command_line = row.get("CommandLine") or ""
        if pattern.search(command_line):
            found.append({"pid": row.get("ProcessId"), "name": row.get("Name")})
    return found, None


def quiet_preflight(args, now, run=subprocess.run):
    date = now.date().isoformat()
    if getattr(args, "night_date", None) and args.night_date != date:
        return False, f"explicit night date {args.night_date} differs from current UTC date {date}"
    inside, window = reserved_window(args.idle_windows_file, now)
    if not inside:
        return False, window
    processes, error = active_heavy_processes(run)
    if error:
        return False, error
    if processes:
        return False, {"reason": "shared build slot has active heavy processes", "processes": processes}
    return True, {"window": window, "activeHeavyProcesses": []}


def first_parent_commits(repo, baseline, head):
    return set(filter(None, git(repo, "rev-list", "--first-parent", f"{baseline}..{head}").splitlines()))


def merges_since_green(repo, github, baseline, head, run=subprocess.run):
    """Return PR merge names on main since the last green head, without implying causality."""
    if not baseline:
        return {"status": "UNOBSERVED", "baseline": "UNOBSERVED", "merges": []}
    ancestry = run(["git", "-C", str(repo), "merge-base", "--is-ancestor", baseline, head],
                   stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                   timeout=30, creationflags=NO_WINDOW)
    if ancestry.returncode:
        return {"status": "UNOBSERVED", "baseline": baseline, "merges": [],
                "reason": "last-green SHA is not an ancestor of current main"}
    commits = first_parent_commits(repo, baseline, head)
    command = ["gh", "pr", "list", "--repo", github, "--state", "merged", "--base", "main",
               "--json", "number,title,mergedAt,mergeCommit", "--limit", "1000"]
    try:
        result = run(command, capture_output=True, text=True, encoding="utf-8", timeout=30,
                     stdin=subprocess.DEVNULL, creationflags=NO_WINDOW)
        if result.returncode:
            return {"status": "UNOBSERVED", "baseline": baseline, "merges": [],
                    "reason": "GitHub merge-list read failed"}
        rows = json.loads(result.stdout)
    except (OSError, subprocess.SubprocessError, ValueError):
        return {"status": "UNOBSERVED", "baseline": baseline, "merges": [],
                "reason": "GitHub merge-list read failed"}
    merges = []
    for row in rows:
        merge = row.get("mergeCommit") or {}
        sha = merge.get("oid")
        if sha and sha in commits:
            merges.append({"number": row.get("number"), "title": row.get("title", ""), "sha": sha})
    merges.sort(key=lambda row: (row["number"] or 0, row["sha"]))
    return {"status": "UNOBSERVED_LIMIT" if len(rows) == 1000 else "observed",
            "baseline": baseline, "merges": merges}


def failure_issue_body(date, head, baseline, merge_read, report_path, red, error=None):
    marker = f"nightly-main-{date}"
    lines = [f"Run key: `{marker}`", "", "The nightly full proof on latest main was red or incomplete.",
             f"- Main SHA: `{head or 'UNOBSERVED'}`",
             f"- Last green SHA: `{baseline or 'UNOBSERVED'}`",
             f"- Receipt: `{report_path}`"]
    if merge_read.get("status") == "UNOBSERVED":
        lines.extend(["- Merges since last green: UNOBSERVED", ""])
    else:
        lines.extend(["- Merges since last green:"])
        if merge_read.get("merges"):
            lines.extend(f"  - #{item['number']} {item['title']} (`{item['sha'][:12]}`)"
                         for item in merge_read["merges"])
        else:
            lines.append("  - None observed.")
        if merge_read.get("status") == "UNOBSERVED_LIMIT":
            lines.append("- Merge list hit the 1,000-item API limit; completeness is UNOBSERVED.")
        lines.append("")
    if error:
        lines.append(f"- Runner error: {error}")
    if red:
        lines.extend(["", "Red steps:", *[f"- `{step}`" for step in red]])
    lines.extend(["", "These merges are candidates only; this issue does not claim that any merge caused the failure."])
    return "\n".join(lines), marker


def find_failure_issue(github, marker, run=subprocess.run):
    command = ["gh", "issue", "list", "--repo", github, "--state", "all", "--search", marker,
               "--json", "number,title,body,url", "--limit", "100"]
    try:
        result = run(command, capture_output=True, text=True, encoding="utf-8", timeout=30,
                     stdin=subprocess.DEVNULL, creationflags=NO_WINDOW)
        if result.returncode:
            raise RuntimeError("GitHub issue search failed")
        return next((item for item in json.loads(result.stdout) if marker in item.get("body", "")), None)
    except (OSError, subprocess.SubprocessError, ValueError) as error:
        raise RuntimeError("GitHub issue search failed") from error


def open_failure_issue(args, date, head, baseline, merge_read, report_path, red, error=None, run=subprocess.run):
    body, marker = failure_issue_body(date, head, baseline, merge_read, report_path, red, error)
    body_path = Path(report_path).parent / "p0-issue.md"
    body_path.write_text(body + "\n", encoding="utf-8", newline="\n")
    try:
        existing = find_failure_issue(args.github, marker, run)
    except RuntimeError:
        return {"status": "failed"}
    if existing:
        return {"status": "existing", "number": existing["number"], "url": existing["url"]}
    title = f"[P0] GraphHelm nightly full proof failed [{marker}]"
    command = ["gh", "issue", "create", "--repo", args.github, "--title", title,
               "--label", "P0", "--body-file", str(body_path)]
    try:
        result = run(command, capture_output=True, text=True, encoding="utf-8", timeout=45,
                     stdin=subprocess.DEVNULL, creationflags=NO_WINDOW)
        url = result.stdout.strip().splitlines()[-1] if result.returncode == 0 and result.stdout.strip() else None
    except (OSError, subprocess.SubprocessError):
        url = None
    # A timed-out create may have succeeded remotely; search by stable run marker before reporting failure.
    try:
        existing = find_failure_issue(args.github, marker, run) if not url else None
    except RuntimeError:
        existing = None
    if existing:
        return {"status": "created_or_existing", "number": existing["number"], "url": existing["url"]}
    return {"status": "created", "url": url} if url else {"status": "failed"}


def run_nightly(args, raw_argv, run=subprocess.run, now=None):
    """Run one latest-main reached proof per UTC night with atomic, no-retry receipts."""
    now = now or utc_now()
    date = args.night_date or night_date(now)
    try:
        datetime.date.fromisoformat(date)
    except ValueError:
        sys.exit("main_whole_run: --night-date must be YYYY-MM-DD")
    root = Path(args.receipt_root)
    receipt_dir = root / date
    receipt_path = receipt_dir / "receipt.json"
    lock_path = root / ".nightly.lock"
    continuing = os.environ.get("MAIN_WHOLE_RUN_CONTINUE") == date
    lock = NightlyLock(lock_path)
    receipt = {"nightDate": date, "status": "running", "startedAt": now.isoformat(),
               "headSha": None, "lastGreenSha": None, "baseline": "UNOBSERVED",
               "mergesSinceLastGreen": {"status": "UNOBSERVED", "merges": []},
               "commands": [], "logs": {}, "redSteps": [], "p0Issue": None}
    try:
        if not continuing:
            lock.__enter__()
    except RuntimeError:
        receipt_dir.mkdir(parents=True, exist_ok=True)
        atomic_json(receipt_dir / f"duplicate-blocked-{now.strftime('%H%M%S')}-{os.getpid()}.json",
                    {"nightDate": date, "status": "duplicate-blocked", "at": now.isoformat()})
        print("main_whole_run: duplicate nightly attempt refused by lock")
        return 2
    try:
        if receipt_path.exists() and not continuing:
            try:
                prior = json.loads(receipt_path.read_text(encoding="utf-8"))
            except (OSError, ValueError):
                prior = {"status": "unreadable"}
            print(f"main_whole_run: nightly attempt already exists ({prior.get('status')}); refusing rerun")
            return 0 if prior.get("status") == "passed" else 1
        receipt_dir.mkdir(parents=True, exist_ok=True)
        receipt = ({"nightDate": date, "status": "running", "startedAt": now.isoformat(),
                    "headSha": None, "lastGreenSha": None, "baseline": "UNOBSERVED",
                    "mergesSinceLastGreen": {"status": "UNOBSERVED", "merges": []},
                    "commands": [], "logs": {}, "redSteps": [], "p0Issue": None}
                   if not continuing else json.loads(receipt_path.read_text(encoding="utf-8")))
        if not continuing:
            atomic_json(receipt_path, receipt)
        if not continuing:
            allowed, preflight = quiet_preflight(args, now, run)
            receipt["preflight"] = preflight
            if not allowed:
                receipt.update(status="blocked", completedAt=utc_now().isoformat())
                atomic_json(receipt_path, receipt)
                print(f"main_whole_run: nightly blocked; receipt {receipt_path}")
                return 2
        work = Path(args.root) / "main-whole-run"
        work.mkdir(parents=True, exist_ok=True)
        git(args.repo, "fetch", "--quiet", "origin", "main")
        head = git(args.repo, "rev-parse", "origin/main")
        receipt["headSha"] = head
        green_path = root / "last-green.json"
        try:
            last_green = json.loads(green_path.read_text(encoding="utf-8"))
            baseline = last_green.get("sha")
        except (OSError, ValueError):
            last_green, baseline = {}, None
        receipt["lastGreenSha"] = baseline
        receipt["baseline"] = baseline or "UNOBSERVED"
        merge_read = merges_since_green(args.repo, args.github, baseline, head, run)
        receipt["mergesSinceLastGreen"] = merge_read
        base = baseline or git(args.repo, "rev-parse", "origin/main^")
        receipt["baseSha"] = base
        tree = work / "tree"
        prepare_tree(args.repo, tree, head)
        copy = newer_copy(tree, __file__)
        if copy and not os.environ.get("MAIN_WHOLE_RUN_REEXEC"):
            if "--nightly" not in copy.read_text(encoding="utf-8", errors="replace"):
                raise RuntimeError("latest main does not contain the nightly runner yet")
            env = dict(os.environ, MAIN_WHOLE_RUN_REEXEC="1", MAIN_WHOLE_RUN_CONTINUE=date)
            atomic_json(receipt_path, receipt)
            receipt["logs"] = {"stdout": str(receipt_dir / "runner.stdout.log"),
                                "stderr": str(receipt_dir / "runner.stderr.log"),
                                "report": str(receipt_dir / "reached.json")}
            try:
                result = reexec(copy, raw_argv, args.timeout, env)
                (receipt_dir / "runner.stdout.log").write_text(result.stdout or "", encoding="utf-8", newline="\n")
                (receipt_dir / "runner.stderr.log").write_text(result.stderr or "", encoding="utf-8", newline="\n")
                return result.returncode
            except subprocess.TimeoutExpired as timeout_error:
                stdout = timeout_error.stdout.decode("utf-8", "replace") if isinstance(timeout_error.stdout, bytes) else (timeout_error.stdout or "")
                stderr = timeout_error.stderr.decode("utf-8", "replace") if isinstance(timeout_error.stderr, bytes) else (timeout_error.stderr or "")
                (receipt_dir / "runner.stdout.log").write_text(stdout, encoding="utf-8", newline="\n")
                (receipt_dir / "runner.stderr.log").write_text(stderr, encoding="utf-8", newline="\n")
                error = "main's pinned copy exceeded the deadline"
                receipt.update(status="incomplete", error=error, completedAt=utc_now().isoformat(),
                               redSteps=[error])
                receipt["p0Issue"] = open_failure_issue(args, date, head, baseline, merge_read,
                                                        receipt_path, receipt["redSteps"], error, run)
                atomic_json(receipt_path, receipt)
                return 1
        report_path = receipt_dir / "reached.json"
        command = child_command(tree, base, head, args.root, report_path, args.timeout)
        plan = {"nightDate": date, "repo": args.github, "checkout": str(tree), "headSha": head,
                "baseSha": base, "lastGreenSha": baseline or "UNOBSERVED", "mergesSinceLastGreen": merge_read,
                "timeoutSeconds": args.timeout, "command": command, "workingDirectory": str(tree)}
        atomic_json(receipt_dir / "plan.json", plan)
        receipt["commands"] = [command]
        receipt["plan"] = str(receipt_dir / "plan.json")
        receipt["logs"] = {"stdout": str(receipt_dir / "runner.stdout.log"),
                            "stderr": str(receipt_dir / "runner.stderr.log"), "report": str(report_path)}
        atomic_json(receipt_path, receipt)
        stdout, stderr, returncode = "", "", 124
        try:
            result = run(command, capture_output=True, text=True, encoding="utf-8", timeout=args.timeout,
                         stdin=subprocess.DEVNULL, creationflags=NO_WINDOW, cwd=str(tree))
            stdout, stderr, returncode = result.stdout or "", result.stderr or "", result.returncode
        except subprocess.TimeoutExpired as error:
            stdout = error.stdout.decode("utf-8", "replace") if isinstance(error.stdout, bytes) else (error.stdout or "")
            stderr = error.stderr.decode("utf-8", "replace") if isinstance(error.stderr, bytes) else (error.stderr or "")
        (receipt_dir / "runner.stdout.log").write_text(stdout, encoding="utf-8", newline="\n")
        (receipt_dir / "runner.stderr.log").write_text(stderr, encoding="utf-8", newline="\n")
        try:
            report = json.loads(report_path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            report = {"status": "incomplete", "error": "no reached report was written"}
        red = red_steps(report)
        if returncode not in (0, None) and not red:
            red = [f"run_reached exited {returncode}"]
        status = "passed" if report.get("status") == "passed" and returncode == 0 else report.get("status", "incomplete")
        receipt.update(status=status, completedAt=utc_now().isoformat(), reachedReturncode=returncode,
                       redSteps=red, reportStatus=report.get("status"), mergesSinceLastGreen=merge_read)
        if status != "passed":
            receipt["p0Issue"] = open_failure_issue(args, date, head, baseline, merge_read, report_path, red,
                                                     report.get("error"), run)
        atomic_json(receipt_path, receipt)
        if status == "passed":
            atomic_json(green_path, {"sha": head, "nightDate": date, "receipt": str(receipt_path),
                                     "greenAt": receipt["completedAt"]})
        print(f"main_whole_run: nightly {status} on {head[:8]}; receipt {receipt_path}")
        if receipt.get("p0Issue", {}).get("status") == "failed":
            print("main_whole_run: red receipt saved, but P0 issue creation failed")
            return 1
        return 0 if status == "passed" else 1
    except Exception as error:
        receipt.update(status="incomplete", completedAt=utc_now().isoformat(), error=type(error).__name__)
        atomic_json(receipt_path, receipt)
        receipt["p0Issue"] = open_failure_issue(args, date, receipt.get("headSha"), receipt.get("lastGreenSha"),
                                                 receipt.get("mergesSinceLastGreen", {"status": "UNOBSERVED", "merges": []}),
                                                 receipt_path, ["nightly setup or runner failure"], type(error).__name__, run)
        atomic_json(receipt_path, receipt)
        print(f"main_whole_run: nightly incomplete; receipt {receipt_path}")
        return 1
    finally:
        if not continuing:
            lock.__exit__(None, None, None)


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
            "--allow-whole-package", "Scheduled main backstop covers the whole packages reached after merges",
            "--budget-seconds", repr(child_budget(timeout))]


def git(repo, *args):
    done = subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True, encoding="utf-8",
                          timeout=300, stdin=subprocess.DEVNULL, creationflags=NO_WINDOW)
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


def reexec(copy, argv, timeout, env=None):
    """Run main's copy of this script in a hidden child and capture its bounded logs."""
    env = dict(os.environ if env is None else env, MAIN_WHOLE_RUN_REEXEC="1")
    return subprocess.run([sys.executable, str(copy), *argv], env=env,
                          stdin=subprocess.DEVNULL, capture_output=True, text=True, encoding="utf-8",
                          creationflags=NO_WINDOW, timeout=timeout)


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
    ap.add_argument("--nightly", action="store_true", help="run at most once per UTC night with a durable receipt")
    ap.add_argument("--receipt-root", default="D:/gh/main-whole-run/nightly",
                    help="directory for nightly receipts and the last-green pointer")
    ap.add_argument("--night-date", help="UTC night key YYYY-MM-DD; defaults to today's UTC date")
    ap.add_argument("--idle-windows-file", default="C:/Users/gabri/OpenBot/Shared/graphhelm-openbot/IDLE-WINDOWS.md",
                    help="declared shared quiet-window file required for nightly execution")
    args = ap.parse_args(argv)
    raw_argv = list(sys.argv[1:] if argv is None else argv)
    if args.nightly:
        return run_nightly(args, raw_argv)
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
            try:
                return reexec(copy, raw_argv, args.timeout)
            except subprocess.TimeoutExpired:
                print("main_whole_run: re-exec exceeded its deadline; receipt is incomplete")
                return 1
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
                   capture_output=True, text=True, encoding="utf-8", stdin=subprocess.DEVNULL,
                   creationflags=NO_WINDOW, timeout=args.timeout)
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
