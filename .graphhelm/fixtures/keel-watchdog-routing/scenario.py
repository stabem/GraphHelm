#!/usr/bin/env python3
"""Route a task by a Keel check verdict through the real GraphHelm CLI (#1045).

The five user steps of the keel-watchdog-routing journey, against a temporary local event store
and keyring (never the shared Runtime): start a task, check an out-of-scope diff, see the task
routed back with the finding, correct it, see the review route ready.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
TIMEOUT_SECONDS = 180
# A fixture key for the temporary keyring only; it seals nothing outside this scenario.
FIXTURE_KEY = "01" * 32
EXECUTION = "exec_feature"
ROOT = Path(__file__).resolve().parents[3]
GRAPH = ROOT / "examples" / "graphs" / "software-feature.yaml"


def run_git(cwd: Path, *args: str) -> str:
    completed = subprocess.run(
        ["git", "-c", "user.name=keel-fixture", "-c", "user.email=keel-fixture@invalid",
         "-c", "commit.gpgsign=false", *args],
        cwd=cwd, check=True, text=True, capture_output=True, encoding="utf-8",
        errors="replace", timeout=30, creationflags=NO_WINDOW,
    )
    return completed.stdout.strip()


def write(repo: Path, path: str, body: str) -> None:
    full = repo / path
    full.parent.mkdir(parents=True, exist_ok=True)
    full.write_text(body, encoding="utf-8")


def cli(binary: Path, *args: str, expect: tuple[int, ...] = (0,), keyed: bool = False) -> dict:
    env = os.environ.copy()
    env.pop("GRAPHHELM_EVENTS_KEY", None)
    if keyed:
        env["GRAPHHELM_EVENTS_KEY"] = FIXTURE_KEY
    completed = subprocess.run(
        [str(binary), "--json", *args], env=env, text=True, capture_output=True,
        encoding="utf-8", errors="replace", timeout=TIMEOUT_SECONDS, creationflags=NO_WINDOW,
    )
    try:
        envelope = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(
            f"{args[:2]} returned no JSON (exit {completed.returncode}): "
            f"{completed.stdout[-800:]} {completed.stderr[-800:]}"
        ) from error
    if completed.returncode not in expect:
        raise RuntimeError(f"{args[:2]} exit {completed.returncode}: {envelope}")
    return envelope


def card(path: Path, scope: list[str]) -> Path:
    path.write_text(json.dumps({
        "promise": "added() exists", "scopePaths": scope, "proof": "cargo test",
        "exportedSymbols": ["added"],
    }), encoding="utf-8")
    return path


def draft(draft_id: str, version: int, hash_field: str, hash_value: str, node: str) -> dict:
    return {
        "id": draft_id, "expectedVersion": version, hash_field: hash_value,
        "operations": [{
            "op": "addNode", "path": f"/spec/nodes/{node}",
            "value": {
                "type": "agent", "name": node, "objective": "Correct the blocked scope",
                "optionality": "required",
                "agent": {"ephemeral": {
                    "purpose": "Correct the blocked scope",
                    "capabilities": ["repository.read"],
                    "allowedTools": ["repository.read"],
                    "prohibitedActions": ["repository.write"],
                    "inputSchema": "schema://ImplementationResult@1",
                    "outputSchema": "schema://ImplementationResult@1",
                    "instructions": "Correct the blocked scope",
                    "completionContract": {"requires": ["source_locations"]},
                    "isolationMinimum": "tier_0",
                }},
                "completion": {"requires": [{"outputSchemaValid": True}]},
                "timeoutSeconds": 900,
            },
        }],
    }


def run_scenario() -> dict:
    cli_value = os.environ.get("GRAPHHELM_CLI") or "graphhelm"
    cli_path = shutil.which(cli_value)
    if cli_path is None:
        raise RuntimeError(f"GraphHelm CLI not found: {cli_value}")
    binary = Path(cli_path).resolve()

    with tempfile.TemporaryDirectory(prefix="scenario-", dir=Path(__file__).resolve().parent) as name:
        scratch = Path(name)
        events, keyring, repo = scratch / "events", scratch / "keyring", scratch / "repo"
        keyring.mkdir()
        repo.mkdir()
        store = ["--events", str(events), "--execution", EXECUTION]
        keys = ["--keyring", str(keyring), "--key-id", "route-key"]

        # Step 1: start a task whose card names its scope.
        cli(binary, "gateway", "keyring", "init", "--keyring", str(keyring),
            "--key-id", "route-key", keyed=True)
        fixtures = scratch / "fixtures.json"
        fixtures.write_text('{"nodeOutcomes":{}}', encoding="utf-8")
        cli(binary, "execution", "start", "--file", str(GRAPH), "--events", str(events),
            "--fixtures", str(fixtures), *keys, "--mode", "supervised",
            "--execution", EXECUTION, "--held", keyed=True)
        graph_hash = cli(binary, "graph", "hash", str(GRAPH))["data"]["hash"]

        # Step 2: submit a diff with a path outside the card.
        run_git(repo, "init", "-q")
        write(repo, "src/lib.rs", "pub fn old() {}\n")
        run_git(repo, "add", ".")
        run_git(repo, "commit", "-q", "-m", "base")
        write(repo, "src/lib.rs", "pub fn old() {}\npub fn added() {}\n")
        write(repo, "docs/notes.md", "outside scope\n")
        run_git(repo, "add", ".")
        run_git(repo, "commit", "-q", "-m", "change")
        head = run_git(repo, "rev-parse", "HEAD")
        binding = scratch / "binding.json"
        binding.write_text(json.dumps({
            "expectedHead": head, "execution": EXECUTION, "task": "issue-1045", "node": "plan",
            "implementDraft": draft("draft-implement", 1, "expectedHash", graph_hash,
                                    "route-correction"),
            "reviewDraft": draft("draft-review", 1, "expectedHash", graph_hash, "route-review"),
        }), encoding="utf-8")
        blocked = cli(binary, "keel", "check", "--diff", "HEAD~1..HEAD", "--repo", str(repo),
                      "--card", str(card(scratch / "card.json", ["src/lib.rs"])), *store, *keys,
                      "--route-proposal", str(binding), expect=(2,), keyed=True)
        finding = blocked["data"].get("routeProposal") or {}
        if finding.get("kind") != "keel.blocked":
            raise AssertionError(f"blocked check did not route: {blocked}")
        rules = sorted(({d.get("code") for d in blocked.get("diagnostics", [])} |
                       {f.get("rule") for f in blocked["data"].get("findings", [])
                        if isinstance(f, dict)}) - {None})
        if "keel.scope.path_outside_card" not in rules:
            raise AssertionError(f"blocked check does not cite the scope finding: {blocked}")

        # Step 3: status shows the task routed back to implement, waiting on its approval.
        status = cli(binary, "execution", "status", *store)["data"]
        pending = [r.get("draft_id") for r in status["attentionReasons"]
                   if r.get("kind") == "pending_draft"]
        if pending != ["draft-implement"]:
            raise AssertionError(f"routed-back draft not pending: {status}")
        cli(binary, "execution", "approve", *store, "--node", "route-correction", *keys,
            "--actor-id", "route-agent",
            "--proposal-digest", status["proposalDigests"]["draft-implement"],
            "--draft-id", "draft-implement", keyed=True)
        routed = cli(binary, "execution", "status", *store)["data"]["nodeStates"]
        if routed.get("route-correction") != "ready" or routed.get("route-review") is not None:
            raise AssertionError(f"task was not routed back to implement: {routed}")
        published = cli(binary, "graph", "replay", "--events", str(events))["data"]["currentGraph"]

        # Step 4: fix the scope and resubmit.
        write(repo, "docs/notes.md", "corrected scope\n")
        run_git(repo, "add", ".")
        run_git(repo, "commit", "-q", "-m", "correct scope")
        binding.write_text(json.dumps({
            "expectedHead": run_git(repo, "rev-parse", "HEAD"), "execution": EXECUTION,
            "task": "issue-1045", "node": "route-correction",
            "implementDraft": draft("draft-implement-next", published["number"],
                                    "expectedSafeHash", published["semanticHash"],
                                    "route-correction-next"),
            "reviewDraft": draft("draft-review", published["number"], "expectedSafeHash",
                                 published["semanticHash"], "route-review"),
        }), encoding="utf-8")
        passed = cli(binary, "keel", "check", "--diff", "HEAD~1..HEAD", "--repo", str(repo),
                     "--card", str(card(scratch / "card-fixed.json",
                                        ["src/lib.rs", "docs/notes.md"])),
                     *store, *keys, "--route-proposal", str(binding), keyed=True)
        if (passed["data"].get("routeProposal") or {}).get("kind") != "keel.passed":
            raise AssertionError(f"corrected check did not pass: {passed}")

        # Step 5: status shows the task advanced to review.
        status = cli(binary, "execution", "status", *store)["data"]
        cli(binary, "execution", "approve", *store, "--node", "route-review", *keys,
            "--actor-id", "route-agent",
            "--proposal-digest", status["proposalDigests"]["draft-review"],
            "--draft-id", "draft-review", keyed=True)
        final = cli(binary, "execution", "status", *store)["data"]["nodeStates"]
        if final.get("route-review") != "ready":
            raise AssertionError(f"task did not advance to review: {final}")

        return {
            "status": "passed",
            "summary": "blocked=routed-to-implement; corrected=advanced-to-review",
            "blockedFindings": rules,
            "graphVersions": [1, published["number"], published["number"] + 1],
        }


if __name__ == "__main__":
    try:
        print(json.dumps(run_scenario(), indent=2, sort_keys=True))
    except Exception as error:
        print(f"scenario failed: {error}", file=sys.stderr)
        raise
