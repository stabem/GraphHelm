"""Admission failure cannot start a worker; admitted runs retain their process result."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
LAUNCHER = Path(__file__).with_name("run_codex.py")


@unittest.skipIf(os.name == "nt", "POSIX executable fixtures; native Windows observation is separate")
class LauncherTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.log = self.root / "calls.jsonl"
        self.prompt = self.root / "prompt with spaces.txt"
        self.prompt.write_text("actual task payload\n")
        self.binding = {"executionId": "team", "lane": "codex-10", "issue": 1739,
                        "branch": "issue-1739-flow", "repo": "owner/repo", "claimSequence": 42}
        self.receipt = self.root / "receipt.json"
        self.receipt.write_text(json.dumps({"command": "tasks.admit", "ok": True,
                                           "data": {"admitted": True, **self.binding}}))
        self.graphhelm = self.program("graphhelm", '''
import json, os, pathlib, sys
with open(os.environ['CALL_LOG'], 'a') as f: f.write(json.dumps(sys.argv[1:])+'\\n')
if 'admit' in sys.argv:
 print(pathlib.Path(os.environ['RECEIPT']).read_text())
 sys.exit(int(os.environ.get('ADMIT_EXIT','0')))
if os.environ.get('END_RECEIPT'):
 print(pathlib.Path(os.environ['END_RECEIPT']).read_text())
sys.exit(int(os.environ.get('END_EXIT','1')))
''')
        self.codex = self.program("codex", '''
import json, os, sys
with open(os.environ['CALL_LOG'], 'a') as f: f.write(json.dumps(['worker', *sys.argv[1:], sys.stdin.read()])+'\\n')
sys.exit(7)
''')
        self.env = {**os.environ, "CALL_LOG": str(self.log), "RECEIPT": str(self.receipt)}
        self.args = ["codex-10", "unique-tag", str(self.prompt), "--execution", "team", "--issue", "1739",
                     "--branch", "issue-1739-flow", "--repo", "owner/repo", "--claim-sequence", "42",
                     "--url", "http://127.0.0.1:8793", "--token-file", str(self.root / "read.token"),
                     "--result-file", str(self.root / "result with spaces.md"),
                     "--graphhelm", str(self.graphhelm), "--codex", str(self.codex)]

    def program(self, name, body):
        path = self.root / name
        path.write_text(f"#!{sys.executable}\n" + body)
        path.chmod(0o700)
        return path

    def run_launcher(self, args=None):
        return subprocess.run([sys.executable, str(LAUNCHER), *(self.args if args is None else args)],
                              env=self.env, capture_output=True, text=True, timeout=10, creationflags=NO_WINDOW)

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []

    def test_missing_binding_starts_nothing(self):
        result = self.run_launcher(self.args[:3])
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.calls(), [])

    def test_refused_admission_never_starts_worker_or_records_end(self):
        self.env['ADMIT_EXIT'] = '1'
        self.assertEqual(self.run_launcher().returncode, 2)
        self.assertEqual(len(self.calls()), 1)
        self.assertIn('admit', self.calls()[0])

    def test_refusal_shows_known_guidance_without_remote_secret_text(self):
        self.env['ADMIT_EXIT'] = '1'
        self.receipt.write_text(json.dumps({"ok": False, "diagnostics": [
            {"code": "GHCLI041_TASK_ADMISSION_REFUSED", "message": "sensitive-value-do-not-print"}]}))
        result = self.run_launcher()
        self.assertEqual(result.returncode, 2)
        self.assertIn('GHCLI041_TASK_ADMISSION_REFUSED', result.stderr)
        self.assertIn('recheck the current own-lane pre-PR claim', result.stderr)
        self.assertNotIn('sensitive-value-do-not-print', result.stderr + result.stdout)
        self.assertEqual(len(self.calls()), 1)

    def test_wrong_or_malformed_receipt_never_starts_worker(self):
        for receipt in ('not json', json.dumps({"ok": True, "command": "tasks.admit", "data":
                         {"admitted": True, **self.binding, "lane": "other-lane"}})):
            with self.subTest(receipt=receipt):
                self.log.unlink(missing_ok=True)
                self.receipt.write_text(receipt)
                self.assertEqual(self.run_launcher().returncode, 2)
                self.assertEqual(len(self.calls()), 1)

    def test_exit_zero_without_valid_recorded_receipt_reports_failure_and_preserves_worker_exit(self):
        ended = self.root / "end-receipt.json"
        self.env.update(END_EXIT='0', END_RECEIPT=str(ended))
        for receipt in (
            json.dumps({"ok": True, "command": "tasks.run-ended", "data":
                        {"recorded": False, "reason": "sensitive-end-reason-do-not-print"}}),
            'not json',
            '[]',
            json.dumps({"ok": True, "command": "tasks.run-ended", "data": None}),
            json.dumps({"ok": True, "command": "tasks.admit", "data": {"recorded": True}}),
            json.dumps({"ok": True, "command": "tasks.run-ended", "data": {}}),
        ):
            with self.subTest(receipt=receipt):
                self.log.unlink(missing_ok=True)
                ended.write_text(receipt)
                result = self.run_launcher()
                self.assertEqual(result.returncode, 7)
                self.assertEqual(len(self.calls()), 3)
                self.assertIn('launcher: run-end recording failed', result.stderr)
                self.assertNotIn('sensitive-end-reason-do-not-print', result.stdout + result.stderr)

    def test_valid_recorded_receipt_preserves_worker_exit_without_failure_diagnostic(self):
        ended = self.root / "end-receipt.json"
        ended.write_text(json.dumps({"ok": True, "command": "tasks.run-ended",
                                     "data": {"recorded": True}}))
        self.env.update(END_EXIT='0', END_RECEIPT=str(ended))
        result = self.run_launcher()
        self.assertEqual(result.returncode, 7)
        self.assertEqual(len(self.calls()), 3)
        self.assertNotIn('run-end recording failed', result.stderr)

    def test_admitted_run_transports_binding_and_preserves_exit_when_end_fails(self):
        self.assertEqual(self.run_launcher().returncode, 7)
        admit, worker, ended = self.calls()
        self.assertEqual(admit[:3], ['--json', 'tasks', 'admit'])
        for flag, value in (('--execution', 'team'), ('--lane', 'codex-10'), ('--issue', '1739'),
                            ('--branch', 'issue-1739-flow'), ('--repo', 'owner/repo'), ('--claim-sequence', '42')):
            self.assertEqual(admit[admit.index(flag)+1], value)
        self.assertEqual(worker, ['worker', 'exec', '-o', str(self.root / 'result with spaces.md'), '-',
                                  'actual task payload\n'])
        self.assertEqual(ended[:3], ['--json', 'tasks', 'run-ended'])
        self.assertEqual(ended[ended.index('--exit')+1], '7')
        self.assertEqual(ended[ended.index('--run-id')+1], 'unique-tag')


@unittest.skipUnless(os.name == "nt", "Windows cmd entry point")
class WindowsEntryTest(unittest.TestCase):
    def test_cmd_entrypoint_rejects_missing_required_arguments(self):
        result = subprocess.run(['cmd', '/d', '/c', str(LAUNCHER.with_name('run-codex.cmd'))],
                                capture_output=True, timeout=10, creationflags=NO_WINDOW)
        self.assertNotEqual(result.returncode, 0)


if __name__ == '__main__':
    unittest.main()
