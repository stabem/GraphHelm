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
  c  the GraphHelm MCP plus the checked-in Keel skill and its contract-first workflow

Usage:
  python tools/token-bench/run.py run  --task 1145 --arm a [--model <id>] [--timeout-min 30]
  python tools/token-bench/run.py table

Results append to tools/token-bench/results.jsonl - one JSON object per run, never rewritten.
The runner has focused tests for verdict completeness; model sessions remain hand-run.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import queue
import shutil
import subprocess
import threading
import time
import uuid
from contextlib import contextmanager, nullcontext
from pathlib import Path, PureWindowsPath

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
TASKS = json.loads((HERE / "tasks.json").read_text(encoding="utf-8"))["tasks"]
RESULTS = HERE / "results.jsonl"
BENCH_VERSION = 11  # v11: fetch only the historical parent into the agent checkout.
KEEL_SKILL = REPO / "extensions" / "builtin" / "graphhelm-development-contracts" / "skills" / "keel" / "SKILL.md"

COMMON_PREFIX = """This issue is OPEN and UNFIXED. This checkout is the only source of truth: do not consult
GitHub, any remote, or any other clone - they do not exist for this task. Fix the defect in this
checkout and add the test that proves it. Do not stop at analysis: the task is done when the change
is on disk. Verify with the targeted tests of the crate or script you touched only: do NOT run
`ci/gate.ps1`, the full workspace test suite, or any other repository-wide gate - the bench runs
its own oracle afterwards, and a gate run inside this checkout measures the gate, not you.

"""

ARM_B_PREFIX = """You are working INSIDE the GraphHelm methodology. Before touching code:
1. call the `graphhelm` MCP tool `start` with a new executionId, mode `supervised`,
   held `true`, and file `examples/graphs/software-feature.yaml`;
2. call `briefing` and `compile_context` on it and use what they return;
3. when done, use `signal` with the shape in `schemas/graph-signal.schema.json` to record
   a finding with the proof command and result. Pass `evidenceOut`: `{signal_path}` because
   this isolated Runtime has no keyring.
Every code decision must be preceded by the context you got from GraphHelm, not from a raw search.

Objective / issue:

"""

ARM_C_PREFIX = """You are working INSIDE the GraphHelm + Keel methodology. Before touching code:
1. call the `graphhelm` MCP tool `start` with a new executionId, mode `supervised`,
   held `true`, and file `examples/graphs/software-feature.yaml`;
2. call `briefing` and `compile_context` on it and use what they return;
3. before editing code, write a JSON Keel card to {card_path} with nonempty `paths` (exact paths,
   no globs), `promise`, `defect`, and `proofCommand` fields;
4. when done, run the named proof and call `signal` on the same execution with a schema-valid
   finding whose evidence names the command and result. Pass `evidenceOut`: `{signal_path}`
   because this isolated Runtime has no keyring. The `evidence` MCP tool is read-only.
Do not claim success from a smaller diff or from a passing command alone: the hidden acceptance and
the pre-existing regression suite are the final observers. Every code decision must use the MCP
context and Keel card before raw repository search.

Objective / issue:

"""


def compose_arm_c_prompt(issue_prompt: str, card_path: Path, signal_path: Path) -> str:
    """Keep the actual task adjacent to its heading, before the supporting skill text."""
    skill = KEEL_SKILL.read_text(encoding="utf-8")
    return (ARM_C_PREFIX.format(card_path=card_path, signal_path=signal_path) + issue_prompt +
            "\n\nPinned Keel skill (digest " + (digest_file(KEEL_SKILL) or "missing") + "):\n" +
            "--- BEGIN PINNED KEEL SKILL ---\n" + skill + "\n--- END PINNED KEEL SKILL ---\n")


def sh(args: list[str], cwd: Path | None = None, check: bool = True, **kw) -> subprocess.CompletedProcess:
    return subprocess.run(args, cwd=cwd, check=check, text=True, capture_output=True, **kw)


def git_show(sha: str, path: str) -> str:
    return sh(["git", "show", f"{sha}:{path}"], cwd=REPO).stdout


def git_show_bytes(sha: str, path: str) -> bytes:
    return subprocess.run(["git", "show", f"{sha}:{path}"], cwd=REPO,
                          check=True, capture_output=True).stdout


def digest_bytes(value: bytes) -> str:
    return "sha256:" + hashlib.sha256(value).hexdigest()


def digest_file(path: Path) -> str | None:
    try:
        return digest_bytes(path.read_bytes())
    except OSError:
        return None


def claude_cli_identity() -> dict:
    """Pin the executable whose transcript and cost format a row interprets."""
    found = os.environ.get("TOKEN_BENCH_CLAUDE_CLI") or shutil.which("claude")
    if not found:
        raise RuntimeError("Claude CLI executable is unavailable")
    path = Path(found).resolve(strict=True)
    digest = digest_file(path)
    version = subprocess.run([str(path), "--version"], check=True, text=True,
                             capture_output=True, timeout=10).stdout.strip()
    if not digest or not version:
        raise RuntimeError("Claude CLI identity could not be verified")
    return {"path": str(path), "sha256": digest, "version": version}


@contextmanager
def isolated_runtime(wt: Path, task_id: str, arm: str):
    """Give one agent its own GraphHelm event store and checkout-bound Runtime."""
    exe_raw = os.environ.get("TOKEN_BENCH_GRAPHHELM_CLI")
    if not exe_raw:
        raise RuntimeError("TOKEN_BENCH_GRAPHHELM_CLI must name the pinned GraphHelm executable")
    exe = Path(exe_raw).resolve(strict=True)
    run_dir = scratch_root() / "runtimes" / f"tb-{task_id}-{arm}-{uuid.uuid4().hex}"
    run_dir.mkdir(parents=True)
    events = run_dir / "events"
    stderr_file = run_dir / "serve.stderr.log"
    log = stderr_file.open("w", encoding="utf-8")
    creationflags = getattr(subprocess, "CREATE_NO_WINDOW", 0) if os.name == "nt" else 0
    proc = subprocess.Popen([str(exe), "serve", "--events", str(events), "--bind", "127.0.0.1:0",
                             "--project", str(wt)], cwd=wt, stdout=subprocess.PIPE, stderr=log,
                            text=True, encoding="utf-8", creationflags=creationflags)
    lines: queue.Queue[str] = queue.Queue(maxsize=1)
    threading.Thread(target=lambda: lines.put(proc.stdout.readline()), daemon=True).start()
    try:
        try:
            line = lines.get(timeout=20)
        except queue.Empty as exc:
            raise RuntimeError("isolated GraphHelm Runtime did not start within 20 seconds") from exc
        try:
            started = json.loads(line)
            data = started["data"]
            address = data["address"]
            executors = data["executors"]
        except (ValueError, KeyError, TypeError) as exc:
            raise RuntimeError("isolated GraphHelm Runtime returned no valid serve.started envelope") from exc
        if started.get("command") != "serve.started" or not address.startswith("127.0.0.1:"):
            raise RuntimeError("isolated GraphHelm Runtime did not bind loopback")
        token_file = run_dir / "events.token"
        if not token_file.is_file():
            raise RuntimeError("isolated GraphHelm Runtime did not create its token")
        config = run_dir / "mcp.json"
        config.write_text(json.dumps({"mcpServers": {"graphhelm": {"command": str(exe), "args": [
            "mcp", "--url", f"http://{address}", "--token-file", str(token_file),
            "--actor", f"bench-{task_id}-{arm}-{run_dir.name[-8:]}"],
        }}}), encoding="utf-8")
        yield config, {"cliDigest": digest_file(exe), "executors": executors,
                       "address": address, "actor": f"bench-{task_id}-{arm}-{run_dir.name[-8:]}",
                       "events": str(events), "shutdown": None}
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill(); proc.wait(timeout=5)
        log.close()


def task_digest(task: dict) -> str:
    """Bind a row to the exact task, prompt, oracle, and regression inputs."""
    pieces = [str(task.get("parentSha", "")).encode(), str(task.get("fixSha", "")).encode()]
    pieces.append((HERE / task["prompt"]).read_bytes())
    oracle = task["oracle"]
    for entry in oracle.get("files") or [oracle]:
        pieces.append((HERE / entry["source"]).read_bytes() if entry.get("source") else git_show(task["fixSha"], entry["file"]).encode())
    pieces.append(json.dumps(oracle.get("command"), sort_keys=True).encode())
    pieces.append(json.dumps(task.get("regression"), sort_keys=True).encode())
    return digest_bytes(b"\0".join(pieces))


def oracle_digest(task: dict) -> str:
    oracle = task["oracle"]
    pieces = [(HERE / entry["source"]).read_bytes() if entry.get("source") else git_show(task["fixSha"], entry["file"]).encode()
              for entry in (oracle.get("files") or [oracle])]
    return digest_bytes(b"\0".join(pieces))


def scratch_root() -> Path:
    raw = os.environ.get("TOKEN_BENCH_SCRATCH")
    if not raw:
        raise RuntimeError("TOKEN_BENCH_SCRATCH must point outside the repository")
    root = Path(raw).expanduser().resolve()
    repo = REPO.resolve()
    if root == repo or repo in root.parents:
        raise RuntimeError("TOKEN_BENCH_SCRATCH must be outside the repository")
    root.mkdir(parents=True, exist_ok=True)
    return root


def make_snapshot(task_id: str, arm: str, parent_sha: str) -> Path:
    # Evaluators need exact source bytes, not Git metadata. The agent gets a fresh one-commit
    # repository from make_worktree; neither kind exposes refs containing the historical fix.
    name = f"tb-{task_id}-{arm}-{dt.datetime.utcnow():%Y%m%dT%H%M%S}-{uuid.uuid4().hex[:6]}"
    base = scratch_root() / "checkouts"
    base.mkdir(parents=True, exist_ok=True)
    wt = base / name
    wt.mkdir(parents=True)
    archive = subprocess.run(["git", "archive", "--format=tar", parent_sha], cwd=REPO, check=True, capture_output=True).stdout
    import io, tarfile
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:") as tar:  # not the shell's tar: msys mangles Windows paths
        tar.extractall(wt)
    return wt


def make_worktree(task_id: str, arm: str, parent_sha: str) -> Path:
    """Give the agent an isolated one-commit repository with no future refs or remote."""
    name = f"tb-{task_id}-{arm}-{dt.datetime.utcnow():%Y%m%dT%H%M%S}-{uuid.uuid4().hex[:6]}"
    wt = scratch_root() / "checkouts" / name
    wt.mkdir(parents=True)
    sh(["git", "init", "-q", "-b", "main"], cwd=wt)
    sh(["git", "fetch", "--no-tags", "--depth=1", "--no-write-fetch-head", REPO.resolve().as_uri(),
        parent_sha], cwd=wt)
    sh(["git", "reset", "--hard", parent_sha], cwd=wt)
    return wt


def validate_prerequisites(arm: str) -> dict:
    """Reject missing launch inputs before creating expensive historical snapshots."""
    scratch_root()
    if arm in {"b", "c"}:
        raw = os.environ.get("TOKEN_BENCH_GRAPHHELM_CLI")
        if not raw or not Path(raw).is_file():
            raise SystemExit("TOKEN_BENCH_GRAPHHELM_CLI must name an existing GraphHelm executable")
        try:
            version = subprocess.run([str(Path(raw).resolve()), "--version"], check=True,
                                     text=True, capture_output=True, timeout=10).stdout.strip()
        except (OSError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as exc:
            raise SystemExit("TOKEN_BENCH_GRAPHHELM_CLI is not launchable") from exc
        if not version.startswith("graphhelm "):
            raise SystemExit("TOKEN_BENCH_GRAPHHELM_CLI did not identify as GraphHelm")
    return claude_cli_identity()


def remove_worktree(wt: Path) -> None:
    target = wt.resolve()
    root = (scratch_root() / "checkouts").resolve()
    if target.parent != root or not target.name.startswith("tb-"):
        raise RuntimeError(f"refusing to remove non-runner checkout: {target}")
    shutil.rmtree(target, ignore_errors=True)


def bench_env(task: dict) -> dict:
    # a Rust task shares ONE warm target dir across its arms (arms run one at a time), so neither
    # the agent nor the oracle pays a cold workspace build; tokens are what is measured, not cargo
    env = dict(os.environ)
    if task.get("sharedTargetDir"):
        env["CARGO_TARGET_DIR"] = str(scratch_root() / task["sharedTargetDir"])
    return env


def run_agent(wt: Path, prompt: str, arm: str, model: str | None, timeout_min: int, task: dict,
              max_budget_usd: float | None, mcp_config: Path | None,
              claude_cli: dict) -> tuple[dict, str, float]:
    session_id = str(uuid.uuid4())
    cmd = [claude_cli["path"], "-p", "--output-format", "json", "--dangerously-skip-permissions",
           "--session-id", session_id, "--setting-sources", "", "--strict-mcp-config",
           "--disallowedTools", "Bash(gh *)", "WebFetch", "WebSearch"]
    if model:
        cmd += ["--model", model]
    if arm in {"b", "c"}:
        if mcp_config is None or not mcp_config.is_file():
            raise RuntimeError(f"arm {arm} needs a per-run MCP config")
        cmd += ["--mcp-config", str(mcp_config), "--allowedTools", "mcp__graphhelm"]
    if max_budget_usd is not None:
        cmd += ["--max-budget-usd", str(max_budget_usd)]
    t0 = time.monotonic()
    try:
        proc = subprocess.run(cmd, cwd=wt, input=prompt, text=True, encoding="utf-8", capture_output=True,
                              timeout=timeout_min * 60, env=bench_env(task))
    except subprocess.TimeoutExpired as exc:
        return {"agentError": "timeout", "timeoutSeconds": timeout_min * 60,
                "session_id": session_id, "stderr": str(exc)}, "timeout", time.monotonic() - t0
    wall = time.monotonic() - t0
    raw = proc.stdout.strip()
    try:
        result = json.loads(raw)
        if not isinstance(result, dict):
            raise json.JSONDecodeError("result is not an object", raw, 0)
    except json.JSONDecodeError:
        result = {"unparseable_stdout": raw[-4000:], "stderr": proc.stderr[-4000:], "exit": proc.returncode}
    if result.get("session_id") and result["session_id"] != session_id:
        result["agentError"] = "session_id_mismatch"
    return result, proc.stderr, wall


def transcript_usage(session_id: str | None) -> dict:
    """Sum the session's own transcript. The `usage` block in `claude -p`'s JSON result is NOT the
    session total - on task 1044 arm b it reported 3 turns / 297 output tokens for a 113-step,
    $8.68 session - so the transcript is the instrument and the JSON is kept only as a cross-check."""
    if not session_id:
        return {}
    hits = list((Path.home() / ".claude" / "projects").glob(f"*/{session_id}.jsonl"))
    if len(hits) != 1:
        return {"missing": True, "matchingTranscripts": len(hits)}
    return sum_transcript_usage(hits[0].read_text(encoding="utf-8", errors="replace").splitlines(), str(hits[0]))


def methodology_observer(session_id: str | None, runtime: dict | None,
                         card_path: Path | None, signal_path: Path | None, arm: str) -> dict:
    """Bind actual MCP calls, the Keel card, and Runtime events to one agent session."""
    if arm == "a":
        return {"status": "NOT_APPLICABLE"}
    result = {"status": "INCOMPLETE", "stages": [], "eventTypes": [],
              "cardDigest": None, "cardOrder": "UNOBSERVED"}
    if not session_id or not runtime:
        return result
    hits = list((Path.home() / ".claude" / "projects").glob(f"*/{session_id}.jsonl"))
    if len(hits) != 1:
        return result
    calls = {}
    successful = []
    for line in hits[0].read_text(encoding="utf-8", errors="replace").splitlines():
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        message = record.get("message") or {}
        if not isinstance(message, dict):
            continue
        content = message.get("content") or []
        if not isinstance(content, list):
            continue
        for block in content:
            if not isinstance(block, dict):
                continue
            if record.get("type") == "assistant" and block.get("type") == "tool_use":
                name = block.get("name", "")
                if name.startswith("mcp__graphhelm__"):
                    calls[block.get("id")] = (name.rsplit("__", 1)[-1], block.get("input") or {})
            elif record.get("type") == "user" and block.get("type") == "tool_result":
                call = calls.get(block.get("tool_use_id"))
                if call and not block.get("is_error"):
                    body = block.get("content")
                    if isinstance(body, list):
                        body = " ".join(str(item.get("text", "")) for item in body if isinstance(item, dict))
                    try:
                        response = json.loads(body) if isinstance(body, str) else None
                    except json.JSONDecodeError:
                        response = None
                    if not isinstance(response, dict) or response.get("ok") is not True:
                        continue
                    successful.append(call)
    required = {"start", "briefing", "compile_context", "signal"}
    result["stages"] = sorted({name for name, _ in successful})
    starts = {args.get("executionId") for name, args in successful if name == "start"}
    starts.discard(None)
    if len(starts) != 1 or not required.issubset(result["stages"]):
        return result
    execution_id = next(iter(starts))
    positions = {name: next(index for index, (stage, _) in enumerate(successful) if stage == name)
                 for name in required}
    if not (positions["start"] < positions["briefing"] < positions["signal"] and
            positions["start"] < positions["compile_context"] < positions["signal"]):
        return result
    if any(args.get("executionId") != execution_id for name, args in successful
           if name in {"briefing", "signal"}):
        return result
    journal = Path(runtime["events"]) / "journal.jsonl"
    if not journal.is_file():
        return result
    event_types = set()
    signal_hashes = set()
    try:
        journal_lines = journal.read_text(encoding="utf-8").splitlines()
    except OSError:
        return result
    for line in journal_lines:
        try:
            batch = json.loads(line)
        except json.JSONDecodeError:
            return result
        if not isinstance(batch, dict) or not isinstance(batch.get("events"), list):
            return result
        for event in batch["events"]:
            if not isinstance(event, dict):
                return result
            kind = event.get("kind")
            actor = event.get("actor")
            if not isinstance(kind, dict) or not isinstance(actor, dict):
                return result
            data = kind.get("data")
            if not isinstance(data, dict):
                return result
            if (actor.get("id") == runtime["actor"] and data.get("executionId") == execution_id):
                event_types.add(kind.get("type"))
                if kind.get("type") == "signal_recorded":
                    if isinstance(data.get("envelopeSha256"), str):
                        signal_hashes.add(data["envelopeSha256"])
    result["eventTypes"] = sorted(event_types)
    if not {"execution_started", "signal_recorded"}.issubset(event_types):
        return result
    if not signal_path or not signal_path.is_file():
        return result
    try:
        signal_bytes = signal_path.read_bytes()
        recorded_signal = json.loads(signal_bytes)
    except (OSError, json.JSONDecodeError):
        return result
    if not isinstance(recorded_signal, dict):
        return result
    signal_digest = digest_bytes(signal_bytes).split(":", 1)[1]
    if signal_digest not in signal_hashes and f"sha256:{signal_digest}" not in signal_hashes:
        return result
    evidence = recorded_signal.get("evidence")
    if (not isinstance(recorded_signal.get("description"), str) or
            not recorded_signal["description"].strip() or
            not isinstance(evidence, list) or not evidence or
            any(not isinstance(item, str) or not item.strip() for item in evidence)):
        return result
    if arm == "c":
        try:
            card = json.loads(card_path.read_text(encoding="utf-8")) if card_path else {}
        except (OSError, json.JSONDecodeError):
            return result
        paths = card.get("paths")
        if (not isinstance(paths, list) or not paths or
                any(not isinstance(path, str) or not path or Path(path).is_absolute() or
                    PureWindowsPath(path).is_absolute() or PureWindowsPath(path).drive or
                    path.startswith(("/", "\\")) or any(char in path for char in "*?[]") or
                    ".." in path.replace("\\", "/").split("/")
                    for path in paths) or
                any(not isinstance(card.get(key), str) or not card[key].strip()
                    for key in ("promise", "defect", "proofCommand"))):
            return result
        if not any(card["proofCommand"] in item for item in evidence):
            return result
        result["cardDigest"] = digest_file(card_path)
    if not any(word in item.lower() for item in evidence for word in ("exit", "passed", "failed", "pass", "fail")):
        return result
    result["status"] = "PASS"
    return result


def sum_transcript_usage(lines: list[str], path: str | None = None) -> dict:
    tot = {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0,
           "assistantMessages": 0, "userMessages": 0, "parseErrors": 0}
    by_id = {}
    models = set()
    tools_seen = {}
    for line in lines:
        try:
            d = json.loads(line)
        except json.JSONDecodeError:
            tot["parseErrors"] += 1
            continue
        if not isinstance(d, dict):
            tot["parseErrors"] += 1
            continue
        if d.get("type") == "user":
            tot["userMessages"] += 1
        if d.get("type") != "assistant":
            continue
        message = d.get("message") or {}
        if not isinstance(message, dict):
            tot["parseErrors"] += 1
            continue
        if isinstance(message.get("model"), str) and message["model"] not in {"<synthetic>", "unknown"}:
            models.add(message["model"])
        for block in message.get("content") or []:
            if isinstance(block, dict) and block.get("type") == "tool_use" and block.get("id"):
                tools_seen[block["id"]] = block.get("name", "unknown")
        message_id = message.get("id") or d.get("uuid")
        u = message.get("usage") or {}
        if not isinstance(u, dict):
            tot["parseErrors"] += 1
            continue
        counts = {"input": u.get("input_tokens", 0), "output": u.get("output_tokens", 0),
                  "cacheRead": u.get("cache_read_input_tokens", 0),
                  "cacheWrite": u.get("cache_creation_input_tokens", 0)}
        key = message_id or f"line-{len(by_id)}"
        if key not in by_id or sum(counts.values()) > sum(by_id[key].values()):
            by_id[key] = counts
    for counts in by_id.values():
        for name, value in counts.items():
            tot[name] += value
    tot["assistantMessages"] = len(by_id)
    tot["models"] = sorted(models)
    tot["toolCalls"] = len(tools_seen)
    tot["toolNames"] = {name: list(tools_seen.values()).count(name) for name in sorted(set(tools_seen.values()))}
    if path:
        tot["path"] = path
    return tot


def usage_audit(usage: dict, terminal: dict) -> dict:
    """Refuse a transcript that cannot account for the CLI's completed result."""
    reasons = []
    if usage.get("parseErrors", 0) != 0:
        reasons.append("transcript_parse_error")
    turns = terminal.get("num_turns")
    # Claude's num_turns tracks user-side turns, including tool results. A live 28-turn
    # session had 28 user records but only 26 distinct assistant message IDs.
    if (not isinstance(turns, int) or turns < 1 or
            usage.get("userMessages", 0) < turns or usage.get("assistantMessages", 0) < 1):
        reasons.append("transcript_turns_incomplete")
    last = terminal.get("usage")
    names = {"input": "input_tokens", "output": "output_tokens",
             "cacheRead": "cache_read_input_tokens", "cacheWrite": "cache_creation_input_tokens"}
    if not isinstance(last, dict):
        reasons.append("terminal_usage_missing")
    else:
        for observed, terminal_name in names.items():
            value = last.get(terminal_name)
            if not isinstance(value, int) or usage.get(observed, -1) < value:
                reasons.append(f"transcript_{observed}_incomplete")
    return {"status": "PASS" if not reasons else "INCOMPLETE", "reasons": reasons}


def coding_model(model_usage: dict | None, transcript_models: list[str]) -> tuple[str | None, list[str]]:
    """The coding model is in assistant records; CLI modelUsage may include background models."""
    coding = sorted(set(transcript_models))
    reported = set(model_usage) if isinstance(model_usage, dict) else set()
    all_models = sorted(reported | set(coding))
    if len(coding) != 1 or not reported or coding[0] not in reported:
        return None, all_models
    return coding[0], all_models


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
            target.write_bytes(git_show_bytes(task["fixSha"], entry["file"]))
    proc = subprocess.run(oracle["command"], cwd=wt, text=True, capture_output=True, env=bench_env(task))
    verdict = "PASS" if proc.returncode == 0 else "FAIL"
    # keep the lines that say WHICH assertion failed; the raw tail was CLIXML progress noise from stderr
    lines = [l for l in proc.stdout.splitlines() if "FAIL" in l or "assertions" in l or "HARNESS" in l]
    tail = "\n".join(lines)[-2000:] or proc.stdout[-800:]
    return verdict, proc.returncode, tail


def run_submitted_suite(wt: Path, task: dict) -> tuple[str, int | None, str]:
    """Observe the agent's checkout before independent test files replace its tests."""
    spec = task.get("regression")
    if not spec:
        return "UNOBSERVED", None, "task has no submitted-suite command"
    command = spec["command"] if isinstance(spec, dict) else spec
    proc = subprocess.run(command, cwd=wt, text=True, capture_output=True, env=bench_env(task))
    verdict = "PASS" if proc.returncode == 0 else "FAIL"
    return verdict, proc.returncode, (proc.stdout + "\n" + proc.stderr)[-2000:]


def run_regression(wt: Path, task: dict) -> tuple[str, int | None, str]:
    """Run only a pre-existing regression observer declared by the task manifest."""
    spec = task.get("regression")
    if not spec:
        return "UNOBSERVED", None, "task has no pre-existing regression command"
    command = spec["command"] if isinstance(spec, dict) else spec
    # Do not let the agent edit the regression test and then report its edited test as proof.
    # Manifests may name files explicitly; a PowerShell -File command is safely inferred too.
    files = (spec.get("files") if isinstance(spec, dict) else None) or []
    if not files and isinstance(command, list) and "-File" in command:
        files = [command[command.index("-File") + 1]]
    for path in files:
        target = wt / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(git_show_bytes(task["parentSha"], path))
    proc = subprocess.run(command, cwd=wt, text=True, capture_output=True, env=bench_env(task))
    verdict = "PASS" if proc.returncode == 0 else "FAIL"
    tail = (proc.stdout + "\n" + proc.stderr)[-2000:]
    return verdict, proc.returncode, tail


def run_proofs(wt: Path, task: dict) -> tuple[tuple[str, int | None, str],
                                               tuple[str, int | None, str],
                                               tuple[str, int, str]]:
    """Observe submitted tests before historical restoration and hidden-oracle installation."""
    submitted = run_submitted_suite(wt, task)
    regression = run_regression(wt, task)
    acceptance = run_oracle(wt, task)
    return submitted, regression, acceptance


def evaluate_outcome(acceptance: str, regression: str, usage: dict, cost: object,
                    session_id: object, task: dict, agent_error: bool = False,
                    submitted_suite: str | None = None) -> tuple[str, list[str]]:
    """Return a conservative delivery verdict; missing proof is never a win."""
    reasons = []
    if acceptance != "PASS":
        reasons.append("hidden_acceptance_failed")
    if regression != "PASS":
        reasons.append("preexisting_regression_" + regression.lower())
    if submitted_suite == "FAIL":
        reasons.append("submitted_suite_failed")
    if not task.get("regression"):
        reasons.append("regression_observer_missing")
    required = ("input", "output", "cacheRead", "cacheWrite", "assistantMessages")
    if (not session_id or usage.get("missing") or usage.get("assistantMessages", 0) <= 0
            or any(not isinstance(usage.get(k), int) for k in required)
            or sum(usage.get(k, 0) for k in required[:4]) <= 0):
        reasons.append("session_tokens_unobserved")
    if not isinstance(cost, (int, float)) or cost <= 0:
        reasons.append("session_cost_unobserved")
    if agent_error:
        reasons.append("agent_execution_unobserved")
    if reasons:
        return ("INCOMPLETE" if "unobserved" in " ".join(reasons) or "missing" in " ".join(reasons) else "FAIL"), reasons
    return "PASS", []


def save_patch_artifact(task_id: str, arm: str, patch: str) -> tuple[str, str]:
    artifact_dir = scratch_root() / "artifacts"
    artifact_dir.mkdir(parents=True, exist_ok=True)
    path = artifact_dir / f"{task_id}-{arm}-{uuid.uuid4().hex}.patch"
    data = patch.encode("utf-8")
    path.write_bytes(data)
    return str(path), digest_bytes(data)


def cmd_run(a: argparse.Namespace) -> None:
    task = TASKS[a.task]
    claude_cli = validate_prerequisites(a.arm)
    preflight_start = time.monotonic()
    # Validate the study instrument before spending a model session. The parent must fail the
    # hidden acceptance, while its unchanged regression suite must pass.
    snapshots = []
    try:
        preflight = make_snapshot(a.task, "preflight", task["parentSha"])
        snapshots.append(preflight)
        oracle_preflight = make_snapshot(a.task, "preflight-oracle", task["parentSha"])
        snapshots.append(oracle_preflight)
        fixed_preflight = make_snapshot(a.task, "preflight-fixed", task["fixSha"])
        snapshots.append(fixed_preflight)
        parent_regression, _, regression_tail = run_regression(preflight, task)
        parent_acceptance, _, _ = run_oracle(oracle_preflight, task)
        known_fix_acceptance, _, _ = run_oracle(fixed_preflight, task)
    finally:
        for snapshot in snapshots:
            remove_worktree(snapshot)
    if parent_acceptance == "PASS" or parent_regression != "PASS" or known_fix_acceptance != "PASS":
        raise SystemExit(
            f"invalid benchmark task {a.task}: parent acceptance={parent_acceptance}, "
            f"regression={parent_regression}, known-fix acceptance={known_fix_acceptance}\n"
            f"{regression_tail[-1000:]}"
        )
    preflight_seconds = time.monotonic() - preflight_start
    prompt = COMMON_PREFIX + (HERE / task["prompt"]).read_text(encoding="utf-8")
    signal_path = None
    if a.arm in {"b", "c"}:
        signal_dir = scratch_root() / "signals"
        signal_dir.mkdir(parents=True, exist_ok=True)
        signal_path = signal_dir / f"{a.task}-{a.arm}-{uuid.uuid4().hex}.json"
    card_path = None
    if a.arm == "b":
        prompt = ARM_B_PREFIX.format(signal_path=signal_path) + prompt
    elif a.arm == "c":
        card_dir = scratch_root() / "cards"
        card_dir.mkdir(parents=True, exist_ok=True)
        card_path = card_dir / f"{a.task}-c-{uuid.uuid4().hex}.json"
        prompt = compose_arm_c_prompt(prompt, card_path, signal_path)
    verdict = "FAIL"
    acceptance = "UNOBSERVED"
    regression = "UNOBSERVED"
    regression_code = None
    regression_tail = ""
    submitted_suite = "UNOBSERVED"
    submitted_code = None
    submitted_tail = ""
    code = None
    oracle_tail = ""
    checkout_start = time.monotonic()
    wt = make_worktree(a.task, a.arm, task["parentSha"])
    checkout_seconds = time.monotonic() - checkout_start
    print(f"[bench] task {a.task} arm {a.arm} worktree {wt}", flush=True)
    result = {"agentError": "agent_not_started"}
    wall = 0.0
    runtime_meta = None
    mcp_config = None
    mcp_digest = None
    diff = ""
    patch_artifact = None
    patch_digest = None
    evaluator_error = None
    try:
        runtime_context = isolated_runtime(wt, a.task, a.arm) if a.arm in {"b", "c"} else nullcontext((None, None))
        with runtime_context as (mcp_config, runtime_meta):
            mcp_digest = digest_file(mcp_config) if mcp_config else None
            result, stderr, wall = run_agent(wt, prompt, a.arm, a.model, a.timeout_min, task,
                                             a.max_budget_usd, mcp_config, claude_cli)
        # everything the agent left behind - committed, staged, unstaged or untracked - against the baseline
        root = sh(["git", "rev-list", "--max-parents=0", "HEAD"], cwd=wt).stdout.strip()
        sh(["git", "add", "-A"], cwd=wt, check=False)
        diff = sh(["git", "diff", "--cached", "--stat", root], cwd=wt, check=False).stdout
        patch = sh(["git", "diff", "--cached", root], cwd=wt, check=False).stdout
        patch_artifact, patch_digest = save_patch_artifact(a.task, a.arm, patch)
        # Some tasks share a test path across all three observers; their order is contractual.
        (submitted_suite, submitted_code, submitted_tail), (
            regression, regression_code, regression_tail), (
            acceptance, code, oracle_tail) = run_proofs(wt, task)
    except Exception as exc:
        evaluator_error = type(exc).__name__
    finally:
        # a failed run keeps its checkout: the diff IS the evidence, and rmtree was silently
        # half-deleting it (open handles) while reporting nothing
        if not a.keep and acceptance == "PASS" and regression == "PASS" and submitted_suite == "PASS":
            remove_worktree(wt)
        else:
            print(f"[bench] checkout kept at {wt}", flush=True)
    usage = transcript_usage(result.get("session_id"))
    usage_check = usage_audit(usage, result)
    method = methodology_observer(result.get("session_id"), runtime_meta, card_path, signal_path, a.arm)
    cost = result.get("total_cost_usd")
    cli_post_digest = digest_file(Path(claude_cli["path"])) if claude_cli else None
    model_usage = result.get("modelUsage")
    code_model, model_ids = coding_model(model_usage, usage.get("models") or [])
    agent_error = bool(result.get("is_error") or result.get("agentError") or result.get("unparseable_stdout"))
    verdict, verdict_reasons = evaluate_outcome(acceptance, regression, usage, cost,
                                                 result.get("session_id"), task, agent_error,
                                                 submitted_suite)
    if a.arm in {"b", "c"} and method["status"] != "PASS":
        verdict = "INCOMPLETE"
        verdict_reasons.append("methodology_execution_unobserved")
    if not code_model:
        verdict = "INCOMPLETE"
        verdict_reasons.append("exact_model_unobserved_or_mixed")
    if usage_check["status"] != "PASS":
        verdict = "INCOMPLETE"
        verdict_reasons.extend(usage_check["reasons"])
    if runtime_meta and runtime_meta["executors"].get("model"):
        verdict = "INCOMPLETE"
        verdict_reasons.append("runtime_model_cost_unobserved")
    if not claude_cli or cli_post_digest != claude_cli["sha256"]:
        verdict = "INCOMPLETE"
        verdict_reasons.append("claude_cli_identity_unstable_or_missing")
    if evaluator_error:
        verdict = "INCOMPLETE"
        verdict_reasons.append("runner_or_evaluator_error")
    row = {
        "at": dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds"),
        "benchVersion": BENCH_VERSION, "task": a.task, "arm": a.arm,
        "mcpConfig": str(mcp_config) if mcp_config else None,
        "mcpConfigDigest": mcp_digest,
        "runtime": runtime_meta,
        "methodologyObserver": method,
        "cardPath": str(card_path) if card_path else None,
        "signalEvidencePath": str(signal_path) if signal_path else None,
        "keelSkill": str(KEEL_SKILL) if a.arm == "c" else None,
        "keelSkillDigest": digest_file(KEEL_SKILL) if a.arm == "c" else None,
        "taskDigest": task_digest(task), "oracleDigest": oracle_digest(task),
        "parentSha": task["parentSha"], "requestedModel": a.model,
        "claudeCli": claude_cli,
        "claudeCliPostDigest": cli_post_digest,
        "actualModels": model_ids,
        "modelUsage": model_usage if isinstance(model_usage, dict) else None,
        "patchArtifact": patch_artifact, "patchDigest": patch_digest,
        "model": code_model,
        "verdict": verdict, "verdictReasons": verdict_reasons,
        "acceptance": acceptance, "oracleExit": code,
        "regression": regression, "regressionExit": regression_code,
        "regressionTail": regression_tail,
        "submittedSuite": submitted_suite, "submittedSuiteExit": submitted_code,
        "submittedSuiteTail": submitted_tail,
        "automatedProof": {
            "acceptance": acceptance == "PASS", "preexistingRegression": regression == "PASS",
            "submittedSuite": submitted_suite == "PASS",
            "sessionUsageComplete": not any("unobserved" in reason for reason in verdict_reasons),
            "qualityClaim": ("preliminary-automated-proof" if acceptance == regression == submitted_suite == "PASS"
                             else "not-proven"),
        },
        "wallSeconds": round(wall, 1),
        "preflightSeconds": round(preflight_seconds, 1),
        "checkoutSeconds": round(checkout_seconds, 1),
        "agentDurationMs": result.get("duration_ms"), "turns": result.get("num_turns"),
        "toolCalls": usage.get("toolCalls"), "toolNames": usage.get("toolNames"),
        "attempts": 1, "repairsObserved": None,
        "costUsd": cost,
        "inputTokens": usage.get("input"), "outputTokens": usage.get("output"),
        "cacheReadTokens": usage.get("cacheRead"),
        "cacheWriteTokens": usage.get("cacheWrite"),
        "sessionId": result.get("session_id"),
        "transcript": usage,
        "usageAudit": usage_check,
        "diffStat": diff.strip()[-800:],
        "oracleTail": oracle_tail,
        "evaluatorError": evaluator_error,
        "agentError": result.get("agentError") or (str(result.get("result", ""))[:500] if result.get("is_error") else None)
                      or (result.get("unparseable_stdout") and {
                          "stdout": result["unparseable_stdout"], "stderr": result.get("stderr"), "exit": result.get("exit")}),
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
    r.add_argument("--arm", required=True, choices=["a", "b", "c"])
    r.add_argument("--model")
    r.add_argument("--timeout-min", type=int, default=30)
    r.add_argument("--max-budget-usd", type=float)
    r.add_argument("--keep", action="store_true", help="keep the worktree for inspection")
    r.set_defaults(fn=cmd_run)
    t = sub.add_parser("table"); t.set_defaults(fn=cmd_table)
    a = p.parse_args(); a.fn(a)


if __name__ == "__main__":
    main()
