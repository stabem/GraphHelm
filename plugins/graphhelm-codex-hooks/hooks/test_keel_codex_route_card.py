"""Table test for the Codex Keel route card (#2997): the hook command from codex-hooks.json prints the card.

Cost: file reads and a few hook-command subprocesses; offline, no Codex host needed. The real Codex run
is not made here (hook trust is the owner's act), so this proves the hook, not the host loading it.
"""
import json
import os
import subprocess
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import keel_codex_route_card as card  # noqa: E402

PLUGIN = HERE.parent
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
BUILT_IN_SUBAGENTS = ("explorer", "worker", "default")  # Codex subagents documentation, "Custom agents"


def hook_entry():
    spec = json.loads((HERE / "codex-hooks.json").read_text("utf-8"))
    entries = [e for e in spec["hooks"]["SessionStart"] if "keel_codex_route_card.py" in json.dumps(e)]
    assert len(entries) == 1
    return entries[0]


def run_card(script, stdin=b""):
    return subprocess.run([sys.executable, str(script)], input=stdin, capture_output=True, timeout=30,
                          creationflags=NO_WINDOW)


class CodexRouteCard(unittest.TestCase):
    def test_the_hook_is_registered_for_session_start_with_the_plugin_root_variable(self):
        entry = hook_entry()
        self.assertEqual(entry["matcher"], "startup|resume|clear|compact")
        self.assertIn('"${PLUGIN_ROOT}/hooks/keel_codex_route_card.py"', entry["hooks"][0]["command"])
        self.assertTrue((PLUGIN / "hooks" / "keel_codex_route_card.py").is_file())

    def test_the_script_prints_the_card_as_session_context_and_nothing_else(self):
        for stdin in (b"", b'{"hook_event_name":"SessionStart","source":"startup","session_id":"s","cwd":"."}', b"not json"):
            with self.subTest(stdin=stdin[:20]):
                done = run_card(HERE / "keel_codex_route_card.py", stdin)
                self.assertEqual((done.returncode, done.stderr), (0, b""))
                out = json.loads(done.stdout.decode("ascii"))["hookSpecificOutput"]
                self.assertEqual((out["hookEventName"], out["additionalContext"]), ("SessionStart", card.CARD))

    def test_the_card_uses_codex_names_and_no_claude_code_ones(self):
        for name in BUILT_IN_SUBAGENTS[:2]:
            self.assertIn(name, card.CARD)
        for claude_name in ("keel:keel-author", "keel:keel-scout", "haiku", "opus", "sonnet", "Agent tool"):
            self.assertNotIn(claude_name, card.CARD)
        self.assertLessEqual(len(card.CARD.splitlines()), 12)
        self.assertLess(len(card.CARD), 1500)  # far under the 2500-token default limit

    def test_the_card_carries_the_three_rules(self):
        self.assertIn("one promise", card.CARD)
        self.assertIn("explorer | cheapest", card.CARD)
        self.assertIn("mechanical write (no judgement) | worker | cheapest", card.CARD)

    def test_the_graphhelm_plugin_ships_the_same_script(self):
        copy = PLUGIN.parent / "graphhelm" / "hooks" / "keel_codex_route_card.py"
        self.assertEqual(copy.read_bytes().replace(b"\r\n", b"\n"),
                         (HERE / "keel_codex_route_card.py").read_bytes().replace(b"\r\n", b"\n"))

    def test_no_lock_ships_here_and_nothing_here_can_refuse(self):
        self.assertFalse((HERE / "keel_agent_lock.py").exists())
        spec = json.loads((HERE / "codex-hooks.json").read_text("utf-8"))
        self.assertNotIn("PreToolUse", spec["hooks"])
        source = (HERE / "keel_codex_route_card.py").read_text("utf-8")
        self.assertNotIn("subprocess", source.replace("no `subprocess`", ""))
        self.assertNotIn("permissionDecision", source)


if __name__ == "__main__":
    unittest.main()
