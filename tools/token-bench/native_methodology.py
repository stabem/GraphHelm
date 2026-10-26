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
        phases = workflow.get("phases")
        if not isinstance(phases, list) or not phases:
            raise ValueError("workflow phases are required")
        phase_ids = set()
        for phase in phases:
            _safe_id(phase.get("id"), "phase id")
            if phase["id"] in phase_ids:
                raise ValueError("phase ids must be unique within a workflow")
            phase_ids.add(phase["id"])
            if not isinstance(phase.get("command"), list) or not phase["command"]:
                raise ValueError("each phase needs a native command")
            if not isinstance(phase.get("prompt"), str) or not phase["prompt"].strip():
                raise ValueError("each phase needs a frozen prompt")
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
    return {"projectRoot": str(project.resolve()), "publicFiles": public, "methodAssets": assets,
            "taskRefs": list(workflow["taskRefs"]), "requiredWitnesses": witnesses}


def candidate_tree_hash(root: Path) -> str:
    """Hash the evolving project candidate without including its Git metadata."""
    root = root.resolve()
    entries = []
    for path in sorted(root.rglob("*")):
        if not path.is_file() or ".git" in path.parts or path.is_symlink():
            continue
        relative = path.relative_to(root).as_posix()
        entries.append((relative, base.digest(path)))
    return _json_hash(entries)


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
               "requiredWitnessKinds": list(phase.get("requiredWitnessKinds", ["native_stream", "native_skill"])),
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
    if (published or not local_git or not patch.is_file() or not review.is_file() or not evaluator.is_file()
            or (reviewer_session is not None and not reviewer_session.is_file())
            or (git_head is not None and not git_head.is_file())):
        return {"status": "incomplete", "reason": "accepted patch endpoint is not independently reviewed local Git"}
    try:
        review_data = base.read(review)
        evaluator_data = base.read(evaluator)
    except (OSError, ValueError, json.JSONDecodeError):
        return {"status": "incomplete", "reason": "review or evaluator receipt is invalid"}
    patch_sha = base.digest(patch)
    if (review_data.get("verdict") not in {"APPROVE", "APPROVE-WITH-RISK"}
            or review_data.get("reviewer") != reviewer or reviewer == author
            or review_data.get("patchSha256") != patch_sha
            or evaluator_data.get("status") != "passed"):
        return {"status": "incomplete", "reason": "review/evaluator receipt is not bound to this patch"}
    if task_id is not None and (review_data.get("taskId") != task_id or evaluator_data.get("taskId") != task_id):
        return {"status": "incomplete", "reason": "review/evaluator task mismatch"}
    if reviewer_session is not None:
        if review_data.get("reviewerSessionSha256") != base.digest(reviewer_session):
            return {"status": "incomplete", "reason": "review session is not bound to verdict"}
    if git_head is not None:
        if git_head.read_text(encoding="utf-8").strip() != patch_sha:
            return {"status": "incomplete", "reason": "local Git head is not bound to accepted patch"}
    return {"status": "observed", "patchPath": str(patch), "patchSha256": patch_sha,
            "reviewPath": str(review), "evaluatorPath": str(evaluator), "published": False}


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
    provider_error = base.native_provider_error(cli_config["cli"], captured["stdout"], captured["stderr"])
    quota_after = _quota_observation(cli_config, project_root, stdout=captured["stdout"])
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
    session_evidence = None
    evidence_path = cli_config.get("sessionEvidencePath")
    if evidence_path:
        try:
            from native_session_evidence import claude_pipeline, codex_tree
            if cli_config["cli"] == "claude":
                session_evidence = claude_pipeline(Path(evidence_path))
            else:
                session_evidence = codex_tree(Path(evidence_path),
                    [Path(item) for item in cli_config.get("childSessionEvidence", [])],
                    project=project_root, root_thread=cli_config.get("rootThread", ""),
                    stdout_usage=parsed_usage.get("counts") or {})
            if session_evidence.get("status") != "observed":
                parsed_usage = {**parsed_usage, "counts": None, "accountingStatus": "native-session-evidence-unknown"}
        except (ImportError, OSError, ValueError, KeyError) as error:
            session_evidence = {"status": "unknown", "reason": str(error)}
            parsed_usage = {**parsed_usage, "counts": None, "accountingStatus": "native-session-evidence-unknown"}
    required = artifact_hashes(project_root, phase.get("requiredArtifacts", [])) if phase.get("requiredArtifacts") else {}
    receipt = {"attempt": attempt, "ordinal": ordinal, "argv": frozen_argv, "capture": {key: captured[key] for key in
               ("exit", "seconds", "timeout", "cleanupConfirmed", "truncated")},
               "usage": parsed_usage, "sessionEvidence": session_evidence, "providerError": provider_error,
               "quotaBefore": quota_before, "quotaAfter": quota_after, "archivedSession": archived,
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
                       and receipt["fidelity"].get("status") == "observed"
                       and archived.get("status") == "observed"
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
        return cli_config.get("quotaObservation")
    executable = cli_config.get("executable")
    if not executable:
        return None
    try:
        from native_quota import codex_allowance
        return codex_allowance(Path(executable), cwd, timeout=20, cutoff=80)
    except (ImportError, OSError, ValueError, subprocess.SubprocessError) as error:
        return {"allowed": False, "reason": "quota-observer-error", "detail": str(error)}


def freeze(registry_path: Path, *, cli_paths: dict[str, Path], models: dict[str, str], output: Path) -> dict:
    """Freeze registry, native executable identities, and phase order before any scored run."""
    registry = load_registry(registry_path)
    with tempfile.TemporaryDirectory() as directory:
        identity_map = {cli: base.identity(path, cli, Path(directory)) for cli, path in cli_paths.items()}
    workflows = registry["workflows"]
    manifest = {"schema": "native-methodology-manifest/1", "registry": {"path": str(registry_path.resolve()),
                "sha256": base.digest(registry_path)}, "registryHash": _json_hash(registry),
                "workflows": workflows, "inputs": {row["id"]: frozen_inputs(row) for row in workflows},
                "identities": identity_map, "models": models,
                "instrumentSha256": base.digest(Path(__file__)), "plannedPhases":
                [{"workflow": row["id"], "phase": phase["id"],
                  "promptSha256": _json_hash(phase["prompt"]),
                  "requiredArtifacts": list(phase.get("requiredArtifacts", [])),
                  "requiredWitnesses": list(phase.get("requiredWitnesses", []))}
                 for row in workflows for phase in row["phases"]],
                "winner": None}
    base.publish(output, manifest)
    return manifest


def report_manifest(manifest_path: Path, attempts_root: Path, *, endpoint: dict | None = None) -> dict:
    """Read only receipts matching the frozen manifest; missing planned phases remain incomplete."""
    manifest = base.read(manifest_path)
    if manifest.get("instrumentSha256") != base.digest(Path(__file__)):
        raise ValueError("native instrument changed after freeze")
    rows = []
    for planned in manifest.get("plannedPhases", []):
        key = f"{planned['workflow']}--{planned['phase']}"
        receipt = attempts_root / key / "phase-receipt.json"
        if not receipt.is_file():
            rows.append({**planned, "status": "incomplete"})
            continue
        row = base.read(receipt)
        valid = isinstance(row.get("attempt"), dict) and row["attempt"].get("planSha256") and isinstance(row.get("capture"), dict)
        attempt_id = row.get("attempt", {}).get("attemptId", "")
        raw_dir = attempts_root / f"phase-{planned['phase']}-{attempt_id}"
        for raw_name, raw_hash in row.get("rawSha256", {}).items():
            raw_file = raw_dir / raw_name
            valid = valid and raw_file.is_file() and base.digest(raw_file) == raw_hash
        if row.get("status") in {"complete", "failed"}:
            valid = valid and row.get("fidelity", {}).get("status") == "observed" and isinstance(row.get("usage"), dict)
        rows.append({**planned, **row, "status": row.get("status", "incomplete") if valid else "incomplete"})
    collection_complete = bool(rows) and len(rows) == len(manifest.get("plannedPhases", [])) and all(row["status"] != "incomplete" for row in rows)
    quality_accepted = collection_complete and all(row["status"] == "complete" for row in rows)
    complete = quality_accepted and all(row["status"] == "complete" for row in rows)
    endpoint_ok = endpoint is not None and endpoint.get("status") == "observed"
    return {"schema": "native-methodology-report/2", "complete": complete and endpoint_ok,
            "collectionComplete": collection_complete, "qualityAccepted": quality_accepted, "attempts": rows,
            "acceptedEndpoint": endpoint, "winner": None,
            "limitations": [] if complete and endpoint_ok else ["missing or incomplete native phase evidence"]}


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
        candidate_before = candidate_tree_hash(Path(workflow["projectRoot"]))
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
        receipt["candidateAfterSha256"] = candidate_tree_hash(Path(workflow["projectRoot"]))
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
                endpoint = accepted_patch_endpoint(patch_path=args.patch, review_path=args.review,
                    evaluator_path=args.evaluator, author=args.author, reviewer=args.reviewer, local_git=True)
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
        task_context = workflow.get("taskContext", {"task": args.workflow + "-task",
            "scope": workflow["pinnedPublicFiles"], "publicTests": workflow["pinnedPublicFiles"],
            "request": args.prompt or "native workflow phase"})
        plan = phase_plan(workflow, {**task_context, "phase": args.phase},
                         plan_id=f"{args.workflow}-frozen")
        identity_record = pinned_identity
        config = native_cli_config(args.cli, Path(identity_record["path"]), manifest["models"][args.cli])
        config["identity"] = identity_record
        if not args.archive_root:
            raise ValueError("run requires private --archive-root outside the project")
        config["archiveRoot"] = str(args.archive_root.resolve())
        if not args.phase:
            results = run_workflow(manifest, workflow_id=args.workflow, cli_config=config, output=args.output,
                                   prompt_override=args.prompt or None)
            return 0 if results and all(row.get("status") != "incomplete" for row in results) else 1
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
