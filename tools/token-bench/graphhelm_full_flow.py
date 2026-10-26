"""Audit a local GraphHelm JPD + Keel benchmark run from native receipts.

The manifest names private evidence files. This command reads them and the local
Runtime journal; it never launches a model, accesses an API, or clears a node.
"""
from __future__ import annotations

import argparse
from collections import Counter
import json
from pathlib import Path
import subprocess

import native_methodology

TOKEN_FIELDS = ("input", "output", "cacheRead", "cacheWrite")
NODES = ("select_plan", "implement_binding", "cli_proof", "independent_review", "local_patch_record")


def read(path: str | Path) -> dict:
    value = json.loads(Path(path).read_text(encoding="utf-8-sig"))
    if not isinstance(value, dict):
        raise ValueError(f"expected a JSON object: {path}")
    return value


def audit(manifest: dict) -> dict:
    """Fail closed on missing attempts, Runtime stages, signals, or patch provenance."""
    errors = []
    attempts = manifest.get("attempts")
    if not isinstance(attempts, list) or not attempts:
        raise ValueError("every native attempt must be listed")
    if manifest.get("provider") != "claude":
        raise ValueError("this full-flow audit accepts only Claude native usage semantics")
    attempts_root = Path(manifest["attemptsRoot"]).resolve()
    prefix = manifest["attemptPrefix"]
    if not isinstance(prefix, str) or not prefix:
        raise ValueError("attempt prefix is required")
    discovered = {path for directory in attempts_root.iterdir()
                  if directory.is_dir() and directory.name.startswith(prefix)
                  for path in (directory / "receipt.json",)}
    declared = {Path(item["receipt"]) for item in attempts}
    if discovered != declared:
        raise ValueError("native attempt inventory differs from the frozen receipt list")
    for path in declared:
        if (path.is_symlink() or path.parent.is_symlink() or path.parent.parent != attempts_root
                or not path.parent.name.startswith(prefix) or path.resolve() != path):
            raise ValueError("native receipt escapes or aliases the attempt directory")
    totals = dict.fromkeys(TOKEN_FIELDS, 0)
    root_sessions = set()
    attempt_rows = []
    for item in attempts:
        try:
            receipt = read(item["receipt"])
            root = receipt["archive"]["rootSession"]
            counts = receipt["archive"]["total"]
            if not root or root in root_sessions:
                raise ValueError("missing or duplicate native root session")
            root_sessions.add(root)
            if (receipt["wire"]["accountingStatus"] != "observed"
                    or receipt["wire"]["countsSource"] != "claude.result.modelUsage"
                    or receipt["archive"]["status"] != "observed"
                    or receipt["wire"]["counts"] != counts
                    or receipt["quota"]["allowed"] is not True
                    or receipt["quota"]["paidFallbackAuthorized"] is not False
                    or any(type(counts.get(key)) is not int or counts[key] < 0 for key in TOKEN_FIELDS)):
                raise ValueError("native usage or subscription boundary is unobserved")
            for key in TOKEN_FIELDS:
                totals[key] += counts[key]
            attempt_rows.append({"stage": item["stage"], "status": receipt["status"],
                                 "rootSession": root, "tokens": {key: counts[key] for key in TOKEN_FIELDS}})
        except (KeyError, OSError, TypeError, ValueError) as error:
            errors.append(f"attempt {item.get('stage', '?')}: {error}")
    stages = [row["stage"] for row in attempt_rows]
    if len(set(stages)) != len(stages):
        errors.append("duplicate attempt stage")
    if not {"planning", "author", "review"} <= {name.split("-", 1)[0] for name in stages}:
        errors.append("planning, author, and review receipts are required")

    try:
        saved_status = read(manifest["runtimeStatus"])["data"]
        command = [manifest["graphhelmCli"], "execution", "status", "--file",
                   manifest["runtimeGraph"], "--events", manifest["runtimeEvents"],
                   "--execution", manifest["executionId"], "--json"]
        live = subprocess.run(command, capture_output=True, text=True, timeout=30,
                              creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0), check=False)
        if live.returncode != 0:
            raise ValueError("GraphHelm could not replay the Runtime journal")
        envelope = json.loads(live.stdout)
        if envelope.get("ok") is not True:
            raise ValueError("GraphHelm refused the Runtime journal")
        status = envelope["data"]
        if (status.get("headSequence") != saved_status.get("headSequence")
                or status.get("nodeStates") != saved_status.get("nodeStates")):
            errors.append("saved Runtime status differs from live replay")
        if status["executionId"] != manifest["executionId"] or status["status"] != "completed":
            errors.append("Runtime execution did not complete")
        if any(status["nodeStates"].get(node) != "succeeded" for node in NODES):
            errors.append("a required Runtime node did not succeed")
        journal = Path(manifest["journal"])
        events = [event for line in journal.read_text(encoding="utf-8").splitlines()
                  for event in json.loads(line).get("events", [])]
        if not events or events[-1]["kind"]["type"] != "execution_completed":
            errors.append("Runtime journal lacks a terminal event")
        stream_ids = {event.get("streamId") for event in events}
        if len(stream_ids) != 1 or not next(iter(stream_ids)):
            errors.append("Runtime journal has mixed or missing stream identities")
        kind_ids_required = {"execution_started", "execution_form_declared", "node_outcome_recorded",
                             "signal_recorded", "completion_claimed", "completion_cleared",
                             "execution_completed"}
        if any(event.get("scope", {}).get("executionId") != manifest["executionId"]
               or (event.get("kind", {}).get("type") in kind_ids_required
                   and event.get("kind", {}).get("data", {}).get("executionId")
                   != manifest["executionId"])
               or ("executionId" in event.get("kind", {}).get("data", {})
                   and event["kind"]["data"]["executionId"] != manifest["executionId"])
               for event in events):
            errors.append("Runtime journal contains another execution")
        if ([event.get("sequence") for event in events] != list(range(1, len(events) + 1))
                or status.get("headSequence") != len(events)
                or events[-1].get("kind", {}).get("data", {}).get("status") != "completed"):
            errors.append("Runtime journal sequence or terminal status differs from replay")
        kinds = Counter(event["kind"]["type"] for event in events)
        if kinds["completion_claimed"] != len(NODES) or kinds["completion_cleared"] != len(NODES):
            errors.append("every Runtime node needs a claim and clearance")
        claims = {event["sequence"]: event["kind"]["data"]["node"] for event in events
                  if event["kind"]["type"] == "completion_claimed"}
        cleared = {event["kind"]["data"]["claimSeq"] for event in events
                   if event["kind"]["type"] == "completion_cleared"}
        if Counter(claims.values()) != Counter(NODES) or set(claims) != cleared:
            errors.append("Runtime claim and clearance do not cover each required node")
        signals = [event["kind"]["data"]["kind"] for event in events
                   if event["kind"]["type"] == "signal_recorded"]
        steps = read(manifest["journey"])["steps"]
        required = Counter({"jpd.journey": 1, "jpd.obligation": len(steps),
                            "keel.card": 1, "keel.proof": 1})
        if Counter(signals) != required or status["signalsRecorded"] != len(signals):
            errors.append("JPD/Keel Runtime signal thread is incomplete")
    except (KeyError, OSError, TypeError, ValueError, json.JSONDecodeError) as error:
        errors.append(f"Runtime evidence: {error}")

    try:
        endpoint = read(manifest["endpoint"])
        rebound = native_methodology.accepted_patch_endpoint(
            patch_path=endpoint["patchPath"], review_path=endpoint["reviewPath"],
            evaluator_path=endpoint["evaluatorPath"], author=endpoint["author"],
            reviewer=endpoint["reviewer"], local_git=True,
            reviewer_session_path=endpoint["reviewerSessionPath"],
            git_head_path=endpoint["gitHeadPath"], task_id=endpoint["taskId"])
        if rebound.get("status") != "observed" or rebound != endpoint:
            errors.append("review or hidden evaluator is not bound to exact Git patch")
        if endpoint["gitHead"] != manifest["candidateHead"]:
            errors.append("candidate head differs from accepted patch")
    except (KeyError, OSError, TypeError, ValueError, subprocess.SubprocessError) as error:
        errors.append(f"accepted endpoint: {error}")

    integration_complete = not errors
    comparison_eligible = (integration_complete and manifest.get("manualIntervention") is False
                           and manifest.get("prStageReviewed") is True
                           and manifest.get("reachedStatus") == "passed")
    return {"schema": "graphhelm-full-flow-audit/1", "integrationComplete": integration_complete,
            "comparisonEligible": comparison_eligible, "errors": errors,
            "attempts": attempt_rows, "tokens": totals,
            "totalProcessedTokens": sum(totals.values()) if len(attempt_rows) == len(attempts) else None,
            "limitations": [name for name, present in (
                ("manual controller repair", manifest.get("manualIntervention") is True),
                ("PR-stage review absent", manifest.get("prStageReviewed") is False),
                ("reached suite unobserved", manifest.get("reachedStatus") != "passed")) if present]}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = audit(read(args.manifest))
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({key: result[key] for key in ("integrationComplete", "comparisonEligible",
                                                    "totalProcessedTokens", "errors")}))
    return 0 if result["integrationComplete"] else 2


if __name__ == "__main__":
    raise SystemExit(main())
