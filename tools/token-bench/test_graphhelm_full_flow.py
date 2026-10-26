"""Fail-closed contracts for the local Runtime and token audit."""
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parent))
import graphhelm_full_flow as flow


class FullFlowAudit(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.endpoint = {"status": "observed", "patchPath": "patch", "reviewPath": "review",
                         "evaluatorPath": "evaluation", "author": "author", "reviewer": "reviewer",
                         "reviewerSessionPath": "session", "gitHeadPath": "repo", "taskId": "task",
                         "gitHead": "a" * 40}
        self.write("endpoint.json", self.endpoint)
        self.write("status.json", {"data": {"executionId": "run", "status": "completed",
                                          "nodeStates": dict.fromkeys(flow.NODES, "succeeded"),
                                          "signalsRecorded": 5}})
        self.write("journey.json", {"steps": [{"stepId": "one"}, {"stepId": "two"}]})
        kinds = (["completion_claimed"] * 5 + ["completion_cleared"] * 5 +
                 ["jpd.journey", "jpd.obligation", "jpd.obligation", "keel.card", "keel.proof"])
        events = []
        for index, kind in enumerate(kinds):
            event_kind = {"type": kind}
            if kind == "completion_claimed":
                event_kind["data"] = {"node": flow.NODES[index], "executionId": "run"}
            if kind == "completion_cleared":
                event_kind["data"] = {"claimSeq": index - 4, "executionId": "run"}
            if kind.startswith(("jpd.", "keel.")):
                event_kind = {"type": "signal_recorded", "data": {"kind": kind,
                                                                    "executionId": "run"}}
            events.append({"sequence": index + 1, "streamId": "stream-1",
                           "scope": {"executionId": "run"}, "kind": event_kind})
        events.append({"sequence": len(events) + 1, "streamId": "stream-1",
                       "scope": {"executionId": "run"},
                       "kind": {"type": "execution_completed", "data": {"executionId": "run",
                                                                      "status": "completed"}}})
        status = self.read_json("status.json")
        status["data"]["headSequence"] = len(events)
        self.write("status.json", status)
        (self.root / "journal.jsonl").write_text(json.dumps({"events": events}) + "\n")
        attempts = []
        (self.root / "runs").mkdir()
        for stage in ("planning", "author", "review"):
            (self.root / "runs" / ("test-" + stage)).mkdir()
            path = self.write("runs/test-" + stage + "/receipt.json", {"archive": {"rootSession": stage,
                         "status": "observed", "total": {"input": 1, "output": 2,
                         "cacheRead": 3, "cacheWrite": 4}}, "wire": {"accountingStatus": "observed",
                         "countsSource": "claude.result.modelUsage",
                         "counts": {"input": 1, "output": 2, "cacheRead": 3, "cacheWrite": 4}},
                         "quota": {"allowed": True, "paidFallbackAuthorized": False},
                         "status": "observed"})
            attempts.append({"stage": stage, "receipt": str(path)})
        self.manifest = {"attempts": attempts, "attemptsRoot": str(self.root / "runs"),
                         "attemptPrefix": "test-", "provider": "claude",
                         "graphhelmCli": "graphhelm", "runtimeGraph": "graph.json",
                         "runtimeEvents": "events", "runtimeStatus": str(self.root / "status.json"),
                         "journal": str(self.root / "journal.jsonl"),
                         "journey": str(self.root / "journey.json"),
                         "endpoint": str(self.root / "endpoint.json"), "executionId": "run",
                         "candidateHead": "a" * 40, "manualIntervention": False,
                         "prStageReviewed": True, "reachedStatus": "passed"}
        self.rebound = patch.object(flow.native_methodology, "accepted_patch_endpoint",
                                    return_value=self.endpoint)
        self.rebound.start()
        self.addCleanup(self.rebound.stop)
        self.live = patch.object(flow.subprocess, "run", return_value=SimpleNamespace(
            returncode=0, stdout=json.dumps({"ok": True, "data": self.read_json("status.json")["data"]})))
        self.live.start()
        self.addCleanup(self.live.stop)

    def write(self, name, value):
        path = self.root / name
        path.write_text(json.dumps(value), encoding="utf-8")
        return path

    def read_json(self, name):
        return json.loads((self.root / name).read_text(encoding="utf-8"))

    def test_completed_flow_counts_every_root_and_all_token_categories(self):
        result = flow.audit(self.manifest)
        self.assertTrue(result["integrationComplete"], result["errors"])
        self.assertTrue(result["comparisonEligible"])
        self.assertEqual(result["totalProcessedTokens"], 30)
        self.manifest["manualIntervention"] = True
        self.assertFalse(flow.audit(self.manifest)["comparisonEligible"])

    def test_missing_obligation_blocks_completed_runtime(self):
        path = self.root / "journal.jsonl"
        row = json.loads(path.read_text())
        row["events"] = [event for index, event in enumerate(row["events"])
                         if index != 12]
        path.write_text(json.dumps(row), encoding="utf-8")
        result = flow.audit(self.manifest)
        self.assertFalse(result["integrationComplete"])
        self.assertIn("signal thread", " ".join(result["errors"]))

    def test_duplicate_node_claim_cannot_stand_in_for_missing_node(self):
        path = self.root / "journal.jsonl"
        row = json.loads(path.read_text())
        row["events"][1]["kind"]["data"]["node"] = flow.NODES[0]
        path.write_text(json.dumps(row), encoding="utf-8")
        result = flow.audit(self.manifest)
        self.assertFalse(result["integrationComplete"])
        self.assertIn("each required node", " ".join(result["errors"]))

    def test_missing_attempt_has_no_comparable_token_total(self):
        self.manifest["attempts"].pop(1)
        with self.assertRaisesRegex(ValueError, "inventory differs"):
            flow.audit(self.manifest)

    def test_other_execution_in_journal_is_refused(self):
        path = self.root / "journal.jsonl"
        row = json.loads(path.read_text())
        for event in row["events"]:
            event["scope"]["executionId"] = "other"
            event["kind"].get("data", {}).update({"executionId": "other"})
        path.write_text(json.dumps(row), encoding="utf-8")
        result = flow.audit(self.manifest)
        self.assertFalse(result["integrationComplete"])
        self.assertIn("another execution", " ".join(result["errors"]))

    def test_missing_claim_execution_id_is_refused(self):
        path = self.root / "journal.jsonl"
        row = json.loads(path.read_text())
        del row["events"][0]["kind"]["data"]["executionId"]
        path.write_text(json.dumps(row), encoding="utf-8")
        result = flow.audit(self.manifest)
        self.assertFalse(result["integrationComplete"])
        self.assertIn("another execution", " ".join(result["errors"]))

    def test_codex_receipt_cannot_use_claude_additive_counters(self):
        self.manifest["provider"] = "codex"
        with self.assertRaisesRegex(ValueError, "only Claude"):
            flow.audit(self.manifest)

    def test_symlinked_receipt_is_refused_when_supported(self):
        original = Path(self.manifest["attempts"][0]["receipt"])
        external = self.root / "external.json"
        external.write_bytes(original.read_bytes())
        original.unlink()
        try:
            original.symlink_to(external)
        except OSError:
            self.skipTest("symlink creation unavailable on this host")
        with self.assertRaisesRegex(ValueError, "escapes or aliases"):
            flow.audit(self.manifest)


if __name__ == "__main__":
    unittest.main()
