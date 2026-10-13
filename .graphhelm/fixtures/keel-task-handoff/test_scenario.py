"""Negative fixture proving the Codex-labelled receiver loads its companion adapter."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import shutil
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


if __name__ == "__main__":
    unittest.main()
