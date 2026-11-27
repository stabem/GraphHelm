"""Proves the hook wiring check: names missing shipped entries, honours plugin enablement, reads the shipped manifest."""
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import hook_wiring_check as check

SHIPPED = json.loads(check.MANIFEST.read_text(encoding="utf-8"))


def run(content: str | None, manifest_path: Path | None = None) -> tuple[int, str]:
    """Run the check's main on a temp settings file; returns (exit code, stdout)."""
    import contextlib
    import io

    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "settings.json"
        if content is not None:
            path.write_text(content, encoding="utf-8")
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
            if manifest_path is None:
                code = check.main(["x", str(path)])
            else:
                with mock.patch.object(check, "MANIFEST", manifest_path):
                    code = check.main(["x", str(path)])
        return code, out.getvalue()


class HookWiringCheck(unittest.TestCase):
    def test_only_skill_record_hook_lists_the_other_thirteen(self):
        settings = {"hooks": {"PostToolUse": [{"matcher": "Skill", "hooks": [
            {"type": "command", "command": "python /x/skill_record_hook.py"}]}]}}
        code, out = run(json.dumps(settings))
        self.assertEqual(code, 1)
        self.assertEqual(len(out.strip().splitlines()), 13)
        self.assertNotIn("skill_record_hook.py", out)
        self.assertIn("event=PreToolUse matcher=Bash|PowerShell script=comment_identity_hook.py", out)

    def test_enabling_the_plugin_lists_nothing(self):
        code, out = run(json.dumps({"enabledPlugins": {"graphhelm@graphhelm": True}}))
        self.assertEqual((code, out), (0, ""))

    def test_wiring_every_shipped_entry_lists_nothing(self):
        hooks = {}
        for event, groups in SHIPPED["hooks"].items():
            hooks[event] = [{"matcher": g.get("matcher", ""), "hooks": [
                {"type": "command", "command": h.get("command", ""), "args": h.get("args", [])}
                for h in g["hooks"]]} for g in groups]
        code, out = run(json.dumps({"hooks": hooks}))
        self.assertEqual((code, out), (0, ""))

    def test_matcher_that_does_not_cover_the_shipped_one_is_missing(self):
        def wired(matcher):
            return json.dumps({"hooks": {"PreToolUse": [{"matcher": matcher, "hooks": [
                {"type": "command", "command": "python /x/comment_identity_hook.py"}]}]}})
        needle = "script=comment_identity_hook.py"
        self.assertIn(needle, run(wired("Edit"))[1])
        self.assertIn(needle, run(wired("Bash"))[1])
        for covering in ("Bash|PowerShell", "PowerShell|Bash|Edit", "", "*"):
            self.assertNotIn(needle, run(wired(covering))[1])

    def test_shipped_entry_without_a_python_script_is_listed_by_command(self):
        fake = {"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "node lock.js"}]}]}}
        with tempfile.TemporaryDirectory() as tmp:
            manifest = Path(tmp) / "hooks.json"
            manifest.write_text(json.dumps(fake), encoding="utf-8")
            self.assertEqual(run("{}", manifest)[1].strip(), "missing: event=Stop matcher=- script=node lock.js")
            wired = json.dumps({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "node lock.js"}]}]}})
            self.assertEqual(run(wired, manifest), (0, ""))

    def test_list_comes_from_the_shipped_manifest(self):
        fake = {"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "python new_lock_hook.py"}]}]}}
        with tempfile.TemporaryDirectory() as tmp:
            manifest = Path(tmp) / "hooks.json"
            manifest.write_text(json.dumps(fake), encoding="utf-8")
            code, out = run("{}", manifest)
        self.assertEqual(code, 1)
        self.assertEqual(out.strip(), "missing: event=Stop matcher=- script=new_lock_hook.py")

    def test_unreadable_or_non_json_is_reported_not_clean(self):
        for content in (None, "not json {", "[1]"):
            code, out = run(content)
            self.assertEqual((code, out), (2, ""))


if __name__ == "__main__":
    unittest.main()
