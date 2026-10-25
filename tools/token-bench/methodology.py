"""Freeze and measure adapted coding protocols with independent acceptance oracles."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import random
import signal
import subprocess
import sys
import tempfile
import time
import uuid

HOME = Path(__file__).resolve().parent
NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)
API_ENV = {"ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL",
           "OPENAI_API_KEY", "OPENAI_BASE_URL", "OPENROUTER_API_KEY"}


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def read(path: Path) -> dict:
    if path.is_symlink() or path.stat().st_size > 2_000_000:
        raise ValueError(f"Unsafe or oversized input: {path.name}")
    return json.loads(path.read_text(encoding="utf-8"))


def publish(path: Path, value: dict) -> None:
    """Publish once; a duplicate attempt cannot overwrite its earlier outcome."""
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + "." + uuid.uuid4().hex + ".tmp")
    try:
        with temporary.open("x", encoding="utf-8") as handle:
            json.dump(value, handle, indent=2, ensure_ascii=False)
            handle.flush()
            os.fsync(handle.fileno())
        os.link(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def capture(argv: list[str], cwd: Path, *, deadline: int, prompt: str = "") -> dict:
    """Own each child; preserve its timeout rather than retrying or using another provider."""
    environment = {key: value for key, value in os.environ.items()
                   if key.upper() not in API_ENV}
    started = time.monotonic()
    with tempfile.TemporaryFile() as output, tempfile.TemporaryFile() as errors:
        child = subprocess.Popen(argv, cwd=cwd, env=environment, stdin=subprocess.PIPE,
                                 stdout=output, stderr=errors, creationflags=NO_WINDOW,
                                 start_new_session=os.name != "nt")
        timed_out = False
        cleanup = True
        try:
            child.communicate(prompt.encode(), timeout=deadline)
        except subprocess.TimeoutExpired:
            timed_out = True
            if os.name == "nt":
                killed = subprocess.run(["taskkill", "/PID", str(child.pid), "/T", "/F"],
                                        capture_output=True, timeout=15, creationflags=NO_WINDOW)
                cleanup = killed.returncode == 0
            else:
                os.killpg(child.pid, signal.SIGKILL)
            try:
                child.wait(timeout=15)
            except subprocess.TimeoutExpired:
                cleanup = False
        output.seek(0)
        errors.seek(0)
        stdout = output.read(8_000_001)
        stderr = errors.read(8_000_001)
    return {"exit": child.returncode, "seconds": time.monotonic() - started,
            "timeout": timed_out, "cleanupConfirmed": cleanup,
            "truncated": len(stdout) > 8_000_000 or len(stderr) > 8_000_000,
            "stdout": stdout[:8_000_000].decode("utf-8", errors="replace"),
            "stderr": stderr[:8_000_000].decode("utf-8", errors="replace")}


def evaluate(case: dict, source: str, root: Path) -> dict:
    """Only the authored source enters evaluation; original tests cannot be weakened."""
    root.mkdir()
    (root / "subject.py").write_text(source, encoding="utf-8")
    results = {}
    for kind in ("public", "hidden"):
        (root / "check.py").write_text(case[kind], encoding="utf-8")
        results[kind] = capture([sys.executable, "-I", "-c",
                                "import sys,runpy;sys.path.insert(0,'.');runpy.run_path('check.py')"],
                               root, deadline=20)
    return results


def qualify(cases: dict, root: Path) -> dict:
    results = []
    for case in cases["cases"]:
        if not case["id"].replace("-", "").isalnum():
            raise ValueError("Case IDs must be safe directory names")
        parent = evaluate(case, case["source"], root / (case["id"] + "-parent"))
        fixed = evaluate(case, case["fix"], root / (case["id"] + "-fixed"))
        expected_failure = parent["hidden"]["exit"] == 1 and "AssertionError" in parent["hidden"]["stderr"]
        passed = (parent["public"]["exit"] == 0 and expected_failure
                  and all(result["exit"] == 0 for result in fixed.values())
                  and not any(result["timeout"] for result in [*parent.values(), *fixed.values()]))
        results.append({"case": case["id"], "passed": passed, "parent": parent, "fixed": fixed})
    return {"passed": all(row["passed"] for row in results), "cases": results}


def identity(executable: Path, cli: str, root: Path) -> dict:
    if executable.is_symlink() or not executable.is_file() or executable.suffix.lower() in {".cmd", ".bat", ".ps1"}:
        raise ValueError("Pin the native CLI executable, not a shell shim")
    version = capture([str(executable), "--version"], root, deadline=30)
    auth = capture([str(executable), *( ["login", "status"] if cli == "codex" else ["auth", "status"])], root, deadline=30)
    if cli == "codex":
        subscription = "Logged in using ChatGPT" in auth["stdout"] + auth["stderr"]
    else:
        try:
            data = json.loads(auth["stdout"])
            subscription = data.get("loggedIn") is True and data.get("authMethod") == "claude.ai" and data.get("apiProvider") == "firstParty"
        except json.JSONDecodeError:
            subscription = False
    if version["exit"] != 0 or auth["exit"] != 0 or not subscription:
        raise ValueError(f"{cli}: subscription authentication or version could not be verified; no fallback")
    return {"path": str(executable.resolve()), "sha256": digest(executable),
            "version": version["stdout"].strip(), "subscription": True}


def freeze(args: argparse.Namespace) -> dict:
    cases, profiles = read(args.cases), read(args.profiles)
    if len(cases["cases"]) != 6 or len(profiles["profiles"]) != 7:
        raise ValueError("This pilot requires exactly six cases and seven profiles")
    for collection in (cases["cases"], profiles["profiles"]):
        identifiers = [row["id"] for row in collection]
        if len(set(identifiers)) != len(identifiers) or any(not identifier.replace("-", "").isalnum() for identifier in identifiers):
            raise ValueError("Case and profile IDs must be unique safe names")
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        calibration = qualify(cases, root)
        if not calibration["passed"]:
            raise ValueError("Calibration failed; run qualify and inspect its receipt")
        identities = {cli: identity(getattr(args, cli), cli, root) for cli in ("codex", "claude")}
    order = []
    tasks = list(range(6))
    random.Random(2051).shuffle(tasks)
    for index in tasks:
        for offset in range(7):
            profile = profiles["profiles"][(offset + index) % 7]["id"]
            for cli in (["codex", "claude"] if index % 2 == 0 else ["claude", "codex"]):
                order.append({"id": f"{len(order):03d}", "case": cases["cases"][index]["id"], "profile": profile, "cli": cli})
    return {"schema": "methodology-pilot/1", "studyType": "protocol_profile_pilot",
            "cases": {"path": str(args.cases.resolve()), "sha256": digest(args.cases)},
            "profiles": {"path": str(args.profiles.resolve()), "sha256": digest(args.profiles)},
            "instrumentSha256": digest(Path(__file__)), "identities": identities,
            "models": {"codex": args.codex_model, "claude": args.claude_model},
            "effort": "low", "deadlineSeconds": 180, "concurrency": 1,
            "nativeFrameworkFidelity": "UNOBSERVED", "plannedAttempts": order,
            "calibrationPassed": True, "billedUsd": None}


def load_frozen(path: Path) -> tuple[dict, dict, dict]:
    manifest = read(path)
    if manifest["instrumentSha256"] != digest(Path(__file__)):
        raise ValueError("Instrument changed after freeze")
    for name in ("cases", "profiles"):
        if digest(Path(manifest[name]["path"])) != manifest[name]["sha256"]:
            raise ValueError(f"{name} changed after freeze")
    return manifest, read(Path(manifest["cases"]["path"])), read(Path(manifest["profiles"]["path"]))


def usage(cli: str, raw: str) -> dict:
    records = []
    parse_errors = 0
    for line in raw.splitlines():
        try:
            value = json.loads(line)
            if isinstance(value, dict): records.append(value)
        except json.JSONDecodeError:
            parse_errors += 1
    if cli == "codex":
        turns = [row["usage"] for row in records if row.get("type") == "turn.completed" and isinstance(row.get("usage"), dict)]
        counts = {key: sum(row[key] for row in turns) for key in ("input_tokens", "cached_input_tokens", "output_tokens")} if turns and all(all(key in row for key in ("input_tokens", "cached_input_tokens", "output_tokens")) for row in turns) else None
        models = sorted({row["model"] for row in records if isinstance(row.get("model"), str)})
    else:
        from run import sum_transcript_usage
        parsed = sum_transcript_usage(raw.splitlines())
        counts = parsed if any(row.get("type") == "assistant" and row.get("message", {}).get("usage") for row in records) else None
        models = sorted({row["message"]["model"] for row in records if row.get("type") == "assistant" and isinstance(row.get("message", {}).get("model"), str)})
    return {"counts": counts, "observedModels": models, "parseErrors": parse_errors}


def run_attempt(args: argparse.Namespace) -> dict:
    manifest, cases, profiles = load_frozen(args.manifest)
    selected = [row for row in manifest["plannedAttempts"] if row["id"] == args.attempt]
    if len(selected) != 1:
        raise ValueError("Select one planned attempt ID")
    cell = selected[0]
    position = manifest["plannedAttempts"].index(cell)
    for earlier in manifest["plannedAttempts"][:position]:
        if not (args.output / (earlier["id"] + ".json")).is_file():
            raise ValueError("Execute the frozen order; an earlier cell is missing")
    receipt = args.output / (cell["id"] + ".json")
    args.output.mkdir(parents=True, exist_ok=True)
    if (args.output / "provider-stop.json").exists():
        raise ValueError("Pilot stopped after a provider or cleanup failure; inspect provider-stop.json")
    # Reserve before any external effect. A crashed reservation is incomplete, never retried silently.
    reservation = args.output / (cell["id"] + ".reserved")
    with reservation.open("x") as handle:
        handle.write(digest(args.manifest))
    case = next(row for row in cases["cases"] if row["id"] == cell["case"])
    profile = next(row for row in profiles["profiles"] if row["id"] == cell["profile"])
    cli = cell["cli"]
    executable = Path(manifest["identities"][cli]["path"])
    result = {**cell, "manifestSha256": digest(args.manifest), "status": "incomplete", "billedUsd": None}
    subject = args.output / ("subject-" + uuid.uuid4().hex)
    subject.mkdir()
    try:
        if identity(executable, cli, subject) != manifest["identities"][cli]:
            raise ValueError("CLI identity changed after freeze")
        (subject / "subject.py").write_text(case["source"], encoding="utf-8")
        (subject / "test_public.py").write_text(case["public"], encoding="utf-8")
        prompt = ("Work only in this directory. Change subject.py to satisfy the request. Do not inspect other directories, use network, delegate, install packages, change settings or credentials. "
                  "Use existing Python standard library. Tests: python test_public.py.\nRequest: " + case["request"] + "\nProcess profile (adapted): " + profile["prompt"])
        if cli == "codex":
            command = [str(executable), "exec", "--json", "--ignore-user-config", "--ephemeral", "--skip-git-repo-check", "--sandbox", "workspace-write", "-m", manifest["models"][cli], "-c", 'model_reasoning_effort="low"', "-c", "project_doc_max_bytes=0", "--disable", "plugins", "--enable", "skip_host_skill_discovery", "-"]
        else:
            command = [str(executable), "-p", "--output-format", "stream-json", "--verbose", "--safe-mode", "--restricted", "--tools", "Read,Edit,Write,Bash,Glob,Grep", "--permission-mode", "acceptEdits", "--permission-prompts", "none", "--no-session-persistence", "--model", manifest["models"][cli], "--effort", "low"]
        agent = capture(command, subject, deadline=manifest["deadlineSeconds"], prompt=prompt)
        result["agent"] = agent
        result["usage"] = usage(cli, agent["stdout"])
        result["measurementStatus"] = "observed" if result["usage"]["counts"] is not None else "usage-unobserved"
        result["modelStatus"] = "matched" if result["usage"]["observedModels"] == [manifest["models"][cli]] else "mismatch-or-unobserved"
        text = (agent["stdout"] + agent["stderr"]).lower()
        if not agent["cleanupConfirmed"] or (agent["exit"] != 0 and any(term in text for term in ("rate limit", "usage limit", "quota", "unauthorized", "authentication", "login"))):
            publish(args.output / "provider-stop.json", {"attempt": cell["id"], "reason": "provider-or-cleanup-failure"})
        source = subject / "subject.py"
        if source.is_symlink() or not source.is_file() or source.stat().st_size > 200_000:
            raise ValueError("Agent source missing, linked or oversized")
        evaluated = evaluate(case, source.read_text(encoding="utf-8"), args.output / (cell["id"] + "-evaluation"))
        result["evaluation"] = evaluated
        passed = agent["exit"] == 0 and not agent["timeout"] and agent["cleanupConfirmed"] and not agent["truncated"] and all(row["exit"] == 0 for row in evaluated.values())
        result["status"] = "passed" if passed else "failed"
        result["sourceSha256"] = digest(source)
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        result["error"] = str(error)
        if not (args.output / "provider-stop.json").exists():
            publish(args.output / "provider-stop.json", {"attempt": cell["id"], "reason": "unresolved-instrument-error"})
    publish(receipt, result)
    return result


def report(args: argparse.Namespace) -> dict:
    manifest, _, _ = load_frozen(args.manifest)
    manifest_hash = digest(args.manifest)
    rows = []
    for cell in manifest["plannedAttempts"]:
        path = args.output / (cell["id"] + ".json")
        if not path.exists():
            reserved = args.output / (cell["id"] + ".reserved")
            rows.append({**cell, "status": "incomplete" if reserved.exists() else "not-run"})
            continue
        row = read(path)
        if row.get("manifestSha256") != manifest_hash or any(row.get(key) != cell[key] for key in cell):
            raise ValueError("Receipt does not match frozen cell")
        rows.append(row)
    groups = []
    for cli in ("codex", "claude"):
        for profile in read(Path(manifest["profiles"]["path"]))["profiles"]:
            selected = [row for row in rows if row["cli"] == cli and row["profile"] == profile["id"]]
            groups.append({"cli": cli, "requestedModel": manifest["models"][cli], "profile": profile["id"],
                           "planned": len(selected), "passed": sum(row["status"] == "passed" for row in selected),
                           "failed": sum(row["status"] == "failed" for row in selected),
                           "missing": sum(row["status"] in {"not-run", "incomplete"} for row in selected),
                           "observedModelSets": sorted({tuple(row.get("usage", {}).get("observedModels", [])) for row in selected}),
                           "usageMissing": sum(row.get("usage", {}).get("counts") is None for row in selected),
                           "agentSeconds": sum(row.get("agent", {}).get("seconds", 0) for row in selected)})
    return {"studyType": manifest["studyType"], "complete": all(row["status"] in {"passed", "failed"} for row in rows),
            "groups": groups, "attempts": rows, "winner": None, "billedUsd": None,
            "limitations": ["Descriptive small-task pilot; no general superiority inference", "Adapted prompts; native framework fidelity UNOBSERVED", "OS-enforced oracle secrecy and method adherence UNOBSERVED", "Requested and observed model identity may differ; do not pool CLIs"]}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["qualify", "freeze", "run", "report"])
    parser.add_argument("--cases", type=Path, default=HOME / "methodology_cases.json")
    parser.add_argument("--profiles", type=Path, default=HOME / "methodology_profiles.json")
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--attempt")
    parser.add_argument("--codex", type=Path)
    parser.add_argument("--claude", type=Path)
    parser.add_argument("--codex-model")
    parser.add_argument("--claude-model")
    args = parser.parse_args()
    try:
        if args.command == "qualify":
            with tempfile.TemporaryDirectory() as directory:
                result = qualify(read(args.cases), Path(directory))
            publish(args.output, result)
            return 0 if result["passed"] else 1
        if args.command == "freeze":
            if not all((args.codex, args.claude, args.codex_model, args.claude_model)):
                raise ValueError("Freeze requires both native CLI paths and explicit model IDs")
            publish(args.output, freeze(args))
        elif args.command == "run":
            result = run_attempt(args)
            return 0 if result["status"] == "passed" else 1
        else:
            print(json.dumps(report(args), indent=2))
        return 0
    except (ValueError, OSError, KeyError, subprocess.SubprocessError) as error:
        print(f"BENCHMARK_REFUSED: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
