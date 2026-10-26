"""Versioned native-framework workflow planning and fail-closed phase evidence."""
from __future__ import annotations

import hashlib
import json
import os
import argparse
from pathlib import Path
import subprocess
import tempfile
import shutil
from typing import Callable, Iterable

import methodology as base
from real_methodology import evaluator as evaluate_real_project
from real_methodology import registry_hashes as pinned_registry_hashes


SCHEMA = "native-methodology-workflow/1"
PUBLIC_CONTEXT_KEYS = {"task", "scope", "publicTests", "request", "workflow", "phase"}
NATIVE_WITNESS_KINDS = {"native_stream", "native_cli", "native_artifact", "native_skill"}

# Shared observers remain single-sourced in the existing pilot/evaluator modules.
capture = base.capture
identity = base.identity
usage = base.usage
native_provider_error = base.native_provider_error


def native_cli_command(cli: str, executable: Path, model: str) -> list[str]:
    """Return the native workflow argv while retaining project configuration and logs."""
    return native_cli_config(cli, executable, model)["argv"]


def _safe_id(value: object, label: str) -> str:
    if not isinstance(value, str) or not value or not value.replace("-", "").replace("_", "").isalnum():
        raise ValueError(f"{label} must be a safe identifier")
    return value


def _json_hash(value: object) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def artifact_hashes(root: Path, relative_paths: Iterable[str]) -> dict[str, str]:
    """Hash declared phase artifacts without following links or accepting paths outside root."""
    root = root.resolve()
    result = {}
    for relative in sorted(set(relative_paths)):
        path = (root / Path(relative)).resolve()
        if root not in path.parents or path.is_symlink() or not path.is_file():
            raise ValueError(f"phase artifact is missing, linked, or outside root: {relative}")
        result[relative.replace("\\", "/")] = base.digest(path)
    return result


def validate_public_context(context: dict) -> dict:
    """Keep task context public and bounded; credentials and hidden oracle data never cross a phase."""
    if not isinstance(context, dict) or set(context) - PUBLIC_CONTEXT_KEYS:
        raise ValueError("task context contains private or unknown fields")
    for key in ("task", "workflow", "phase"):
        _safe_id(context.get(key), key)
    if not isinstance(context.get("scope"), list) or not all(isinstance(value, str) for value in context["scope"]):
        raise ValueError("task context scope must be a list of public paths")
    if not isinstance(context.get("publicTests"), list) or not all(isinstance(value, str) for value in context["publicTests"]):
        raise ValueError("task context publicTests must be a list")
    if not isinstance(context.get("request"), str) or not context["request"].strip():
        raise ValueError("task context request is required")
    return json.loads(json.dumps(context, sort_keys=True))


def validate_registry(registry: dict) -> dict:
    """Validate a versioned registry while allowing framework-specific stage definitions."""
    if registry.get("schema") != SCHEMA:
        raise ValueError("unsupported native workflow schema")
    _safe_id(registry.get("version"), "registry version")
    workflows = registry.get("workflows")
    if not isinstance(workflows, list) or not workflows:
        raise ValueError("registry requires workflows")
    seen = set()
    for workflow in workflows:
        _safe_id(workflow.get("id"), "workflow id")
        if workflow["id"] in seen:
            raise ValueError("workflow ids must be unique")
        seen.add(workflow["id"])
        _safe_id(workflow.get("version"), "workflow version")
        if not isinstance(workflow.get("framework"), str) or not workflow["framework"].strip():
            raise ValueError("workflow framework is required")
        if type(workflow.get("maxFixAttempts")) is not int or workflow["maxFixAttempts"] < 0:
            raise ValueError("workflow must declare a nonnegative maxFixAttempts")
        if not isinstance(workflow.get("projectRoot"), str) or not Path(workflow["projectRoot"]).is_dir():
            raise ValueError("workflow projectRoot must name an existing project")
        if not isinstance(workflow.get("taskRefs"), list) or not workflow["taskRefs"]:
            raise ValueError("workflow taskRefs are required")
        if not isinstance(workflow.get("pinnedPublicFiles"), list):
            raise ValueError("workflow pinnedPublicFiles are required")
        if not isinstance(workflow.get("methodAssetDigests"), dict):
            raise ValueError("workflow methodAssetDigests are required")
        context = workflow.get("taskContext")
        if (not isinstance(context, dict) or not isinstance(context.get("task"), str)
                or not isinstance(context.get("scope"), list) or not isinstance(context.get("publicTests"), list)
                or not isinstance(context.get("request"), str) or not context["request"].strip()):
            raise ValueError("workflow taskContext must include public task, scope, publicTests, and request")
        phases = workflow.get("phases")
        if not isinstance(phases, list) or not phases:
            raise ValueError("workflow phases are required")
        phase_ids = set()
        for phase in phases:
            _safe_id(phase.get("id"), "phase id")
            if phase["id"] in phase_ids:
                raise ValueError("phase ids must be unique within a workflow")
            phase_ids.add(phase["id"])
            if not isinstance(phase.get("command"), list):
                raise ValueError("each phase needs a native command list")
            if not isinstance(phase.get("prompt"), str) or not phase["prompt"].strip():
                raise ValueError("each phase needs a frozen prompt")
            for operation in phase.get("requiredOperations", []):
                pattern = operation.get("commandPattern") if isinstance(operation, dict) else None
                if not isinstance(pattern, str) or not pattern.strip() or pattern.lstrip().startswith(("read ", "inspect ", "preserve ", "follow ", "invoke ")):
                    raise ValueError("required native operations must name executable commands")
    return registry


def load_registry(path: Path) -> dict:
    registry = base.read(path)
    return validate_registry(registry)


def build_vendor_registry(**kwargs) -> dict:
    """Build the pinned vendor registry without coupling the worker to assets."""
    from native_vendor_workflows import build_vendor_registry as factory
    return validate_registry(factory(**kwargs))


def frozen_inputs(workflow: dict) -> dict:
    """Hash the project files and pinned method assets that every native cell must reuse."""
    project = Path(workflow["projectRoot"])
    public = artifact_hashes(project, workflow["pinnedPublicFiles"])
    assets = {}
    for path_text, expected in workflow["methodAssetDigests"].items():
        path = Path(path_text)
        if not path.is_absolute():
            path = project / path
        if not path.is_file() or path.is_symlink() or not isinstance(expected, str) or base.digest(path) != expected:
            raise ValueError(f"method asset changed or missing: {path_text}")
        assets[str(path.resolve())] = expected
    witnesses = []
    for phase in workflow["phases"]:
        for witness in phase.get("requiredWitnesses", []):
            path = Path(witness.get("path", ""))
            if not path.is_absolute():
                path = project / path
            expected = witness.get("sha256")
            if not path.is_file() or path.is_symlink() or not isinstance(expected, str) or base.digest(path) != expected:
                raise ValueError(f"required native witness changed or missing: {path}")
            witnesses.append({"phase": phase["id"], "kind": witness.get("kind"),
                              "path": str(path.resolve()), "sha256": expected})
    return {"projectRoot": str(project.resolve()), "publicFiles": public,
            "publicTree": _public_tree(project), "methodAssets": assets,
            "taskRefs": list(workflow["taskRefs"]), "requiredWitnesses": witnesses}


def _public_tree(root: Path) -> dict[str, str]:
    """Freeze every available public project file, including untracked source context."""
    ignored = {".git", ".native-observations", "target", "node_modules", ".pytest_cache",
               "__pycache__", ".venv", "events", "cache"}
    result = {}
    total_bytes = 0
    for path in sorted(root.rglob("*")):
        if any(part in ignored for part in path.relative_to(root).parts):
            continue
        if path.is_symlink():
            raise ValueError("public project tree contains a linked path")
        if not path.is_file():
            continue
        if path.name.startswith(".env") or path.name.endswith((".token", ".key")):
            continue
        total_bytes += path.stat().st_size
        if len(result) >= 10000 or total_bytes > 256 * 1024 * 1024:
            raise ValueError("public project tree exceeds frozen context bounds")
        result[path.relative_to(root).as_posix()] = base.digest(path)
    return result


def _public_tree_unchanged(frozen: dict, workflow: dict, root: Path) -> bool:
    if not isinstance(frozen, dict) or not frozen:
        return False
    context = workflow.get("taskContext", {})
    mutable = set(context.get("scope", [])) | set(context.get("allowedTestPaths", []))
    prefixes = tuple(prefix.rstrip("/") + "/" for prefix in workflow.get("methodArtifactPrefixes", []))
    def immutable(entries: dict[str, str]) -> dict[str, str]:
        return {relative: digest for relative, digest in entries.items()
                if relative not in mutable and not relative.startswith(prefixes)}
    try:
        return immutable(_public_tree(root)) == immutable(frozen)
    except (OSError, ValueError):
        return False


def candidate_tree_hash(root: Path, declared_paths: list[str] | None = None) -> str:
    """Hash the evolving project candidate without including its Git metadata."""
    root = root.resolve()
    entries = []
    candidates = ((root / relative for relative in declared_paths)
                  if declared_paths is not None else root.rglob("*"))
    volatile = {".git", ".graphhelm", ".native-observations", "target", "node_modules",
                ".pytest_cache", "__pycache__", ".venv", "events", "cache"}
    for path in sorted(candidates):
        if not path.is_file() or any(part in volatile for part in path.parts) or path.is_symlink():
            continue
        if path.name.startswith(".env") or path.name.endswith((".token", ".key")):
            continue
        relative = path.relative_to(root).as_posix()
        entries.append((relative, base.digest(path)))
    return _json_hash(entries)


def _declared_candidate_paths(workflow: dict, root: Path) -> list[str]:
    """Expand declared artifact globs into stable project-relative candidate paths."""
    paths = list(workflow.get("pinnedPublicFiles", []))
    context = workflow.get("taskContext", {})
    paths.extend(context.get("scope", []))
    paths.extend(context.get("allowedTestPaths", []))
    paths.extend(workflow.get("methodAssetDigests", {}).keys())
    for phase in workflow.get("phases", []):
        paths.extend(phase.get("requiredArtifacts", []))
        for pattern in phase.get("requiredArtifactGlobs", []):
            paths.extend(path.relative_to(root).as_posix() for path in sorted(root.glob(pattern))
                          if path.is_file() and not path.is_symlink())
    return paths


def _write_native_review_binding(workflow: dict, project: Path) -> Path:
    """Pin a committed candidate before the independent native review starts."""
    baseline = workflow.get("baselineHead")
    if not isinstance(baseline, str) or len(baseline) != 40 or any(c not in "0123456789abcdef" for c in baseline):
        raise ValueError("native review has no frozen Git baseline")
    flags = base.NO_WINDOW
    head_result = subprocess.run(["git", "rev-parse", "HEAD"], cwd=project, capture_output=True,
                                 text=True, timeout=15, creationflags=flags, check=False)
    head = head_result.stdout.strip() if head_result.returncode == 0 else ""
    if len(head) != 40 or head == baseline:
        raise ValueError("native review requires a committed candidate after the frozen baseline")
    diff = subprocess.run(["git", "-c", "core.autocrlf=false", "diff", "--binary", baseline, head],
                          cwd=project, capture_output=True, timeout=15, creationflags=flags, check=False)
    if diff.returncode != 0 or not diff.stdout:
        raise ValueError("native review cannot bind a nonempty committed patch")
    path = project / ".native-observations/review-binding.json"
    base.publish(path, {"schema": "native-review-binding/1", "taskId": workflow["taskRefs"][0],
                        "reviewer": "native-reviewer", "baseHead": baseline, "gitHead": head,
                        "patchSha256": hashlib.sha256(diff.stdout).hexdigest()})
    return path


def native_cli_config(cli: str, executable: Path, model: str, *, project_doc: Path | None = None,
                      extra: list[str] | None = None,
                      graphhelm_env: dict[str, str] | None = None) -> dict:
    """Build native CLI configuration; project settings and skills remain enabled."""
    if cli == "codex":
        argv = [str(executable), "exec", "--json", "--ignore-user-config", "--skip-git-repo-check",
                "-c", 'default_permissions=":workspace"', "-c", 'windows.sandbox="elevated"',
                "-c", "features.prefer_mxc=true", "-c", 'web_search="disabled"',
                "-m", model, "-"]
    elif cli == "claude":
        argv = [str(executable), "-p", "--output-format", "stream-json", "--verbose", "--model", model]
    else:
        raise ValueError(f"unsupported native CLI: {cli}")
    if extra:
        argv.extend(extra)
    return {"cli": cli, "executable": str(executable.resolve()), "model": model,
            "argv": argv, "projectDoc": str(project_doc.resolve()) if project_doc else None,
            "projectSkills": "enabled", "safeMode": "native-default", "plugins": "native-default",
            "sessionPersistence": "enabled", "projectConfig": "native-default",
            "graphhelmEnv": dict(graphhelm_env or {})}


def phase_plan(workflow: dict, context: dict, *, plan_id: str, predecessor: str | None = None) -> dict:
    """Create an immutable plan with public context and explicit framework-native phases."""
    validate_registry({"schema": SCHEMA, "version": workflow["version"], "workflows": [workflow]})
    public = validate_public_context({**context, "workflow": workflow["id"]})
    phases = [{"id": phase["id"], "command": list(phase["command"]),
               "frameworkStage": phase.get("frameworkStage", phase["id"]),
               "prompt": phase["prompt"], "promptSha256": _json_hash(phase["prompt"]),
               "requiredArtifacts": list(phase.get("requiredArtifacts", [])),
               "requiredOperations": list(phase.get("requiredOperations", [])),
               "requiredArtifactGlobs": list(phase.get("requiredArtifactGlobs", [])),
               "requiredWitnessKinds": list(phase.get("requiredWitnessKinds", ["native_stream"])),
               "requiredWitnesses": list(phase.get("requiredWitnesses", [])),
               "costKinds": list(phase.get("costKinds", [phase["id"]]))} for phase in workflow["phases"]]
    return {"schema": "native-methodology-plan/1", "planId": _safe_id(plan_id, "plan id"),
            "workflow": workflow["id"], "workflowVersion": workflow["version"],
            "predecessorPlan": predecessor, "maxFixAttempts": workflow["maxFixAttempts"],
            "context": public, "phases": phases,
            "planSha256": _json_hash({"workflow": workflow, "context": public, "phases": phases})}


def phase_attempt(plan: dict, phase_id: str, attempt_id: str, *, retry_of: str | None = None,
                  stage_root: str | None = None) -> dict:
    """Reserve one fresh phase stream; retry links point to a prior attempt and never replace it."""
    _safe_id(attempt_id, "attempt id")
    phase = None
    for planned_phase in plan["phases"]:
        if planned_phase["id"] == phase_id:
            phase = planned_phase
            break
    if phase is None:
        raise ValueError(f"phase is not in plan: {phase_id}")
    if retry_of == attempt_id:
        raise ValueError("an attempt cannot retry itself")
    return {"schema": "native-methodology-phase-attempt/1", "attemptId": attempt_id,
            "planId": plan["planId"], "planSha256": plan["planSha256"], "phase": phase_id,
            "frameworkStage": phase["frameworkStage"], "retryOf": retry_of,
            "stageRoot": stage_root, "status": "reserved", "artifactHashes": {},
            "fidelity": "unknown", "completeness": "unknown"}


def accepted_patch_endpoint(*, patch_path: str, review_path: str, evaluator_path: str,
                            author: str, reviewer: str, local_git: bool, published: bool = False,
                            reviewer_session_path: str | None = None, git_head_path: str | None = None,
                            task_id: str | None = None) -> dict:
    """Verify an independent local review and evaluator receipt; booleans cannot self-attest."""
    patch = Path(patch_path)
    review = Path(review_path)
    evaluator = Path(evaluator_path)
    reviewer_session = Path(reviewer_session_path) if reviewer_session_path else None
    git_head = Path(git_head_path) if git_head_path else None
    if (published or not local_git or not task_id or not reviewer_session_path or not git_head_path
            or not patch.is_file() or not review.is_file() or not evaluator.is_file()
            or (reviewer_session is not None and not reviewer_session.is_file())
            or (git_head is not None and not (git_head.is_dir() or git_head.is_file()))):
        return {"status": "incomplete", "reason": "accepted patch endpoint is not independently reviewed local Git"}
    try:
        review_data = base.read(review)
        evaluator_data = base.read(evaluator)
    except (OSError, ValueError, json.JSONDecodeError):
        return {"status": "incomplete", "reason": "review or evaluator receipt is invalid"}
    patch_sha = base.digest(patch)
    reviewer_head = review_data.get("gitHead")
    evaluator_head = evaluator_data.get("gitHead")
    from native_session_evidence import native_review
    native = native_review(reviewer_session)
    native_verdict = native.get("verdict", {})
    if (review_data.get("verdict") not in {"APPROVE", "APPROVE-WITH-RISK"}
            or review_data.get("reviewer") != reviewer or reviewer == author
            or review_data.get("patchSha256") != patch_sha
            or evaluator_data.get("status") != "passed"
            or not isinstance(reviewer_head, str) or len(reviewer_head) != 40
            or not isinstance(evaluator_head, str) or len(evaluator_head) != 40
            or reviewer_head != evaluator_head
            or not isinstance(review_data.get("reviewerSessionSha256"), str)
            or not isinstance(review_data.get("reviewerRootSessionId"), str)
            or not isinstance(review_data.get("authorRootSessionId"), str)
            or review_data["reviewerRootSessionId"] == review_data["authorRootSessionId"]
            or not isinstance(native_verdict, dict)
            or native.get("status") != "observed"
            or native_verdict.get("verdict") != review_data.get("verdict")
            or native_verdict.get("reviewer") != reviewer
            or native_verdict.get("taskId") != task_id
            or native_verdict.get("gitHead") != reviewer_head
            or native_verdict.get("patchSha256") != patch_sha
            or native.get("rootSession") != review_data.get("reviewerRootSessionId")):
        return {"status": "incomplete", "reason": "review/evaluator receipt is not bound to this patch"}
    if review_data.get("taskId") != task_id or evaluator_data.get("taskId") != task_id:
        return {"status": "incomplete", "reason": "review/evaluator task mismatch"}
    if evaluator_data.get("patchSha256") != patch_sha:
        return {"status": "incomplete", "reason": "evaluator patch digest mismatch"}
    if review_data.get("reviewerSessionSha256") != base.digest(reviewer_session):
        return {"status": "incomplete", "reason": "review session is not bound to verdict"}
    if not git_head.is_dir():
        return {"status": "incomplete", "reason": "Git HEAD binding must name a repository"}
    completed = subprocess.run(["git", "rev-parse", "HEAD"], cwd=git_head,
                               capture_output=True, text=True, timeout=15,
                               creationflags=base.NO_WINDOW, check=False)
    head = completed.stdout.strip() if completed.returncode == 0 else ""
    if len(head) != 40 or any(char not in "0123456789abcdef" for char in head.lower()) or head != reviewer_head:
        return {"status": "incomplete", "reason": "local Git head is not bound to accepted patch"}
    base_head = review_data.get("baseHead")
    if (not isinstance(base_head, str) or len(base_head) != 40
            or any(c not in "0123456789abcdef" for c in base_head)
            or evaluator_data.get("baseHead") != base_head):
        return {"status": "incomplete", "reason": "accepted patch has no shared Git baseline"}
    diff = subprocess.run(["git", "-c", "core.autocrlf=false", "diff", "--binary", base_head, head],
                          cwd=git_head, capture_output=True, timeout=15,
                          creationflags=base.NO_WINDOW, check=False)
    if diff.returncode != 0 or not diff.stdout or hashlib.sha256(diff.stdout).hexdigest() != patch_sha:
        return {"status": "incomplete", "reason": "accepted patch differs from the actual Git candidate"}
    try:
        from real_methodology import wrapper_completed
        registry_path = Path(evaluator_data["registryPath"])
        if (base.digest(registry_path) != evaluator_data["registrySha256"]
                or any(path.resolve().is_relative_to(git_head.resolve())
                       for path in (review, evaluator, reviewer_session, registry_path))):
            raise ValueError("acceptance evidence must be private and pinned")
        task = next(t for t in base.read(registry_path)["tasks"] if t["id"] == task_id)
        evaluation = evaluator_data["evaluation"]
        if not all(wrapper_completed(task, evaluation[phase], phase)[0] for phase in ("public", "hidden")):
            raise ValueError("external evaluator wrappers did not complete")
        expected = evaluator_data["productionDigests"]
        if set(expected) != set(task["scope"]) or artifact_hashes(git_head, task["scope"]) != expected:
            raise ValueError("evaluated production differs from the candidate")
    except (KeyError, TypeError, OSError, ValueError, StopIteration):
        return {"status": "incomplete", "reason": "external acceptance is missing or bound to another candidate"}
    return {"status": "observed", "patchPath": str(patch), "patchSha256": patch_sha,
            "reviewPath": str(review), "evaluatorPath": str(evaluator),
            "reviewerSessionPath": str(reviewer_session), "gitHeadPath": str(git_head), "taskId": task_id,
            "gitHead": reviewer_head, "baseHead": base_head, "reviewerSessionSha256": review_data["reviewerSessionSha256"],
            "reviewerRootSessionId": review_data["reviewerRootSessionId"],
            "authorRootSessionId": review_data["authorRootSessionId"],
            "evaluatorStatus": evaluator_data["status"], "author": author, "reviewer": reviewer,
            "published": False}


def reconcile_nested_usage(records: object, *, expected_sessions: list[str], parent_usage: dict | None = None) -> dict:
    """Reconcile owned native child records; absent sessions are unknown and block totals."""
    if not isinstance(records, list) or not all(isinstance(row, dict) for row in records):
        raise ValueError("nested usage records must be a list")
    by_id = {row.get("sessionId"): row for row in records if isinstance(row.get("sessionId"), str)}
    missing = sorted(set(expected_sessions) - set(by_id))
    invalid = []
    for row in records:
        values = row.get("usage")
        if row.get("owned") is not True or row.get("rawSha256") is None:
            invalid.append(row.get("sessionId", ""))
            continue
        if (not isinstance(values, dict) or any(type(values.get(key)) is not int or values[key] < 0
                                                for key in ("input", "output", "cacheRead", "cacheWrite"))):
            invalid.append(row.get("sessionId", ""))
    duplicates = len(by_id) != len(records)
    overlap = any(row.get("overlaps") for row in records)
    complete = not missing and not invalid and not duplicates and not overlap
    total = None
    if complete:
        if parent_usage is None and any(row.get("includedInParent") is True for row in records):
            complete = False
        if parent_usage is not None and (not isinstance(parent_usage, dict) or any(
                type(parent_usage.get(key)) is not int or parent_usage[key] < 0
                for key in ("input", "output", "cacheRead", "cacheWrite"))):
            complete = False
        else:
            total = {key: (parent_usage.get(key, 0) if parent_usage else 0) + sum(
                row["usage"].get(key, 0) for row in records if row.get("includedInParent") is not True)
                     for key in ("input", "output", "cacheRead", "cacheWrite")}
    return {"status": "observed" if complete else "unknown", "total": total,
            "missingSessions": missing, "invalidSessions": invalid,
            "duplicateSessions": duplicates, "overlap": overlap,
            "expectedSessions": sorted(expected_sessions)}


def collect_owned_child_usage(cli: str, raw_records: object, *, parent_session: str,
                              parent_usage: dict | None = None) -> dict:
    """Extract child totals from owned native session observations without summing cumulative events."""
    if not isinstance(raw_records, list) or not raw_records:
        return {"status": "unknown", "reason": "child session observations missing"}
    rows = []
    expected = []
    for raw in raw_records:
        if not isinstance(raw, dict) or raw.get("owned") is not True:
            continue
        session = raw.get("sessionId")
        if raw.get("parentSessionId") == parent_session and isinstance(session, str):
            expected.append(session)
        usage = raw.get("finalUsage")
        if cli == "claude" and isinstance(raw.get("tool_use_result"), dict):
            usage = raw["tool_use_result"].get("usage") or usage
        if cli == "codex":
            usage = raw.get("finalTokenUsage") or usage
        if isinstance(session, str):
            rows.append({"sessionId": session, "parentSessionId": raw.get("parentSessionId"),
                         "owned": True, "usage": usage, "rawSha256": raw.get("rawSha256"),
                         "includedInParent": raw.get("includedInParent", False),
                         "overlaps": raw.get("overlaps", False)})
    return reconcile_nested_usage(rows, expected_sessions=sorted(set(expected)), parent_usage=parent_usage)


def validate_method_operation_witness(observation: object, *, expected_command: str,
                                     expected_artifact: str | None = None) -> dict:
    """Validate a real native method operation and its result artifact."""
    if not isinstance(observation, dict):
        return {"status": "incomplete", "reason": "native operation observation missing"}
    call = observation.get("toolCall") or observation.get("command")
    result = observation.get("toolResult") or observation.get("result")
    if call != expected_command or not isinstance(result, dict):
        return {"status": "incomplete", "reason": "native operation command/result mismatch"}
    if result.get("is_error") is True or result.get("status") in {"denied", "permission_denied"}:
        return {"status": "incomplete", "reason": "native method operation was denied"}
    artifact = result.get("artifact")
    if expected_artifact and (artifact != expected_artifact or not result.get("artifactChanged")):
        return {"status": "incomplete", "reason": "native operation artifact mismatch"}
    return {"status": "observed", "command": expected_command, "artifact": artifact}


def evaluate_accepted_candidate(task: dict, candidate: Path, output: Path,
                                frozen_hashes: dict) -> dict:
    """Run the real-project evaluator at the candidate endpoint and persist its receipt."""
    result = evaluate_real_project(task, candidate, output, frozen_hashes)
    public = result.get("public", {})
    hidden = result.get("hidden", {})
    passed = (public.get("exit") == 0 and hidden.get("exit") == 0
              and not public.get("timeout") and not hidden.get("timeout")
              and public.get("cleanupConfirmed") and hidden.get("cleanupConfirmed"))
    receipt = {"schema": "native-methodology-evaluator/1", "taskId": task.get("id"),
               "status": "passed" if passed else "failed", "result": result,
               "candidate": str(candidate.resolve())}
    base.publish(output / "evaluator-receipt.json", receipt)
    return receipt


def fidelity_witnesses(witnesses: object, *, required_kinds: set[str] | None = None) -> dict:
    """Require observable native evidence; prose claiming fidelity is not evidence."""
    if not isinstance(witnesses, list):
        return {"status": "incomplete", "reason": "fidelity witnesses missing"}
    valid = []
    for row in witnesses:
        if not isinstance(row, dict) or row.get("kind") not in NATIVE_WITNESS_KINDS:
            continue
        path = Path(row.get("path", ""))
        if not path.is_file() or path.is_symlink() or not isinstance(row.get("sha256"), str):
            continue
        if base.digest(path) != row["sha256"]:
            continue
        if row["kind"] == "native_stream":
            text = path.read_text(encoding="utf-8", errors="replace")
            if not any(token in text for token in ("turn.completed", "tool_use", '"type": "result"')):
                continue
        valid.append({**row, "sha256": base.digest(path)})
    kinds = {row["kind"] for row in valid}
    required = required_kinds or {"native_stream", "native_skill"}
    if not valid or not required.issubset(kinds):
        return {"status": "incomplete", "reason": "native fidelity witness missing"}
    return {"status": "observed", "witnesses": valid}


def complete_attempt(attempt: dict, *, auth: bool, quota: bool, cleanup: bool,
                     usage: dict, witnesses: object, artifact_hashes_value: dict[str, str],
                     costs: dict[str, dict], required_costs: set[str] | None = None,
                     required_witness_kinds: set[str] | None = None) -> dict:
    """Close a phase only when every quality and accounting control is observed."""
    fidelity = fidelity_witnesses(witnesses, required_kinds=required_witness_kinds)
    cost_keys = set(costs) if isinstance(costs, dict) else set()
    usage_ok = isinstance(usage, dict) and usage.get("status") == "observed" and usage.get("total") is not None
    controls = {"auth": auth, "quota": quota, "cleanup": cleanup, "usage": usage_ok,
                "fidelity": fidelity["status"] == "observed",
                "costs": (required_costs or set()).issubset(cost_keys)}
    result = {**attempt, "artifactHashes": dict(sorted(artifact_hashes_value.items())), "fidelity": fidelity,
              "usage": usage, "costs": costs, "controls": controls,
              "status": "complete" if all(controls.values()) else "incomplete",
              "completeness": "observed" if all(controls.values()) else "unknown"}
    return result


def broker_phase(plan: dict, phase_id: str, attempt_id: str, *, runner: Callable[[dict, Path], dict],
                 root: Path, prior_artifacts: dict[str, str] | None = None) -> dict:
    """Run one native phase in a fresh directory and retain its raw result for later closure."""
    stage = root / f"phase-{phase_id}-{attempt_id}"
    stage.mkdir(parents=True, exist_ok=False)
    attempt = phase_attempt(plan, phase_id, attempt_id, stage_root=str(stage))
    result = runner(attempt, stage)
    if not isinstance(result, dict):
        raise ValueError("native phase runner returned no receipt")
    raw = stage / "native-result.json"
    base.publish(raw, result)
    artifact = artifact_hashes(stage, ["native-result.json"])
    if prior_artifacts and result.get("predecessorArtifacts") != prior_artifacts:
        result["status"] = "incomplete"
        result["error"] = "phase artifact hash continuity mismatch"
    return {**attempt, **result, "artifactHashes": artifact}


def execute_native_phase(plan: dict, phase_id: str, attempt_id: str, *, cli_config: dict,
                         prompt: str, root: Path, project_root: Path,
                         environment: dict[str, str] | None = None, ordinal: int = 0,
                         predecessor_artifacts: dict[str, str] | None = None) -> dict:
    """Execute one real native CLI phase, retaining stdout/stderr and native usage evidence."""
    executable = Path(cli_config["executable"])
    if base.identity(executable, cli_config["cli"], root) != cli_config["identity"]:
        raise ValueError("native CLI identity changed before phase")
    stage = root / f"phase-{phase_id}-{attempt_id}"
    stage.mkdir(parents=True, exist_ok=False)
    phase = next(row for row in plan["phases"] if row["id"] == phase_id)
    if _json_hash(prompt) != phase["promptSha256"]:
        raise ValueError("phase prompt changed after freeze")
    if predecessor_artifacts and not isinstance(predecessor_artifacts, dict):
        raise ValueError("predecessor artifact hashes are invalid")
    quota_before = _quota_observation(cli_config, project_root)
    if quota_before is not None and not quota_before.get("allowed"):
        raise ValueError("native quota guard refused phase before capture")
    attempt = phase_attempt(plan, phase_id, attempt_id, stage_root=str(stage))
    frozen_argv = list(cli_config["argv"]) + list(phase["command"])
    captured = base.capture(frozen_argv, project_root, deadline=int(cli_config.get("deadlineSeconds", 300)),
                            prompt=prompt, environment=environment,
                            graphhelm_overrides=cli_config.get("graphhelmEnv"))
    stdout = stage / "native.stdout.jsonl"
    stderr = stage / "native.stderr.log"
    stdout.write_text(captured["stdout"], encoding="utf-8")
    stderr.write_text(captured["stderr"], encoding="utf-8")
    parsed_usage = base.usage(cli_config["cli"], captured["stdout"])
    served_models = sorted(set(parsed_usage.get("observedModels", []))
                           | set(parsed_usage.get("accountingModels", [])))
    provider_error = base.native_provider_error(cli_config["cli"], captured["stdout"], captured["stderr"])
    quota_after = _quota_observation(cli_config, project_root, stdout=captured["stdout"])
    operations = {"status": "observed", "results": []}
    if phase.get("requiredOperations"):
        try:
            from native_session_evidence import required_operations
            operations = required_operations(stdout, cli=cli_config["cli"], requirements=phase["requiredOperations"])
        except (ImportError, OSError, ValueError, KeyError) as error:
            operations = {"status": "unknown", "reason": str(error)}
    required_globs = []
    required_glob_hashes = {}
    artifacts_observed = True
    for pattern in phase.get("requiredArtifactGlobs", []):
        matches = [path for path in project_root.glob(pattern) if path.is_file() and not path.is_symlink()]
        if not matches:
            artifacts_observed = False
        for path in matches:
            required_globs.append(str(path))
            required_glob_hashes[path.relative_to(project_root).as_posix()] = base.digest(path)
    archive_root = cli_config.get("archiveRoot")
    if not archive_root:
        raise ValueError("native session archive root is required")
    try:
        from native_session_capture import archive_phase
        archived = archive_phase(cli_config["cli"], stdout, project=project_root,
                                 archive=Path(archive_root) / f"{phase_id}-{attempt_id}",
                                 root_counts=parsed_usage.get("counts") or {},
                                 environment=base.child_environment(environment, cli_config.get("graphhelmEnv")))
    except (ImportError, OSError, ValueError, KeyError) as error:
        archived = {"status": "unknown", "reason": str(error)}
    root_wire_usage = parsed_usage
    archived_total = _usage_total(archived.get("total"), cli=cli_config["cli"]) if archived.get("status") == "observed" else None
    if archived_total is None:
        parsed_usage = {**parsed_usage, "counts": None, "accountingStatus": "archived-native-usage-unknown"}
    else:
        parsed_usage = {**parsed_usage, "counts": archived_total,
                        "rootWireUsage": root_wire_usage.get("counts"),
                        "countsSource": "native-session-archive.total", "accountingStatus": "observed"}
    skill_witnesses = [row for row in phase.get("requiredWitnesses", [])
                       if row.get("kind") == "native_skill"]
    skill_loads = []
    for skill_witness in skill_witnesses:
        try:
            from native_session_evidence import native_skill_load
            if not archived.get("archivedRoot"):
                raise ValueError("native session archive missing")
            loaded = native_skill_load(Path(archived["archivedRoot"]), cli=cli_config["cli"],
                                       expected_path=Path(skill_witness["path"]),
                                       expected_sha256=skill_witness["sha256"], frozen_prompt=phase["prompt"])
        except (ImportError, OSError, ValueError, KeyError) as error:
            loaded = {"status": "incomplete", "reason": str(error)}
        skill_loads.append(loaded)
    skill_load = {"status": "observed" if skill_witnesses and all(
        row.get("status") == "observed" for row in skill_loads) else "incomplete",
                  "loads": skill_loads}
    # archive_phase discovers and binds owned native sessions automatically;
    # caller-supplied session paths are intentionally not accepted.
    session_evidence = archived
    required = artifact_hashes(project_root, phase.get("requiredArtifacts", [])) if phase.get("requiredArtifacts") else {}
    receipt = {"attempt": attempt, "ordinal": ordinal, "argv": frozen_argv, "capture": {key: captured[key] for key in
               ("exit", "seconds", "timeout", "cleanupConfirmed", "truncated")},
               "usage": parsed_usage, "rootWireUsage": root_wire_usage, "cli": cli_config["cli"],
               "requestedModel": cli_config["model"], "servedModels": served_models,
               "sessionEvidence": session_evidence, "providerError": provider_error,
               "quotaBefore": quota_before, "quotaAfter": quota_after, "archivedSession": archived,
               "skillLoad": skill_load,
               "operations": operations, "requiredArtifactFiles": sorted(required_globs),
               "requiredArtifactGlobHashes": required_glob_hashes,
               "stdoutPath": stdout.name, "stderrPath": stderr.name,
               "rawSha256": {stdout.name: base.digest(stdout), stderr.name: base.digest(stderr)},
               "requiredArtifactHashes": required, "predecessorArtifacts": predecessor_artifacts}
    base.publish(stage / "phase-receipt.json", receipt)
    witnesses = [{"kind": "native_stream", "path": str(stdout), "sha256": base.digest(stdout)},
                 {"kind": "native_cli", "path": str(stage / "phase-receipt.json"),
                  "sha256": base.digest(stage / "phase-receipt.json")}]
    configured_witnesses = list(phase.get("requiredWitnesses", []))
    receipt["fidelity"] = fidelity_witnesses(witnesses + configured_witnesses + [
        {"kind": "native_artifact", "path": str(project_root / relative), "sha256": digest}
        for relative, digest in required.items()], required_kinds=set(phase["requiredWitnessKinds"]))
    native_complete = (not provider_error and captured["exit"] == 0 and not captured["timeout"]
                       and captured["cleanupConfirmed"] and not captured["truncated"]
                       and parsed_usage.get("counts") is not None
                       and served_models == [cli_config["model"]]
                       and receipt["fidelity"].get("status") == "observed"
                       and ("native_skill" not in phase["requiredWitnessKinds"]
                            or skill_load.get("status") == "observed")
                       and archived.get("status") == "observed"
                       and operations.get("status") == "observed" and artifacts_observed
                       and (quota_before is not None and quota_after is not None
                            and quota_before.get("allowed") is True and quota_after.get("allowed") is True))
    receipt["status"] = "complete" if native_complete else ("failed" if provider_error or captured["exit"] != 0 else "incomplete")
    return receipt


def _quota_observation(cli_config: dict, cwd: Path, *, stdout: str | None = None) -> dict | None:
    """Read the native allowance guard without mutating account state."""
    if cli_config.get("cli") != "codex":
        if stdout is not None:
            try:
                from native_quota import claude_subscription_observation
                return claude_subscription_observation(stdout)
            except (ImportError, ValueError, TypeError):
                return {"allowed": False, "reason": "native Claude quota observer unavailable"}
        try:
            status = base.capture([str(cli_config["executable"]), "auth", "status"], cwd,
                                  deadline=20, environment=None,
                                  graphhelm_overrides=cli_config.get("graphhelmEnv"))
            payload = json.loads(status["stdout"])
            allowed = (status["exit"] == 0 and payload.get("loggedIn") is True
                       and payload.get("authMethod") == "claude.ai"
                       and payload.get("apiProvider") == "firstParty")
            return {"allowed": allowed, "kind": "authentication-only",
                    "reason": "native Claude first-party authentication" if allowed
                    else "native Claude authentication unavailable"}
        except (OSError, ValueError, TypeError, json.JSONDecodeError, KeyError):
            return {"allowed": False, "kind": "authentication-only",
                    "reason": "native Claude authentication observer unavailable"}
    executable = cli_config.get("executable")
    if not executable:
        return None
    try:
        from native_quota import codex_allowance
        return codex_allowance(Path(executable), cwd, timeout=20, cutoff=80)
    except (ImportError, OSError, ValueError, subprocess.SubprocessError) as error:
        return {"allowed": False, "reason": "quota-observer-error", "detail": str(error)}


def _usage_total(value: object, *, cli: str | None = None) -> dict | None:
    """Normalize the archived native pipeline total without adding cache subsets."""
    if not isinstance(value, dict):
        return None
    aliases = {"input": ("input", "input_tokens"), "output": ("output", "output_tokens"),
               "cacheRead": ("cacheRead", "cache_read_input_tokens"),
               "cacheWrite": ("cacheWrite", "cache_creation_input_tokens")}
    result = {}
    for target, keys in aliases.items():
        found = None
        for key in keys:
            if key in value:
                found = value[key]
                break
        if type(found) is not int or found < 0:
            return None
        result[target] = found
    if cli != "claude" and result["cacheRead"] > result["input"]:
        return None
    return result


def _usage_metrics(counts: dict, *, cli: str) -> dict:
    """Derive provider-specific comparable totals without mixing raw cache semantics."""
    if cli == "codex":
        context = counts["input"]
    elif cli == "claude":
        context = counts["input"] + counts["cacheRead"] + counts["cacheWrite"]
    else:
        raise ValueError("unsupported native provider")
    return {"contextInputTokens": context,
            "totalProcessedTokens": context + counts["output"]}


def _codex_root_counters(envelope: object) -> dict:
    """Read Codex's root wire names before comparing them with archived cumulative usage."""
    if not isinstance(envelope, dict) or not isinstance(envelope.get("counts"), dict):
        raise ValueError("Codex root wire counters are missing")
    from native_session_evidence import counters
    return counters(envelope["counts"])


def freeze(registry_path: Path, *, cli_paths: dict[str, Path], models: dict[str, str], output: Path) -> dict:
    """Freeze registry, native executable identities, and phase order before any scored run."""
    registry = load_registry(registry_path)
    with tempfile.TemporaryDirectory() as directory:
        identity_map = {cli: base.identity(path, cli, Path(directory)) for cli, path in cli_paths.items()}
    workflows = registry["workflows"]
    source_names = ["methodology.py", "native_usage.py", "real_methodology.py", "native_methodology.py",
                    "native_session_evidence.py", "native_session_capture.py", "native_quota.py"]
    source_root = Path(__file__).resolve().parent
    source_hashes = {name: base.digest(source_root / name) for name in source_names if (source_root / name).is_file()}
    manifest = {"schema": "native-methodology-manifest/1", "registry": {"path": str(registry_path.resolve()),
                "sha256": base.digest(registry_path)}, "registryHash": _json_hash(registry),
                "workflows": workflows, "inputs": {row["id"]: frozen_inputs(row) for row in workflows},
                "identities": identity_map, "models": models,
                "instrumentSha256": base.digest(Path(__file__)), "instrumentSources": source_hashes,
                "plannedPhases":
                [{"workflow": row["id"], "phase": phase["id"],
                  "promptSha256": _json_hash(phase["prompt"]),
                  "requiredArtifacts": list(phase.get("requiredArtifacts", [])),
                  "requiredWitnesses": list(phase.get("requiredWitnesses", [])),
                  "requiredOperations": list(phase.get("requiredOperations", [])),
                  "requiredArtifactGlobs": list(phase.get("requiredArtifactGlobs", []))}
                 for row in workflows for phase in row["phases"]],
                "winner": None}
    base.publish(output, manifest)
    return manifest


def report_manifest(manifest_path: Path, attempts_root: Path, *, endpoint: dict | None = None) -> dict:
    """Read only receipts matching the frozen manifest; missing planned phases remain incomplete."""
    manifest = base.read(manifest_path)
    if manifest.get("instrumentSha256") != base.digest(Path(__file__)):
        raise ValueError("native instrument changed after freeze")
    source_root = Path(__file__).resolve().parent
    if any(not (source_root / name).is_file() or base.digest(source_root / name) != digest
           for name, digest in manifest.get("instrumentSources", {}).items()):
        raise ValueError("native instrument dependency changed after freeze")
    rows = []
    workflow_specs = {row["id"]: row for row in manifest.get("workflows", [])}
    context_integrity = {identifier: _public_tree_unchanged(
        manifest.get("inputs", {}).get(identifier, {}).get("publicTree"), workflow,
        Path(workflow["projectRoot"])) for identifier, workflow in workflow_specs.items()}
    candidate_chain = {}
    for planned in manifest.get("plannedPhases", []):
        key = f"{planned['workflow']}--{planned['phase']}"
        receipt = attempts_root / key / "phase-receipt.json"
        if not receipt.is_file():
            rows.append({**planned, "status": "incomplete"})
            continue
        row = base.read(receipt)
        workflow_spec = next((item for item in manifest.get("workflows", [])
                              if item.get("id") == planned["workflow"]), None)
        expected_plan = None
        if workflow_spec is not None:
            try:
                context = {**workflow_spec["taskContext"], "phase": planned["phase"]}
                expected_plan = phase_plan(workflow_spec, context,
                                           plan_id=f"{planned['workflow']}-frozen")["planSha256"]
            except (KeyError, TypeError, ValueError):
                expected_plan = None
        valid = (expected_plan is not None and isinstance(row.get("attempt"), dict)
                 and context_integrity.get(planned["workflow"]) is True
                 and row["attempt"].get("planSha256") == expected_plan
                 and row["attempt"].get("planId") == f"{planned['workflow']}-frozen"
                 and row["attempt"].get("phase") == planned["phase"]
                 and row.get("workflow") == planned["workflow"]
                 and row.get("phase") == planned["phase"]
                 and row.get("manifestSha256") == _json_hash(manifest)
                 and isinstance(row.get("capture"), dict)
                 and isinstance(row.get("rawSha256"), dict) and bool(row["rawSha256"])
                 and isinstance(row.get("candidateBeforeSha256"), str)
                 and isinstance(row.get("candidateAfterSha256"), str))
        if candidate_chain.get(planned["workflow"]) is not None:
            valid = valid and row.get("candidateBeforeSha256") == candidate_chain[planned["workflow"]]
        capture = row.get("capture", {})
        valid = valid and capture.get("exit") == 0 and capture.get("timeout") is False
        valid = valid and capture.get("cleanupConfirmed") is True and capture.get("truncated") is False
        phase_spec = next((phase for phase in workflow_spec.get("phases", [])
                           if phase.get("id") == planned["phase"]), None) if workflow_spec else None
        if planned["phase"] == "review" and workflow_spec:
            review_path = Path(workflow_spec["projectRoot"]) / ".native-observations/review-binding.json"
            try:
                binding = base.read(review_path)
                valid = valid and base.digest(review_path) == row.get("reviewBindingSha256")
                valid = valid and binding.get("baseHead") == workflow_spec.get("baselineHead")
                valid = valid and binding.get("taskId") == workflow_spec["taskRefs"][0]
                valid = valid and binding.get("reviewer") == "native-reviewer"
            except (OSError, ValueError, TypeError, KeyError):
                valid = False
        required_kinds = set(phase_spec.get("requiredWitnessKinds", ["native_stream"])) if phase_spec else set()
        if "native_skill" in required_kinds:
            expected_skills = [w for w in phase_spec.get("requiredWitnesses", [])
                               if w.get("kind") == "native_skill"] if phase_spec else []
            loads = row.get("skillLoad", {}).get("loads")
            valid = valid and bool(expected_skills) and isinstance(loads, list)
            valid = valid and len(loads) == len(expected_skills) and all(
                isinstance(load, dict) and load.get("status") == "observed"
                and load.get("skillSha256") == witness.get("sha256")
                for load, witness in zip(loads, expected_skills))
        if phase_spec and phase_spec.get("requiredOperations"):
            valid = valid and row.get("operations", {}).get("status") == "observed"
        valid = valid and row.get("quotaBefore", {}).get("allowed") is True
        valid = valid and row.get("quotaAfter", {}).get("allowed") is True
        requested_model = row.get("requestedModel")
        served_models = row.get("servedModels")
        valid = valid and isinstance(requested_model, str) and isinstance(served_models, list)
        valid = valid and requested_model == manifest.get("models", {}).get(row.get("cli"))
        valid = valid and served_models == [requested_model]
        usage_models = sorted(set(row.get("usage", {}).get("observedModels", []))
                              | set(row.get("usage", {}).get("accountingModels", [])))
        valid = valid and usage_models == [requested_model]
        attempt_id = row.get("attempt", {}).get("attemptId", "")
        raw_dir = attempts_root / f"phase-{planned['phase']}-{attempt_id}"
        valid = valid and set(row.get("rawSha256", {})) == {"native.stdout.jsonl", "native.stderr.log"}
        for raw_name, raw_hash in row.get("rawSha256", {}).items():
            raw_file = raw_dir / raw_name
            valid = valid and raw_file.is_file() and base.digest(raw_file) == raw_hash
        if phase_spec and phase_spec.get("requiredOperations"):
            try:
                from native_session_evidence import required_operations
                replayed_operations = required_operations(
                    raw_dir / "native.stdout.jsonl", cli=row.get("cli"),
                    requirements=phase_spec["requiredOperations"])
                valid = (valid and replayed_operations.get("status") == "observed"
                         and replayed_operations.get("sha256") == row.get("operations", {}).get("sha256"))
            except (OSError, ValueError, TypeError, KeyError):
                valid = False
        if row.get("status") in {"complete", "failed"}:
            valid = valid and row.get("fidelity", {}).get("status") == "observed" and isinstance(row.get("usage"), dict)
        archived_total = _usage_total(row.get("archivedSession", {}).get("total"), cli=row.get("cli"))
        recorded_total = _usage_total(row.get("usage", {}).get("counts"), cli=row.get("cli"))
        if archived_total is None or recorded_total != archived_total:
            valid = False
        archive = row.get("archivedSession", {})
        try:
            from native_session_evidence import (codex_tree, claude_pipeline, claude_owned_tree,
                                                 counters, records, verified_archive)
            cli = row.get("cli")
            project = Path(workflow_spec["projectRoot"]) if workflow_spec else None
            root_log, child_logs = verified_archive(archive, cli=cli)
            stdout_file = raw_dir / "native.stdout.jsonl"
            if not project or not project.is_dir() or not root_log.is_file() or not stdout_file.is_file():
                raise ValueError("archived native project is missing")
            replayed_wire = base.usage(cli, stdout_file.read_text(encoding="utf-8"))
            replayed_models = sorted(set(replayed_wire.get("observedModels", []))
                                     | set(replayed_wire.get("accountingModels", [])))
            valid = (valid and replayed_wire.get("accountingStatus") == "observed"
                     and replayed_wire.get("counts") == row.get("rootWireUsage", {}).get("counts")
                     and replayed_models == served_models
                     and replayed_wire.get("observedModels") == row.get("usage", {}).get("observedModels")
                     and replayed_wire.get("accountingModels", []) == row.get("usage", {}).get("accountingModels", []))
            if cli == "codex":
                parsed_archive = codex_tree(root_log, child_logs, project=project,
                                            root_thread=archive["rootSession"],
                                            stdout_usage=_codex_root_counters(row.get("rootWireUsage")))
            elif cli == "claude":
                native_rows = records(stdout_file)
                finals = [native_row for native_row in native_rows
                          if native_row.get("type") == "result"]
                root_rows = records(root_log)
                metadata = [native_row for native_row in root_rows
                            if native_row.get("type") in {"user", "assistant"}]
                if (len(finals) != 1 or finals[0].get("session_id") != archive["rootSession"]
                        or not metadata or any(
                            native_row.get("sessionId") != archive["rootSession"]
                            or native_row.get("version") != "2.1.310"
                            or native_row.get("isSidechain") is not False
                            or native_row.get("agentId") is not None
                            or not Path(native_row.get("cwd", "")).resolve().is_relative_to(project.resolve())
                            for native_row in metadata)):
                    raise ValueError("Claude archived root identity differs from native stream")
                if child_logs:
                    parsed_archive = claude_owned_tree(stdout_file, child_logs, project=project,
                                                       session_id=archive["rootSession"])
                else:
                    parsed_archive = claude_pipeline(stdout_file)
            else:
                raise ValueError("unsupported archived native provider")
            parsed_total = _usage_total(parsed_archive.get("total"), cli=cli)
            valid = valid and parsed_archive.get("status") == "observed" and parsed_total == archived_total
            if "native_skill" in required_kinds and phase_spec:
                from native_session_evidence import native_skill_load
                expected_skills = [w for w in phase_spec.get("requiredWitnesses", [])
                                   if w.get("kind") == "native_skill"]
                stated_loads = row.get("skillLoad", {}).get("loads", [])
                replayed_loads = [native_skill_load(root_log, cli=cli,
                    expected_path=Path(w["path"]), expected_sha256=w["sha256"],
                    frozen_prompt=phase_spec["prompt"]) for w in expected_skills]
                valid = valid and len(replayed_loads) == len(stated_loads) and all(
                    replayed.get("status") == "observed"
                    and replayed.get("skillSha256") == stated.get("skillSha256")
                    for replayed, stated in zip(replayed_loads, stated_loads))
        except (ImportError, KeyError, OSError, TypeError, ValueError):
                valid = False
        if workflow_spec is None:
            valid = False
        else:
            glob_hashes = row.get("requiredArtifactGlobHashes")
            for pattern in next((phase.get("requiredArtifactGlobs", [])
                                 for phase in workflow_spec["phases"] if phase["id"] == planned["phase"]), []):
                matches = [path for path in Path(workflow_spec["projectRoot"]).glob(pattern)
                           if path.is_file() and not path.is_symlink()]
                valid = valid and bool(matches)
                if isinstance(glob_hashes, dict):
                    valid = valid and all(glob_hashes.get(path.relative_to(Path(workflow_spec["projectRoot"])).as_posix())
                                          == base.digest(path) for path in matches)
                else:
                    valid = False
        rows.append({**planned, **row, "status": row.get("status", "incomplete") if valid else "incomplete"})
        if isinstance(row.get("candidateAfterSha256"), str):
            candidate_chain[planned["workflow"]] = row["candidateAfterSha256"]
    collection_complete = bool(rows) and len(rows) == len(manifest.get("plannedPhases", [])) and all(row["status"] != "incomplete" for row in rows)
    quality_accepted = collection_complete and all(row["status"] == "complete" for row in rows)
    complete = quality_accepted and all(row["status"] == "complete" for row in rows)
    workflow_ids = {row.get("workflow") for row in rows if row.get("workflow")}
    rebound = None
    if endpoint is not None and endpoint.get("status") == "observed":
        try:
            rebound = accepted_patch_endpoint(
                patch_path=endpoint["patchPath"], review_path=endpoint["reviewPath"],
                evaluator_path=endpoint["evaluatorPath"], author=endpoint["author"],
                reviewer=endpoint["reviewer"], local_git=True,
                reviewer_session_path=endpoint["reviewerSessionPath"],
                git_head_path=endpoint["gitHeadPath"], task_id=endpoint["taskId"])
        except (KeyError, OSError, ValueError, subprocess.SubprocessError):
            rebound = None
    endpoint_workflow = endpoint.get("workflow") if endpoint else None
    only_workflow_id = list(workflow_ids)[0] if len(workflow_ids) == 1 else None
    single_workflow = only_workflow_id is not None and endpoint_workflow in {None, only_workflow_id}
    expected_task = ""
    expected_project = None
    expected_candidate = None
    if single_workflow:
        spec = workflow_specs.get(only_workflow_id)
        expected_task = spec.get("taskContext", {}).get("task", "") if spec else ""
        expected_project = Path(spec["projectRoot"]).resolve() if spec else None
        if spec:
            try:
                expected_candidate = candidate_tree_hash(expected_project,
                                                         _declared_candidate_paths(spec, expected_project))
            except (OSError, ValueError):
                expected_candidate = None
    endpoint_project = Path(endpoint["gitHeadPath"]).resolve() if endpoint and endpoint.get("gitHeadPath") else None
    final_candidate = rows[-1].get("candidateAfterSha256") if rows else None
    endpoint_ok = (single_workflow and rebound is not None and rebound.get("status") == "observed"
                   and isinstance(endpoint.get("taskId"), str) and bool(endpoint["taskId"])
                   and endpoint.get("taskId") == expected_task
                   and endpoint_project == expected_project
                   and expected_candidate is not None
                   and final_candidate == expected_candidate
                   and isinstance(endpoint.get("gitHead"), str) and len(endpoint["gitHead"]) == 40
                   and isinstance(endpoint.get("patchSha256"), str) and len(endpoint["patchSha256"]) == 64
                   and isinstance(endpoint.get("reviewerSessionSha256"), str)
                   and isinstance(endpoint.get("reviewerRootSessionId"), str)
                   and isinstance(endpoint.get("authorRootSessionId"), str)
                   and endpoint["reviewerRootSessionId"] != endpoint["authorRootSessionId"]
                   and endpoint.get("evaluatorStatus") == "passed")
    # The independent reviewer must be an archived planned phase. Its counters
    # are already included in rows; an extra self-declared usage field would
    # either allow forged cost or count the same session twice.
    reviewer_rows = [row for row in rows if row.get("phase") == "review"
                     and row.get("status") == "complete"
                     and row.get("archivedSession", {}).get("rootSession") ==
                         (endpoint or {}).get("reviewerRootSessionId")
                     and row.get("rawSha256", {}).get("native.stdout.jsonl") ==
                         (endpoint or {}).get("reviewerSessionSha256")]
    endpoint_ok = endpoint_ok and len(reviewer_rows) == 1
    aggregate = {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}
    aggregate_known = bool(rows) and all(row.get("status") in {"complete", "failed"}
                                         for row in rows)
    accounted_raw_dirs = {f"phase-{row.get('phase')}-{row.get('attempt', {}).get('attemptId')}"
                          for row in rows if isinstance(row.get("attempt"), dict)}
    unplanned_attempt_dirs = sorted(path.name for path in attempts_root.glob("phase-*")
                                    if path.is_dir() and path.name not in accounted_raw_dirs)
    if unplanned_attempt_dirs:
        aggregate_known = False
        endpoint_ok = False
    usage_groups = {}
    for row in rows:
        cli = row.get("cli")
        counts = _usage_total(row.get("usage", {}).get("counts"), cli=cli)
        if counts is None:
            aggregate_known = False
            break
        for key in aggregate:
            aggregate[key] += counts[key]
        if cli not in {"codex", "claude"}:
            aggregate_known = False
            break
        workflow = row.get("workflow", "")
        served = row.get("servedModels")
        model = served[0] if isinstance(served, list) and len(served) == 1 else "unobserved"
        task = row.get("taskContext", {}).get("task") or workflow_specs.get(workflow, {}).get("taskContext", {}).get("task", "")
        group_key = json.dumps([cli, model, workflow, task], separators=(",", ":"))
        group = usage_groups.setdefault(group_key, {"cli": cli, "model": model,
            "workflow": workflow, "task": task, "raw": {"input": 0, "output": 0,
            "cacheRead": 0, "cacheWrite": 0}, "contextInputTokens": 0,
            "totalProcessedTokens": 0})
        for key in group["raw"]:
            group["raw"][key] += counts[key]
        metrics = _usage_metrics(counts, cli=cli)
        group["contextInputTokens"] += metrics["contextInputTokens"]
        group["totalProcessedTokens"] += metrics["totalProcessedTokens"]
    return {"schema": "native-methodology-report/2", "complete": complete and endpoint_ok,
            "collectionComplete": collection_complete, "qualityAccepted": quality_accepted, "attempts": rows,
            "usageAggregate": aggregate if aggregate_known else None,
            "usageGroups": list(usage_groups.values()) if aggregate_known else None,
            "acceptedEndpoint": endpoint, "winner": None,
            "unplannedAttemptDirs": unplanned_attempt_dirs,
            "limitations": [] if complete and endpoint_ok else
                (["unplanned native attempts require a complete token ledger"]
                 if unplanned_attempt_dirs else ["missing or incomplete native phase evidence"])}


def report_attempts(manifest: dict, attempts: list[dict]) -> dict:
    """Report phase outcomes without collapsing failed, incomplete, or unknown evidence."""
    planned = {(row["workflow"], row["phase"]) for row in manifest.get("plannedPhases", [])}
    observed = {(row.get("workflow"), row.get("phase")) for row in attempts}
    statuses = {row.get("status", "incomplete") for row in attempts}
    complete = bool(planned) and observed == planned and statuses <= {"complete"}
    return {"schema": "native-methodology-report/1", "workflow": manifest.get("workflow"),
            "complete": complete, "attempts": attempts, "winner": None,
            "limitations": [] if complete else ["Native fidelity, nested ownership, quota, cleanup, or usage evidence is incomplete"]}


def run_workflow(manifest: dict, *, workflow_id: str, cli_config: dict, output: Path,
                 prompt_override: str | None = None) -> list[dict]:
    """Execute every frozen phase once in order, preserving retries and predecessor hashes."""
    workflow = next(row for row in manifest["workflows"] if row["id"] == workflow_id)
    if frozen_inputs(workflow) != manifest["inputs"][workflow_id]:
        raise ValueError("frozen project or method assets changed")
    phases = workflow["phases"]
    output.mkdir(parents=True, exist_ok=True)
    results = []
    predecessor = None
    predecessor_candidate = None
    project_root = Path(workflow["projectRoot"])
    fix_attempts = 0
    for ordinal, phase in enumerate(phases):
        if phase["id"] == "fix":
            fix_attempts += 1
            if fix_attempts > workflow["maxFixAttempts"]:
                raise ValueError("workflow fix budget exceeded")
        key = f"{workflow_id}--{phase['id']}"
        destination = output / key
        receipt_path = destination / "phase-receipt.json"
        reservation = output / (key + ".reserved")
        if receipt_path.exists() or reservation.exists():
            raise ValueError(f"phase already reserved or completed: {key}")
        base.publish(reservation, {"workflow": workflow_id, "phase": phase["id"], "ordinal": ordinal,
                                   "manifestSha256": _json_hash(manifest)})
        task_context = workflow.get("taskContext", {"task": workflow_id + "-task",
            "scope": workflow["pinnedPublicFiles"], "publicTests": workflow["pinnedPublicFiles"],
            "request": prompt_override or phase["prompt"]})
        plan = phase_plan(workflow, {**task_context, "phase": phase["id"]},
                         plan_id=workflow_id + "-frozen")
        review_binding = _write_native_review_binding(workflow, project_root) if phase["id"] == "review" else None
        candidate_before = candidate_tree_hash(project_root, _declared_candidate_paths(workflow, project_root))
        if predecessor_candidate is not None and candidate_before != predecessor_candidate:
            raise ValueError("candidate changed outside a recorded native phase")
        receipt = execute_native_phase(plan, phase["id"], f"{ordinal:03d}", cli_config=cli_config,
                                      prompt=phase["prompt"], root=output,
                                      project_root=Path(workflow["projectRoot"]), ordinal=ordinal,
                                      predecessor_artifacts=predecessor)
        receipt["workflow"] = workflow_id
        receipt["phase"] = phase["id"]
        receipt["manifestSha256"] = _json_hash(manifest)
        receipt["candidateBeforeSha256"] = candidate_before
        receipt["candidateAfterSha256"] = candidate_tree_hash(project_root,
                                                               _declared_candidate_paths(workflow, project_root))
        if review_binding is not None:
            receipt["reviewBindingSha256"] = base.digest(review_binding)
        if phase["id"] == "review" and receipt["candidateAfterSha256"] != candidate_before:
            receipt["status"] = "incomplete"
            receipt["error"] = "independent reviewer mutated the candidate"
        destination.mkdir(parents=True, exist_ok=False)
        base.publish(receipt_path, receipt)
        predecessor = receipt.get("requiredArtifactHashes", {})
        predecessor_candidate = receipt["candidateAfterSha256"]
        results.append(receipt)
        if receipt.get("status") != "complete":
            break
    return results


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("qualify", "freeze", "run", "report"))
    parser.add_argument("--registry", type=Path)
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--workflow")
    parser.add_argument("--phase")
    parser.add_argument("--attempt")
    parser.add_argument("--cli")
    parser.add_argument("--executable", type=Path)
    parser.add_argument("--model")
    parser.add_argument("--prompt", default="")
    parser.add_argument("--patch")
    parser.add_argument("--review")
    parser.add_argument("--evaluator")
    parser.add_argument("--reviewer-session")
    parser.add_argument("--git-head")
    parser.add_argument("--task-id")
    parser.add_argument("--archive-root", type=Path)
    parser.add_argument("--author", default="native-author")
    parser.add_argument("--reviewer", default="native-reviewer")
    args = parser.parse_args()
    try:
        if args.command == "qualify":
            registry = load_registry(args.registry)
            evidence = []
            for workflow in registry["workflows"]:
                evidence.append({"workflow": workflow["id"], "projectRoot": workflow["projectRoot"],
                                 "inputs": frozen_inputs(workflow)})
            base.publish(args.output, {"schema": "native-methodology-qualification/2", "passed": bool(evidence),
                                       "registrySha256": base.digest(args.registry), "evidence": evidence})
            return 0
        if args.command == "freeze":
            if not all((args.registry, args.cli, args.executable, args.model)):
                raise ValueError("freeze requires registry, executable, cli and model")
            freeze(args.registry, cli_paths={args.cli: args.executable}, models={args.cli: args.model}, output=args.output)
            return 0
        if args.command == "report":
            endpoint = None
            if args.patch or args.review or args.evaluator:
                if not all((args.patch, args.review, args.evaluator)):
                    raise ValueError("endpoint requires patch, review and evaluator receipts")
                if not all((args.reviewer_session, args.git_head, args.task_id)):
                    raise ValueError("endpoint requires reviewer session, Git HEAD and task id")
                endpoint = accepted_patch_endpoint(patch_path=args.patch, review_path=args.review,
                    evaluator_path=args.evaluator, author=args.author, reviewer=args.reviewer, local_git=True,
                    reviewer_session_path=args.reviewer_session, git_head_path=args.git_head,
                    task_id=args.task_id)
            result = report_manifest(args.manifest, args.output, endpoint=endpoint)
            print(json.dumps(result, indent=2))
            return 0 if result["complete"] else 1
        if not all((args.manifest, args.workflow, args.cli, args.executable, args.model)):
            raise ValueError("run requires manifest, workflow, cli, executable and model")
        manifest = base.read(args.manifest)
        workflow = next(row for row in manifest["workflows"] if row["id"] == args.workflow)
        if frozen_inputs(workflow) != manifest["inputs"][args.workflow]:
            raise ValueError("frozen project or method assets changed")
        pinned_identity = manifest["identities"][args.cli]
        if str(args.executable.resolve()) != pinned_identity["path"] or args.model != manifest["models"][args.cli]:
            raise ValueError("native executable or model differs from frozen manifest")
        identity_record = pinned_identity
        config = native_cli_config(args.cli, Path(identity_record["path"]), manifest["models"][args.cli])
        config["identity"] = identity_record
        if not args.archive_root:
            raise ValueError("run requires private --archive-root outside the project")
        config["archiveRoot"] = str(args.archive_root.resolve())
        if not args.phase:
            results = run_workflow(manifest, workflow_id=args.workflow, cli_config=config, output=args.output,
                                   prompt_override=args.prompt or None)
            return 0 if (len(results) == len(workflow["phases"])
                         and all(row.get("status") == "complete" for row in results)) else 1
        task_context = workflow.get("taskContext", {"task": args.workflow + "-task",
            "scope": workflow["pinnedPublicFiles"], "publicTests": workflow["pinnedPublicFiles"],
            "request": args.prompt or "native workflow phase"})
        plan = phase_plan(workflow, {**task_context, "phase": args.phase},
                         plan_id=f"{args.workflow}-frozen")
        phase = next(row for row in plan["phases"] if row["id"] == args.phase)
        receipt = execute_native_phase(plan, args.phase, args.attempt, cli_config=config,
                                      prompt=phase["prompt"], root=args.output,
                                      project_root=Path(workflow["projectRoot"]), ordinal=0)
        destination = args.output / f"{args.workflow}--{args.phase}"
        destination.mkdir(parents=True, exist_ok=False)
        base.publish(destination / "phase-receipt.json", {"workflow": args.workflow, "phase": args.phase, **receipt})
        return 0 if receipt.get("status") == "complete" else 1
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        print(f"BENCHMARK_REFUSED: {error}", file=__import__("sys").stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
