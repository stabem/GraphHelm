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


class RunnerContracts(unittest.TestCase):
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
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
            (repo / "x").write_text("x", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True)
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
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
            (repo / "x").write_text("x", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True)
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
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
            (repo / "x").write_text("x", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True)
            output = Path(temp).parent / (Path(temp).name + "-browser-mixed-report.json")
            marker = Path(temp).parent / (Path(temp).name + "-browser-mixed-marker")
            plan = {"steps": [
                {"argv": ["python", "-c", "raise SystemExit(99)"], "cwd": ".", "slot": False, "observer": "browser"},
                {"argv": ["python", "-c", f"from pathlib import Path; Path(r'{marker}').write_text('ran')"], "cwd": ".", "slot": False},
            ]}
            args = type("Args", (), {"repo": str(repo), "output": str(output), "base": "HEAD", "head": "HEAD",
                                      "root": "D:/gh", "lane": "test", "budget_seconds": 180,
                                      "allow_whole_package": None, "plan": False, "include_browser": False})()
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
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
            (repo / "x").write_text("x", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True)
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
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
            (repo / "x").write_text("x", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True)
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
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
            (repo / "tracked").write_text("x", encoding="utf-8")
            (repo / ".gitignore").write_text("__pycache__/\n.pytest_cache/\n", encoding="utf-8")
            subprocess.run(["git", "add", "tracked", "test_fixture.py", "test_control.py", ".gitignore"], cwd=repo, check=True)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True)
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
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
            (repo / "empty_test.py").write_text("# no unittest cases\n", encoding="utf-8")
            (repo / ".gitignore").write_text("__pycache__/\n.pytest_cache/\n", encoding="utf-8")
            subprocess.run(["git", "add", "empty_test.py", ".gitignore"], cwd=repo, check=True)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True)
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
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
            (repo / "tracked").write_text("x", encoding="utf-8")
            (repo / ".gitignore").write_text("__pycache__/\n.pytest_cache/\n", encoding="utf-8")
            subprocess.run(["git", "add", "tracked", "test_empty.py", "conftest.py", ".gitignore"], cwd=repo, check=True)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True)
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

    def test_each_slot_step_gets_its_own_slot_process(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp)
            subprocess = __import__("subprocess")
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
            (repo / "x").write_text("x", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True)
            output = Path(temp).parent / (Path(temp).name + "-report.json")
            plan = {"steps": [{"argv": ["cargo", "+1.97.1", "test", "--lib"], "cwd": ".", "slot": True},
                               {"argv": ["python", "-c", "pass"], "cwd": ".", "slot": False}]}
            calls = []

            def fake_run(command, **kwargs):
                if command[0] == "git":
                    return subprocess.CompletedProcess(command, 0, "HEAD\n" if command[1] == "rev-parse" else "", "")
                calls.append(command)
                stream = kwargs["stdout"]
                stream.write('{"command":"workspace.slot","data":{"waitedSeconds":1,"heldSeconds":2}}')
                return subprocess.CompletedProcess(command, 0, "", "")

            with patch.object(rr, "_selector", return_value=plan), patch.object(rr.subprocess, "run", side_effect=fake_run):
                args = type("Args", (), {"repo": str(repo), "output": str(output), "base": "HEAD", "head": "HEAD",
                                          "root": "D:/gh", "lane": "test", "budget_seconds": 180,
                                          "allow_whole_package": None, "plan": False,
                                          "graphhelm": sys.executable})()
                self.assertEqual(rr.run(args), 0)
            self.assertEqual(len(calls), 2)
            self.assertEqual(calls[0][0:4], [sys.executable, "--json", "workspace", "slot"])
            self.assertEqual(calls[1][0], sys.executable)
            self.assertEqual(json.loads(output.read_text(encoding="utf-8"))["heldSeconds"], 2.0)

    def test_unsupported_targets_are_incomplete_before_any_step(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp)
            subprocess = __import__("subprocess")
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
            (repo / "x").write_text("x", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True)
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
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
            (repo / "x").write_text("x", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"], cwd=repo, check=True)
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
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
            (repo / "x").write_text("one", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "one"], cwd=repo, check=True)
            old = subprocess.run(["git", "rev-parse", "HEAD"], cwd=repo, check=True, capture_output=True, text=True).stdout.strip()
            (repo / "x").write_text("two", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "two"], cwd=repo, check=True)
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
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
            (repo / "x").write_text("one", encoding="utf-8")
            subprocess.run(["git", "add", "x"], cwd=repo, check=True)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "one"], cwd=repo, check=True)
            output = Path(temp).parent / (Path(temp).name + "-report.json")
            plan = {"steps": [{"argv": ["python", "-c", "open('x','w').write('two')"], "cwd": ".", "slot": False}]}
            with patch.object(rr, "_selector", return_value=plan):
                args = type("Args", (), {"repo": str(repo), "output": str(output), "base": "HEAD", "head": "HEAD",
                                          "root": "D:/gh", "lane": "test", "budget_seconds": 180,
                                          "allow_whole_package": None, "plan": False})()
                self.assertEqual(rr.run(args), 1)
            self.assertEqual(json.loads(output.read_text(encoding="utf-8"))["status"], "incomplete")

            subprocess.run(["git", "checkout", "--", "x"], cwd=repo, check=True)
            with patch.object(rr, "_selector", side_effect=lambda *unused: (time.sleep(.02) or {"steps": []})):
                args.budget_seconds = .001
                self.assertEqual(rr.run(args), 1)
            self.assertEqual(json.loads(output.read_text(encoding="utf-8"))["status"], "budgetExceeded")


if __name__ == "__main__":
    unittest.main()
