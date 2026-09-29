"""py-mcp-meta: `tools/call` with the standard MCP `_meta` object is dispatched, while any other
unknown params field, an identity override and an oversized frame are still refused before Runtime
I/O. The known fix's test file (5f3e500b) runs green, plus one cell for another unknown field. The
Codex companion ships the same adapter, byte for byte."""
from _lib import WT, fail, hidden_unittest, ok

FIX = "5f3e500b9ec69584ef0ac8ca39edebc64de2d7fc"
EXTRA = '''
def _doless_other_fields_are_still_refused(self):
    env = os.environ.copy(); env.update(self.env)
    env["GRAPHHELM_MCP_HOST"] = "codex"
    env["GRAPHHELM_SESSION_ID"] = "receiver-1"
    before = len(RuntimeHandler.requests)
    call = {"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "status", "arguments": {}, "cursor": "x"}}
    result = subprocess.run([sys.executable, str(SCRIPT), "--mcp-stdio"], env=env,
                            input=json.dumps(call) + "\\n", capture_output=True, text=True)
    replies = [json.loads(line) for line in result.stdout.splitlines()]
    self.assertEqual(len(replies), 1, result.stdout)
    refused = "error" in replies[0] or replies[0].get("result", {}).get("isError")
    self.assertTrue(refused, replies[0])
    self.assertEqual(len(RuntimeHandler.requests), before)


HandoffTests.test_doless_other_fields_are_still_refused = _doless_other_fields_are_still_refused
'''
hidden_unittest(FIX, "plugins/graphhelm/hooks/test_task_handoff.py", EXTRA)
main = (WT / "plugins/graphhelm/hooks/task_handoff.py").read_bytes()
companion = (WT / "plugins/graphhelm-codex-hooks/hooks/task_handoff.py").read_bytes()
if main != companion:
    fail("the Codex companion's task_handoff.py no longer matches the main plugin's")
ok("_meta is accepted and every other refusal holds in both copies")
