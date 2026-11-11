"""A session that loads a skill leaves one `skill.loaded` signal on its bound execution.

No network: the binding and the request function are patched. The script itself is run as a
subprocess for the exit-code and empty-output cells.
"""
import contextlib
import io
import json
import os
import subprocess
import sys
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import session_hook
import skill_record_hook

# Owner rule: children run hidden in the calling terminal, never in a new console window.
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
BOUND = ("demo", "token-file", "http://127.0.0.1:1", "origin", "implementation")
MARKER = "ARGS-SECRET-MARKER"


def skill_payload(**extra):
    return {"session_id": "s1", "tool_name": "Skill",
            "tool_input": {"skill": "graphhelm-development-contracts:implement", "args": MARKER},
            "tool_response": {"output": MARKER}, **extra}


class Calls:
    """Patches the binding and the request function and keeps what was posted."""

    def __init__(self, bound=BOUND, error=None):
        self.requests = []
        self.bound, self.error = bound, error

    def request(self, url, token, method, body=None, headers=None, timeout=1.0, **_):
        if self.error:
            raise self.error
        self.requests.append((url, token, method, body, headers, timeout))
        return {}

    def __enter__(self):
        self.stack = contextlib.ExitStack()
        self.stack.enter_context(mock.patch.object(session_hook, "binding", lambda payload=None: self.bound))
        self.stack.enter_context(mock.patch.object(session_hook, "token_from_file", lambda path: "tok"))
        self.stack.enter_context(mock.patch.object(session_hook, "request", self.request))
        return self

    def __exit__(self, *exc):
        self.stack.close()


def run_main(payload_text, calls):
    out, err = io.StringIO(), io.StringIO()
    with calls, mock.patch.object(sys, "stdin", io.StringIO(payload_text)), \
            contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        code = skill_record_hook.main()
    return code, out.getvalue(), err.getvalue()


class SkillLoadedTest(unittest.TestCase):
    def test_a_skill_load_in_a_bound_execution_posts_one_skill_loaded_signal(self):
        calls = Calls()
        code, out, err = run_main(json.dumps(skill_payload()), calls)
        self.assertEqual((code, out, err), (0, "", ""))
        self.assertEqual(len(calls.requests), 1)
        url, token, method, body, headers, timeout = calls.requests[0]
        self.assertEqual((url, method, token), ("http://127.0.0.1:1/v1/executions/demo/signal", "POST", "tok"))
        signal = body["signal"]
        self.assertEqual(signal["type"], "skill.loaded")
        self.assertEqual(signal["severity"], "low")
        self.assertEqual(signal["description"], "Skill loaded: graphhelm-development-contracts:implement")
        self.assertEqual(signal["evidence"], ["skill: graphhelm-development-contracts:implement", "session: s1"])
        self.assertEqual(signal["source"], {"type": "node", "id": "implementation"})
        self.assertEqual(headers["Idempotency-Key"], signal["id"])
        self.assertLessEqual(timeout, 2.0)

    def test_a_subagent_is_named_in_the_evidence(self):
        calls = Calls()
        run_main(json.dumps(skill_payload(agent_id="child-1")), calls)
        self.assertEqual(calls.requests[0][3]["signal"]["evidence"][-1], "agent: child-1")

    def test_the_skill_name_is_cut_to_128_characters(self):
        calls = Calls()
        payload = skill_payload()
        payload["tool_input"]["skill"] = "x" * 300
        run_main(json.dumps(payload), calls)
        signal = calls.requests[0][3]["signal"]
        self.assertEqual(signal["evidence"][0], "skill: " + "x" * 128)
        self.assertEqual(signal["description"], "Skill loaded: " + "x" * 128)

    def test_args_and_other_tool_input_never_appear_in_the_request(self):
        calls = Calls()
        run_main(json.dumps(skill_payload()), calls)
        self.assertEqual(len(calls.requests), 1)
        self.assertNotIn(MARKER, json.dumps(calls.requests, default=str))

    def test_no_binding_sends_nothing(self):
        calls = Calls(bound=None)
        self.assertEqual(run_main(json.dumps(skill_payload()), calls), (0, "", ""))
        self.assertEqual(calls.requests, [])

    def test_other_tools_and_bad_skill_names_send_nothing(self):
        cases = [{**skill_payload(), "tool_name": "Bash"},
                 {**skill_payload(), "tool_input": {}},
                 {**skill_payload(), "tool_input": {"skill": ""}},
                 {**skill_payload(), "tool_input": {"skill": 7}},
                 {**skill_payload(), "tool_input": "not an object"},
                 ["not", "an", "object"]]
        for case in cases:
            calls = Calls()
            self.assertEqual(run_main(json.dumps(case), calls), (0, "", ""), case)
            self.assertEqual(calls.requests, [], case)

    def test_unparsable_stdin_sends_nothing(self):
        calls = Calls()
        self.assertEqual(run_main("{not json", calls), (0, "", ""))
        self.assertEqual(calls.requests, [])

    def test_a_failing_request_exits_zero_and_prints_nothing(self):
        calls = Calls(error=OSError("runtime down"))
        self.assertEqual(run_main(json.dumps(skill_payload()), calls), (0, "", ""))

    def test_a_refused_binding_exits_zero_and_prints_nothing(self):
        with mock.patch.object(session_hook, "binding", side_effect=ValueError("invalid execution binding")):
            out, err = io.StringIO(), io.StringIO()
            with mock.patch.object(sys, "stdin", io.StringIO(json.dumps(skill_payload()))), \
                    contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                self.assertEqual(skill_record_hook.main(), 0)
            self.assertEqual((out.getvalue(), err.getvalue()), ("", ""))


class ScriptRunTest(unittest.TestCase):
    def run_script(self, stdin):
        env = {k: v for k, v in os.environ.items() if not k.startswith("GRAPHHELM_")}
        return subprocess.run([sys.executable, str(HERE / "skill_record_hook.py")], input=stdin,
                              capture_output=True, text=True, env=env, cwd=str(HERE),
                              creationflags=NO_WINDOW, timeout=30)

    def test_outside_an_execution_the_script_is_silent_and_exits_zero(self):
        for stdin in (json.dumps(skill_payload()), "{not json", ""):
            result = self.run_script(stdin)
            self.assertEqual((result.returncode, result.stdout, result.stderr), (0, "", ""), stdin)


class WiringTest(unittest.TestCase):
    def test_hooks_json_runs_the_script_after_a_skill_tool_call(self):
        hooks = json.loads((HERE / "hooks.json").read_text(encoding="utf-8"))["hooks"]
        entries = [(group["matcher"], hook) for group in hooks["PostToolUse"] for hook in group["hooks"]]
        self.assertEqual(len(entries), 1)
        matcher, hook = entries[0]
        self.assertEqual(matcher, "Skill")
        self.assertIn("skill_record_hook.py", hook["command"])
        self.assertLessEqual(hook["timeout"], 5)
        self.assertTrue((HERE / "skill_record_hook.py").is_file())


if __name__ == "__main__":
    unittest.main()
