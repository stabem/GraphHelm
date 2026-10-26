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
    return {"projectRoot": str(project.resolve()), "publicFiles": public, "methodAssets": assets,
            "taskRefs": list(workflow["taskRefs"])}


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
               "frameworkStage": phase.get("frameworkStage", phase["id"]),
               "prompt": phase["prompt"], "promptSha256": _json_hash(phase["prompt"]),
               "requiredArtifacts": list(phase.get("requiredArtifacts", [])),
               "requiredWitnessKinds": list(phase.get("requiredWitnessKinds", ["native_stream", "native_skill"])),
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
                            author: str, reviewer: str, local_git: bool, published: bool = False) -> dict:
    """Verify an independent local review and evaluator receipt; booleans cannot self-attest."""
    patch = Path(patch_path)
    review = Path(review_path)
    evaluator = Path(evaluator_path)
    if published or not local_git or not patch.is_file() or not review.is_file() or not evaluator.is_file():
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
    attempt = phase_attempt(plan, phase_id, attempt_id, stage_root=str(stage))
    frozen_argv = list(cli_config["argv"]) + list(phase["command"])
    captured = base.capture(frozen_argv, project_root, deadline=int(cli_config.get("deadlineSeconds", 300)),
                            prompt=prompt, environment=environment)
    stdout = stage / "native.stdout.jsonl"
    stderr = stage / "native.stderr.log"
    stdout.write_text(captured["stdout"], encoding="utf-8")
    stderr.write_text(captured["stderr"], encoding="utf-8")
    parsed_usage = base.usage(cli_config["cli"], captured["stdout"])
    provider_error = base.native_provider_error(cli_config["cli"], captured["stdout"], captured["stderr"])
    required = artifact_hashes(project_root, phase.get("requiredArtifacts", [])) if phase.get("requiredArtifacts") else {}
    receipt = {"attempt": attempt, "ordinal": ordinal, "argv": frozen_argv, "capture": {key: captured[key] for key in
               ("exit", "seconds", "timeout", "cleanupConfirmed", "truncated")},
               "usage": parsed_usage, "providerError": provider_error,
               "stdoutPath": stdout.name, "stderrPath": stderr.name,
               "rawSha256": {stdout.name: base.digest(stdout), stderr.name: base.digest(stderr)},
               "requiredArtifactHashes": required, "predecessorArtifacts": predecessor_artifacts}
    base.publish(stage / "phase-receipt.json", receipt)
    witnesses = [{"kind": "native_stream", "path": str(stdout), "sha256": base.digest(stdout)},
                 {"kind": "native_cli", "path": str(stage / "phase-receipt.json"),
                  "sha256": base.digest(stage / "phase-receipt.json")}]
    receipt["fidelity"] = fidelity_witnesses(witnesses + [
        {"kind": "native_artifact", "path": str(project_root / relative), "sha256": digest}
        for relative, digest in required.items()], required_kinds=set(phase["requiredWitnessKinds"]))
    receipt["status"] = "failed" if provider_error or captured["exit"] != 0 else "incomplete"
    if captured["timeout"] or not captured["cleanupConfirmed"] or captured["truncated"]:
        receipt["status"] = "incomplete"
    return receipt


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
                  "requiredArtifacts": list(phase.get("requiredArtifacts", []))}
                 for row in workflows for phase in row["phases"]],
                "winner": None}
    base.publish(output, manifest)
    return manifest


def report_manifest(manifest_path: Path, attempts_root: Path) -> dict:
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
        raw_dir = receipt.parent.parent
        for raw_name, raw_hash in row.get("rawSha256", {}).items():
            raw_file = raw_dir / raw_name
            valid = valid and raw_file.is_file() and base.digest(raw_file) == raw_hash
        if row.get("status") in {"complete", "failed"}:
            valid = valid and row.get("fidelity", {}).get("status") == "observed" and isinstance(row.get("usage"), dict)
        rows.append({**planned, **row, "status": row.get("status", "incomplete") if valid else "incomplete"})
    collection_complete = bool(rows) and len(rows) == len(manifest.get("plannedPhases", [])) and all(row["status"] != "incomplete" for row in rows)
    quality_accepted = collection_complete and all(row["status"] in {"complete", "failed"} for row in rows)
    complete = quality_accepted and all(row["status"] == "complete" for row in rows)
    return {"schema": "native-methodology-report/2", "complete": complete,
            "collectionComplete": collection_complete, "qualityAccepted": quality_accepted, "attempts": rows,
            "winner": None, "limitations": [] if complete else ["missing or incomplete native phase evidence"]}


def report_attempts(manifest: dict, attempts: list[dict]) -> dict:
    """Report phase outcomes without collapsing failed, incomplete, or unknown evidence."""
    planned = {(row["workflow"], row["phase"]) for row in manifest.get("plannedPhases", [])}
    observed = {(row.get("workflow"), row.get("phase")) for row in attempts}
    statuses = {row.get("status", "incomplete") for row in attempts}
    complete = bool(planned) and observed == planned and statuses <= {"complete"}
    return {"schema": "native-methodology-report/1", "workflow": manifest.get("workflow"),
            "complete": complete, "attempts": attempts, "winner": None,
            "limitations": [] if complete else ["Native fidelity, nested ownership, quota, cleanup, or usage evidence is incomplete"]}


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
            result = report_manifest(args.manifest, args.output)
            print(json.dumps(result, indent=2))
            return 0 if result["complete"] else 1
        if not all((args.manifest, args.workflow, args.phase, args.attempt, args.cli, args.executable, args.model)):
            raise ValueError("run requires manifest, workflow, phase, attempt, cli, executable and model")
        manifest = base.read(args.manifest)
        workflow = next(row for row in manifest["workflows"] if row["id"] == args.workflow)
        if frozen_inputs(workflow) != manifest["inputs"][args.workflow]:
            raise ValueError("frozen project or method assets changed")
        pinned_identity = manifest["identities"][args.cli]
        if str(args.executable.resolve()) != pinned_identity["path"] or args.model != manifest["models"][args.cli]:
            raise ValueError("native executable or model differs from frozen manifest")
        plan = phase_plan(workflow, {"task": args.workflow + "-task", "scope": workflow["pinnedPublicFiles"], "publicTests": workflow["pinnedPublicFiles"],
                                    "request": args.prompt or "native workflow phase", "phase": args.phase},
                         plan_id=f"{args.workflow}-frozen")
        identity_record = pinned_identity
        config = native_cli_config(args.cli, Path(identity_record["path"]), manifest["models"][args.cli])
        config["identity"] = identity_record
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
