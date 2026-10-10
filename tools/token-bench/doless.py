#!/usr/bin/env python3
"""do-less: does the agent do only what the task defined? (Keel item 4, #1335)

token-bench's `run.py` asks "was the defect fixed, and at what cost". This runner asks the other
half of Keel's job: the change stays inside its card, adds no file, test or public symbol the task
did not ask for, and breaks nothing. A run is one fresh `claude -p` session on one frozen task
(`doless/tasks.json`) in one arm:

  a  the agent and the task prompt only
  k  the same, plus the pinned Keel surfaces - the skill, the AGENTS.md Keel section and keel.yaml -
     read from `--surface-dir` (default: this checkout). The hillclimb edits copies of exactly these.
  d  the same as a, plus the short Keel digest (`doless/surfaces/keel-digest.md`, also read from
     `--surface-dir`) and the path of the full skill: the cheap alternative to k.

Every task has two prompts: `prompt` states the task with its temptations and keep-working rules
named (the first sets), `promptIssue` states only the symptom and the desired outcome, the way a
user files an issue. `--prompt-style` picks one; the row records which.

Commands:
  python tools/token-bench/doless.py qualify [--task ID]      parent regression green, oracle red
                                                              on the parent, oracle green on the
                                                              known fix; the known fix is scored too
  python tools/token-bench/doless.py run --task ID --arm a|k|d [--runs 3] [--model sonnet]
                                          [--prompt-style explicit|issue]
  python tools/token-bench/doless.py table [--split train|test] [--arm a] [--prompt-style issue]
  python tools/token-bench/doless.py split                    prints the frozen train/test split
  python tools/token-bench/doless.py pareto [--split test] [--prompt-style issue] [--svg chart.svg]
                                          score AND cost per arm, and which arms are on the
                                          Pareto frontier (also: `table --pareto`)

`run --arm a --treatment routed --inference-config FILE` selects each task's declared `kind`
using doless/routed.json. FILE maps small, standard and large to the existing uniform capability
contracts (version, host, provider, exact model, effort, supportedEfforts, capabilitySource).
The route replaces each contract's effort; every mapped effort must be explicitly supported.
The whole policy and all tier contracts define one comparison digest; each row also records its
chosen kind/tier/model/effort. No retries escalate in this first slice. Model IDs and capability
claims are operator declarations, not provider observations. Running this command starts model
sessions; offline tests mock that boundary, and paid/held-out experiments need separate approval.

Needs TOKEN_BENCH_SCRATCH (outside the repository) and TOKEN_BENCH_GRAPHHELM_CLI (a graphhelm
executable that has `keel check`). Rows append to tools/token-bench/doless/results.jsonl.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import importlib.util
import json
import math
import os
import re
import shutil
import subprocess
import sys
import time
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
DOLESS = HERE / "doless"
MANIFEST = DOLESS / "tasks.json"
RESULTS = DOLESS / "results.jsonl"
DOLESS_VERSION = 2

_spec = importlib.util.spec_from_file_location("token_bench_runner", HERE / "run.py")
runner = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(runner)
REPO = runner.REPO

PREFIX = """Work only in this checkout; it is the only source of truth (no GitHub, remote or other
clone exists for this task). Make the change the task below defines and leave it on disk. Verify it
with the smallest check that observes it; do NOT run `ci/gate.ps1`, a workspace-wide build or the
full test suite. If what the task asks you to prove cannot be observed from this checkout and
machine, change nothing and end your answer with `OBSERVER_MISSING: <what is missing>`.

Task:

"""

SURFACES = {
    "skill": Path("extensions/builtin/graphhelm-development-contracts/skills/keel/REFERENCE.md"),
    "agents": Path("AGENTS.md"),
    "keelYaml": Path("extensions/builtin/graphhelm-development-contracts/policies/keel.yaml"),
    "digest": Path("tools/token-bench/doless/surfaces/keel-digest.md"),
}
PROMPT_KEYS = {"explicit": "prompt", "issue": "promptIssue"}
AGENTS_KEEL_HEADING = "### Keel: how code is written here"
BLOCKING_SURFACE = ("pathsOutsideCard", "unrequestedNewFiles", "unrequestedNewTests",
                    "undeclaredPublicSymbols", "newTestsGreenOnParent")


def load_tasks() -> dict:
    return json.loads(MANIFEST.read_text(encoding="utf-8"))["tasks"]


def split_of(task_id: str, tasks: dict) -> str:
    """Frozen before any run, stratified so every category has train cases the hillclimber can read
    and test cases it never sees: within a category, tasks ranked by sha256(id); rank 1, 4, 7... is test.
    A task with `splitPin` takes that split and leaves the ranking (it shares history with a task
    already on that side; see `lineage_overlaps`)."""
    if tasks[task_id].get("splitPin"):
        return tasks[task_id]["splitPin"]
    category = tasks[task_id]["category"]
    ranked = sorted((t for t in tasks if tasks[t]["category"] == category and not tasks[t].get("splitPin")),
                    key=lambda t: hashlib.sha256(t.encode()).hexdigest())
    return "test" if ranked.index(task_id) % 3 == 1 else "train"


def lineage(task: dict) -> set:
    """The commits a task's known answer is made of: its parent and its fix. A task with no fix (the
    observer-missing kind, whose answer is the empty diff) has no answer to leak."""
    return {task["parentSha"], task["fixSha"]} if task.get("fixSha") else set()


def lineage_overlaps(tasks: dict) -> list[tuple[str, str]]:
    """(test task, train task) pairs that share a parent or fix commit: reading the train task's
    transcripts would show the test task's answer."""
    return sorted((t, u) for t in tasks for u in tasks
                  if tasks[t]["split"] == "test" and tasks[u]["split"] == "train"
                  and lineage(tasks[t]) & lineage(tasks[u]))


def agents_keel_section(text: str) -> str:
    """The AGENTS.md Keel section alone: from its heading to the next heading of level <= 3."""
    start = text.find(AGENTS_KEEL_HEADING)
    if start < 0:
        raise ValueError("AGENTS.md has no Keel section")
    rest = text[start + len(AGENTS_KEEL_HEADING):]
    ends = [i for i in (rest.find("\n## "), rest.find("\n### ")) if i >= 0]
    return AGENTS_KEEL_HEADING + (rest[:min(ends)] if ends else rest)


def keel_surfaces(surface_dir: Path) -> tuple[str, dict]:
    skill = (surface_dir / SURFACES["skill"]).read_text(encoding="utf-8")
    agents = agents_keel_section((surface_dir / SURFACES["agents"]).read_text(encoding="utf-8"))
    keel_yaml = (surface_dir / SURFACES["keelYaml"]).read_text(encoding="utf-8")
    digests = {name: runner.digest_bytes(body.encode("utf-8"))
               for name, body in (("skill", skill), ("agentsKeel", agents), ("keelYaml", keel_yaml))}
    text = ("\n\nThis repository writes code under Keel. Its rules follow; they apply to this task.\n"
            "--- BEGIN KEEL SKILL ---\n" + skill + "\n--- END KEEL SKILL ---\n"
            "--- BEGIN AGENTS.md KEEL SECTION ---\n" + agents + "\n--- END AGENTS.md KEEL SECTION ---\n"
            "--- BEGIN keel.yaml ---\n" + keel_yaml + "\n--- END keel.yaml ---\n")
    return text, digests


def keel_digest(surface_dir: Path) -> tuple[str, dict]:
    digest = (surface_dir / SURFACES["digest"]).read_text(encoding="utf-8")
    text = "\n\n--- BEGIN KEEL DIGEST ---\n" + digest + "\n--- END KEEL DIGEST ---\n"
    return text, {"digest": runner.digest_bytes(digest.encode("utf-8"))}


def compose_prompt(task: dict, arm: str, surface_dir: Path, style: str = "explicit") -> tuple[str, dict | None]:
    body = PREFIX + (DOLESS / task[PROMPT_KEYS[style]]).read_text(encoding="utf-8")
    if arm == "a":
        return body, None
    extra, digests = keel_digest(surface_dir) if arm == "d" else keel_surfaces(surface_dir)
    return body + extra, digests


# ---------------------------------------------------------------- checkouts and observers

def git(args: list[str], cwd: Path) -> subprocess.CompletedProcess:
    return subprocess.run(["git", *args], cwd=cwd, check=True,
                          text=True, capture_output=True, encoding="utf-8", errors="replace")


def apply_known_fix(wt: Path, task: dict) -> None:
    """Leave the historical fix as uncommitted changes on the parent, exactly as an agent would.
    `fixPaths` limits it to the part of a historical commit the task asks for."""
    paths = ["--", *task["fixPaths"]] if task.get("fixPaths") else []
    patch = subprocess.run(["git", "diff", "--binary", task["parentSha"], task["fixSha"], *paths], cwd=REPO,
                           check=True, capture_output=True).stdout
    subprocess.run(["git", "apply", "--whitespace=nowarn", "-"], cwd=wt, input=patch, check=True,
                   capture_output=True)


# A red that says the build could not touch its own files, not that the code is wrong: the MSVC
# linker could not open the test .exe, or Windows refused a file another process holds. Scored
# INCOMPLETE, never FAIL (#1346). A cargo lock wait that outlives the step is a TIMEOUT, already
# INCOMPLETE.
INFRA_FAULT = re.compile(r"LNK1104|os error 32|being used by another process")


def run_command(command: list[str], cwd: Path, env: dict | None = None,
                timeout: int = 900) -> tuple[str, int | None, str]:
    command = [sys.executable if part == "{python}" else part.replace("{doless}", str(DOLESS))
               for part in command]
    proc, cleanup_unconfirmed = runner._run_captured(command, cwd=cwd, env=env,
                                                     timeout=timeout, errors="replace")
    if isinstance(proc, subprocess.TimeoutExpired):
        detail = f"timed out after {timeout}s"
        if cleanup_unconfirmed:
            detail += "; observer process exit unconfirmed after bounded cleanup"
        return "TIMEOUT", None, detail
    output = proc.stdout + "\n" + proc.stderr
    if proc.returncode == 0:
        return "PASS", 0, output[-1500:]
    return ("INFRA" if INFRA_FAULT.search(output) else "FAIL"), proc.returncode, output[-1500:]


def run_target(wt: Path) -> Path:
    """This checkout's own cargo target. Parallel runs that share one (an inherited
    CARGO_TARGET_DIR) lock each other's test .exe (#1346)."""
    return runner.scratch_root() / "doless-targets" / wt.name


def remove_checkout(wt: Path) -> None:
    """Remove the checkout and its cargo target. Incremental objects under a target exceed MAX_PATH
    on Windows, so the target is removed through the extended-length form of its path."""
    runner.remove_worktree(wt)
    target = str(run_target(wt).resolve())
    if os.name == "nt" and not target.startswith("\\\\?\\"):
        target = "\\\\?\\" + target
    shutil.rmtree(target, ignore_errors=True)


def run_regression(wt: Path, task: dict) -> tuple[str, int | None, str]:
    """Restore the regression files from the parent (the agent may not grade itself), then run."""
    spec = task.get("regression")
    if not spec:
        return "UNOBSERVED", None, "no regression observer"
    for path in spec.get("files", []):
        target = wt / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(runner.git_show_bytes(task["parentSha"], path))
    env = dict(os.environ, CARGO_TARGET_DIR=str(run_target(wt) / "regression"))
    return run_command(spec["command"], wt, env=env)


def run_oracle(wt: Path, task: dict, answer: str) -> tuple[str, int | None, str]:
    """The oracle lives outside the checkout; it gets the checkout path and the agent's answer."""
    answer_file = runner.scratch_root() / "answers" / f"{uuid.uuid4().hex}.txt"
    answer_file.parent.mkdir(parents=True, exist_ok=True)
    answer_file.write_text(answer or "", encoding="utf-8")
    env = dict(os.environ, DOLESS_ANSWER=str(answer_file), PYTHONIOENCODING="utf-8",
               CARGO_TARGET_DIR=str(run_target(wt) / "oracle"))
    try:
        return run_command([sys.executable, str(DOLESS / task["oracle"]), str(wt)], wt, env=env)
    finally:
        answer_file.unlink(missing_ok=True)


def snapshot_commit(wt: Path) -> tuple[str, str]:
    """Commit whatever the session left (staged, unstaged, untracked) so keel check reads a range."""
    base = git(["rev-list", "--max-parents=0", "HEAD"], wt).stdout.strip()
    git(["add", "-A"], wt)
    git(["-c", "user.name=doless", "-c", "user.email=doless@invalid", "-c", "commit.gpgsign=false",
         "commit", "-q", "--allow-empty", "--no-verify", "-m", "doless: session result"], wt)
    return base, git(["rev-parse", "HEAD"], wt).stdout.strip()


def keel_check(wt: Path, base: str, head: str, card: Path, prove: bool) -> dict:
    exe = os.environ.get("TOKEN_BENCH_GRAPHHELM_CLI")
    if not exe or not Path(exe).is_file():
        return {"error": "TOKEN_BENCH_GRAPHHELM_CLI does not name a graphhelm executable"}
    cmd = [exe, "--json", "keel", "check", "--diff", f"{base}..{head}", "--card", str(card), "--repo", str(wt)]
    if prove:
        cmd += ["--prove-new-tests", "--prove-target-dir", str(run_target(wt) / "prove")]
    proc = subprocess.run(cmd, cwd=wt, text=True, capture_output=True, encoding="utf-8", errors="replace")
    try:
        envelope = json.loads(proc.stdout)
    except json.JSONDecodeError:
        return {"error": f"keel check exit {proc.returncode}: {proc.stderr[-500:]}"}
    if proc.returncode not in (0, 2) or not isinstance(envelope.get("data"), dict):
        return {"error": f"keel check exit {proc.returncode}: {proc.stdout[-500:]}"}
    return {"exit": proc.returncode, "data": envelope["data"]}


def added_files(wt: Path, base: str, head: str) -> list[str]:
    out = git(["diff", "--name-only", "--diff-filter=A", "--no-renames", base, head], wt).stdout
    return sorted(line for line in out.splitlines() if line.strip())


def changed_lines(wt: Path, base: str, head: str) -> int:
    total = 0
    for line in git(["diff", "--numstat", base, head], wt).stdout.splitlines():
        added, deleted = (line.split("\t") + ["", ""])[:2]
        total += (int(added) if added.isdigit() else 0) + (int(deleted) if deleted.isdigit() else 0)
    return total


OUTSIDE_CARD_RULE = "keel.scope.path_outside_card"
REQUIRED_SURFACE_FIELDS = ("newTests", "newPublicSymbols", "undeclaredPublicSymbols")


def envelope_gap(check: dict, prove: bool) -> str | None:
    """Why a keel check envelope cannot be scored, or None. A missing field is an error, never a zero,
    and a refusal the do-less columns cannot explain is an error, never a pass."""
    exit_code, data = check.get("exit"), check.get("data")
    if exit_code not in (0, 2) or not isinstance(data, dict):
        return f"unexpected keel check exit {exit_code!r}"
    surface, findings = data.get("surface"), data.get("findings")
    if not isinstance(surface, dict):
        return "keel check output has no surface section"
    missing = [key for key in REQUIRED_SURFACE_FIELDS if key not in surface]
    if missing:
        return "keel check surface is missing " + ", ".join(missing)
    if not isinstance(findings, list):
        return "keel check output has no findings list"
    if prove and not isinstance((data.get("testProof") or {}).get("proofs"), list):
        return "keel check was asked to prove new tests and returned no testProof.proofs"
    blocking = sorted({str(f.get("rule")) for f in findings if f.get("blocking")})
    if exit_code == 0 and blocking:
        return "keel check exited 0 with blocking findings: " + ", ".join(blocking)
    if exit_code == 2:
        others = [rule for rule in blocking if rule != OUTSIDE_CARD_RULE]
        if others or not blocking:
            return "keel check refused for a rule the do-less columns do not score: " + (
                ", ".join(others) or "no blocking finding")
    return None


def surface_columns(check: dict, new_files: list[str], expected: dict, prove: bool = False) -> dict:
    """The do-less columns. `expected` is the surface the task itself asks for (files, test count,
    public symbols); everything beyond it is unrequested."""
    if check.get("error"):
        return {"keelCheck": "ERROR", "keelCheckError": check["error"]}
    gap = envelope_gap(check, prove)
    if gap:
        return {"keelCheck": "ERROR", "keelCheckError": gap}
    data = check["data"]
    surface = data["surface"]
    outside = sorted({f.get("path", "") for f in data["findings"] if f.get("rule") == OUTSIDE_CARD_RULE})
    undeclared = surface["undeclaredPublicSymbols"]
    allowed_symbols = set(expected.get("publicSymbols", []))
    unrequested_symbols = sorted(s for s in undeclared if s not in allowed_symbols)
    proofs = ((data.get("testProof") or {}).get("proofs")) or []
    allowed_files = set(expected.get("newFiles", []))
    new_tests = int(surface["newTests"])
    return {
        "keelCheck": "PASS" if check["exit"] == 0 else "REFUSED",
        "pathsOutsideCard": len(outside),
        "pathsOutsideCardList": outside,
        "unrequestedNewFiles": sorted(f for f in new_files if f not in allowed_files),
        "newTests": new_tests,
        "unrequestedNewTests": max(0, new_tests - int(expected.get("newTests", 0))),
        "newPublicSymbols": surface["newPublicSymbols"],
        "undeclaredPublicSymbols": unrequested_symbols,
        "newTestsGreenOnParent": sum(1 for p in proofs if p.get("verdict") == "green_on_parent"),
        "newTestsProved": len(proofs),
    }


def do_less_verdict(row: dict) -> tuple[str, list[str]]:
    """PASS only when the task was done, nothing broke, and nothing unrequested was added.
    A missing observer or a broken instrument is INCOMPLETE, never a pass or a fail."""
    reasons = []
    if row.get("oracle") != "PASS":
        reasons.append("oracle_" + str(row.get("oracle")).lower())
    if row.get("regression") != "PASS":
        reasons.append("regression_" + str(row.get("regression")).lower())
    cols = row.get("surface") or {}
    explained_refusal = cols.get("keelCheck") == "REFUSED" and cols.get("pathsOutsideCard", 0) > 0
    if (cols.get("keelCheck") != "PASS" and not explained_refusal) or any(k not in cols for k in BLOCKING_SURFACE):
        reasons.append("keel_check_error")
    else:
        reasons.extend(key for key in BLOCKING_SURFACE if cols.get(key))
    if row.get("agentError"):
        reasons.append("agent_error")
    incomplete = {"keel_check_error", "agent_error", "regression_unobserved", "regression_timeout",
                  "oracle_timeout", "regression_infra", "oracle_infra"}
    if incomplete & set(reasons):
        return "INCOMPLETE", reasons
    return ("PASS" if not reasons else "FAIL"), reasons


def score_checkout(wt: Path, task: dict, answer: str, prove: bool) -> dict:
    base, head = snapshot_commit(wt)
    new_files = added_files(wt, base, head)
    lines = changed_lines(wt, base, head)
    check = keel_check(wt, base, head, DOLESS / task["card"], prove)
    surface = surface_columns(check, new_files, task.get("expected", {}), prove)
    regression = run_regression(wt, task)
    oracle = run_oracle(wt, task, answer)
    return {"diffHead": head, "changedLines": lines, "newFiles": new_files, "surface": surface,
            "regression": regression[0], "regressionTail": regression[2][-600:],
            "oracle": oracle[0], "oracleTail": oracle[2][-600:]}


# ---------------------------------------------------------------- commands

def cmd_split(_: argparse.Namespace) -> None:
    tasks = load_tasks()
    for task_id, task in sorted(tasks.items()):
        computed = split_of(task_id, tasks)
        mark = "" if task["split"] == computed else f"  MISMATCH (rule says {computed})"
        print(f"{task['split']:5}  {task['category']:16}  {task_id}{mark}")


def cmd_qualify(a: argparse.Namespace) -> None:
    tasks = load_tasks()
    failures = 0
    for task_id in ([a.task] if a.task else sorted(tasks)):
        task = tasks[task_id]
        t0 = time.monotonic()
        parent = runner.make_worktree(f"dl-{task_id}", "parent", task["parentSha"])
        fixed = runner.make_worktree(f"dl-{task_id}", "fix", task["parentSha"])
        try:
            parent_reg = run_regression(parent, task)
            parent_oracle = run_oracle(parent, task, "")
            if task.get("fixSha"):
                apply_known_fix(fixed, task)
            fix = score_checkout(fixed, task, task.get("referenceAnswer", ""), a.prove)
        finally:
            remove_checkout(parent)
            remove_checkout(fixed)
        verdict, reasons = do_less_verdict(fix)
        ok = parent_reg[0] == "PASS" and parent_oracle[0] == "FAIL" and verdict == "PASS"
        failures += not ok
        print(json.dumps({"task": task_id, "qualified": ok, "parentRegression": parent_reg[0],
                          "parentOracle": parent_oracle[0], "parentOracleSays": parent_oracle[2].strip()[-200:],
                          "fixOracle": fix["oracle"], "fixRegression": fix["regression"],
                          "fixDoLess": verdict, "fixReasons": reasons, "fixSurface": fix["surface"],
                          "fixChangedLines": fix["changedLines"], "seconds": round(time.monotonic() - t0, 1)},
                         ensure_ascii=False), flush=True)
        if not ok:
            print(f"  fix oracle: {fix['oracleTail'][-400:]}\n  fix regression: {fix['regressionTail'][-400:]}"
                  f"\n  parent regression: {parent_reg[2][-400:]}", flush=True)
    raise SystemExit(1 if failures else 0)


def load_inference_config(path: Path, requested_model: str | None) -> dict:
    """Freeze an operator-declared host/model capability contract; no model-name inference."""
    config = json.loads(path.read_text(encoding="utf-8"))
    return freeze_inference_config(config, requested_model)


def freeze_inference_config(config: dict, requested_model: str | None) -> dict:
    """Validate the same capability contract for uniform and routed selections."""
    fields = {"version", "host", "provider", "model", "effort", "supportedEfforts", "capabilitySource"}
    if isinstance(config, dict) and config.get("host") == "codex":
        fields.add("pricesUsdPerMillion")
    if not isinstance(config, dict) or set(config) != fields:
        raise ValueError("inference config must contain exactly the documented fields")
    if type(config["version"]) is not int or config["version"] != 1 or config["host"] not in {"claude_code", "codex"}:
        raise ValueError("unsupported inference config version or host")
    for name in ("provider", "model", "effort", "capabilitySource"):
        if not isinstance(config[name], str) or not config[name].strip() or len(config[name]) > 2048:
            raise ValueError(f"inference config requires a bounded nonempty {name}")
    if config["model"] in {"default", "best", "sonnet", "opus", "haiku", "fable", "opusplan"} or "[" in config["model"]:
        raise ValueError("inference comparison requires an exact model ID, not an alias")
    efforts = config["supportedEfforts"]
    if (not isinstance(efforts, list) or not efforts or len(efforts) > 16
            or any(not isinstance(e, str) or not re.fullmatch(r"[a-z][a-z0-9_-]{0,31}", e) for e in efforts)
            or len(set(efforts)) != len(efforts) or config["effort"] not in efforts):
        raise ValueError("requested effort is not explicitly supported by the declared capability contract")
    if requested_model is not None and requested_model != config["model"]:
        raise ValueError("--model conflicts with the frozen inference configuration")
    if config["host"] == "codex":
        prices = config["pricesUsdPerMillion"]
        if (not isinstance(prices, dict) or set(prices) != {"input", "cachedInput", "output"}
                or any(type(p) not in (int, float) or not math.isfinite(p) or p < 0 for p in prices.values())):
            raise ValueError("Codex requires declared finite nonnegative pricesUsdPerMillion (input, cachedInput, output)")
    canonical = json.dumps(config, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return {"configuration": config, "digest": runner.digest_bytes(canonical)}


def load_routed_config(path: Path, task: dict) -> dict:
    """Freeze the whole route, recording the per-task selection outside its arm digest."""
    policy = json.loads((DOLESS / "routed.json").read_text(encoding="utf-8"))
    kind = task.get("kind")
    if not isinstance(kind, str) or kind not in policy or not isinstance(policy[kind], dict):
        raise ValueError("routed treatment requires a declared delegation kind")
    tiers = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(tiers, dict) or set(tiers) != {"small", "standard", "large"}:
        raise ValueError("routed inference config requires small, standard and large capability contracts")
    for capability in tiers.values():
        freeze_inference_config(capability, None)
    for rule in policy.values():
        if isinstance(rule, dict):
            freeze_inference_config(tiers[rule["tier"]] | {"effort": rule["effort"]}, None)
    rule = policy[kind]
    selection = {"kind": kind, "tier": rule["tier"],
                 "model": tiers[rule["tier"]]["model"], "effort": rule["effort"]}
    config = {"treatment": "routed", "policy": policy, "tiers": tiers}
    canonical = json.dumps(config, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return {"configuration": config, "digest": runner.digest_bytes(canonical), "selection": selection}


def observe_effort_flag(claude_cli: dict) -> dict:
    """Observe only local CLI flag support; provider acceptance/effective effort remain unobserved."""
    probe = subprocess.run([claude_cli["path"], "--help"], check=True, text=True, encoding="utf-8",
                           capture_output=True, timeout=10)
    if not re.search(r"(?m)^\s*--effort(?:\s|[=,])", probe.stdout):
        raise RuntimeError("pinned Claude CLI does not advertise --effort")
    return {"flag": "--effort", "source": "cli_help",
            "helpDigest": runner.digest_bytes(probe.stdout.encode("utf-8"))}


def comparison_arm(row: dict) -> str:
    """Never pool a model/effort treatment into its legacy methodology arm."""
    config = row.get("inferenceConfig")
    arm = f"{row['arm']}@{config['digest']}" if config else row["arm"]
    return f"{row['executor']}:{arm}" if row.get("executor", "claude") != "claude" else arm


def observed_cost(row: dict) -> float | None:
    cost = row.get("costUsd")
    return float(cost) if type(cost) in (int, float) and math.isfinite(cost) and cost >= 0 else None


def cmd_run(a: argparse.Namespace) -> None:
    task = load_tasks()[a.task]
    executor = getattr(a, "executor", "claude")
    config_path = getattr(a, "inference_config", None)
    if getattr(a, "treatment", None) == "routed":
        if a.model is not None or a.arm != "a" or not config_path:
            raise ValueError("routed requires --arm a and --inference-config, without --model")
        inference = load_routed_config(Path(config_path), task)
        selected = inference["selection"]
    else:
        inference = load_inference_config(Path(config_path), a.model) if config_path else None
        selected = inference["configuration"] if inference else {}
    model = selected.get("model", a.model)
    effort = selected.get("effort")
    contracts = (list(inference["configuration"]["tiers"].values()) if inference and
                 "tiers" in inference["configuration"] else [selected])
    if executor == "codex" and not inference:
        raise ValueError("Codex requires --inference-config with a model, supported effort and declared prices")
    expected_host = "codex" if executor == "codex" else "claude_code"
    if inference and any(c["host"] != expected_host for c in contracts):
        raise ValueError("inference config host conflicts with --executor")
    if executor == "codex" and a.max_budget_usd is not None:
        raise ValueError("Codex has no USD budget cap; omit --max-budget-usd")
    claude_cli = (runner.validate_prerequisites("a", executor="codex") if executor == "codex"
                  else runner.validate_prerequisites("a"))
    effort_capability = (observe_effort_flag(claude_cli) if inference and executor == "claude" else None)
    surface_dir = Path(a.surface_dir).resolve() if a.surface_dir else REPO
    prompt, surface_digests = compose_prompt(task, a.arm, surface_dir, a.prompt_style)
    task_digest = runner.digest_bytes(b"\0".join((DOLESS / task[k]).read_bytes()
                                                 for k in (PROMPT_KEYS[a.prompt_style], "oracle", "card")))
    for index in range(a.runs):
        wt = runner.make_worktree(f"dl-{a.task}", a.arm, task["parentSha"])
        result, wall, row = {"agentError": "agent_not_started"}, 0.0, {}
        try:
            if executor == "codex":
                result, _stderr, wall = runner.run_codex_agent(wt, prompt, model, effort, a.timeout_min, claude_cli)
            else:
                result, _stderr, wall = runner.run_agent(wt, prompt, "a", model, a.timeout_min, {},
                    a.max_budget_usd if a.max_budget_usd is not None else 1.0, None, claude_cli, effort=effort)
            row = score_checkout(wt, task, str(result.get("result") or ""), a.prove)
        except Exception as exc:  # the row still records what was observed
            row["evaluatorError"] = f"{type(exc).__name__}: {exc}"[:500]
        finally:
            if not a.keep:
                remove_checkout(wt)
        usage = runner.transcript_usage(result.get("session_id")) if executor == "claude" else {}
        if executor == "codex" and result.get("tokens") is not None:
            tokens = result["tokens"]
            contract = (inference["configuration"]["tiers"][selected["tier"]]
                        if "tier" in selected else selected)
            prices = contract["pricesUsdPerMillion"]
            result["total_cost_usd"] = ((tokens["input_tokens"] - tokens["cached_input_tokens"]) * prices["input"]
                + tokens["cached_input_tokens"] * prices["cachedInput"] + tokens["output_tokens"] * prices["output"]) / 1_000_000
        agent_error = result.get("agentError") or (str(result.get("result", ""))[:300] if result.get("is_error") else None)
        row.update({
            "at": dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds"),
            "dolessVersion": DOLESS_VERSION, "executor": executor, "task": a.task, "split": task["split"],
            "category": task["category"], "arm": a.arm, "promptStyle": a.prompt_style, "run": index + 1,
            "requestedModel": model, "models": usage.get("models"),
            "inferenceConfig": inference, "requestedEffort": effort, "observedEffort": None,
            "effortObservation": "unobserved", "effortCapability": effort_capability,
            "costProvenance": "cli_estimate" if observed_cost({"costUsd": result.get("total_cost_usd")}) is not None else "unavailable",
            "claudeCli": claude_cli, "surfaceDigests": surface_digests,
            "promptDigest": runner.digest_bytes(prompt.encode("utf-8")), "taskDigest": task_digest,
            "agentError": agent_error, "sessionId": result.get("session_id"),
            "costUsd": result.get("total_cost_usd"), "wallSeconds": round(wall, 1),
            "turns": result.get("num_turns"), "transcript": usage,
            "answerTail": str(result.get("result") or "")[-400:],
        })
        if executor == "codex":
            row.pop("claudeCli")
            row.update({"codexCli": claude_cli, "tokens": result.get("tokens"),
                        "costProvenance": "declared_price_table" if observed_cost(row) is not None else "unavailable"})
        if row.get("evaluatorError"):
            row["verdict"], row["verdictReasons"] = "INCOMPLETE", ["evaluator_error"]
        else:
            row["verdict"], row["verdictReasons"] = do_less_verdict(row)
        if inference and executor == "claude":
            row["usageAudit"] = runner.usage_audit(usage, result)
            row["claudeCliPostDigest"] = runner.digest_file(Path(claude_cli["path"]))
            missing = []
            if usage.get("models") != [model]:
                missing.append("configured_model_unobserved_or_mixed")
            if row["usageAudit"]["status"] != "PASS":
                missing.append("usage_incomplete")
            if observed_cost(row) is None:
                missing.append("cost_unobserved")
            if row["claudeCliPostDigest"] != claude_cli["sha256"]:
                missing.append("cli_identity_changed")
            if missing:
                row["verdict"] = "INCOMPLETE"
                row["verdictReasons"].extend(missing)
        if executor == "codex":
            row["codexCliPostDigest"] = runner.digest_file(Path(claude_cli["path"]))
            missing = []
            if result.get("tokens") is None:
                missing.append("usage_incomplete")
            if observed_cost(row) is None:
                missing.append("cost_unobserved")
            if row["codexCliPostDigest"] != claude_cli["sha256"]:
                missing.append("cli_identity_changed")
            if missing:
                row["verdict"] = "INCOMPLETE"
                row["verdictReasons"].extend(missing)
        with RESULTS.open("a", encoding="utf-8") as f:
            f.write(json.dumps(row, ensure_ascii=False) + "\n")
        print(json.dumps({k: row.get(k) for k in ("task", "arm", "run", "verdict", "verdictReasons", "oracle",
                                                  "regression", "changedLines", "costUsd", "wallSeconds")}), flush=True)


def summarise(rows: list[dict]) -> dict:
    """Per (task, arm): runs, do-less passes over scored runs, total cost. INCOMPLETE is counted, not scored."""
    out: dict = {}
    for r in rows:
        cell = out.setdefault((r["task"], comparison_arm(r)), {"runs": 0, "scored": 0, "pass": 0, "cost": 0.0,
                                                      "split": r.get("split"), "reasons": {}})
        cell["runs"] += 1
        cost = observed_cost(r)
        cell["cost"] = cell["cost"] + cost if cell["cost"] is not None and cost is not None else None
        for reason in r.get("verdictReasons", []):
            cell["reasons"][reason] = cell["reasons"].get(reason, 0) + 1
        if r["verdict"] == "INCOMPLETE":
            continue
        cell["scored"] += 1
        cell["pass"] += r["verdict"] == "PASS"
    return out


def cmd_table(a: argparse.Namespace) -> None:
    if a.pareto:
        cmd_pareto(a); return
    if not RESULTS.exists():
        print("no results yet"); return
    rows = read_rows(a)
    print("task | split | arm | runs | do-less pass/scored | USD total | reasons (count)")
    for (task, arm), c in sorted(summarise(rows).items()):
        print(f"{task} | {c['split']} | {arm} | {c['runs']} | {c['pass']}/{c['scored']} | {_usd(c['cost'])} | "
              + ", ".join(f"{k} x{v}" for k, v in sorted(c["reasons"].items())))


# ---------------------------------------------------------------- score vs cost (#136)
#
# An arm is judged by where it lands on score AND cost, never on one alone (after Replit's "Free
# the models": harness configurations compared as score vs cost-per-task Pareto frontiers).

def wilson_interval(passes: int, n: int, z: float = 1.959964) -> tuple[float, float] | None:
    """95% Wilson score interval for a pass rate; None when nothing was scored. Wilson, not the
    normal approximation: it stays inside [0, 1] and is honest at 0/n and n/n with small n."""
    if n <= 0:
        return None
    p = passes / n
    centre = (p + z * z / (2 * n)) / (1 + z * z / n)
    half = z * ((p * (1 - p) / n + z * z / (4 * n * n)) ** 0.5) / (1 + z * z / n)
    return max(0.0, centre - half), min(1.0, centre + half)


def arm_scores(rows: list[dict]) -> dict:
    """Per arm: score over scored rows (INCOMPLETE excluded), cost over EVERY row (an INCOMPLETE run
    was still paid for). A row without `costUsd` is counted in `costMissing` and leaves the arm's
    cost unplaceable rather than silently cheaper."""
    out: dict = {}
    for r in rows:
        cell = out.setdefault(comparison_arm(r), {"runs": 0, "scored": 0, "passes": 0, "incomplete": 0,
                                         "costs": [], "costMissing": 0, "tokens": {}, "tokensMissing": 0})
        if r.get("executor") == "codex":
            if r.get("tokens") is None:
                cell["tokensMissing"] += 1
            else:
                model = r.get("requestedModel", "unknown")
                totals = cell["tokens"].setdefault(model, [0, 0, 0])
                for i, key in enumerate(("input_tokens", "cached_input_tokens", "output_tokens")):
                    totals[i] += r["tokens"][key]
        cell["runs"] += 1
        cost = observed_cost(r)
        if cost is not None:
            cell["costs"].append(cost)
        else:
            cell["costMissing"] += 1
        if r.get("verdict") == "INCOMPLETE":
            cell["incomplete"] += 1
            continue
        cell["scored"] += 1
        cell["passes"] += r.get("verdict") == "PASS"
    for cell in out.values():
        costs = sorted(cell.pop("costs"))
        n = cell["scored"]
        cell["passRate"] = cell["passes"] / n if n else None
        cell["interval"] = wilson_interval(cell["passes"], n)
        placeable = bool(costs) and not cell["costMissing"]
        total = sum(costs)
        mid = len(costs) // 2
        cell["costTotal"] = total if placeable else None
        cell["costMean"] = total / len(costs) if placeable else None
        cell["costMedian"] = (costs[mid] if len(costs) % 2 else (costs[mid - 1] + costs[mid]) / 2) if placeable else None
        cell["costPerPass"] = total / cell["passes"] if placeable and cell["passes"] else None
    mark_frontier(out)
    return out


def mark_frontier(arms: dict) -> None:
    """`frontier` is True for an arm no other arm dominates on (pass rate, mean cost per run).
    B dominates A when B scores at least as high AND costs at most as much, and is strictly better
    on one of the two. Two arms with identical score and cost dominate neither each other and both
    stay on the frontier. An arm with no scored run or no placeable cost is not placed (None) and
    dominates nothing."""
    placed = {a: c for a, c in arms.items() if c["passRate"] is not None and c["costMean"] is not None}
    for arm, cell in arms.items():
        if arm not in placed:
            cell["frontier"] = None
            continue
        s, k = cell["passRate"], cell["costMean"]
        cell["frontier"] = not any(o["passRate"] >= s and o["costMean"] <= k and (o["passRate"] > s or o["costMean"] < k)
                                   for other, o in placed.items() if other != arm)


def _usd(value: float | None) -> str:
    return "-" if value is None else f"{value:.3f}"


def pareto_lines(arms: dict) -> list[str]:
    lines = ["arm | runs | scored | INCOMPLETE | pass rate [95% CI] | USD mean/run | USD median/run | "
             "USD per pass | frontier | Codex tokens by requested model (input/cached/output)"]
    for arm, c in sorted(arms.items()):
        rate = "-" if c["passRate"] is None else (
            f"{c['passes']}/{c['scored']} = {c['passRate']:.0%} [{c['interval'][0]:.0%}, {c['interval'][1]:.0%}]")
        mark = {True: "YES", False: "no", None: "unplaced"}[c["frontier"]]
        missing = f" ({c['costMissing']} without cost)" if c["costMissing"] else ""
        lines.append(f"{arm} | {c['runs']} | {c['scored']} | {c['incomplete']} | {rate} | {_usd(c['costMean'])}{missing} | "
                     f"{_usd(c['costMedian'])} | {_usd(c['costPerPass'])} | {mark} | "
                     + ", ".join(f"{model}: {'/'.join(map(str, counts))}" for model, counts in sorted(c.get("tokens", {}).items()))
                     + (f" ({c['tokensMissing']} without usage)" if c.get("tokensMissing") else ""))
    return lines


def pareto_svg(arms: dict, title: str = "do-less: score vs cost per run") -> str:
    """A small self-contained SVG: pass rate (Y, with its 95% interval) against mean USD per run
    (X). Frontier arms are filled and joined by the frontier line; dominated arms are hollow."""
    placed = sorted(((a, c) for a, c in arms.items() if c["frontier"] is not None), key=lambda x: x[1]["costMean"])
    w, h, left, right, top, bottom = 640, 400, 64, 24, 40, 48
    xmax = max([c["costMean"] for _, c in placed] + [0.0]) * 1.15 or 1.0
    def x(v): return left + (w - left - right) * v / xmax
    def y(v): return top + (h - top - bottom) * (1 - v)
    esc = lambda t: str(t).replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}" '
           'font-family="sans-serif" font-size="12">',
           '<style>.axis{stroke:#888}.grid{stroke:#ddd}.front{stroke:#1f6feb;fill:none;stroke-width:2}'
           '.ci{stroke:#555}.on{fill:#1f6feb}.off{fill:#fff;stroke:#555;stroke-width:1.5}text{fill:#222}</style>',
           f'<text x="{w / 2}" y="20" text-anchor="middle" font-size="14">{esc(title)}</text>']
    for i in range(5):
        v = i / 4
        out.append(f'<line class="grid" x1="{left}" x2="{w - right}" y1="{y(v):.1f}" y2="{y(v):.1f}"/>'
                   f'<text x="{left - 6}" y="{y(v) + 4:.1f}" text-anchor="end">{v:.0%}</text>')
        out.append(f'<text x="{x(xmax * v):.1f}" y="{h - bottom + 18}" text-anchor="middle">{xmax * v:.3f}</text>')
    out.append(f'<line class="axis" x1="{left}" x2="{left}" y1="{top}" y2="{h - bottom}"/>'
               f'<line class="axis" x1="{left}" x2="{w - right}" y1="{h - bottom}" y2="{h - bottom}"/>'
               f'<text x="{w / 2}" y="{h - 8}" text-anchor="middle">mean USD per run (INCOMPLETE runs included)</text>'
               f'<text x="16" y="{h / 2}" transform="rotate(-90 16 {h / 2})" text-anchor="middle">'
               'do-less pass rate (INCOMPLETE excluded)</text>')
    front = [(x(c["costMean"]), y(c["passRate"])) for _, c in placed if c["frontier"]]
    if len(front) > 1:
        out.append('<polyline class="front" points="' + " ".join(f"{a:.1f},{b:.1f}" for a, b in front) + '"/>')
    for arm, c in placed:
        cx, cy = x(c["costMean"]), y(c["passRate"])
        lo, hi = c["interval"]
        out.append(f'<line class="ci" x1="{cx:.1f}" x2="{cx:.1f}" y1="{y(lo):.1f}" y2="{y(hi):.1f}"/>'
                   f'<circle class="{"on" if c["frontier"] else "off"}" cx="{cx:.1f}" cy="{cy:.1f}" r="5">'
                   f'<title>{esc(arm)}: {c["passes"]}/{c["scored"]}, USD {c["costMean"]:.3f}/run</title></circle>'
                   f'<text x="{cx + 8:.1f}" y="{cy - 6:.1f}">{esc(arm)}</text>')
    out.append("</svg>")
    return "\n".join(out) + "\n"


def read_rows(a: argparse.Namespace) -> list[dict]:
    rows = [json.loads(l) for l in RESULTS.read_text(encoding="utf-8").splitlines() if l.strip()]
    return [r for r in rows if (not a.split or r.get("split") == a.split) and (not a.arm or r["arm"] == a.arm)
            and (not a.prompt_style or r.get("promptStyle", "explicit") == a.prompt_style)]


def cmd_pareto(a: argparse.Namespace) -> None:
    if not RESULTS.exists():
        print("no results yet"); return
    arms = arm_scores(read_rows(a))
    print("\n".join(pareto_lines(arms)))
    if a.svg:
        svg = pareto_svg(arms)
        target = Path(a.svg)
        if target.suffix.lower() in (".html", ".htm"):
            svg = "<!doctype html><meta charset=\"utf-8\"><title>do-less Pareto</title>\n" + svg
        target.write_text(svg, encoding="utf-8")
        print(f"chart: {target}")


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    q = sub.add_parser("qualify"); q.add_argument("--task"); q.add_argument("--prove", action="store_true")
    q.set_defaults(fn=cmd_qualify)
    r = sub.add_parser("run")
    r.add_argument("--task", required=True)
    r.add_argument("--arm", required=True, choices=["a", "k", "d"])
    r.add_argument("--prompt-style", choices=sorted(PROMPT_KEYS), default="explicit")
    r.add_argument("--runs", type=int, default=3)
    r.add_argument("--model")
    r.add_argument("--executor", choices=["claude", "codex"], default="claude")
    r.add_argument("--treatment", choices=["routed"], help="route declared task kinds on arm a; no reuse")
    r.add_argument("--inference-config", help="frozen exact model/effort capability JSON; no automatic downgrade")
    r.add_argument("--timeout-min", type=int, default=15)
    r.add_argument("--max-budget-usd", type=float, help="Claude only (default 1.0); Codex has no USD cap")
    r.add_argument("--surface-dir", help="directory holding the candidate Keel surfaces (a hillclimb copy)")
    r.add_argument("--prove", action="store_true", help="also run keel check --prove-new-tests")
    r.add_argument("--keep", action="store_true")
    r.set_defaults(fn=cmd_run)
    t = sub.add_parser("table"); t.add_argument("--split", choices=["train", "test"]); t.add_argument("--arm")
    t.add_argument("--prompt-style", choices=sorted(PROMPT_KEYS))
    t.add_argument("--pareto", action="store_true", help="print the score-vs-cost report instead")
    t.add_argument("--svg", help="with --pareto: also write the chart (.svg, or .html to wrap it)")
    t.set_defaults(fn=cmd_table)
    f = sub.add_parser("pareto"); f.add_argument("--split", choices=["train", "test"]); f.add_argument("--arm")
    f.add_argument("--prompt-style", choices=sorted(PROMPT_KEYS))
    f.add_argument("--svg", help="also write the chart (.svg, or .html to wrap it)")
    f.set_defaults(fn=cmd_pareto)
    s = sub.add_parser("split"); s.set_defaults(fn=cmd_split)
    a = p.parse_args(); a.fn(a)


if __name__ == "__main__":
    main()
