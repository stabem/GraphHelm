"""Focused contract tests for run_reached.py; no Cargo, Runtime, browser, or shell."""
import json
import os
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import run_reached as rr

# Owner rule: children run hidden in the calling terminal, never in a new console window.
NO_WINDOW = rr.NO_WINDOW


_FIXED_FINGERPRINT = {"tree": "t", "tools": {}, "toolsSha256": "a", "envSha256": "b"}


class RunnerContracts(unittest.TestCase):
    """Orchestration contracts; the fingerprint and preflight seams have their own suite below."""

    def setUp(self):
        for name, value in (("_fingerprint", lambda *unused: dict(_FIXED_FINGERPRINT)),
                            ("_preflight", lambda *unused: ([], [])),
                            ("_reusable_results", lambda *unused: ({}, None))):
            patcher = patch.object(rr, name, side_effect=value)
            patcher.start()
            self.addCleanup(patcher.stop)

    def test_batch_cli_defaults_on_and_supports_explicit_opt_out(self):
        with patch.object(rr, "run", return_value=0) as run:
            self.assertEqual(rr.main(["--output", "feedback.json"]), 0)
            self.assertTrue(run.call_args.args[0].batch_slot)
            self.assertEqual(rr.main(["--output", "feedback.json", "--no-batch-slot"]), 0)
            self.assertFalse(run.call_args.args[0].batch_slot)
            self.assertEqual(rr.main(["--output", "feedback.json", "--batch-slot"]), 0)
            self.assertTrue(run.call_args.args[0].batch_slot)

    def test_batch_order_hoists_only_fmt_and_keeps_selector_indices(self):
        steps, error = rr._steps({"steps": [
            {"argv": ["python", "-c", "pass"], "cwd": ".", "slot": False},
            {"argv": ["cargo", "+1.97.1", "test", "--lib"], "cwd": ".", "slot": True},
            {"argv": ["cargo", "+1.97.1", "fmt", "--all", "--", "--check"], "cwd": ".", "slot": False},
            {"argv": ["cargo", "+1.97.1", "clippy", "-p", "x"], "cwd": ".", "slot": True},
        ]})
        self.assertIsNone(error)
        ordered = rr._execution_order(steps, True)
        self.assertEqual([entry["index"] for entry in ordered], [0, 2, 1, 3])

    def test_batch_result_rejects_old_cli_and_requires_exact_prefix(self):
        parsed, error = rr._batch_result(None, 2, 0)
        self.assertIsNone(parsed)
        self.assertIn("malformed", error)
        parsed, error = rr._batch_result({"schema": "graphhelm.slot-batch-result/1",
                                          "completed": [{"index": 1, "exitCode": 0, "elapsedSeconds": 1}],
                                          "remaining": [1], "stopReason": "leaseBoundary"}, 2, 0)
        self.assertIsNone(parsed)
        self.assertIn("ordered prefix", error)

    def test_batch_result_rejects_false_green_shapes(self):
        base = {"schema": "graphhelm.slot-batch-result/1", "completed": [], "remaining": [0], "stopReason": "leaseBoundary"}
        for mutation, message in [
            ({"stopReason": "exhausted", "remaining": [], "completed": [{"index": 0, "exitCode": 3, "elapsedSeconds": 1}]}, "successfully"),
            ({"stopReason": "leaseBoundary", "completed": [{"index": True, "exitCode": 0, "elapsedSeconds": 1}]}, "ordered prefix"),
            ({"stopReason": "leaseBoundary", "remaining": [0, 0]}, "exact complement"),
            ({"stopReason": "leaseBoundary", "remaining": [], "completed": []}, "exact complement"),
        ]:
            value = dict(base)
            value.update(mutation)
            parsed, error = rr._batch_result(value, 1, 0)
            self.assertIsNone(parsed)
            self.assertIn(message, error)
        child = {"schema": "graphhelm.slot-batch-result/1",
                 "completed": [{"index": 0, "exitCode": 4, "elapsedSeconds": 1}],
                 "remaining": [], "stopReason": "childFailure"}
        parsed, error = rr._batch_result(child, 1, 4)
        self.assertEqual(error, None)
        self.assertEqual(parsed["stopReason"], "childFailure")

    def test_batch_envelope_rejects_contradictory_outer_exit(self):
        with tempfile.TemporaryDirectory() as temp:
            log = Path(temp) / "slot.log"
            valid = {"command": "workspace.slot", "ok": True,
                     "data": {"waitedSeconds": 1, "heldSeconds": 1, "exitCode": 7, "batch": {}}}
            log.write_text(json.dumps(valid) + "\n", encoding="utf-8")
            self.assertEqual(rr._batch_envelope(log, 7)[1:], (1.0, 1.0))
            for mutation in (
                {"ok": False},
                {"data": {**valid["data"], "exitCode": 0}},
                {"data": {key: value for key, value in valid["data"].items() if key != "exitCode"}},
                {"data": {**valid["data"], "exitCode": True}},
            ):
                invalid = {**valid, **mutation}
                log.write_text(json.dumps(invalid) + "\n", encoding="utf-8")
                self.assertEqual(rr._batch_envelope(log, 7), (None, None, None))

    def test_batch_runner_resumes_boundary_without_duplicate_prefix(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp)
            subprocess = __import__("subprocess")
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True, creationflags=NO_WINDOW)
            (repo / "x").write_text("x", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            output = Path(temp).parent / (Path(temp).name + "-batch-report.json")
            plan = {"steps": [
                {"argv": ["cargo", "+1.97.1", "test", "--lib"], "cwd": ".", "slot": True},
                {"argv": ["cargo", "+1.97.1", "clippy", "-p", "x"], "cwd": ".", "slot": True},
                {"argv": [sys.executable, "-c", "pass"], "cwd": ".", "slot": False},
            ]}
            calls = []

            def fake_git(_repo, *args):
                return "HEAD" if args[0] == "rev-parse" else ""

            def fake_run(command, **kwargs):
                if command[0] == "git":
                    return subprocess.CompletedProcess(command, 0, "", "")
                calls.append((command, kwargs["env"]))
                batch = {"schema": "graphhelm.slot-batch-result/1",
                         "completed": [{"index": 0, "exitCode": 0, "elapsedSeconds": 1}],
                         "remaining": [1], "stopReason": "leaseBoundary"} if len(calls) == 1 else {
                         "schema": "graphhelm.slot-batch-result/1",
                         "completed": [{"index": 0, "exitCode": 0, "elapsedSeconds": 1}],
                         "remaining": [], "stopReason": "exhausted"}
                kwargs["stdout"].write(json.dumps({"command": "workspace.slot", "ok": True, "data": {
                    "waitedSeconds": 1, "heldSeconds": 2, "exitCode": 0, "batch": batch}}) + "\n")
                return subprocess.CompletedProcess(command, 0, "", "")

            args = type("Args", (), {"repo": str(repo), "output": str(output), "base": "HEAD", "head": "HEAD",
                                      "root": "D:/gh", "lane": "test", "budget_seconds": 180,
                                      "allow_whole_package": None, "plan": False,
                                      "graphhelm": sys.executable})()
            with patch.object(rr, "_selector", return_value=plan), patch.object(rr, "_git", side_effect=fake_git), \
                    patch.object(rr.subprocess, "run", side_effect=fake_run):
                self.assertEqual(rr.run(args), 0)
            report = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(report["status"], "passed")
            self.assertEqual([item["index"] for item in report["completed"]], [0, 1, 2])
            first = json.loads(calls[0][1]["GRAPHHELM_SLOT_BATCH"])
            second = json.loads(calls[1][1]["GRAPHHELM_SLOT_BATCH"])
            self.assertEqual(len(first["commands"]), 2)
            self.assertEqual(len(second["commands"]), 1)
            self.assertEqual(second["commands"][0], plan["steps"][1]["argv"])
            self.assertEqual(first["deadlineUnixMs"], second["deadlineUnixMs"])

    def test_batch_runner_observes_deadline_and_final_child_failure(self):
        def run_case(batch, cli_code):
            with tempfile.TemporaryDirectory() as temp:
                repo = Path(temp)
                subprocess = __import__("subprocess")
                subprocess.run(["git", "init", "-q"], cwd=repo, check=True, creationflags=NO_WINDOW)
                (repo / "x").write_text("x", encoding="utf-8")
                subprocess.run(["git", "add", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
                subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
                output = Path(temp).parent / (Path(temp).name + "-report.json")
                plan = {"steps": [{"argv": ["cargo", "+1.97.1", "test", "--lib"], "cwd": ".", "slot": True}]}
                def fake_run(command, **kwargs):
                    if command[0] == "git":
                        return subprocess.CompletedProcess(command, 0, "", "")
                    kwargs["stdout"].write(json.dumps({"command": "workspace.slot", "ok": True,
                        "data": {"waitedSeconds": 1, "heldSeconds": 1, "exitCode": cli_code, "batch": batch}}) + "\n")
                    return subprocess.CompletedProcess(command, cli_code, "", "")
                args = type("Args", (), {"repo": str(repo), "output": str(output), "base": "HEAD", "head": "HEAD",
                    "root": "D:/gh", "lane": "test", "budget_seconds": 180, "allow_whole_package": None,
                    "plan": False, "batch_slot": True, "graphhelm": sys.executable})()
                def fake_git(_repo, *git_args):
                    return "HEAD" if git_args[0] == "rev-parse" else ""
                with patch.object(rr, "_selector", return_value=plan), patch.object(rr, "_git", side_effect=fake_git), \
                        patch.object(rr.subprocess, "run", side_effect=fake_run):
                    self.assertEqual(rr.run(args), 1)
                return json.loads(output.read_text(encoding="utf-8"))

        deadline = run_case({"schema": "graphhelm.slot-batch-result/1", "completed": [],
                             "remaining": [0], "stopReason": "deadline"}, 0)
        self.assertEqual(deadline["status"], "budgetExceeded")
        self.assertEqual(deadline["completed"], [])
        failed = run_case({"schema": "graphhelm.slot-batch-result/1",
                           "completed": [{"index": 0, "exitCode": 7, "elapsedSeconds": 1}],
                           "remaining": [], "stopReason": "childFailure"}, 7)
        self.assertEqual(failed["status"], "failed")
        self.assertEqual(failed["completed"][0]["returncode"], 7)

    def test_steps_preserve_cwd_and_slot_without_shell(self):
        steps, error = rr._steps({"steps": [{"argv": ["cargo", "+1.97.1", "test", "--lib"], "cwd": ".", "slot": True},
                                   {"argv": ["npx", "tsc", "-b"], "cwd": "apps/studio", "slot": False}]})
        self.assertIsNone(error)
        self.assertTrue(steps[0]["slot"])
        self.assertEqual(steps[1]["cwd"], "apps/studio")
        self.assertIn("selector steps", rr._steps({"commands": ["cargo test"]})[1])

    def test_steps_reject_unscoped_cargo_and_shell_strings(self):
        _, error = rr._steps({"steps": [{"argv": ["cargo", "+1.97.1", "test", "-p", "graphhelm-cli"],
                                           "cwd": ".", "slot": True}]})
        self.assertIn("explicit target", error)
        _, error = rr._steps({"steps": [{"argv": ["cmd", "/c", "echo bad"],
                                           "cwd": ".", "slot": False}]})
        self.assertIn("shell syntax", error)

    def test_slot_telemetry_uses_final_envelope_after_child_output(self):
        with tempfile.TemporaryDirectory() as temp:
            log = Path(temp) / "slot.log"
            log.write_text("child output\n{\"command\":\"workspace.slot\",\"data\":{\"waitedSeconds\":1.5,\"heldSeconds\":2}}\n", encoding="utf-8")
            self.assertEqual(rr._slot_telemetry(log, ["graphhelm", "--json", "workspace", "slot"]), (1.5, 2))
            self.assertEqual(rr._slot_telemetry(log, [str(Path(temp) / "graphhelm"), "--json", "workspace", "slot"]), (1.5, 2))
            log.write_text("child output\n{\"ok\":false,\"command\":\"workspace.slot\",\"data\":{\"waitedSeconds\":0.5,\"heldSeconds\":0},\"diagnostics\":[{\"code\":\"GHCLI037_WORKSPACE_REFUSED\"}]}\n", encoding="utf-8")
            self.assertEqual(rr._slot_telemetry(log, ["graphhelm", "--json", "workspace", "slot"]), (0.5, 0))

    def test_explicit_graphhelm_overrides_configured_environment_and_path(self):
        with tempfile.TemporaryDirectory() as temp:
            explicit = Path(temp) / "chosen-graphhelm"
            explicit.write_text("fixture", encoding="utf-8")
            args = type("Args", (), {"graphhelm": str(explicit)})()
            with patch.dict(os.environ, {"GRAPHHELM_CLI": str(Path(temp) / "missing")}), \
                    patch.object(rr.shutil, "which", return_value=str(Path(temp) / "wrong")):
                self.assertEqual(rr._resolve_graphhelm(args), explicit.resolve())

    def test_missing_explicit_graphhelm_fails_before_selector_or_child(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp)
            subprocess = __import__("subprocess")
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True, creationflags=NO_WINDOW)
            (repo / "x").write_text("x", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            output = Path(temp).parent / (Path(temp).name + "-missing-cli-report.json")
            plan = {"steps": [{"argv": ["python", "-c", "pass"], "cwd": ".", "slot": True}]}
            args = type("Args", (), {"repo": str(repo), "output": str(output), "base": "HEAD", "head": "HEAD",
                                      "root": "D:/gh", "lane": "test", "budget_seconds": 180,
                                      "allow_whole_package": None, "plan": False,
                                      "graphhelm": str(Path(temp) / "does-not-exist")})()
            with patch.object(rr, "_git", side_effect=["HEAD", "HEAD", "", "HEAD", ""]), \
                    patch.object(rr, "_selector", side_effect=AssertionError("selector must not run")):
                self.assertEqual(rr.run(args), 1)
            report = json.loads(output.read_text(encoding="utf-8"))
            self.assertIn("does-not-exist", report["error"])

    def test_python_only_plan_does_not_require_graphhelm_on_path(self):
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp).parent / (Path(temp).name + "-python-only-report.json")
            plan = {"steps": [{"argv": ["python", "-c", "pass"], "cwd": ".", "slot": False}]}
            subprocess = __import__("subprocess")
            args = type("Args", (), {"repo": temp, "output": str(output), "base": "HEAD", "head": "HEAD",
                                      "root": "D:/gh", "lane": "test", "budget_seconds": 180,
                                      "allow_whole_package": None, "plan": False})()

            def fake_child(command, **kwargs):
                return subprocess.CompletedProcess(command, 0, "", "")

            with patch.dict(os.environ, {"GRAPHHELM_CLI": ""}), \
                    patch.object(rr.shutil, "which", return_value=None), \
                    patch.object(rr, "_git", side_effect=["HEAD", "HEAD", "", "HEAD", ""]), \
                    patch.object(rr, "_selector", return_value=plan), \
                    patch.object(rr.subprocess, "run", side_effect=fake_child):
                self.assertEqual(rr.run(args), 0)
            self.assertNotIn("graphhelmPath", json.loads(output.read_text(encoding="utf-8")))

    def test_plan_rejects_missing_explicit_graphhelm_before_selector(self):
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp).parent / (Path(temp).name + "-plan-report.json")
            args = type("Args", (), {"repo": temp, "output": str(output), "base": "HEAD", "head": "HEAD",
                                      "root": "D:/gh", "lane": "test", "budget_seconds": 180,
                                      "allow_whole_package": None, "plan": True,
                                      "graphhelm": str(Path(temp) / "missing")})()
            with patch.object(rr, "_git", side_effect=["HEAD", "HEAD", ""]), \
                    patch.object(rr, "_selector", side_effect=AssertionError("selector must not run")):
                self.assertEqual(rr.run(args), 1)
            self.assertIn("missing", json.loads(output.read_text(encoding="utf-8"))["error"])

    def test_whole_package_requires_explicit_reason(self):
        self.assertIn("requires", rr._whole_reason({"packages": ["graphhelm-cli"]}, None))
        self.assertIsNone(rr._whole_reason({"packages": ["graphhelm-cli"]}, "shared parser change"))

    def test_browser_observer_is_skipped_by_default_and_never_started(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp)
            subprocess = __import__("subprocess")
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True, creationflags=NO_WINDOW)
            (repo / "x").write_text("x", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            output = Path(temp).parent / (Path(temp).name + "-browser-report.json")
            plan = {"steps": [{"argv": ["python", "-c", "raise SystemExit(99)"], "cwd": ".", "slot": False, "observer": "browser"}]}
            args = type("Args", (), {"repo": str(repo), "output": str(output), "base": "HEAD", "head": "HEAD",
                                      "root": "D:/gh", "lane": "test", "budget_seconds": 180,
                                      "allow_whole_package": None, "plan": False, "include_browser": False})()
            with patch.object(rr, "_selector", return_value=plan):
                self.assertEqual(rr.run(args), 0)
            report = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(report["completed"], [])
            self.assertEqual(report["pending"], [])
            self.assertEqual(report["skipped"], [{"index": 0, "reason": "no observer"}])
            self.assertEqual(report["status"], "passed")

    def test_browser_observer_is_skipped_by_default_while_plain_steps_run(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp)
            subprocess = __import__("subprocess")
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True, creationflags=NO_WINDOW)
            (repo / "x").write_text("x", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            output = Path(temp).parent / (Path(temp).name + "-browser-mixed-report.json")
            marker = Path(temp).parent / (Path(temp).name + "-browser-mixed-marker")
            plan = {"steps": [
                {"argv": ["python", "-c", "raise SystemExit(99)"], "cwd": ".", "slot": False, "observer": "browser"},
                {"argv": ["python", "-c", f"from pathlib import Path; Path(r'{marker}').write_text('ran')"], "cwd": ".", "slot": False},
            ]}
            args = type("Args", (), {"repo": str(repo), "output": str(output), "base": "HEAD", "head": "HEAD",
                                      "root": "D:/gh", "lane": "test", "budget_seconds": 180,
                                      "allow_whole_package": None, "plan": False, "include_browser": False, "batch_slot": True})()
            with patch.object(rr, "_selector", return_value=plan):
                self.assertEqual(rr.run(args), 0)
            report = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(report["status"], "passed")
            self.assertEqual(report["completed"][0]["index"], 1)
            self.assertEqual(report["skipped"], [{"index": 0, "reason": "no observer"}])
            self.assertEqual(report["pending"], [])
            self.assertEqual(marker.read_text(encoding="utf-8"), "ran")

    def test_browser_opt_in_requires_local_playwright_toolchain(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp)
            subprocess = __import__("subprocess")
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True, creationflags=NO_WINDOW)
            (repo / "x").write_text("x", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            output = Path(temp).parent / (Path(temp).name + "-browser-optin-report.json")
            plan = {"steps": [{"argv": ["node", "browser.test.mjs"], "cwd": ".", "slot": False, "observer": "browser"}]}
            args = type("Args", (), {"repo": str(repo), "output": str(output), "base": "HEAD", "head": "HEAD",
                                      "root": "D:/gh", "lane": "test", "budget_seconds": 180,
                                      "allow_whole_package": None, "plan": False, "include_browser": True})()
            with patch.object(rr, "_selector", return_value=plan), patch.dict(os.environ, {"GRAPHHELM_JOURNEY_TOOLCHAIN_PROJECT": ""}):
                self.assertEqual(rr.run(args), 1)
            report = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(report["completed"], [])
            self.assertIn("local @playwright/test", report["error"])

    def test_runner_uses_separate_direct_subprocesses_and_reports_failure(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp)
            subprocess = __import__("subprocess")
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True, creationflags=NO_WINDOW)
            (repo / "x").write_text("x", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            output = Path(temp).parent / (Path(temp).name + "-report.json")
            plan = {"steps": [{"argv": ["python", "-c", "raise SystemExit(7)"], "cwd": ".", "slot": False}]}
            with patch.object(rr, "_selector", return_value=plan):
                args = type("Args", (), {"repo": str(repo), "output": str(output), "base": "HEAD", "head": "HEAD",
                                          "root": "D:/gh", "lane": "test", "budget_seconds": 180,
                                          "allow_whole_package": None, "plan": False})()
                self.assertEqual(rr.run(args), 1)
            self.assertEqual(json.loads(output.read_text(encoding="utf-8"))["status"], "failed")

    def test_runner_executes_selected_pytest_and_unittest_fixtures(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp)
            subprocess = __import__("subprocess")
            fixture = repo / "test_fixture.py"
            fixture.write_text("def test_failure():\n    assert False\n", encoding="utf-8")
            control = repo / "test_control.py"
            control.write_text("import unittest\nclass Control(unittest.TestCase):\n def test_ok(self): self.assertTrue(True)\n", encoding="utf-8")
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True, creationflags=NO_WINDOW)
            (repo / "tracked").write_text("x", encoding="utf-8")
            (repo / ".gitignore").write_text("__pycache__/\n.pytest_cache/\n", encoding="utf-8")
            subprocess.run(["git", "add", "tracked", "test_fixture.py", "test_control.py", ".gitignore"], cwd=repo, check=True, creationflags=NO_WINDOW)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            output = Path(temp).parent / (Path(temp).name + "-pytest-report.json")
            plan = {"steps": [{"argv": [sys.executable, "-m", "pytest", "-q", "test_fixture.py"], "cwd": ".", "slot": False},
                              {"argv": [sys.executable, "-m", "unittest", "test_control"], "cwd": ".", "slot": False}]}
            with patch.object(rr, "_selector", return_value=plan):
                args = type("Args", (), {"repo": str(repo), "output": str(output), "base": "HEAD", "head": "HEAD",
                                          "root": "D:/gh", "lane": "test", "budget_seconds": 180,
                                          "allow_whole_package": None, "plan": False})()
                self.assertEqual(rr.run(args), 1)
            report = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(report["status"], "failed")
            self.assertEqual(report["completed"][0]["returncode"], 1)
    def test_empty_unittest_module_cannot_claim_green(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp)
            subprocess = __import__("subprocess")
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True, creationflags=NO_WINDOW)
            (repo / "empty_test.py").write_text("# no unittest cases\n", encoding="utf-8")
            (repo / ".gitignore").write_text("__pycache__/\n.pytest_cache/\n", encoding="utf-8")
            subprocess.run(["git", "add", "empty_test.py", ".gitignore"], cwd=repo, check=True, creationflags=NO_WINDOW)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            output = Path(temp).parent / (Path(temp).name + "-empty-unittest-report.json")
            plan = {"steps": [{"argv": ["python", "-m", "unittest", "empty_test.py"],
                               "cwd": ".", "slot": False}]}
            args = type("Args", (), {"repo": str(repo), "output": str(output), "base": "HEAD", "head": "HEAD",
                                      "root": "D:/gh", "lane": "test", "budget_seconds": 180,
                                      "allow_whole_package": None, "plan": False})()
            with patch.object(rr, "_selector", return_value=plan):
                self.assertEqual(rr.run(args), 1)
            report = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(report["status"], "incomplete")
            self.assertTrue(report["completed"][0]["zeroTests"])
            self.assertIn("zero tests", report["error"])
    def test_runner_rejects_selected_pytest_file_with_zero_collected_tests(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp)
            subprocess = __import__("subprocess")
            # A comment can fool the selector's source scan, but pytest collects no tests.
            (repo / "test_empty.py").write_text("# def test_foo(): pass\n", encoding="utf-8")
            (repo / "conftest.py").write_text(
                "def pytest_sessionfinish(session, exitstatus):\n"
                "    if session.testscollected == 0:\n"
                "        session.exitstatus = 0\n",
                encoding="utf-8",
            )
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True, creationflags=NO_WINDOW)
            (repo / "tracked").write_text("x", encoding="utf-8")
            (repo / ".gitignore").write_text("__pycache__/\n.pytest_cache/\n", encoding="utf-8")
            subprocess.run(["git", "add", "tracked", "test_empty.py", "conftest.py", ".gitignore"], cwd=repo, check=True, creationflags=NO_WINDOW)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            output = Path(temp).parent / (Path(temp).name + "-zero-pytest-report.json")
            plan = {"steps": [{"argv": [sys.executable, "-m", "pytest", "-q", "test_empty.py"], "cwd": ".", "slot": False}]}
            with patch.object(rr, "_selector", return_value=plan):
                args = type("Args", (), {"repo": str(repo), "output": str(output), "base": "HEAD", "head": "HEAD",
                                          "root": "D:/gh", "lane": "test", "budget_seconds": 180,
                                          "allow_whole_package": None, "plan": False})()
                self.assertEqual(rr.run(args), 1)
            report = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(report["status"], "failed")
            self.assertEqual(report["completed"][0]["returncode"], 1)
            self.assertEqual(report["completed"][0]["error"], "pytest collected no tests")
            self.assertIn("no tests ran", report["completed"][0]["tail"])

    def test_no_batch_opt_out_keeps_each_slot_step_in_its_own_process(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp)
            subprocess = __import__("subprocess")
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True, creationflags=NO_WINDOW)
            (repo / "x").write_text("x", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            output = Path(temp).parent / (Path(temp).name + "-report.json")
            plan = {"steps": [{"argv": ["cargo", "+1.97.1", "test", "--lib"], "cwd": ".", "slot": True},
                               {"argv": ["python", "-c", "pass"], "cwd": ".", "slot": False}]}
            calls = []
            envs = []

            def fake_run(command, **kwargs):
                if command[0] == "git":
                    return subprocess.CompletedProcess(command, 0, "HEAD\n" if command[1] == "rev-parse" else "", "")
                calls.append(command)
                envs.append(kwargs["env"])
                stream = kwargs["stdout"]
                stream.write('{"command":"workspace.slot","data":{"waitedSeconds":1,"heldSeconds":2}}')
                return subprocess.CompletedProcess(command, 0, "", "")

            with patch.object(rr, "_selector", return_value=plan), patch.object(rr.subprocess, "run", side_effect=fake_run):
                args = type("Args", (), {"repo": str(repo), "output": str(output), "base": "HEAD", "head": "HEAD",
                                          "root": "D:/gh", "lane": "test", "budget_seconds": 180,
                                          "allow_whole_package": None, "plan": False, "batch_slot": False,
                                          "graphhelm": sys.executable})()
                self.assertEqual(rr.run(args), 0)
            self.assertEqual(len(calls), 2)
            self.assertEqual(calls[0][0:4], [sys.executable, "--json", "workspace", "slot"])
            self.assertEqual(calls[1][0], sys.executable)
            self.assertNotIn("GRAPHHELM_SLOT_BATCH", envs[1])
            self.assertEqual(json.loads(output.read_text(encoding="utf-8"))["heldSeconds"], 2.0)

    def test_unsupported_targets_are_incomplete_before_any_step(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp)
            subprocess = __import__("subprocess")
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True, creationflags=NO_WINDOW)
            (repo / "x").write_text("x", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            output = Path(temp).parent / (Path(temp).name + "-unsupported-report.json")
            marker = Path(temp).parent / (Path(temp).name + "-unsupported-marker")
            plan = {"unsupported": [{"package": "graphhelm-cli", "target": "keel", "reason": "target requires features: keel"}],
                    "steps": [{"argv": ["python", "-c", f"from pathlib import Path; Path(r'{marker}').write_text('ran')"],
                               "cwd": ".", "slot": False}]}
            args = type("Args", (), {"repo": str(repo), "output": str(output), "base": "HEAD", "head": "HEAD",
                                      "root": "D:/gh", "lane": "test", "budget_seconds": 180,
                                      "allow_whole_package": None, "plan": False})()
            with patch.object(rr, "_selector", return_value=plan):
                self.assertEqual(rr.run(args), 1)
            report = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(report["status"], "incomplete")
            self.assertEqual(report["completed"], [])
            self.assertEqual(report["pending"], [0])
            self.assertEqual(report["unsupported"], plan["unsupported"])
            self.assertIn("target requires features", report["error"])
            self.assertFalse(marker.exists())

    def test_a_child_that_finishes_after_deadline_is_recorded_and_stops_next_step(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp)
            subprocess = __import__("subprocess")
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True, creationflags=NO_WINDOW)
            (repo / "x").write_text("x", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            output = Path(temp).parent / (Path(temp).name + "-report.json")
            marker = Path(temp).parent / (Path(temp).name + "-child-marker")
            plan = {"steps": [{"argv": ["python", "-c", f"from pathlib import Path; Path(r'{marker}').write_text('done')"], "cwd": ".", "slot": False},
                               {"argv": ["python", "-c", "raise SystemExit(99)"], "cwd": ".", "slot": False}]}
            real_clock = time.monotonic
            clock_calls = [0]

            def controlled_clock():
                clock_calls[0] += 1
                value = real_clock()
                return value + (2.0 if clock_calls[0] >= 3 else 0)

            with patch.object(rr, "_selector", return_value=plan):
                args = type("Args", (), {"repo": str(repo), "output": str(output), "base": "HEAD", "head": "HEAD",
                                          "root": "D:/gh", "lane": "test", "budget_seconds": 1.0,
                                          "allow_whole_package": None, "plan": False})()
                with patch.object(rr.time, "monotonic", side_effect=controlled_clock):
                    self.assertEqual(rr.run(args), 1)
            report = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(report["status"], "budgetExceeded")
            self.assertEqual(report["completed"][0]["returncode"], 0)
            self.assertTrue(report["completed"][0]["late"])
            self.assertEqual(report["pending"], [1])
            self.assertEqual(marker.read_text(encoding="utf-8"), "done")

    def test_requested_head_must_be_the_current_head(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp)
            subprocess = __import__("subprocess")
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True, creationflags=NO_WINDOW)
            (repo / "x").write_text("one", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "one"], cwd=repo, check=True, creationflags=NO_WINDOW)
            old = subprocess.run(["git", "rev-parse", "HEAD"], cwd=repo, check=True, capture_output=True, text=True, creationflags=NO_WINDOW).stdout.strip()
            (repo / "x").write_text("two", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "two"], cwd=repo, check=True, creationflags=NO_WINDOW)
            output = Path(temp).parent / (Path(temp).name + "-report.json")
            args = type("Args", (), {"repo": str(repo), "output": str(output), "base": old, "head": old,
                                      "root": "D:/gh", "lane": "test", "budget_seconds": 180,
                                      "allow_whole_package": None, "plan": False})()
            self.assertEqual(rr.run(args), 1)
            self.assertIn("actual HEAD", json.loads(output.read_text(encoding="utf-8"))["error"])

    def test_dirty_after_step_is_incomplete_and_slow_empty_plan_expires(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp)
            subprocess = __import__("subprocess")
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True, creationflags=NO_WINDOW)
            (repo / "x").write_text("one", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "one"], cwd=repo, check=True, creationflags=NO_WINDOW)
            output = Path(temp).parent / (Path(temp).name + "-report.json")
            plan = {"steps": [{"argv": ["python", "-c", "open('x','w').write('two')"], "cwd": ".", "slot": False}]}
            with patch.object(rr, "_selector", return_value=plan):
                args = type("Args", (), {"repo": str(repo), "output": str(output), "base": "HEAD", "head": "HEAD",
                                          "root": "D:/gh", "lane": "test", "budget_seconds": 180,
                                          "allow_whole_package": None, "plan": False})()
                self.assertEqual(rr.run(args), 1)
            self.assertEqual(json.loads(output.read_text(encoding="utf-8"))["status"], "incomplete")

            subprocess.run(["git", "checkout", "--", "x"], cwd=repo, check=True, creationflags=NO_WINDOW)
            with patch.object(rr, "_selector", side_effect=lambda *unused: (time.sleep(.02) or {"steps": []})):
                args.budget_seconds = .001
                self.assertEqual(rr.run(args), 1)
            self.assertEqual(json.loads(output.read_text(encoding="utf-8"))["status"], "budgetExceeded")


class ResumeAndPreflightContracts(unittest.TestCase):
    """Real git fixtures; a counter file outside the repo proves whether a step actually ran."""

    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.repo = Path(temp.name) / "repo"
        self.repo.mkdir()
        self.output = Path(temp.name) / "out" / "feedback.json"
        self.counter = Path(temp.name) / "count.txt"
        self.git("init", "-q")
        self.git("config", "user.name", "t")
        self.git("config", "user.email", "t@t")
        self.commit("one")
        self.ok = ["python", "-c", f"open({str(self.counter)!r}, 'a').write('ok\\n')"]
        self.bad = ["python", "-c", f"open({str(self.counter)!r}, 'a').write('bad\\n'); raise SystemExit(3)"]
        self.steps = [self.ok]

    def git(self, *args):
        subprocess = __import__("subprocess")
        return subprocess.run(["git", *args], cwd=self.repo, check=True, capture_output=True, text=True, creationflags=NO_WINDOW).stdout

    def commit(self, text):
        (self.repo / "x").write_text(text, encoding="utf-8")
        self.git("add", "x")
        self.git("commit", "-qm", text)

    def run_once(self, **extra):
        plan = {"steps": [{"argv": argv, "cwd": "apps/studio" if argv[:2] == ["npx", "vitest"] else ".", "slot": False} for argv in self.steps]}
        fields = {"repo": str(self.repo), "output": str(self.output), "base": "HEAD", "head": "HEAD",
                  "root": "D:/gh", "lane": "test", "budget_seconds": 180, "allow_whole_package": None,
                  "plan": False, "min_free_gb": 0.0}
        fields.update(extra)
        with patch.object(rr, "_selector", return_value=plan):
            code = rr.run(type("Args", (), fields)())
        return code, json.loads(self.output.read_text(encoding="utf-8"))

    def runs(self):
        return self.counter.read_text(encoding="utf-8").split() if self.counter.exists() else []

    def test_vitest_exit_zero_without_report_is_incomplete_and_not_reused(self):
        self.steps = [["npx", "vitest", "run", "--maxWorkers=1"]]
        (self.repo / "apps/studio").mkdir(parents=True)
        real_run = rr.subprocess.run
        calls = []

        def child(command, **kwargs):
            if command[0] != "node" or "stdout" not in kwargs:
                return real_run(command, **kwargs)
            calls.append(command)
            kwargs["stdout"].write(" RUN  v4.1.11\n")
            return __import__("subprocess").CompletedProcess(command, 0)

        with patch.object(rr, "_resolve_argv", side_effect=lambda repo, cwd, argv: ["node", "vitest", *argv[2:]]), patch.object(rr.subprocess, "run", side_effect=child):
            code, report = self.run_once()
            self.assertEqual(code, 1)
            self.assertEqual(report["status"], "incomplete")
            self.assertEqual(report["completed"][0]["returncode"], 0)
            self.assertNotIn("reusable", report["completed"][0])
            self.run_once()
        self.assertEqual(len(calls), 2)

    def test_vitest_structured_results_and_resume(self):
        self.steps = [["npx", "vitest", "run", "--maxWorkers=1"]]
        (self.repo / "apps/studio").mkdir(parents=True)
        real_run = rr.subprocess.run
        calls = []
        sample = {"numTotalTests": 1, "numPassedTests": 1, "numFailedTests": 0,
                  "numPendingTests": 0, "numTodoTests": 0, "numFailedTestSuites": 0,
                  "success": True, "testResults": [{"name": "proof.test.ts", "status": "passed"}]}

        def child(command, **kwargs):
            if command[0] != "node" or "stdout" not in kwargs:
                return real_run(command, **kwargs)
            calls.append(command)
            target = Path(next(arg.split("=", 1)[1] for arg in command if arg.startswith("--outputFile.json=")))
            target.write_text(json.dumps(sample), encoding="utf-8")
            kwargs["stdout"].write("Tests 1 passed\n")
            return __import__("subprocess").CompletedProcess(command, 0)

        with patch.object(rr, "_resolve_argv", side_effect=lambda repo, cwd, argv: ["node", "vitest", *argv[2:]]), patch.object(rr.subprocess, "run", side_effect=child):
            code, report = self.run_once()
            self.assertEqual((code, report["status"], report["completed"][0]["vitest"]["passed"]), (0, "passed", 1))
            self.assertTrue(self.run_once()[1]["completed"][0]["reused"])
            self.assertEqual(len(calls), 1)
            sample.update(numPassedTests=0, numFailedTests=1, numFailedTestSuites=1, success=False)
            self.assertEqual(self.run_once(resume=False)[1]["status"], "failed")
            sample.update(numFailedTests=0, numFailedTestSuites=0, numPendingTests=1, success=True)
            self.assertEqual(self.run_once(resume=False)[1]["status"], "incomplete")
            sample.clear()
            self.assertEqual(self.run_once(resume=False)[1]["status"], "incomplete")

    def test_matching_pass_is_reused_without_running_the_child(self):
        self.assertEqual(self.run_once()[0], 0)
        code, report = self.run_once()
        self.assertEqual(code, 0)
        self.assertEqual(self.runs(), ["ok"])
        self.assertTrue(report["completed"][0]["reused"])
        self.assertEqual(report["measure"]["reusedSteps"], 1)

    def test_changed_code_reruns(self):
        self.run_once()
        self.commit("two")
        self.assertFalse(self.run_once()[1]["completed"][0].get("reused"))
        self.assertEqual(self.runs(), ["ok", "ok"])

    def test_changed_environment_reruns_and_unrelated_variable_does_not(self):
        self.run_once()
        with patch.dict(os.environ, {"UNRELATED_VARIABLE_FOR_TEST": "1"}):
            self.assertTrue(self.run_once()[1]["completed"][0]["reused"])
        with patch.dict(os.environ, {"GRAPHHELM_TEST_ONLY_VAR": "1"}):
            self.assertFalse(self.run_once()[1]["completed"][0].get("reused"))
        self.assertEqual(self.runs(), ["ok", "ok"])

    def test_changed_tool_version_reruns(self):
        self.run_once()
        with patch.object(rr, "_tool_fingerprint", return_value={"python": "other"}):
            self.assertFalse(self.run_once()[1]["completed"][0].get("reused"))
        self.assertEqual(self.runs(), ["ok", "ok"])

    def test_failure_is_never_reused_and_resume_starts_at_it(self):
        self.steps = [self.ok, self.bad]
        self.assertEqual(self.run_once()[0], 1)
        code, report = self.run_once()
        self.assertEqual(code, 1)
        self.assertEqual(self.runs(), ["ok", "bad", "bad"])
        self.assertEqual([e.get("reused", False) for e in report["completed"]], [True, False])
        self.assertNotIn("reusable", report["completed"][1])

    def test_unreadable_previous_report_reuses_nothing(self):
        self.output.parent.mkdir(parents=True)
        self.output.write_text("{not json", encoding="utf-8")
        code, report = self.run_once()
        self.assertEqual(code, 0)
        self.assertIn("unreadable", report["resumeNote"])
        self.assertEqual(self.runs(), ["ok"])

    def test_forged_pass_without_reusable_mark_is_not_reused(self):
        self.run_once()
        report = json.loads(self.output.read_text(encoding="utf-8"))
        report["completed"][0].pop("reusable")
        self.output.write_text(json.dumps(report), encoding="utf-8")
        self.assertFalse(self.run_once()[1]["completed"][0].get("reused"))
        self.assertEqual(self.runs(), ["ok", "ok"])

    def test_pass_from_a_run_that_dirtied_the_tree_is_not_reusable(self):
        self.steps = [["python", "-c", "open('x', 'w').write('dirty')"]]
        code, report = self.run_once()
        self.assertEqual(code, 1)
        self.assertNotIn("reusable", report["completed"][0])

    def test_no_resume_runs_everything(self):
        self.run_once()
        self.run_once(resume=False)
        self.assertEqual(self.runs(), ["ok", "ok"])

    def test_preflight_blocks_before_any_step_and_can_be_overridden(self):
        code, report = self.run_once(min_free_gb=1e12)
        self.assertEqual((code, report["status"]), (1, "blocked"))
        self.assertIn("disk:", report["preflight"]["problems"][0])
        self.assertEqual(self.runs(), [])
        self.assertEqual(self.run_once(min_free_gb=1e12, skip_preflight=True)[0], 0)
        self.assertEqual(self.runs(), ["ok"])

    def test_preflight_flags_missing_cli_path_and_relative_target(self):
        with patch.dict(os.environ, {"GRAPHHELM_CLI": str(self.repo / "missing.exe"), "CARGO_TARGET_DIR": "rel"}):
            problems, _ = rr._preflight(self.repo, self.output, [{"argv": ["cargo", "test", "-p", "x"]}], 0.0)
        self.assertTrue(any("GRAPHHELM_CLI" in p for p in problems))
        self.assertTrue(any("relative" in p for p in problems))

    def test_default_stdout_is_the_short_summary(self):
        self.steps = [self.ok, self.bad]
        buffer = __import__("io").StringIO()
        with patch("sys.stdout", buffer):
            self.run_once()
        text = buffer.getvalue()
        self.assertTrue(text.startswith("reached-tests: failed | passed 1 (reused 0) | failed 1 | pending 0"))
        self.assertIn("FAILED step 1", text)
        self.assertIn(str(self.output), text)
        self.assertNotIn('"schema"', text)


if __name__ == "__main__":
    unittest.main()
