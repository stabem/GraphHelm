"""The preflight refuses the environments that wasted earlier pilot sessions."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import native_preflight as pre

NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0


class Preflight(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)

    def settings(self, deny):
        (self.root / ".claude").mkdir(exist_ok=True)
        (self.root / ".claude" / "settings.json").write_text(
            json.dumps({"permissions": {"deny": deny}}), encoding="utf-8")

    def test_deny_everything_is_refused_before_a_session(self):
        # Pilot 010 and the first pilot 012 planner ran under deny:["*"].
        self.settings(["*"])
        self.assertTrue(pre.permission_problems(self.root, ["Edit"]))
        self.settings(["Bash(curl *)"])
        self.assertEqual(pre.permission_problems(self.root, ["Edit", "Bash"]), [])
        self.settings(["Edit(*)"])
        self.assertTrue(pre.permission_problems(self.root, ["Edit"]))

    def test_only_a_subscription_login_without_api_keys_passes(self):
        ok = json.dumps({"loggedIn": True, "authMethod": "claude.ai"})
        self.assertEqual(pre.auth_problems("claude", ok, {}), [])
        self.assertTrue(pre.auth_problems("claude", ok, {"ANTHROPIC_API_KEY": "x"}))
        self.assertTrue(pre.auth_problems("claude", json.dumps({"loggedIn": False}), {}))
        self.assertTrue(pre.auth_problems("claude", "Not logged in", {}))
        self.assertEqual(pre.auth_problems("codex", "Logged in using ChatGPT", {}), [])
        self.assertTrue(pre.auth_problems("codex", "Logged in using an API key", {}))

    def test_a_listed_but_failed_mcp_server_is_missing(self):
        listing = ("plugin:graphhelm:graphhelm: C:\\g.exe mcp --project . - \u2714 Connected\n"
                   "obs: x - \u2718 Failed to connect\n")
        self.assertEqual(pre.mcp_problems(listing, ["plugin:graphhelm:graphhelm"]), [])
        self.assertTrue(pre.mcp_problems(listing, ["graphhelm"]))
        self.assertTrue(pre.mcp_problems(listing, ["obs"]))

    def test_mirror_must_sit_clean_on_the_candidate_head(self):
        git = lambda *a: subprocess.run(["git", "-C", str(self.root), *a], capture_output=True,
                                        text=True, check=True, creationflags=NO_WINDOW).stdout.strip()
        git("init", "-q")
        (self.root / "f").write_text("1", encoding="utf-8")
        git("add", "f")
        git("-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "c")
        head = git("rev-parse", "HEAD")
        self.assertEqual(pre.mirror_problems(self.root, head), [])
        self.assertTrue(pre.mirror_problems(self.root, "0" * 40))
        (self.root / "f").write_text("2", encoding="utf-8")
        self.assertTrue(pre.mirror_problems(self.root, head))
        self.assertTrue(pre.mirror_problems(self.root / "absent", head))

    def test_runner_must_match_the_frozen_digest(self):
        runner = self.root / "run_reached.py"
        runner.write_bytes(b"v1")
        digest = hashlib.sha256(b"v1").hexdigest()
        self.assertEqual(pre.runner_problems(runner, digest), [])
        runner.write_bytes(b"v2")
        self.assertTrue(pre.runner_problems(runner, digest))


if __name__ == "__main__":
    unittest.main()
