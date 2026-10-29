"""Exercise startup rule delivery through the real hook in scratch plugin/project trees."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("session_hook.py")
CONTRACTS = SCRIPT.resolve().parents[2] / "graphhelm-development-contracts"
if not CONTRACTS.exists():
    CONTRACTS = SCRIPT.resolve().parents[3] / "extensions/builtin/graphhelm-development-contracts"


class SessionRules(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.project = self.root / "project"
        self.project.mkdir()
        self.plugin = self.root / "plugins/graphhelm"
        (self.plugin / "hooks").mkdir(parents=True)
        shutil.copy2(SCRIPT, self.plugin / "hooks")
        self.skills = self.root / "plugins/graphhelm-development-contracts/skills"
        for name in ("task-plan", "implement"):
            (self.skills / name).mkdir(parents=True)
            shutil.copy2(CONTRACTS / "skills" / name / "SKILL.md", self.skills / name)
        self.policy = self.project / "extensions/builtin/graphhelm-development-contracts/policies/keel.yaml"
        self.policy.parent.mkdir(parents=True)
        self.policy.write_text('version: "1"\ncard:\n  maxScopePaths: 12\n', encoding="utf-8")
        self.env = {k: v for k, v in os.environ.items() if not k.startswith("GRAPHHELM_")}
        self.env.update({"HOME": str(self.root), "USERPROFILE": str(self.root)})

    def run_hook(self, source="startup", portable=False):
        argv = [sys.executable, str(self.plugin / "hooks/session_hook.py"), "start", "--host", "claude"]
        if portable:
            argv += ["--format", "portable"]
        result = subprocess.run(argv, input=json.dumps({"hook_event_name": "SessionStart",
            "session_id": "rules-session", "cwd": str(self.project), "source": source}),
            text=True, capture_output=True, env=self.env, timeout=20,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout) if result.stdout else {}
        context = report.get("context", "") if portable else report.get("hookSpecificOutput", {}).get("additionalContext", "")
        return context, result.stderr

    def test_policy_delivers_source_sections_and_self_review_file(self):
        context, errors = self.run_hook()
        self.assertEqual(errors, "")
        self.assertIn("Keel step rules", context)
        self.assertIn(".graphhelm/self-review.json", context)
        self.assertIn("write the test first", context)
        self.assertLessEqual(len(context), 11000)
        for name in ("task-plan", "implement"):
            source = (self.skills / name / "SKILL.md").read_text(encoding="utf-8")
            method = source.split("## Method\n", 1)[1].split("\n## ", 1)[0].strip()
            self.assertIn(method, context)

    def test_no_policy_and_reentry_do_not_deliver_step_rules(self):
        for source in ("resume", "clear", "compact"):
            with self.subTest(source=source):
                self.assertNotIn("Keel step rules", self.run_hook(source)[0])
        self.policy.unlink()
        self.assertEqual(self.run_hook()[0], "")
        self.env["GRAPHHELM_KEEL_CONTEXT"] = "1"
        self.assertNotIn("Keel step rules", self.run_hook()[0])

    def test_installed_skill_changes_reach_portable_context(self):
        path = self.skills / "implement/SKILL.md"
        path.write_text(path.read_text(encoding="utf-8").replace("## Method\n", "## Method\n\nSource fidelity sentinel.\n"), encoding="utf-8")
        self.assertIn("Source fidelity sentinel.", self.run_hook(portable=True)[0])

    def test_binding_failure_keeps_local_step_rules(self):
        self.env["GRAPHHELM_EXECUTION_ID"] = "missing-binding"
        context, errors = self.run_hook()
        self.assertIn("Keel step rules", context)
        self.assertIn("UNOBSERVED", context)
        self.assertIn("unobserved", errors)

    def test_missing_or_oversized_skill_is_reported_without_partial_rules(self):
        path = self.skills / "implement/SKILL.md"
        for value in (None, "x" * 40000):
            with self.subTest(value="missing" if value is None else "oversized"):
                if value is None:
                    path.unlink()
                else:
                    path.write_text(value, encoding="utf-8")
                context, errors = self.run_hook()
                self.assertNotIn("Keel step rules", context)
                self.assertIn("step rules unobserved", errors)


if __name__ == "__main__":
    unittest.main()
