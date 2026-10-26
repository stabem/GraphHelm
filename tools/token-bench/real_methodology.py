"""Offline benchmark runner for frozen real-project task snapshots."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import random
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
import uuid

HOME = Path(__file__).resolve().parent
MAX_JSON = 2_000_000
MAX_FILE = 200_000
DEADLINE = 300
EVALUATOR_DEADLINE = 90
API_ENV_SUFFIXES = ("_API_KEY", "_API_TOKEN", "_BASE_URL", "_ENDPOINT")
API_ENV_NAMES = {"CODEX_API_KEY", "ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN",
                 "OPENAI_API_KEY", "OPENROUTER_API_KEY", "ANTHROPIC_BASE_URL",
                 "OPENAI_BASE_URL", "OPENROUTER_BASE_URL"}
NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def tree_digest(root: Path, paths: list[str]) -> dict[str, str]:
    return {path: digest(safe_file(root, path)) for path in sorted(paths)}


def read(path: Path) -> dict:
    if path.is_symlink() or not path.is_file() or path.stat().st_size > MAX_JSON:
        raise ValueError(f"unsafe or oversized JSON: {path}")
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"invalid JSON: {path}") from error
    if not isinstance(value, dict):
        raise ValueError(f"JSON object required: {path}")
    return value


def publish(path: Path, value: dict) -> None:
    """Append an outcome once; a duplicate ID can never replace its first result."""
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


def safe_relative(value: str) -> str:
    path = PurePosixPath(value.replace("\\", "/"))
    if not value or path.is_absolute() or ".." in path.parts or ".git" in path.parts:
        raise ValueError(f"unsafe relative path: {value!r}")
    return str(path)


def safe_file(root: Path, relative: str) -> Path:
    safe = safe_relative(relative)
    path = (root / Path(*PurePosixPath(safe).parts)).resolve()
    if root.resolve() not in path.parents:
        raise ValueError(f"path escapes snapshot: {relative}")
    if path.is_symlink() or not path.is_file() or path.stat().st_size > MAX_FILE:
        raise ValueError(f"unsafe or oversized snapshot file: {relative}")
    return path


def child_env() -> dict[str, str]:
    return {key: value for key, value in os.environ.items()
            if key not in API_ENV_NAMES and not key.upper().endswith(API_ENV_SUFFIXES)}


def capture(argv: list[str], cwd: Path, *, deadline: int, prompt: str = "") -> dict:
    started = time.monotonic()
    with tempfile.TemporaryFile() as output, tempfile.TemporaryFile() as errors:
        child = subprocess.Popen(argv, cwd=cwd, env=child_env(), stdin=subprocess.PIPE,
                                 stdout=output, stderr=errors, creationflags=NO_WINDOW,
                                 start_new_session=os.name != "nt")
        timed_out = False
        cleanup = True
        try:
            child.communicate(prompt.encode("utf-8"), timeout=deadline)
        except subprocess.TimeoutExpired:
            timed_out = True
            if os.name == "nt":
                killed = subprocess.run(["taskkill", "/PID", str(child.pid), "/T", "/F"],
                                        capture_output=True, timeout=15, creationflags=NO_WINDOW)
                cleanup = killed.returncode == 0
            else:
                child.kill()
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


def snapshot_root(task: dict, kind: str) -> Path:
    key = "parentSnapshot" if kind == "parent" else "fixSnapshot"
    value = task.get(key) or task.get("snapshots", {}).get(kind)
    if not isinstance(value, str):
        raise ValueError(f"task {task.get('id')} has no {kind} snapshot")
    root = Path(value).resolve()
    if root.is_symlink() or not root.is_dir():
        raise ValueError(f"missing or linked {kind} snapshot: {root}")
    return root


def task_paths(task: dict) -> tuple[list[str], list[str]]:
    scope = [safe_relative(path) for path in task.get("scope", [])]
    closure = [safe_relative(path) for path in task.get("productionImportClosure", scope)]
    if not scope or not set(scope).issubset(closure):
        raise ValueError(f"task {task.get('id')} has invalid production scope")
    public = [safe_relative(path) for path in task.get("publicTests", [])]
    if len(public) != 1:
        raise ValueError("each task needs exactly one public test file")
    return scope, sorted(set(closure + public))


def copy_paths(source: Path, target: Path, paths: list[str]) -> None:
    for relative in paths:
        source_file = safe_file(source, relative)
        destination = target / Path(*PurePosixPath(relative).parts)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source_file, destination)


def command_for(cli: str, executable: Path, model: str) -> list[str]:
    if cli == "codex":
        return [str(executable), "exec", "--json", "--ignore-user-config", "--ephemeral",
                "--skip-git-repo-check", "-c", 'default_permissions=":workspace"',
                "-c", 'windows.sandbox="elevated"', "-c", "features.prefer_mxc=true",
                "-m", model, "-c", 'model_reasoning_effort="low"', "-c",
                "project_doc_max_bytes=0", "--disable", "plugins", "--enable",
                "skip_host_skill_discovery", "-"]
    if cli == "claude":
        return [str(executable), "-p", "--output-format", "stream-json", "--verbose",
                "--safe-mode", "--restricted", "--tools", "Read,Edit,Write,Bash,Glob,Grep",
                "--allowedTools", "Bash(python test_public.py)", "--permission-mode",
                "acceptEdits", "--permission-prompts", "none", "--no-session-persistence",
                "--model", model, "--effort", "low"]
    raise ValueError(f"unsupported CLI: {cli}")


def verify_identity(executable: Path, cli: str, root: Path) -> dict:
    """Require the already-installed first-party subscription before a cohort starts."""
    version = capture([str(executable), "--version"], root, deadline=30)
    status_args = ["login", "status"] if cli == "codex" else ["auth", "status"]
    auth = capture([str(executable), *status_args], root, deadline=30)
    text = auth["stdout"] + auth["stderr"]
    if cli == "codex":
        subscribed = "Logged in using ChatGPT" in text
    else:
        try:
            data = json.loads(auth["stdout"])
            subscribed = (data.get("loggedIn") is True and data.get("authMethod") == "claude.ai"
                          and data.get("apiProvider") == "firstParty")
        except json.JSONDecodeError:
            subscribed = False
    if version["exit"] != 0 or auth["exit"] != 0 or not subscribed:
        raise ValueError(f"{cli}: first-party subscription authentication could not be verified")
    return {"path": str(executable.resolve()), "sha256": digest(executable),
            "version": version["stdout"].strip(), "subscription": True}


def public_command() -> list[str]:
    return [sys.executable, "test_public.py"]


def run_public(root: Path, deadline: int = EVALUATOR_DEADLINE) -> dict:
    return capture(public_command(), root, deadline=deadline)


def hidden_command(task: dict, root: Path) -> list[str]:
    hidden = task.get("hiddenOracle")
    if not isinstance(hidden, dict) or not isinstance(hidden.get("command"), (list, str)):
        raise ValueError(f"task {task.get('id')} requires a private argv hidden oracle")
    command = ([str(item) for item in hidden["command"]] if isinstance(hidden["command"], list)
               else shlex.split(hidden["command"], posix=os.name != "nt"))
    if not command:
        raise ValueError("hidden oracle command cannot be empty")
    if any(".." in PurePosixPath(item.replace("\\", "/")).parts for item in command if not item.startswith("-")):
        raise ValueError("hidden oracle command escapes its evaluator")
    return command


def evaluate(task: dict, candidate: Path, output: Path) -> dict:
    scope, closure_and_public = task_paths(task)
    parent = snapshot_root(task, "parent")
    fixed = snapshot_root(task, "fix")
    result = {}
    for label, source in (("public", parent), ("hidden", fixed)):
        root = output / label
        root.mkdir(parents=True)
        # Baseline is the frozen parent closure. Only declared production scope comes from agent.
        copy_paths(parent, root, closure_and_public)
        for relative in scope:
            source_file = safe_file(candidate, relative)
            destination = root / Path(*PurePosixPath(relative).parts)
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source_file, destination)
        if label == "public":
            command = public_command()
        else:
            hidden = task["hiddenOracle"]
            oracle_root = Path(hidden["root"]).resolve()
            if oracle_root.is_symlink() or not oracle_root.is_dir():
                raise ValueError("hidden oracle root missing or linked")
            copy_paths(oracle_root, root, [safe_relative(hidden["file"])])
            command = hidden_command(task, root)
        observed = capture(command, root, deadline=EVALUATOR_DEADLINE)
        result[label] = {"exit": observed["exit"], "seconds": observed["seconds"],
                         "timeout": observed["timeout"], "cleanupConfirmed": observed["cleanupConfirmed"],
                         "accepted": observed["exit"] == 0 and not observed["timeout"]
                         and observed["cleanupConfirmed"], "stderr": observed["stderr"]}
    return result


def enforce_candidate_scope(candidate: Path, task: dict, before: dict[str, str]) -> None:
    scope, _ = task_paths(task)
    allowed = set(scope) | {safe_relative(task["publicTests"][0]), "test_public.py"}
    observed = {}
    for path in candidate.rglob("*"):
        if not path.is_file() or path.is_symlink():
            continue
        relative = path.relative_to(candidate).as_posix()
        if relative not in allowed:
            raise ValueError(f"agent changed file outside declared scope: {relative}")
        if path.stat().st_size > MAX_FILE:
            raise ValueError(f"agent file is oversized: {relative}")
        observed[relative] = digest(path)
    for relative, original in before.items():
        if observed.get(relative) != original:
            continue
    if any(relative not in observed for relative in scope):
        raise ValueError("agent removed a declared production file")


def validate_task(task: dict) -> None:
    identifier = task.get("id")
    if not isinstance(identifier, str) or not identifier.replace("-", "").isalnum():
        raise ValueError("task IDs must be safe")
    scope, paths = task_paths(task)
    for kind in ("parent", "fix"):
        root = snapshot_root(task, kind)
        for relative in paths:
            safe_file(root, relative)
    for key in ("parentSha", "knownFixSha"):
        if not isinstance(task.get(key), str) or len(task[key]) != 40:
            raise ValueError(f"task {identifier} requires {key}")
    if not isinstance(task.get("request"), str) or not task["request"].strip():
        raise ValueError(f"task {identifier} requires a request")
    hidden = task.get("hiddenOracle")
    if not isinstance(hidden, dict) or not isinstance(hidden.get("root"), str):
        raise ValueError(f"task {identifier} requires private hidden oracle root")
    oracle_root = Path(hidden["root"]).resolve()
    safe_file(oracle_root, safe_relative(hidden["file"]))


def qualify(registry: dict, output: Path) -> dict:
    tasks = registry.get("tasks")
    if not isinstance(tasks, list) or len(tasks) != 3:
        raise ValueError("qualification requires exactly three real-project tasks")
    rows = []
    for task in tasks:
        validate_task(task)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            parent = root / "parent"
            fixed = root / "fixed"
            scope, paths = task_paths(task)
            copy_paths(snapshot_root(task, "parent"), parent, paths)
            copy_paths(snapshot_root(task, "fix"), fixed, paths)
            hidden = task["hiddenOracle"]
            oracle_root = Path(hidden["root"]).resolve()
            hidden_file = safe_relative(hidden["file"])
            copy_paths(oracle_root, parent, [hidden_file])
            copy_paths(oracle_root, fixed, [hidden_file])
            parent_result = {"public": run_public(parent),
                             "hidden": capture(hidden_command(task, parent), parent, deadline=EVALUATOR_DEADLINE)}
            fixed_result = {"public": run_public(fixed),
                            "hidden": capture(hidden_command(task, fixed), fixed, deadline=EVALUATOR_DEADLINE)}
        expected_red = parent_result["hidden"]["exit"] != 0
        passed = (parent_result["public"]["exit"] == 0 and expected_red
                  and fixed_result["public"]["exit"] == 0 and fixed_result["hidden"]["exit"] == 0)
        rows.append({"task": task["id"], "passed": passed,
                     "parent": {key: value["exit"] for key, value in parent_result.items()},
                     "fixed": {key: value["exit"] for key, value in fixed_result.items()}})
    result = {"schema": "real-methodology-qualification/1", "passed": all(row["passed"] for row in rows), "tasks": rows}
    publish(output, result)
    return result


def load_registry(path: Path) -> dict:
    registry = read(path)
    if registry.get("schema") not in {"real-methodology-registry/1", "real-project-benchmark/1"}:
        raise ValueError("unsupported registry schema")
    return registry


def freeze(args: argparse.Namespace) -> dict:
    registry = load_registry(args.registry)
    tasks = registry.get("tasks", [])
    profiles = registry.get("profiles", [])
    if len(tasks) != 3 or len(profiles) != 7:
        raise ValueError("freeze requires three tasks and seven profiles")
    for task in tasks:
        validate_task(task)
    with tempfile.TemporaryDirectory() as directory:
        calibration = qualify(registry, Path(directory) / "qualification.json")
    if not calibration["passed"]:
        raise ValueError("qualification failed; parent/fix evidence is not frozen")
    ids = [task["id"] for task in tasks]
    if len(set(ids)) != len(ids) or len({profile.get("id") for profile in profiles}) != 7:
        raise ValueError("task and profile IDs must be unique")
    order = []
    for task_index in random.Random(2051).sample(range(3), 3):
        for profile_index in range(7):
            profile = profiles[(profile_index + task_index) % 7]
            for cli in (("codex", "claude") if task_index % 2 == 0 else ("claude", "codex")):
                order.append({"id": f"{len(order):03d}", "task": ids[task_index],
                              "profile": profile["id"], "cli": cli})
    identities = {}
    with tempfile.TemporaryDirectory() as directory:
        identity_root = Path(directory)
        for cli, executable in (("codex", args.codex), ("claude", args.claude)):
            if executable.is_symlink() or not executable.is_file() or executable.suffix.lower() in {".cmd", ".bat", ".ps1"}:
                raise ValueError("freeze requires native CLI executables")
            identities[cli] = verify_identity(executable, cli, identity_root)
    return {"schema": "real-methodology-manifest/1", "studyType": "real_project_methodology",
            "registry": {"path": str(args.registry.resolve()), "sha256": digest(args.registry)},
            "instrumentSha256": digest(Path(__file__)), "identities": identities,
            "models": {"codex": args.codex_model, "claude": args.claude_model},
            "profiles": profiles, "tasks": tasks, "plannedAttempts": order,
            "deadlineSeconds": DEADLINE, "evaluatorDeadlineSeconds": EVALUATOR_DEADLINE,
            "repeats": 1, "calibrationPassed": True, "winner": None}


def load_frozen(path: Path) -> dict:
    manifest = read(path)
    if manifest.get("instrumentSha256") != digest(Path(__file__)):
        raise ValueError("instrument changed after freeze")
    registry_path = Path(manifest["registry"]["path"])
    if digest(registry_path) != manifest["registry"]["sha256"]:
        raise ValueError("registry changed after freeze")
    return manifest


def run_attempt(args: argparse.Namespace) -> dict:
    manifest = load_frozen(args.manifest)
    cells = [cell for cell in manifest["plannedAttempts"] if cell["id"] == args.attempt]
    if len(cells) != 1:
        raise ValueError("select one planned attempt ID")
    cell = cells[0]
    receipt = args.output / (cell["id"] + ".json")
    if receipt.exists() or (args.output / (cell["id"] + ".reserved")).exists():
        raise ValueError("attempt ID already reserved or completed")
    args.output.mkdir(parents=True, exist_ok=True)
    publish(args.output / (cell["id"] + ".reserved"), {"manifestSha256": digest(args.manifest), "reserved": True})
    task = next(task for task in manifest["tasks"] if task["id"] == cell["task"])
    profile = next(profile for profile in manifest["profiles"] if profile["id"] == cell["profile"])
    result = {**cell, "manifestSha256": digest(args.manifest), "status": "incomplete", "billedUsd": None}
    with tempfile.TemporaryDirectory() as directory:
        subject = Path(directory) / "subject"
        subject.mkdir()
        scope, paths = task_paths(task)
        parent = snapshot_root(task, "parent")
        copy_paths(parent, subject, paths)
        public_source = safe_file(parent, task["publicTests"][0])
        shutil.copy2(public_source, subject / "test_public.py")
        before = {relative: digest(safe_file(subject, relative)) for relative in scope}
        executable = Path(manifest["identities"][cell["cli"]]["path"])
        if digest(executable) != manifest["identities"][cell["cli"]]["sha256"]:
            raise ValueError("CLI identity changed after freeze")
        prompt = ("Work only in this directory. Edit only the declared production files and public test. "
                  "Do not inspect other directories, use network, delegate, install packages, or change settings. "
                  "Run python test_public.py.\nRequest: " + task["request"] + "\nProfile: " + profile.get("prompt", ""))
        agent = capture(command_for(cell["cli"], executable, manifest["models"][cell["cli"]]), subject,
                        deadline=manifest["deadlineSeconds"], prompt=prompt)
        result["agent"] = {key: value for key, value in agent.items() if key not in {"stdout", "stderr"}}
        from methodology import usage
        result["usage"] = usage(cell["cli"], agent["stdout"])
        enforce_candidate_scope(subject, task, before)
        source_ok = all(not safe_file(subject, path).is_symlink() for path in scope)
        result["evaluation"] = evaluate(task, subject, args.output / (cell["id"] + "-evaluation")) if source_ok else {}
        passed = (agent["exit"] == 0 and not agent["timeout"] and agent["cleanupConfirmed"]
                  and all(row["accepted"] for row in result["evaluation"].values()))
        result["status"] = "passed" if passed else "failed"
        result["sourceSha256"] = tree_digest(subject, scope)
        text = (agent["stdout"] + agent["stderr"]).lower()
        if any(term in text for term in ("rate limit", "usage limit", "quota", "unauthorized", "authentication")):
            publish(args.output / "provider-stop.json", {"attempt": cell["id"], "reason": "provider-or-quota-failure"})
    publish(receipt, result)
    return result


def report(args: argparse.Namespace) -> dict:
    manifest = load_frozen(args.manifest)
    manifest_hash = digest(args.manifest)
    attempts = []
    for cell in manifest["plannedAttempts"]:
        path = args.output / (cell["id"] + ".json")
        if not path.exists():
            status = "incomplete" if (args.output / (cell["id"] + ".reserved")).exists() else "not-run"
            attempts.append({**cell, "status": status})
            continue
        row = read(path)
        if row.get("manifestSha256") != manifest_hash or any(row.get(key) != cell[key] for key in cell):
            raise ValueError("receipt does not match frozen cell")
        attempts.append(row)
    return {"schema": "real-methodology-report/1", "studyType": manifest["studyType"],
            "complete": all(row["status"] in {"passed", "failed"} for row in attempts),
            "attempts": attempts, "winner": None,
            "limitations": ["Neutral exploratory cohort; no statistical superiority claim",
                            "Native framework fidelity and method adherence remain unobserved",
                            "Missing usage or model identity is unknown, never zero"]}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["qualify", "freeze", "run", "report"])
    parser.add_argument("--registry", type=Path)
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
            if not args.registry:
                raise ValueError("qualify requires --registry")
            return 0 if qualify(load_registry(args.registry), args.output)["passed"] else 1
        if args.command == "freeze":
            if not all((args.registry, args.codex, args.claude, args.codex_model, args.claude_model)):
                raise ValueError("freeze requires registry, native CLIs and explicit models")
            publish(args.output, freeze(args))
            return 0
        if not args.manifest:
            raise ValueError(f"{args.command} requires --manifest")
        if args.command == "run":
            return 0 if run_attempt(args)["status"] == "passed" else 1
        print(json.dumps(report(args), indent=2))
        return 0
    except (ValueError, OSError, KeyError, subprocess.SubprocessError) as error:
        print(f"BENCHMARK_REFUSED: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
