"""Table test for the Keel agent lock (#2987): which callers are bound, which commands are refused.

Cost: pure functions plus one subprocess-free read of the graphhelm copy; offline, no host needed.
"""
import json
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import keel_agent_lock as lock  # noqa: E402

COPY = HERE.parents[1] / "graphhelm" / "hooks" / "keel_agent_lock.py"

REFUSED = [
    ("cat <<EOF\nx\nEOF", "heredoc"),
    ("cat <<-EOF\nx\nEOF", "heredoc"),
    ("cat << 'EOF'\nx\nEOF", "heredoc"),
    ('cat <<"EOF"\nx\nEOF', "heredoc"),
    ("cat <<\\EOF\nx\nEOF", "heredoc"),
    ('git commit -m "$(cat <<\'EOF\'\nmsg\nEOF\n)"', "heredoc"),
    ("echo `cat <<EOF\nx\nEOF`", "heredoc"),
    ("sleep 5", "sleep"),
    ("sleep 0.5 && echo done", "sleep"),
    ("make; sleep 2", "sleep"),
    ("if x; then sleep 1; fi", "sleep"),
    ("time sleep 5", "sleep"),
    ("/usr/bin/sleep 5", "sleep"),
    ("x=$(sleep 5)", "sleep"),
    ("Start-Sleep -Seconds 5", "sleep"),
    ("start-sleep 5", "sleep"),
    ("git stash", "stash"),
    ("git stash pop", "stash"),
    ("git stash push -m x", "stash"),
    ("git stash drop", "stash"),
    ("git -C x stash pop", "stash"),
    ("git -c core.pager=cat stash apply", "stash"),
    ("cd x && git stash", "stash"),
]
PASSED = [
    ("", None),
    ("echo hi", None),
    ('git commit -m "use << in the message"', None),
    ("git commit -m 'git stash and sleep 5 and cat <<EOF'", None),
    ('echo "sleep 5"', None),
    ("cat <<< word", None),
    ("echo $((1<<2))", None),
    ("echo a # <<EOF", None),
    ("git stash list", None),
    ("git stash show -p", None),
    ("git -C x stash list", None),
    ("git checkout stash-fix", None),
    ("git log --grep stash", None),
    ("grep sleep f", None),
    ("cat tools/sleep.txt", None),
    ("python sleep.py 5", None),
    ("./sleep.sh 5", None),
    ("echo sleep 5", None),
    ("git status", None),
]
CALLERS = {
    "keel:keel-author": True,
    "keel:keel-reviewer": True,
    "keel:keel-scout": True,
    "graphhelm:keel-author": True,
    "graphhelm:keel-reviewer": True,
    None: False,
    "": False,
    "keel-author": False,
    "other:keel-author": False,
    "keel:keel-authority": False,
    "keel:keel-journey-qa": False,
    "general-purpose": False,
    7: False,
}


class KeelAgentLock(unittest.TestCase):
    def test_who_is_bound(self):
        for agent_type, bound in CALLERS.items():
            with self.subTest(agent_type=agent_type):
                self.assertIs(lock.is_keel_agent(agent_type), bound)

    def test_refused_and_passed_commands_in_bash(self):
        for command, rule in REFUSED:
            with self.subTest(command=command):
                found = lock.refusal(command, "Bash")
                self.assertIsNotNone(found)
                self.assertEqual(found[0], rule)
                self.assertTrue(found[1])
        for command, _ in PASSED:
            with self.subTest(command=command):
                self.assertIsNone(lock.refusal(command, "Bash"))

    def test_powershell_keeps_sleep_and_stash_but_not_the_heredoc_rule(self):
        self.assertEqual(lock.refusal("Start-Sleep -Seconds 5", "PowerShell")[0], "sleep")
        self.assertEqual(lock.refusal("git stash pop", "PowerShell")[0], "stash")
        self.assertEqual(lock.refusal("sleep 3", "PowerShell")[0], "sleep")
        self.assertIsNone(lock.refusal("git stash list", "PowerShell"))
        self.assertIsNone(lock.refusal("Get-Content C:\\sleep\\a.txt", "PowerShell"))
        self.assertIsNone(lock.refusal("$a = 'x'; Write-Output \"it's << fine\"", "PowerShell"))

    def test_message_names_the_rule_and_the_allowed_form(self):
        self.assertIn("file", lock.refusal("cat <<EOF", "Bash")[1])
        self.assertIn("itself", lock.refusal("sleep 5", "Bash")[1])
        self.assertIn("branch", lock.refusal("git stash", "Bash")[1])

    def test_decision_denies_only_a_bound_caller_with_exit_zero_json(self):
        event = {"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": "sleep 5"}}
        main_session = json.dumps(event).encode()
        self.assertIsNone(lock.decide(main_session))
        agent = json.dumps({**event, "agent_type": "keel:keel-author"}).encode()
        reply = json.loads(lock.decide(agent))
        out = reply["hookSpecificOutput"]
        self.assertEqual((out["hookEventName"], out["permissionDecision"]), ("PreToolUse", "deny"))
        self.assertIn("sleep", out["permissionDecisionReason"])
        clean = json.dumps({**event, "agent_type": "keel:keel-author", "tool_input": {"command": "ls"}}).encode()
        self.assertIsNone(lock.decide(clean))
        other_tool = json.dumps({**event, "agent_type": "keel:keel-author", "tool_name": "Read"}).encode()
        self.assertIsNone(lock.decide(other_tool))

    def test_a_broken_input_lets_the_command_through_and_says_so(self):
        for raw in (b"", b"not json", b"[]", b'{"agent_type": "keel:keel-author", "tool_name": "Bash"}',
                    b'{"agent_type": "keel:keel-author", "tool_name": "Bash", "tool_input": {"command": 5}}',
                    b"\xff\xfe"):
            with self.subTest(raw=raw):
                reply = lock.decide(raw)
                if reply is not None:
                    self.assertNotIn("permissionDecision", reply)
                    self.assertIn("skipped", reply)

    def test_non_ascii_command_is_read_as_utf8(self):
        raw = json.dumps({"agent_type": "keel:keel-author", "tool_name": "Bash",
                          "tool_input": {"command": "echo '\u00e7\u00e3o' && sleep 1"}}).encode()
        self.assertIn("deny", lock.decide(raw))

    def test_the_graphhelm_plugin_ships_the_same_file(self):
        # A plugin cannot read a sibling plugin's files, so each ships the file; this is the one source.
        self.assertEqual(COPY.read_bytes().replace(b"\r\n", b"\n"),
                         (HERE / "keel_agent_lock.py").read_bytes().replace(b"\r\n", b"\n"))


if __name__ == "__main__":
    unittest.main()
