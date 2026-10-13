"""Check that fixture exit status reflects the CLI's guidance envelope."""

import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("fixture_run", Path(__file__).with_name("run.py"))
fixture = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fixture)

ROWS = {
    "docs": ("summary_only", []),
    "bounded-code": ("bounded_code", ["promise", "scope", "proof"]),
    "expanded": ("expanded", ["full_card", "defect_naming_proof", "jpd"]),
}


class GuidanceExitTests(unittest.TestCase):
    def invoke(self, mode, guidance):
        responses = []
        if mode == "expanded":
            responses.extend([
                {"data": {"findings": [{"rule": "keel.invariant.persistence"}]}},
                {"data": {"findings": []}},
            ])
        responses.append({"data": {"findings": [], **({"cardGuidance": guidance} if guidance is not None else {})}})
        envelopes = iter(responses)

        def fake_run(args, cwd):
            if args[0] == "git":
                return subprocess.CompletedProcess(args, 0, b"", b"")
            return subprocess.CompletedProcess(args, 0, json.dumps(next(envelopes)).encode(), b"")

        with patch.object(fixture, "run", side_effect=fake_run), \
             patch.object(fixture, "graphhelm", return_value="graphhelm"), \
             patch.object(sys, "argv", ["run.py", mode]), \
             patch.object(sys, "stdout", new_callable=lambda: _Output()) as output, \
             patch.object(sys, "stderr", new_callable=io.StringIO) as errors:
            status = fixture.main()
            return status, output.buffer.getvalue(), errors.getvalue()

    def test_missing_guidance_rejected_in_every_positive_mode(self):
        for mode in ROWS:
            with self.subTest(mode=mode):
                status, _, error = self.invoke(mode, None)
                self.assertNotEqual(status, 0)
                self.assertIn("cardGuidance", error)

    def test_wrong_row_and_missing_obligations_rejected(self):
        for mode, (kind, required) in ROWS.items():
            with self.subTest(mode=mode, defect="wrong row"):
                status, _, error = self.invoke(mode, {"kind": "wrong", "required": required})
                self.assertNotEqual(status, 0)
                self.assertIn("cardGuidance", error)
            if required:
                with self.subTest(mode=mode, defect="missing obligation"):
                    status, _, error = self.invoke(mode, {"kind": kind, "required": required[:-1]})
                    self.assertNotEqual(status, 0)
                    self.assertIn("cardGuidance", error)

    def test_correct_rows_keep_cli_envelope_on_stdout(self):
        for mode, (kind, required) in ROWS.items():
            with self.subTest(mode=mode):
                status, output, error = self.invoke(mode, {"kind": kind, "required": required})
                self.assertEqual(status, 0, error)
                self.assertEqual(json.loads(output)["data"]["cardGuidance"]["kind"], kind)


class _Output:
    def __init__(self):
        self.buffer = io.BytesIO()

    def flush(self):
        pass


if __name__ == "__main__":
    unittest.main()
