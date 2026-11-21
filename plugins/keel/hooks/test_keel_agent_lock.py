"""Table test for the Keel agent lock (#2987) and the route card (#2995).

Cost: pure functions, file reads and two hook-command subprocesses for the card; offline, no host needed.
"""
import json
import os
import re
import subprocess
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import keel_agent_lock as lock  # noqa: E402
import keel_route_card as route_card  # noqa: E402

COPY = HERE.parents[1] / "graphhelm" / "hooks" / "keel_agent_lock.py"
ROUTE_COPY = HERE.parents[1] / "graphhelm" / "hooks" / "keel_route_card.py"
PLUGINS = HERE.parents[1]
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0

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
    ("timeout 5 sleep 1", "sleep"),
    ("timeout -s KILL 5 sleep 1", "sleep"),
    ("timeout --signal=KILL --kill-after 2 5 sleep 1", "sleep"),
    ("make && timeout 5m git stash", "stash"),
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
    ("timeout 30 python x.py", None),
    ("timeout -s KILL 30 python sleep.py", None),
    ("timeout 5", None),
]


def agent_call(subagent_type, prompt, tool="Agent"):
    return json.dumps({"tool_name": tool, "tool_input": {"subagent_type": subagent_type, "prompt": prompt}}).encode()


BAD_BRIEFS = [
    "no identity line at all",
    "Session: keel-author fix the lock",
    "Session: keel-reviewer",
    "`Session: keel-author <task>` is what I was given",
    "Session: <keel-scout> [x]",
    "Session: keel:keel-author [a1]",
    "Session: Keel-Author [a1]",
    "Session:",
    "Session: `keel-author`",
    # #2995: an unfilled template is refused. The first case was a good brief in #2987 (the `<` was
    # stripped); it is refused now by intent.
    "Do the task.\nSession: <session name> [ref]",
    "Session: <your ListAgents name [ref]> · Head: <sha8>",
    "Session: Real name [a1] (keel-author subagent)\nSession: <your session name> [ref]",
    "Session: <your session name> [ref] (keel-author subagent)\nSession: Real name [a1]",
    "`Session: <your session name> [ref] (keel-author subagent)`",
    "Quote of the template: `Session: <name>` and nothing else",
]
GOOD_BRIEFS = [
    "Session: Token economy [a39368] (keel-author subagent) · Head: f4c6ea82",
    "`Session: Token economy GraphHelm subagents [a39368] (keel-author subagent)`",
    "Session: orchestrator without a ref (keel-reviewer subagent)",
    "Wrong: `Session: keel-author`\nSession: Real name [a1] (keel-reviewer subagent)",
    # a filled line with a placeholder after the name is the identity line, not a template
    "Session: Token economy [a1] (keel-author subagent) · Head: <sha8 of your head>",
    # the template quoted in a code span, after or before a filled line, is not refused
    "Session: Real name [a1] (keel-author subagent)\nDo not write `Session: <your session name> [ref]` literally.",
    "Wrong: `Session: <your session name>`\nSession: Real name [a1] (keel-reviewer subagent)",
    "Session: Real name [a1] (keel-author subagent)\n`Session: <name>`",
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
    "graphhelm:keel-journey-qa": False,
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

    def test_a_brief_that_starts_a_keel_agent_needs_a_session_name_that_is_not_a_role(self):
        for agent in ("keel-author", "keel:keel-reviewer", "graphhelm:keel-scout", "keel-journey-qa"):
            for brief in BAD_BRIEFS:
                with self.subTest(agent=agent, brief=brief):
                    out = json.loads(lock.decide(agent_call(agent, brief)))["hookSpecificOutput"]
                    self.assertEqual(out["permissionDecision"], "deny")
                    self.assertIn("Session:", out["permissionDecisionReason"])
                    self.assertIn("(keel-author subagent)", out["permissionDecisionReason"])
            for brief in GOOD_BRIEFS:
                with self.subTest(agent=agent, brief=brief):
                    self.assertIsNone(lock.decide(agent_call(agent, brief)))
        self.assertIn("deny", lock.decide(agent_call("keel-author", 5)))

    def test_any_other_agent_is_never_refused(self):
        for agent in ("general-purpose", "Explore", "other:keel-author", "keel:keel-authority", "", None, 7):
            with self.subTest(agent=agent):
                self.assertIsNone(lock.decide(agent_call(agent, "Session: keel-author")))
        for raw in (b'{"tool_name": "Agent"}', b'{"tool_name": "Agent", "tool_input": []}'):
            self.assertIsNone(lock.decide(raw))
        self.assertIsNone(lock.decide(json.dumps({"tool_name": "Agent", "tool_input": {"prompt": "x"}}).encode()))
        self.assertIsNone(lock.decide(json.dumps({"tool_name": "Read", "tool_input": {
            "subagent_type": "keel-author", "prompt": "x"}}).encode()))

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


STEPS = ("read-only question", "write", "mechanical write", "review, then merge", "journey QA")


def rows():
    out = {}
    for line in route_card.CARD.splitlines():
        cells = [cell.strip() for cell in line.split(" | ")]
        if len(cells) == 4:
            for step in STEPS:
                if cells[0] == step or cells[0].startswith(step + " ("):
                    out[step] = cells
    return out


def agent_model(agent):
    namespace, name = agent.split(":")
    text = (PLUGINS / ("keel" if namespace == "keel" else "graphhelm") / "agents" / (name + ".md")).read_text("utf-8")
    return re.search(r"^model: (\w+)$", text, re.MULTILINE).group(1)


class RouteCard(unittest.TestCase):
    def test_the_card_has_one_row_per_step_and_is_short(self):
        self.assertEqual(tuple(rows()), STEPS)
        self.assertLessEqual(len(route_card.CARD.splitlines()), 25)

    def test_every_agent_exists_with_the_model_the_card_names(self):
        for step, (_, agent, model, _) in rows().items():
            with self.subTest(step=step):
                if step == "mechanical write":
                    self.assertTrue(model.startswith("haiku"))  # a per-call override of the author's model
                    self.assertEqual(agent, rows()["write"][1])
                else:
                    self.assertEqual(agent_model(agent), model)

    def test_every_skill_the_card_names_is_shipped_by_the_keel_plugin(self):
        for step, (_, _, _, skills) in rows().items():
            for skill in re.findall(r"keel:([\w-]+)", skills):
                with self.subTest(step=step, skill=skill):
                    self.assertTrue((PLUGINS / "keel" / "skills" / skill / "SKILL.md").is_file())

    def test_the_default_family_is_keel_and_the_exception_is_named(self):
        self.assertIn("default family is `keel:`", route_card.CARD)
        self.assertEqual([step for step, row in rows().items() if row[1].startswith("graphhelm:")], ["journey QA"])
        self.assertFalse((PLUGINS / "keel" / "agents" / "keel-journey-qa.md").exists())

    def test_the_hook_command_from_hooks_json_prints_the_card_as_session_context(self):
        for plugin in ("keel", "graphhelm"):
            with self.subTest(plugin=plugin):
                spec = json.loads((PLUGINS / plugin / "hooks" / "hooks.json").read_text("utf-8"))
                entries = [e for e in spec["hooks"]["SessionStart"]
                           if "keel_route_card.py" in json.dumps(e)]
                self.assertEqual(len(entries), 1)
                self.assertEqual(entries[0]["matcher"], "startup|resume|clear|compact")
                hook = entries[0]["hooks"][0]
                root = str(PLUGINS / plugin)
                args = [a.replace("${CLAUDE_PLUGIN_ROOT}", root) for a in hook["args"]]
                done = subprocess.run([sys.executable, *args], input=b"", capture_output=True, timeout=30, creationflags=NO_WINDOW)
                self.assertEqual(done.returncode, 0)
                self.assertEqual(done.stderr, b"")
                out = json.loads(done.stdout.decode("ascii"))["hookSpecificOutput"]
                self.assertEqual((out["hookEventName"], out["additionalContext"]), ("SessionStart", route_card.CARD))

    def test_a_script_that_cannot_run_prints_nothing_and_exits_zero(self):
        spec = json.loads((PLUGINS / "keel" / "hooks" / "hooks.json").read_text("utf-8"))
        hook = [e for e in spec["hooks"]["SessionStart"]][0]["hooks"][0]
        args = [a.replace("${CLAUDE_PLUGIN_ROOT}", str(HERE / "missing")) for a in hook["args"]]
        done = subprocess.run([sys.executable, *args], input=b"", capture_output=True, timeout=30, creationflags=NO_WINDOW)
        self.assertEqual((done.returncode, done.stdout, done.stderr), (0, b"", b""))

    def test_the_graphhelm_plugin_ships_the_same_file(self):
        self.assertEqual(ROUTE_COPY.read_bytes().replace(b"\r\n", b"\n"),
                         (HERE / "keel_route_card.py").read_bytes().replace(b"\r\n", b"\n"))


if __name__ == "__main__":
    unittest.main()
