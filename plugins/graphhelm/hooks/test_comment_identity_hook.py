"""The identity-line hook, run as the real process the way the host runs it."""
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HOOK = Path(__file__).resolve().parent / "comment_identity_hook.py"
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
GOOD = "Session: Ronda Keel [local_ab12cd34] (keel-author subagent) · Head: 65c3b500"
COMMANDS = ("gh pr comment 5", "gh issue comment 5", "gh pr review 5 --comment")


def run(command, tool="Bash", cwd=None):
    event = {"tool_name": tool, "tool_input": {"command": command}}
    if cwd:
        event["cwd"] = cwd
    done = subprocess.run([sys.executable, str(HOOK)], input=json.dumps(event).encode(), capture_output=True,
                          timeout=20, creationflags=NO_WINDOW)
    assert done.returncode == 0, done.stderr
    out = done.stdout.decode().strip()
    return json.loads(out)["hookSpecificOutput"]["permissionDecision"] if out else "allow"


class Body(unittest.TestCase):
    def test_body_flag_denied_without_line_allowed_with_it(self):
        for base in COMMANDS:
            for flag in ("--body", "-b"):
                with self.subTest(base=base, flag=flag):
                    self.assertEqual(run(f'{base} {flag} "reviewer: x"'), "deny")
                    self.assertEqual(run(f'{base} {flag} "{GOOD}\n\nreviewer: x"'), "allow")
        self.assertEqual(run('gh pr comment 5 --body="hello"', tool="PowerShell"), "deny")
        self.assertEqual(run(f"gh pr comment 5 --body '{GOOD}'", tool="PowerShell"), "allow")

    def test_body_file_denied_without_line_allowed_with_it(self):
        with tempfile.TemporaryDirectory() as folder:
            bad, good, bom = (Path(folder) / n for n in ("bad.md", "good.md", "bom.md"))
            bad.write_text("no line\n", encoding="utf-8")
            good.write_text("\n" + GOOD + "\nbody\n", encoding="utf-8")
            bom.write_bytes(b"\xef\xbb\xbf" + GOOD.encode() + b"\n")
            for base in COMMANDS:
                for flag in ("--body-file", "-F"):
                    with self.subTest(base=base, flag=flag):
                        self.assertEqual(run(f"{base} {flag} {bad}"), "deny")
                        self.assertEqual(run(f"{base} {flag} {good}"), "allow")
                        self.assertEqual(run(f"{base} {flag} {bom}"), "allow")
            self.assertEqual(run("gh pr comment 5 -F bad.md", cwd=folder), "deny")

    def test_placeholder_line_denied(self):
        for line in ("Session: <name> [ref] · Head: 65c3b500", "Session: Ronda [<ref>] · Head: 65c3b500",
                     "Session: Ronda [ref] · Head: <sha8>", "Session: Ronda [ref] · Head: 65c3b50"):
            with self.subTest(line=line):
                self.assertEqual(run(f'gh issue comment 5 --body "{line}"'), "deny")

    def test_deny_message_shows_the_expected_form(self):
        event = {"tool_name": "Bash", "tool_input": {"command": 'gh pr comment 1 -b "x"'}}
        out = subprocess.run([sys.executable, str(HOOK)], input=json.dumps(event).encode(),
                             capture_output=True, timeout=20, creationflags=NO_WINDOW).stdout.decode()
        self.assertIn("Head: <sha8>", json.loads(out)["hookSpecificOutput"]["permissionDecisionReason"])


class Allowed(unittest.TestCase):
    def test_unknown_body_source_is_allowed(self):
        for command in ("gh pr comment 5 --body-file -", "gh pr comment 5 --body-file /no/such/file.md",
                        'gh pr comment 5 --body "$(cat <<\'EOF\'\nx\nEOF\n)"', "gh pr comment 5",
                        "gh pr review 5 --approve", "gh pr comment 5 --body", "gh pr comment 'unterminated",
                        "gh pr comment 5 --body-file $env:BODY"):
            with self.subTest(command=command):
                self.assertEqual(run(command), "allow")
        self.assertEqual(subprocess.run([sys.executable, str(HOOK)], input=b"not json", capture_output=True,
                                        timeout=20, creationflags=NO_WINDOW).returncode, 0)

    def test_unrelated_commands_untouched(self):
        for command in ("gh pr view 5 --json body", "gh pr create --body x", "gh issue create --body x",
                        "git commit -m 'gh pr comment --body x'", "ls", 'echo "pr comment"'):
            with self.subTest(command=command):
                self.assertEqual(run(command), "allow")
        self.assertEqual(run('gh pr comment 5 --body "x"', tool="Edit"), "allow")

    def test_a_later_command_in_a_chain_is_still_checked(self):
        self.assertEqual(run('git status && gh pr comment 5 --body "x"'), "deny")
        self.assertEqual(run('gh pr view 5; gh pr comment 5 --body "x"'), "deny")


if __name__ == "__main__":
    unittest.main()
