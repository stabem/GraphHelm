"""Versioned native-framework workflow planning and fail-closed phase evidence."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
from typing import Callable, Iterable

import methodology as base


SCHEMA = "native-methodology-workflow/1"
PHASES = ("setup", "planner", "author", "independent-review", "fix", "verify", "archive")
PUBLIC_CONTEXT_KEYS = {"task", "scope", "publicTests", "request", "workflow", "phase"}
NATIVE_WITNESS_KINDS = {"native_stream", "native_cli", "native_artifact", "native_skill"}
REQUIRED_COSTS = {"setup", "planner", "author", "independent-review", "fix", "verify", "archive"}


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
        phases = workflow.get("phases")
        if not isinstance(phases, list) or not phases:
            raise ValueError("workflow phases are required")
        for phase in phases:
            _safe_id(phase.get("id"), "phase id")
            if phase["id"] not in PHASES:
                raise ValueError(f"unsupported phase: {phase['id']}")
            if not isinstance(phase.get("command"), list) or not phase["command"]:
                raise ValueError("each phase needs a native command")
    return registry


def load_registry(path: Path) -> dict:
    registry = base.read(path)
    return validate_registry(registry)


def native_cli_config(cli: str, executable: Path, model: str, *, project_doc: Path | None = None,
                      extra: list[str] | None = None) -> dict:
    """Build native CLI configuration; project settings and skills remain enabled."""
    if cli == "codex":
        argv = [str(executable), "exec", "--json", "--skip-git-repo-check", "-m", model, "-"]
    elif cli == "claude":
        argv = [str(executable), "-p", "--output-format", "stream-json", "--verbose", "--model", model]
    else:
        raise ValueError(f"unsupported native CLI: {cli}")
    if extra:
        argv.extend(extra)
    return {"cli": cli, "executable": str(executable.resolve()), "model": model,
            "argv": argv, "projectDoc": str(project_doc.resolve()) if project_doc else None,
            "projectSkills": "enabled", "safeMode": "native-default", "plugins": "native-default",
            "sessionPersistence": "enabled", "projectConfig": "native-default"}


def phase_plan(workflow: dict, context: dict, *, plan_id: str, predecessor: str | None = None) -> dict:
    """Create an immutable plan with public context and explicit framework-native phases."""
    validate_registry({"schema": SCHEMA, "version": workflow["version"], "workflows": [workflow]})
    public = validate_public_context({**context, "workflow": workflow["id"]})
    phases = [{"id": phase["id"], "command": list(phase["command"]),
               "frameworkStage": phase.get("frameworkStage", phase["id"])} for phase in workflow["phases"]]
    return {"schema": "native-methodology-plan/1", "planId": _safe_id(plan_id, "plan id"),
            "workflow": workflow["id"], "workflowVersion": workflow["version"],
            "predecessorPlan": predecessor, "maxFixAttempts": workflow["maxFixAttempts"],
            "context": public, "phases": phases,
            "planSha256": _json_hash({"workflow": workflow, "context": public, "phases": phases})}


def phase_attempt(plan: dict, phase_id: str, attempt_id: str, *, retry_of: str | None = None,
                  stage_root: str | None = None) -> dict:
    """Reserve one fresh phase stream; retry links point to a prior attempt and never replace it."""
    _safe_id(attempt_id, "attempt id")
    phase = next((item for item in plan["phases"] if item["id"] == phase_id), None)
    if phase is None:
        raise ValueError(f"phase is not in plan: {phase_id}")
    if retry_of == attempt_id:
        raise ValueError("an attempt cannot retry itself")
    return {"schema": "native-methodology-phase-attempt/1", "attemptId": attempt_id,
            "planId": plan["planId"], "planSha256": plan["planSha256"], "phase": phase_id,
            "frameworkStage": phase["frameworkStage"], "retryOf": retry_of,
            "stageRoot": stage_root, "status": "reserved", "artifactHashes": {},
            "fidelity": "unknown", "completeness": "unknown"}


def accepted_patch_endpoint(*, patch_path: str, patch_sha256: str, reviewed: bool,
                            local_git: bool, published: bool = False) -> dict:
    """Describe the independently reviewed local patch endpoint; publishing is never implied."""
    if not patch_path or not patch_sha256 or not reviewed or not local_git or published:
        return {"status": "incomplete", "reason": "accepted patch endpoint is not independently reviewed local Git"}
    return {"status": "observed", "patchPath": patch_path, "patchSha256": patch_sha256,
            "reviewed": True, "localGit": True, "published": False}


def reconcile_nested_usage(records: object, *, expected_sessions: list[str]) -> dict:
    """Reconcile owned native child records; absent sessions are unknown and block totals."""
    if not isinstance(records, list) or not all(isinstance(row, dict) for row in records):
        raise ValueError("nested usage records must be a list")
    by_id = {row.get("sessionId"): row for row in records if isinstance(row.get("sessionId"), str)}
    missing = sorted(set(expected_sessions) - set(by_id))
    invalid = sorted(row.get("sessionId", "") for row in records if row.get("usage") is None or row.get("owned") is not True)
    duplicates = len(by_id) != len(records)
    overlap = any(row.get("overlaps") for row in records)
    complete = not missing and not invalid and not duplicates and not overlap
    total = None
    if complete:
        total = {key: sum(row["usage"].get(key, 0) for row in records) for key in ("input", "output", "cacheRead", "cacheWrite")}
    return {"status": "observed" if complete else "unknown", "total": total,
            "missingSessions": missing, "invalidSessions": invalid,
            "duplicateSessions": duplicates, "overlap": overlap,
            "expectedSessions": sorted(expected_sessions)}


def fidelity_witnesses(witnesses: object) -> dict:
    """Require observable native evidence; prose claiming fidelity is not evidence."""
    if not isinstance(witnesses, list):
        return {"status": "incomplete", "reason": "fidelity witnesses missing"}
    valid = [row for row in witnesses if isinstance(row, dict) and row.get("kind") in NATIVE_WITNESS_KINDS
             and isinstance(row.get("path"), str) and row.get("sha256")]
    if not valid:
        return {"status": "incomplete", "reason": "native fidelity witness missing"}
    return {"status": "observed", "witnesses": valid}


def complete_attempt(attempt: dict, *, auth: bool, quota: bool, cleanup: bool,
                     usage: dict, witnesses: object, artifact_hashes_value: dict[str, str],
                     costs: dict[str, dict]) -> dict:
    """Close a phase only when every quality and accounting control is observed."""
    fidelity = fidelity_witnesses(witnesses)
    cost_keys = set(costs) if isinstance(costs, dict) else set()
    usage_ok = isinstance(usage, dict) and usage.get("status") == "observed" and usage.get("total") is not None
    controls = {"auth": auth, "quota": quota, "cleanup": cleanup, "usage": usage_ok,
                "fidelity": fidelity["status"] == "observed", "costs": REQUIRED_COSTS.issubset(cost_keys)}
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


def report_attempts(manifest: dict, attempts: list[dict]) -> dict:
    """Report phase outcomes without collapsing failed, incomplete, or unknown evidence."""
    statuses = {row.get("status", "incomplete") for row in attempts}
    complete = bool(attempts) and statuses <= {"complete"}
    return {"schema": "native-methodology-report/1", "workflow": manifest.get("workflow"),
            "complete": complete, "attempts": attempts, "winner": None,
            "limitations": [] if complete else ["Native fidelity, nested ownership, quota, cleanup, or usage evidence is incomplete"]}
