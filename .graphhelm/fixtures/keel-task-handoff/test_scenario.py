"""Negative fixture proving the Codex-labelled receiver loads its companion adapter."""

from __future__ import annotations

import importlib.util
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


REPO = Path(__file__).resolve().parents[3]
SCENARIO = REPO / ".graphhelm/fixtures/keel-task-handoff/scenario.py"
SENDER = REPO / "plugins/graphhelm/hooks"
COMPANION = REPO / "plugins/graphhelm-codex-hooks/hooks"
REFUSAL = "codex-companion-fixture-refusal"


class ScenarioAdapterSelection(unittest.TestCase):
    def test_codex_refusal_is_observed_from_the_companion_package(self):
        """A broken receiver package must fail instead of silently using the sender package.

        Cost: temporary copies and one bounded Python child; no Runtime, network, Cargo or model.
        The existing full scenario covers the positive handoff and Runtime contract, but cannot
        catch the fixture routing both host labels through the Claude package.
        """
        with tempfile.TemporaryDirectory(prefix="handoff-adapter-selection-") as temp:
            root = Path(temp)
            fixture = root / ".graphhelm/fixtures/keel-task-handoff"
            fixture.mkdir(parents=True)
            shutil.copyfile(SCENARIO, fixture / "scenario.py")
            sender = root / "plugins/graphhelm/hooks"
            companion = root / "plugins/graphhelm-codex-hooks/hooks"
            shutil.copytree(SENDER, sender)
            shutil.copytree(COMPANION, companion)
            (companion / "task_handoff.py").write_text(
                f"import sys\nprint('{REFUSAL}', file=sys.stderr)\nsys.exit(23)\n",
                encoding="utf-8",
            )

            spec = importlib.util.spec_from_file_location("handoff_scenario_fixture", fixture / "scenario.py")
            self.assertIsNotNone(spec)
            self.assertIsNotNone(spec.loader)
            scenario = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(scenario)
            self.assertEqual((sender / "task_handoff.py").read_bytes(),
                             (SENDER / "task_handoff.py").read_bytes())

            with self.assertRaisesRegex(scenario.ScenarioFailure, REFUSAL):
                scenario._run_adapter(
                    "status", "codex", "codex-fixture", root / "token", "http://127.0.0.1:1",
                    offer_id="fixture-offer", owned=root,
                )

    def test_receive_refusal_is_reported_as_structured_result(self):
        """Receive refusal JSON must be observed instead of treated as a raised failure.

        Cost: one short Python child and temporary copied files; no Runtime, network or Cargo.
        """
        with tempfile.TemporaryDirectory(prefix="handoff-structured-refusal-") as temp:
            root = Path(temp)
            fixture = root / ".graphhelm/fixtures/keel-task-handoff"
            fixture.mkdir(parents=True)
            shutil.copyfile(SCENARIO, fixture / "scenario.py")
            companion = root / "plugins/graphhelm-codex-hooks/hooks"
            companion.mkdir(parents=True)
            (companion / "task_handoff.py").write_text(
                "def receive(*args):\n"
                f"    raise ValueError({REFUSAL!r})\n",
                encoding="utf-8",
            )

            spec = importlib.util.spec_from_file_location(
                "handoff_structured_refusal", fixture / "scenario.py")
            self.assertIsNotNone(spec)
            self.assertIsNotNone(spec.loader)
            scenario = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(scenario)

            result = scenario._run_adapter(
                "receive", "codex", "codex-fixture", root / "token",
                "http://127.0.0.1:1", offer_id="fixture-offer", owned=fixture,
            )
            self.assertEqual(result.get("phase"), "receive")
            self.assertEqual(result.get("activation"), "unobserved")
            self.assertEqual(result.get("reason"), REFUSAL)

    def test_prior_task_evidence_is_recovered_from_runtime_history(self):
        """A fresh receiver must recover literal pre-handoff evidence, not sender memory.

        Contract: prior task evidence remains readable after a Claude-to-Codex handoff.
        Regression: checking only the offer and receipt can hide loss of work that predates
        the handshake. The existing scenario covers handoff continuity but seeds no earlier
        task record. This assertion requires literal content, Runtime actor/type/execution,
        sealed evidence identity/digest, and ordering before the offer. It adds no production
        seam; one bounded local Runtime scenario and its short-lived child processes are used.
        """
        if not os.environ.get("GRAPHHELM_CLI"):
            self.fail("OBSERVER_MISSING: set GRAPHHELM_CLI to the pinned executable")

        scenario = load_scenario()
        starts = []
        source_exited_before_replacement = []
        started_urls = {}
        recovery_reads = []
        real_popen = scenario.subprocess.Popen
        real_wait_started = scenario._wait_started
        real_run_adapter = scenario._run_adapter
        real_prior_reader = scenario._run_prior_task_evidence_reader

        def observing_popen(args, *positional, **kwargs):
            command = [str(part) for part in args]
            process = real_popen(args, *positional, **kwargs)
            if len(command) > 2 and command[1] == "serve" and "--events" in command:
                events = Path(command[command.index("--events") + 1]).resolve()
                keyring = Path(command[command.index("--keyring") + 1]).resolve()
                if events.name == "store" and len([item for item in starts if item["events"].name == "store"]) == 1:
                    source_exited_before_replacement.append(starts[0]["process"].poll() is not None)
                starts.append({
                    "process": process,
                    "events": events,
                    "keyring": keyring,
                })
            return process

        def observing_wait_started(log_path, child):
            url = real_wait_started(log_path, child)
            if Path(log_path).name == "runtime-restarted.log":
                started_urls[id(child)] = url
            return url

        def observing_run_adapter(phase, host, session, token_file, base_url, *args, **kwargs):
            recovery_reads.append(("adapter", phase, base_url))
            return real_run_adapter(phase, host, session, token_file, base_url, *args, **kwargs)

        def observing_prior_reader(base_url, *args, **kwargs):
            recovery_reads.append(("prior-evidence", base_url))
            return real_prior_reader(base_url, *args, **kwargs)

        scenario.subprocess.Popen = observing_popen
        scenario._wait_started = observing_wait_started
        scenario._run_adapter = observing_run_adapter
        scenario._run_prior_task_evidence_reader = observing_prior_reader
        try:
            result = scenario.run()
        finally:
            scenario.subprocess.Popen = real_popen
            scenario._wait_started = real_wait_started
            scenario._run_adapter = real_run_adapter
            scenario._run_prior_task_evidence_reader = real_prior_reader
        source_starts = [item for item in starts if item["events"].name == "store"]
        self.assertEqual(len(source_starts), 2, "expected exactly two source Runtime process starts")
        self.assertEqual(source_exited_before_replacement, [True],
                         "first source Runtime had not exited before the replacement started")
        self.assertEqual(source_starts[0]["events"], source_starts[1]["events"],
                         "replacement Runtime did not use the original event store")
        self.assertEqual(source_starts[0]["keyring"], source_starts[1]["keyring"],
                         "replacement Runtime did not use the original keyring")
        self.assertIsNotNone(source_starts[0]["process"].poll(),
                             "first source Runtime was still alive after the scenario")
        self.assertTrue(all(item["process"].poll() is not None for item in starts),
                        "scenario left a fixture-owned Runtime process alive")
        replacement_url = started_urls.get(id(source_starts[1]["process"]))
        self.assertTrue(replacement_url, "replacement Runtime did not report its serving endpoint")
        self.assertIn(("adapter", "receive", replacement_url), recovery_reads,
                      "Codex companion did not receive through the replacement Runtime")
        self.assertIn(("prior-evidence", replacement_url), recovery_reads,
                      "prior task evidence was not read through the replacement Runtime")
        self.assertTrue(result.get("journalStableAcrossRestart"),
                        "restart recovery appended or changed Runtime journal records")
        self.assertEqual(result.get("restartReceiptId"), result.get("receiptId"),
                         "replacement Runtime did not recover the original receipt")
        self.assertEqual(result.get("restartReceiptState"), "already_recorded")
        recovered = result.get("priorTaskEvidence")
        self.assertIsInstance(recovered, dict, "fresh receiver returned no prior task evidence")
        self.assertEqual(recovered.get("objective"),
                         "Synthetic objective: restore the fixture search path without changing production state.")
        self.assertEqual(recovered.get("keelCard"),
                         "Scope: fixture-only. Promise: prior evidence remains readable after handoff. Proof: recover this record from Runtime history.")
        self.assertEqual(recovered.get("priorProof"),
                         "Synthetic prior proof: the isolated dry-run observed the fixture search path.")
        self.assertEqual(recovered.get("recordType"), "signal_recorded")
        self.assertEqual(recovered.get("signalType"), "task.claimed")
        self.assertEqual(recovered.get("executionId"), "keel-handoff-fixture")
        self.assertEqual(recovered.get("sourceKind"), "user")
        self.assertEqual(recovered.get("sourceId"), result.get("priorTaskActor"))
        self.assertEqual(recovered.get("actor", {}).get("type"), "agent")
        self.assertEqual(recovered.get("actor", {}).get("id"), result.get("priorTaskActor"))
        self.assertEqual(recovered.get("actor", {}).get("id"), recovered.get("sourceId"))
        self.assertTrue(recovered.get("signalId"))
        self.assertTrue(recovered.get("evidenceId"))
        self.assertEqual(recovered.get("evidenceId"), recovered.get("eventEvidenceId"))
        self.assertEqual(recovered.get("contentSha256"), recovered.get("envelopeSha256"))
        self.assertEqual(hashlib.sha256(recovered.get("sealedContent", "").encode("utf-8")).hexdigest(),
                         recovered.get("contentSha256"))
        self.assertEqual(json.loads(recovered["sealedContent"])["id"], recovered["signalId"])
        self.assertLess(recovered.get("sequence", 0), result.get("offerSequence", 0))
        self.assertEqual(result.get("priorTaskEvidenceNegativeControls"), {
            "healthyEmpty": "no matching record discovered",
            "differentSynthetic": "different record and literal content rejected as the original",
        })


def load_scenario():
    spec = importlib.util.spec_from_file_location("handoff_positive_scenario", SCENARIO)
    if spec is None or spec.loader is None:
        raise RuntimeError("could not load the isolated handoff scenario")
    scenario = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(scenario)
    return scenario


if __name__ == "__main__":
    unittest.main()
