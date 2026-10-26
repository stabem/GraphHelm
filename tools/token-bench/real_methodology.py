"""Offline benchmark runner for frozen real-project task snapshots."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import random
import shutil
import subprocess
import sys
import tempfile
import time

import methodology as base

# These names are the shared pilot boundaries; this runner does not fork them.

HOME = Path(__file__).resolve().parent
MAX_FILE = 2_000_000
MAX_NATIVE_FILE = 256 * 1024 * 1024
MAX_EXTERNAL_FILES = 50_000
MAX_EXTERNAL_BYTES = 2 * 1024 * 1024 * 1024
DEADLINE = 300
EVALUATOR_DEADLINE = 90


digest = base.digest
read = base.read
publish = base.publish


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


def snapshot_root(task: dict, kind: str) -> Path:
    value = task.get("parentSnapshot" if kind == "parent" else "fixSnapshot")
    if not isinstance(value, str):
        value = task.get("snapshots", {}).get(kind)
    if not isinstance(value, str):
        raise ValueError(f"task {task.get('id')} has no {kind} snapshot")
    root = Path(value).resolve()
    if root.is_symlink() or not root.is_dir():
        raise ValueError(f"missing or linked {kind} snapshot: {root}")
    return root


def task_paths(task: dict) -> tuple[list[str], list[str], list[str]]:
    scope = [safe_relative(path) for path in task.get("scope", [])]
    closure = [safe_relative(path) for path in task.get("productionImportClosure", scope)]
    public = [safe_relative(path) for path in task.get("publicTests", [])]
    support = [safe_relative(path) for path in task.get("supportFiles", [])]
    if not scope or not public or not set(scope).issubset(closure):
        raise ValueError(f"task {task.get('id')} has invalid scope or public tests")
    if "test_public.py" not in support:
        raise ValueError("supportFiles must include immutable test_public.py")
    return scope, sorted(set(closure)), sorted(set(public + support))


def all_snapshot_paths(task: dict) -> list[str]:
    scope, closure, support = task_paths(task)
    return sorted(set(closure + support + task["scope"] + task["publicTests"]))


def external_dependency_hashes(task: dict) -> dict[str, str]:
    result = {}
    for entry in task.get("externalDependencies", []):
        definition = entry if isinstance(entry, dict) else {"path": entry, "kind": "file"}
        path = Path(definition["path"]).resolve()
        kind = definition.get("kind", "file")
        if path.is_symlink():
            raise ValueError(f"external dependency is linked: {path}")
        if kind == "file":
            if not path.is_file() or path.stat().st_size > MAX_NATIVE_FILE:
                raise ValueError(f"external dependency is missing or oversized: {path}")
            result[str(path)] = digest(path)
            continue
        if kind != "directory" or not path.is_dir():
            raise ValueError(f"invalid external dependency: {path}")
        ignored = {safe_relative(item).rstrip("/") for item in definition.get("ignore", [])}
        files, total = [], 0
        for current, directories, names in os.walk(path, followlinks=False):
            current_path = Path(current)
            directories[:] = [name for name in directories
                              if name not in ignored and not (current_path / name).is_symlink()]
            for name in names:
                file_path = current_path / name
                if file_path.is_symlink():
                    raise ValueError(f"external dependency contains linked file: {file_path}")
                relative = file_path.relative_to(path).as_posix()
                if any(relative == item or relative.startswith(item + "/") for item in ignored):
                    continue
                size = file_path.stat().st_size
                total += size
                if len(files) >= MAX_EXTERNAL_FILES or total > MAX_EXTERNAL_BYTES:
                    raise ValueError(f"external dependency directory exceeds bounds: {path}")
                files.append((relative, size, digest(file_path)))
        tree = hashlib.sha256()
        for relative, size, content_hash in sorted(files):
            tree.update(relative.encode("utf-8"))
            tree.update(b"\0")
            tree.update(str(size).encode("ascii"))
            tree.update(b"\0")
            tree.update(content_hash.encode("ascii"))
            tree.update(b"\n")
        result[str(path)] = tree.hexdigest()
    return result


def tree_hashes(root: Path, paths: list[str]) -> dict[str, str]:
    return {path: digest(safe_file(root, path)) for path in paths}


def copy_paths(source: Path, target: Path, paths: list[str]) -> None:
    for relative in paths:
        source_file = safe_file(source, relative)
        destination = target / Path(*PurePosixPath(relative).parts)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source_file, destination)


def hidden_files(task: dict) -> list[str]:
    oracle = task["hiddenOracle"]
    files = oracle.get("files", [])
    if not files and oracle.get("file"):
        files = [oracle["file"]]
    return [safe_relative(path) for path in files]


def mount_path(root: Path, mount: str) -> Path:
    relative = safe_relative(mount)
    target = root / Path(*PurePosixPath(relative).parts)
    if root not in target.parents and target != root:
        raise ValueError(f"dependency mount escapes workspace: {mount}")
    return target


def _is_redirected_directory(path: Path) -> bool:
    junction_probe = getattr(os.path, "isjunction", None)
    return path.is_symlink() or bool(junction_probe(path)) if junction_probe else path.is_symlink()


def validate_mount_parents(root: Path, target: Path) -> None:
    """Reject redirected ancestors; the final declared mount may be a junction."""
    relative = target.relative_to(root)
    current = root
    for part in relative.parts[:-1]:
        current /= part
        if _is_redirected_directory(current):
            raise ValueError(f"dependency mount ancestor is redirected: {current}")


def prepare_mounts(task: dict, root: Path) -> None:
    """Create only registry-declared read-only dependency junctions in one workspace."""
    for entry in task.get("externalDependencies", []):
        if not isinstance(entry, dict) or entry.get("kind") != "directory" or not entry.get("mount"):
            continue
        source = Path(entry["path"]).resolve()
        if source.is_symlink() or not source.is_dir():
            raise ValueError(f"dependency mount source is missing or linked: {source}")
        target = mount_path(root, entry["mount"])
        validate_mount_parents(root, target)
        target.parent.mkdir(parents=True, exist_ok=True)
        if target.exists() or target.is_symlink():
            if target.resolve() == source:
                continue
            raise ValueError(f"dependency mount target is occupied: {target}")
        if os.name == "nt":
            import _winapi
            _winapi.CreateJunction(str(source), str(target))
        else:
            target.symlink_to(source, target_is_directory=True)


def hidden_command(task: dict) -> list[str]:
    command = task.get("hiddenOracle", {}).get("command")
    if not isinstance(command, list) or not command:
        raise ValueError("hidden oracle command must be a nonempty argv")
    return [str(item) for item in command]


def run_process(argv: list[str], cwd: Path, deadline: int) -> dict:
    result = base.capture(argv, cwd, deadline=deadline)
    result["cwd"] = str(cwd)
    return result


def wrapper_completed(task: dict, result: dict, phase: str) -> tuple[bool, str]:
    if result["exit"] != 0 or result["timeout"] or not result["cleanupConfirmed"]:
        return False, "wrapper did not complete successfully"
    completion = task.get("completion", {}).get(phase, {})
    marker = completion.get("stdoutMarker")
    file_name = completion.get("file")
    if not marker and not file_name:
        return False, "trusted completion evidence is not configured"
    if marker and marker not in result["stdout"]:
        return False, "trusted completion marker missing"
    if file_name and not (Path(result["cwd"]) / safe_relative(file_name)).is_file():
        return False, "trusted completion file missing"
    return True, "completed"


def classify_hidden_red(result: dict, assertion_marker: str | None = None) -> tuple[bool, str]:
    text = result["stdout"] + result["stderr"]
    if result["timeout"] or not result["cleanupConfirmed"]:
        return False, "timeout or cleanup failure"
    if result["exit"] == 0:
        return False, "oracle unexpectedly passed"
    if assertion_marker and assertion_marker not in result["stdout"]:
        return False, "intended assertion marker missing"
    if "AssertionError" in text or "assertion" in text.lower():
        return True, "intended assertion failure"
    return False, "infrastructure or tool failure"


def task_prompt(task: dict, profile: dict) -> str:
    return ("Work only in this directory. Edit only declared production files and original public test files. "
            "Do not inspect other directories, use network, delegate, install packages, or change settings. "
            "Run python test_public.py.\nDeclared production scope: " + json.dumps(task["scope"], sort_keys=True) +
            "\nDeclared public tests: " + json.dumps(task["publicTests"], sort_keys=True) +
            "\nRequest: " + task["request"] + "\nProfile: " + profile.get("prompt", ""))


def validate_task(task: dict) -> None:
    identifier = task.get("id")
    if not isinstance(identifier, str) or not identifier.replace("-", "").isalnum():
        raise ValueError("task IDs must be safe")
    scope, closure, support = task_paths(task)
    for kind in ("parent", "fix"):
        root = snapshot_root(task, kind)
        for relative in sorted(set(closure + support)):
            safe_file(root, relative)
    oracle = task.get("hiddenOracle")
    if not isinstance(oracle, dict) or not isinstance(oracle.get("root"), str):
        raise ValueError(f"task {identifier} requires a private hidden oracle")
    oracle_root = Path(oracle["root"]).resolve()
    if oracle_root.is_symlink() or not oracle_root.is_dir():
        raise ValueError("oracle root missing or linked")
    for relative in hidden_files(task):
        safe_file(oracle_root, relative)
    external_dependency_hashes(task)
    for key in ("parentSha", "knownFixSha"):
        if not isinstance(task.get(key), str) or len(task[key]) != 40:
            raise ValueError(f"task {identifier} requires {key}")
    if not isinstance(task.get("request"), str) or not task["request"].strip():
        raise ValueError(f"task {identifier} requires a request")


def qualify(registry: dict, output: Path) -> dict:
    tasks = registry.get("tasks")
    if not isinstance(tasks, list) or len(tasks) != 3:
        raise ValueError("qualification requires exactly three tasks")
    rows = []
    for task in tasks:
        validate_task(task)
        row = {"task": task["id"], "passed": False, "reasons": []}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for label, kind in (("parent", "parent"), ("fixed", "fix")):
                target = root / label
                target.mkdir()
                copy_paths(snapshot_root(task, kind), target, task_paths(task)[1] + task_paths(task)[2])
                prepare_mounts(task, target)
                copy_paths(Path(task["hiddenOracle"]["root"]), target, hidden_files(task))
                public = run_process([sys.executable, "test_public.py"], target, EVALUATOR_DEADLINE)
                hidden = run_process(hidden_command(task), target, EVALUATOR_DEADLINE)
                hidden_red, reason = classify_hidden_red(hidden, task.get("completion", {}).get("hidden", {}).get("assertionMarker"))
                row[label] = {"public": public, "hidden": hidden, "hiddenReason": reason}
                if public["exit"] != 0:
                    row["reasons"].append(f"{label} public failed")
                if label == "parent" and not hidden_red:
                    row["reasons"].append(f"parent hidden: {reason}")
                if label == "fixed" and hidden["exit"] != 0:
                    row["reasons"].append("known fix hidden failed")
                if public["exit"] == 0 and not wrapper_completed(task, public, "public")[0]:
                    row["reasons"].append("public trusted completion missing")
                if label == "fixed" and hidden["exit"] == 0 and not wrapper_completed(task, hidden, "hidden")[0]:
                    row["reasons"].append("hidden trusted completion missing")
        row["passed"] = not row["reasons"]
        for label in ("parent", "fixed"):
            for phase in ("public", "hidden"):
                row[label][phase].pop("cwd", None)
        rows.append(row)
    result = {"schema": "real-methodology-qualification/2", "passed": all(row["passed"] for row in rows), "tasks": rows}
    publish(output, result)
    return result


def load_registry(path: Path) -> dict:
    registry = read(path)
    if registry.get("schema") not in {"real-methodology-registry/1", "real-project-benchmark/1"}:
        raise ValueError("unsupported registry schema")
    return registry


def registry_hashes(registry: dict) -> dict:
    result = {}
    for task in registry["tasks"]:
        validate_task(task)
        for kind in ("parent", "fix"):
            result[f"{task['id']}:{kind}"] = tree_hashes(snapshot_root(task, kind), all_snapshot_paths(task))
        result[f"{task['id']}:oracle"] = tree_hashes(Path(task["hiddenOracle"]["root"]), hidden_files(task))
        result[f"{task['id']}:external"] = external_dependency_hashes(task)
    return result


def freeze(args: argparse.Namespace) -> dict:
    registry = load_registry(args.registry)
    tasks, profiles = registry.get("tasks", []), registry.get("profiles", [])
    if len(tasks) != 3 or len(profiles) != 7:
        raise ValueError("freeze requires three tasks and seven profiles")
    calibration = qualify(registry, args.output.with_suffix(".qualification.json"))
    if not calibration["passed"]:
        raise ValueError("qualification failed; inspect qualification receipt")
    with tempfile.TemporaryDirectory() as directory:
        identities = {cli: base.identity(executable, cli, Path(directory)) for cli, executable in
                      (("codex", args.codex), ("claude", args.claude))}
    order = []
    for task_index in random.Random(2051).sample(range(3), 3):
        for profile_index in range(7):
            for cli in (("codex", "claude") if task_index % 2 == 0 else ("claude", "codex")):
                profile = profiles[(profile_index + task_index) % 7]
                cell = {"id": f"{len(order):03d}", "task": tasks[task_index]["id"],
                        "profile": profile["id"], "cli": cli}
                cell["promptSha256"] = hashlib.sha256(task_prompt(tasks[task_index], profile).encode()).hexdigest()
                order.append(cell)
    return {"schema": "real-methodology-manifest/2", "studyType": "real_project_methodology",
            "registry": {"path": str(args.registry.resolve()), "sha256": digest(args.registry)},
            "registryFiles": registry_hashes(registry), "instrumentSha256": digest(Path(__file__)),
            "parserSha256": digest(HOME / "methodology.py"),
            "runParserSha256": digest(HOME / "run.py"), "identities": identities,
            "models": {"codex": args.codex_model, "claude": args.claude_model}, "profiles": profiles,
            "tasks": tasks, "plannedAttempts": order,
            "argv": {cli: base.native_cli_command(cli, identities[cli]["path"], model) for cli, model in
                     (("codex", args.codex_model), ("claude", args.claude_model))},
            "python": {"path": sys.executable, "sha256": digest(Path(sys.executable))},
            "environment": {key: os.environ.get(key, "") for key in ("PATH", "PATHEXT", "PYTHONPATH", "PYTEST_DISABLE_PLUGIN_AUTOLOAD")},
            "deadlineSeconds": DEADLINE, "evaluatorDeadlineSeconds": EVALUATOR_DEADLINE,
            "calibrationPassed": True, "winner": None}


def load_frozen(path: Path) -> dict:
    manifest = read(path)
    if (manifest.get("instrumentSha256") != digest(Path(__file__))
            or manifest.get("parserSha256") != digest(HOME / "methodology.py")
            or manifest.get("runParserSha256") != digest(HOME / "run.py")):
        raise ValueError("instrument or parser changed after freeze")
    registry_path = Path(manifest["registry"]["path"])
    if digest(registry_path) != manifest["registry"]["sha256"]:
        raise ValueError("registry changed after freeze")
    registry = load_registry(registry_path)
    if registry_hashes(registry) != manifest["registryFiles"]:
        raise ValueError("snapshot or oracle files changed after freeze")
    if digest(Path(manifest["python"]["path"])) != manifest["python"]["sha256"]:
        raise ValueError("Python changed after freeze")
    return manifest


def candidate_files(root: Path) -> dict[str, str]:
    files = {}
    for current, directories, names in os.walk(root, followlinks=False):
        directories[:] = [name for name in directories
                          if name not in {"__pycache__", ".pytest_cache", "node_modules", ".vite"}
                          and not (Path(current) / name).is_symlink()]
        for name in names:
            path = Path(current) / name
            if path.is_symlink():
                continue
            files[path.relative_to(root).as_posix()] = digest(path)
    return files


def validate_external_links(task: dict, candidate: Path) -> None:
    for entry in task.get("externalDependencies", []):
        if not isinstance(entry, dict):
            continue
        candidate_path = entry.get("candidatePath") or entry.get("mount")
        if not candidate_path:
            continue
        target = mount_path(candidate, candidate_path)
        validate_mount_parents(candidate, target)
        expected = Path(entry["path"]).resolve()
        if not target.exists() or target.resolve() != expected:
            raise ValueError(f"candidate dependency link is missing or points elsewhere: {target}")


def enforce_scope(task: dict, candidate: Path, before: dict[str, str]) -> None:
    scope, closure, support = task_paths(task)
    validate_external_links(task, candidate)
    allowed_changes = set(scope) | set(task["publicTests"])
    now = candidate_files(candidate)
    for relative, value in now.items():
        if relative in allowed_changes:
            continue
        if before.get(relative) != value:
            raise ValueError(f"agent changed file outside declared scope: {relative}")
    for relative in scope + task["publicTests"]:
        if relative not in now:
            raise ValueError(f"agent removed declared file: {relative}")
    immutable = set(closure + support) - allowed_changes
    for relative in immutable:
        if relative not in now:
            raise ValueError(f"agent removed immutable dependency/support file: {relative}")


def evaluator(task: dict, candidate: Path, output: Path, frozen_hashes: dict) -> dict:
    scope, closure, support = task_paths(task)
    current = registry_hashes({"tasks": [task]})
    expected = {key: value for key, value in frozen_hashes.items() if key.startswith(task["id"] + ":")}
    if current != expected:
        raise ValueError("snapshot or oracle drifted before evaluation")
    output.mkdir(parents=True, exist_ok=True)
    root = output / "evaluator"
    root.mkdir()
    copy_paths(snapshot_root(task, "parent"), root, closure + support)
    prepare_mounts(task, root)
    for relative in scope:
        destination = root / Path(*PurePosixPath(relative).parts)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(safe_file(candidate, relative), destination)
    # Always restore original public tests; agent edits cannot weaken evaluation.
    copy_paths(snapshot_root(task, "parent"), root, task["publicTests"])
    public = run_process([sys.executable, "test_public.py"], root, EVALUATOR_DEADLINE)
    copy_paths(Path(task["hiddenOracle"]["root"]), root, hidden_files(task))
    hidden = run_process(hidden_command(task), root, EVALUATOR_DEADLINE)
    return {"public": public, "hidden": hidden}


def run_attempt(args: argparse.Namespace) -> dict:
    manifest = load_frozen(args.manifest)
    cells = manifest["plannedAttempts"]
    selected = [cell for cell in cells if cell["id"] == args.attempt]
    if len(selected) != 1:
        raise ValueError("select one planned attempt ID")
    cell = selected[0]
    args.output.mkdir(parents=True, exist_ok=True)
    receipt = args.output / (cell["id"] + ".json")
    reservation = args.output / (cell["id"] + ".reserved")
    if receipt.exists() or reservation.exists():
        raise ValueError("attempt ID already reserved or completed")
    for earlier in cells[:cells.index(cell)]:
        if not (args.output / (earlier["id"] + ".json")).is_file():
            raise ValueError("frozen cell order has an earlier missing result")
    if (args.output / "provider-stop.json").exists():
        raise ValueError("provider-stop exists; cohort is stopped")
    lock = args.output / "cohort.lock"
    try:
        with lock.open("x", encoding="utf-8") as handle:
            handle.write(str(os.getpid()))
    except FileExistsError as error:
        raise ValueError("cohort is already running") from error
    publish(reservation, {"manifestSha256": digest(args.manifest), "reserved": True})
    task = next(task for task in manifest["tasks"] if task["id"] == cell["task"])
    profile = next(profile for profile in manifest["profiles"] if profile["id"] == cell["profile"])
    result = {**cell, "manifestSha256": digest(args.manifest), "status": "incomplete", "measurementStatus": "unknown", "billedUsd": None}
    subject = args.output / (cell["id"] + "-candidate")
    try:
        subject.mkdir()
        scope, closure, support = task_paths(task)
        copy_paths(snapshot_root(task, "parent"), subject, closure + support + task["publicTests"])
        prepare_mounts(task, subject)
        before = candidate_files(subject)
        executable = Path(manifest["identities"][cell["cli"]]["path"])
        current_environment = {key: os.environ.get(key, "") for key in manifest["environment"]}
        if current_environment != manifest["environment"]:
            raise ValueError("frozen environment changed after freeze")
        if base.native_cli_command(cell["cli"], executable, manifest["models"][cell["cli"]]) != manifest["argv"][cell["cli"]]:
            raise ValueError("frozen CLI argv changed after freeze")
        if base.identity(executable, cell["cli"], subject) != manifest["identities"][cell["cli"]]:
            raise ValueError("CLI identity or authentication changed after freeze")
        prompt = task_prompt(task, profile)
        if hashlib.sha256(prompt.encode()).hexdigest() != cell["promptSha256"]:
            raise ValueError("frozen prompt changed after freeze")
        started = time.monotonic()
        agent = base.capture(base.native_cli_command(cell["cli"], executable, manifest["models"][cell["cli"]]), subject,
                             deadline=manifest["deadlineSeconds"], prompt=prompt)
        result["elapsedSeconds"] = time.monotonic() - started
        result["agent"] = agent
        result["usage"] = base.usage(cell["cli"], agent["stdout"])
        provider_error = base.native_provider_error(cell["cli"], agent["stdout"], agent["stderr"])
        if provider_error:
            result["providerError"] = provider_error
            raise ValueError("native provider control error observed")
        observed = result["usage"].get("observedModels", [])
        result["modelStatus"] = "unobserved" if not observed else ("matched" if observed == [manifest["models"][cell["cli"]]] else "mismatch")
        result["measurementStatus"] = "observed" if result["usage"].get("counts") is not None else "unknown"
        if result["modelStatus"] == "mismatch":
            raise ValueError("observed model differs from requested model")
        if agent["truncated"] or not agent["cleanupConfirmed"]:
            raise ValueError("agent output truncated or cleanup uncertain")
        enforce_scope(task, subject, before)
        result["evaluation"] = evaluator(task, subject, args.output / (cell["id"] + "-evaluation"), manifest["registryFiles"])
        public_complete, public_reason = wrapper_completed(task, result["evaluation"]["public"], "public")
        hidden_complete, hidden_reason = wrapper_completed(task, result["evaluation"]["hidden"], "hidden")
        result["evaluationStatus"] = {"public": public_reason, "hidden": hidden_reason}
        result["status"] = "passed" if agent["exit"] == 0 and not agent["timeout"] and public_complete and hidden_complete and all(row["exit"] == 0 for row in result["evaluation"].values()) else "failed"
        result["sourceSha256"] = {relative: digest(safe_file(subject, relative)) for relative in scope}
        if result["measurementStatus"] == "unknown":
            publish(args.output / "provider-stop.json", {"attempt": cell["id"], "reason": "native accounting missing after quality outcome"})
    except Exception as error:
        result["error"] = str(error)
        publish(args.output / "provider-stop.json", {"attempt": cell["id"], "reason": str(error)})
    finally:
        publish(receipt, result)
        lock.unlink(missing_ok=True)
    return result


def report(args: argparse.Namespace) -> dict:
    manifest = load_frozen(args.manifest)
    rows = []
    for cell in manifest["plannedAttempts"]:
        path = args.output / (cell["id"] + ".json")
        if not path.exists():
            rows.append({**cell, "status": "incomplete" if (args.output / (cell["id"] + ".reserved")).exists() else "not-run"})
            continue
        row = read(path)
        if row.get("manifestSha256") != digest(args.manifest) or any(row.get(key) != cell[key] for key in cell):
            raise ValueError("receipt does not match frozen cell")
        rows.append(row)
    return {"schema": "real-methodology-report/2", "studyType": manifest["studyType"],
            "complete": all(row["status"] in {"passed", "failed"} for row in rows),
            "attempts": rows, "winner": None,
            "limitations": ["Neutral exploratory cohort; no statistical superiority claim", "Missing usage is unknown, never free"]}


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
