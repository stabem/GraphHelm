"""Focused contract tests for the token-bench verdict, not model behavior."""

import argparse
import importlib.util
import json
import os
import select
import subprocess
import sys
import threading
import time
import uuid
from pathlib import Path

import pytest


MODULE_PATH = Path(__file__).with_name("run.py")
spec = importlib.util.spec_from_file_location("token_bench_run", MODULE_PATH)
assert spec and spec.loader
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


def usage():
    return {"input": 10, "output": 4, "cacheRead": 2, "cacheWrite": 1, "assistantMessages": 1}


@pytest.mark.parametrize("model,effort", [("gpt-6-luna", "low"), ("gpt-6.1-sol", "high"),
                                        ("gpt-6-astra", "medium")])
def test_codex_exec_argv_and_usage_stream(tmp_path, monkeypatch, model, effort):
    """Protect fresh sandboxed dispatch and cached-token accounting; mocked process I/O, <1s."""
    # Same current-CLI stream shape/usage as the model-gateway adapter's advisory-item receipt.
    stream = '\n'.join([
        '{"type":"thread.started","thread_id":"thread-fixture"}',
        '{"type":"item.completed","item":{"id":"warning","type":"error","message":"Skill descriptions were shortened"}}',
        '{"type":"item.completed","item":{"type":"agent_message","text":"done"}}',
        '{"type":"turn.completed","usage":{"input_tokens":22982,"cached_input_tokens":1024,"output_tokens":9,"reasoning_tokens":2}}'])
    def capture(cmd, **kw):
        assert cmd == ["codex-fixture", "exec", "--json", "-m", model, "-c",
                       f'model_reasoning_effort="{effort}"', "--sandbox", "workspace-write",
                       "--ignore-user-config", "--ephemeral", "-"]
        assert kw["cwd"] == tmp_path and kw["input"] == "prompt"
        return subprocess.CompletedProcess(cmd, 0, stream, ""), False
    monkeypatch.setattr(runner, "_run_captured", capture)
    result, _, _ = runner.run_codex_agent(tmp_path, "prompt", model, effort, 1, {"path": "codex-fixture"})
    assert result["result"] == "done" and result["session_id"] == "thread-fixture"
    assert result["tokens"] == {"input_tokens": 22982, "cached_input_tokens": 1024, "output_tokens": 9}
    assert not result.get("agentError")


@pytest.mark.parametrize("ending", ["missing", "failed", "timeout", "malformed", "negative", "excess_cache"])
def test_codex_incomplete_keeps_observed_tokens(tmp_path, monkeypatch, ending):
    """Catches false PASS and discarded spend after errors; mocked process I/O, <1s."""
    stream = '{"type":"turn.completed","usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":20}}\n'
    if ending == "missing":
        stream = '{"type":"thread.started","thread_id":"fixture"}\n'
    elif ending == "failed":
        stream += '{"type":"turn.failed","error":{"message":"failed"}}\n'
    elif ending == "malformed":
        stream += 'not json\n'
    elif ending == "negative":
        stream += '{"type":"turn.completed","usage":{"input_tokens":-1,"cached_input_tokens":0,"output_tokens":0}}\n'
    elif ending == "excess_cache":
        stream += '{"type":"turn.completed","usage":{"input_tokens":1,"cached_input_tokens":2,"output_tokens":0}}\n'
    proc = (subprocess.TimeoutExpired("codex", 60, output=stream.encode()) if ending == "timeout"
            else subprocess.CompletedProcess([], 0, stream, ""))
    monkeypatch.setattr(runner, "_run_captured", lambda *a, **kw: (proc, False))
    result, _, _ = runner.run_codex_agent(tmp_path, "prompt", "gpt-6-luna", "low", 1, {"path": "fixture"})
    assert result.get("agentError")
    assert result.get("tokens") == (None if ending == "missing" else
        {"input_tokens": 100, "cached_input_tokens": 40, "output_tokens": 20})


def test_acceptance_and_regression_are_both_required_for_pass():
    """Catches the old false green where the hidden oracle passed after a regression broke."""
    verdict, reasons = runner.evaluate_outcome("PASS", "FAIL", usage(), 0.2, "session", {"regression": {"command": ["test"]}})
    assert verdict == "FAIL"
    assert "preexisting_regression_fail" in reasons


def test_missing_session_usage_is_incomplete_even_when_code_passes():
    """Catches token claims made from a missing or partial Claude transcript."""
    verdict, reasons = runner.evaluate_outcome("PASS", "PASS", {}, None, "session", {"regression": {"command": ["test"]}})
    assert verdict == "INCOMPLETE"
    assert "session_tokens_unobserved" in reasons
    assert "session_cost_unobserved" in reasons


def test_expired_auth_without_model_tokens_is_not_a_coding_failure():
    """A failed OAuth refresh had one assistant error message but no model invocation."""
    zero = {key: 0 for key in ("input", "output", "cacheRead", "cacheWrite")}
    zero["assistantMessages"] = 1
    verdict, reasons = runner.evaluate_outcome("FAIL", "PASS", zero, 0, "session",
                                                {"regression": {"command": ["test"]}}, agent_error=True)
    assert verdict == "INCOMPLETE"
    assert "session_tokens_unobserved" in reasons
    assert "agent_execution_unobserved" in reasons


def test_agent_prompt_round_trips_unicode_on_legacy_windows_code_page(tmp_path, monkeypatch):
    """The pinned Keel skill must reach the agent even when the host code page is cp1252."""
    real_popen = subprocess.Popen

    def echo_prompt(_cmd, **kwargs):
        return real_popen([sys.executable, "-c",
                         "import json,sys; print(json.dumps({'result': sys.stdin.buffer.read().decode('utf-8')}))"], **kwargs)

    monkeypatch.setattr(runner.subprocess, "Popen", echo_prompt)
    monkeypatch.setattr(subprocess.locale, "getencoding", lambda: "cp1252")
    prompt = "Keel → GraphHelm: ação comprovada"
    result, _stderr, _wall = runner.run_agent(
        tmp_path, prompt, "a", "sonnet", 1, {}, None, None, {"path": str(tmp_path / "unlaunchable-agent-fixture")})
    assert result["result"] == prompt


def test_timeout_keeps_pinned_session_id_for_partial_usage(tmp_path, monkeypatch):
    """A timed-out CLI has no terminal receipt, but its transcript must remain identifiable."""
    observed = {}

    def timeout(cmd, **_kwargs):
        observed["cmd"] = cmd
        raise subprocess.TimeoutExpired(cmd, 60)

    monkeypatch.setattr(runner.subprocess, "Popen", timeout)
    result, _stderr, _wall = runner.run_agent(
        tmp_path, "prompt", "a", "sonnet", 1, {}, None, None, {"path": str(tmp_path / "unlaunchable-agent-fixture")})
    assert result["agentError"] == "timeout"
    assert str(uuid.UUID(result["session_id"])) == result["session_id"]
    index = observed["cmd"].index("--session-id")
    assert observed["cmd"][index + 1] == result["session_id"]



@pytest.mark.skipif(os.name != "posix" or not hasattr(os, "pidfd_open"),
                    reason="requires POSIX and a Linux pidfd process-exit observer")
@pytest.mark.parametrize("parent_exits", [False, True])
def test_agent_timeout_stops_descendant_before_scoring(tmp_path, monkeypatch, parent_exits):
    """#201: real finite configured executable; ~1s API timeout, not a fractional CLI flag."""
    fake = tmp_path / "finite-agent"
    fake.write_text("#!" + sys.executable + "\n" + """import json, os, sys, time
from pathlib import Path
if sys.argv[1:]==['--version']:
    print('finite-offline-fixture 1.0'); raise SystemExit
root=Path.cwd()
if sys.argv[1:]==['--fixture-child']:
    (root/'ready.tmp').write_text(str(os.getpid()))
    (root/'ready.tmp').rename(root/'ready')
    deadline=time.monotonic()+8
    while not (root/'release').exists() and time.monotonic()<deadline:
        time.sleep(.01)
    if (root/'release').exists():
        (root/'effect').write_text('late agent effect')
    raise SystemExit
import subprocess
(root/'launch.json').write_text(json.dumps({'argv':sys.argv, 'input':sys.stdin.read()}))
subprocess.Popen([sys.executable, __file__, '--fixture-child'])
time.sleep(0 if (root/'parent-exits').exists() else 8)
""")
    fake.chmod(0o700)
    monkeypatch.setenv("TOKEN_BENCH_CLAUDE_CLI", str(fake))
    monkeypatch.setenv("TOKEN_BENCH_SCRATCH", str(tmp_path / "scratch"))
    cli = runner.validate_prerequisites("a")
    assert cli["path"] == str(fake) and cli["version"] == "finite-offline-fixture 1.0"
    if parent_exits:
        (tmp_path / "parent-exits").touch()
    mcp = tmp_path / "mcp.json"
    mcp.write_text("{}")
    result = []
    finished = threading.Event()

    def invoke():
        try:
            result.append(runner.run_agent(tmp_path, "finite → sessão", "c", "fixture-model",
                                           1 / 60, {}, 0.25, mcp, cli, effort="medium"))
        finally:
            finished.set()

    worker = threading.Thread(target=invoke, daemon=True)
    worker.start()
    pidfd = None
    try:
        deadline = time.monotonic() + 5
        while not (tmp_path / "ready").exists() and time.monotonic() < deadline:
            time.sleep(.01)
        pidfd = os.pidfd_open(int((tmp_path / "ready").read_text()))
        assert finished.wait(4), "agent output collection outlived the bound"
        reply, stderr, _wall = result[0]
        launch = json.loads((tmp_path / "launch.json").read_text())
        argv = launch["argv"]
        assert reply["agentError"] == "timeout" and reply["timeoutSeconds"] == 1
        assert reply["session_id"] == argv[argv.index("--session-id") + 1]
        assert stderr == "timeout" and "timed out" in reply["stderr"]
        assert launch["input"] == "finite → sessão"
        for flag, value in (("--model", "fixture-model"), ("--effort", "medium"),
                            ("--max-budget-usd", "0.25"), ("--mcp-config", str(mcp)),
                            ("--allowedTools", "mcp__graphhelm")):
            assert argv[argv.index(flag) + 1] == value
        assert runner.digest_file(fake) == cli["sha256"]
        # Release only after run_agent returns; wait for kernel-confirmed exit before
        # inspecting effects so an unscheduled live child cannot masquerade as stopped.
        (tmp_path / "release").touch()
        assert select.select([pidfd], [], [], 3)[0], "agent descendant remains live"
        assert not (tmp_path / "effect").exists(), "timed-out agent child could still act"
    finally:
        (tmp_path / "release").touch()
        if pidfd is not None:
            os.close(pidfd)
        worker.join(timeout=10)
        assert not worker.is_alive(), "finite agent fixture did not finish"


def test_keel_prompt_puts_real_task_under_objective_heading(tmp_path, monkeypatch):
    """A misplaced skill made Claude read an empty objective and stop before fixing code."""
    skill = tmp_path / "SKILL.md"
    skill.write_text("PINNED_SKILL_CONTENT", encoding="utf-8")
    monkeypatch.setattr(runner, "KEEL_SKILL", skill)
    prompt = runner.compose_arm_c_prompt(
        "TASK_SENTINEL: fix the Unicode child prompt", tmp_path / "card.json", tmp_path / "signal.json")
    assert "Objective / issue:\n\nTASK_SENTINEL" in prompt
    assert prompt.index("TASK_SENTINEL") < prompt.index("PINNED_SKILL_CONTENT")
    assert prompt.count("TASK_SENTINEL") == 1


def test_no_regression_observer_cannot_be_a_quality_win():
    """Catches treating a task without a pre-existing regression as quality evidence."""
    verdict, reasons = runner.evaluate_outcome("PASS", "UNOBSERVED", usage(), 0.2, "session", {})
    assert verdict == "INCOMPLETE"
    assert "regression_observer_missing" in reasons


def test_regression_restoration_preserves_exact_source_bytes(tmp_path, monkeypatch):
    """A newline rewrite made a PowerShell source observer fail on an unchanged parent."""
    monkeypatch.setattr(runner, "git_show_bytes", lambda _sha, _path: b"one\ntwo\n")
    command = [sys.executable, "-c", "from pathlib import Path; assert Path('check.ps1').read_bytes() == b'one\\ntwo\\n'"]
    task = {"parentSha": "parent", "regression": {"files": ["check.ps1"], "command": command}}
    verdict, code, _ = runner.run_regression(tmp_path, task)
    assert (verdict, code) == ("PASS", 0)


def test_proof_orchestration_runs_submitted_suite_before_restoring_tests(tmp_path, monkeypatch):
    """The runner must not overwrite a red submitted test before observing it."""
    test_file = tmp_path / "check.py"
    test_file.write_text("raise SystemExit(7)\n", encoding="utf-8")
    monkeypatch.setattr(runner, "git_show_bytes", lambda sha, _path:
                        b"print('old suite passes')\n" if sha == "parent" else b"print('oracle passes')\n")
    command = [sys.executable, "check.py"]
    task = {"parentSha": "parent", "fixSha": "fix",
            "regression": {"files": ["check.py"], "command": command},
            "oracle": {"file": "check.py", "command": command}}
    submitted, historical, hidden = runner.run_proofs(tmp_path, task)
    assert (submitted[:2], historical[:2], hidden[:2]) == (
        ("FAIL", 7), ("PASS", 0), ("PASS", 0))
    assert test_file.read_bytes() == b"print('oracle passes')\n"


def test_failed_submitted_suite_blocks_proven_delivery():
    """Hidden acceptance and old regression cannot hide a red test left by the agent."""
    task = {"regression": {"command": ["test"]}}
    verdict, reasons = runner.evaluate_outcome(
        "PASS", "PASS", usage(), 0.2, "session", task, submitted_suite="FAIL")
    assert verdict == "FAIL"
    assert "submitted_suite_failed" in reasons


def test_transcript_usage_deduplicates_repeated_assistant_records():
    """Catches inflated cost when a JSONL consumer repeats one assistant message."""
    message = {"type": "assistant", "uuid": "m1", "message": {
        "id": "m1", "usage": {"input_tokens": 10, "output_tokens": 4,
                                  "cache_read_input_tokens": 2, "cache_creation_input_tokens": 1}}}
    lines = [runner.json.dumps(message), runner.json.dumps(message)]
    result = runner.sum_transcript_usage(lines)
    assert result["assistantMessages"] == 1
    assert result["input"] == 10


def test_transcript_usage_keeps_completed_count_for_repeated_message():
    """A streaming partial record must not hide the later completed usage for the same ID."""
    def record(output):
        return runner.json.dumps({"type": "assistant", "message": {"id": "m1", "usage": {
            "input_tokens": 10, "output_tokens": output, "cache_read_input_tokens": 0,
            "cache_creation_input_tokens": 0}}})
    result = runner.sum_transcript_usage([record(0), record(7)])
    assert result["assistantMessages"] == 1
    assert result["output"] == 7


def test_partial_transcript_cannot_claim_session_cost():
    """A truncated JSONL tail must not be accepted from a positive token subtotal."""
    message = runner.json.dumps({"type": "assistant", "message": {"id": "m1", "usage": {
        "input_tokens": 5, "output_tokens": 1, "cache_read_input_tokens": 0,
        "cache_creation_input_tokens": 0}}})
    observed = runner.sum_transcript_usage([message, "{broken"])
    audit = runner.usage_audit(observed, {"num_turns": 2, "usage": {
        "input_tokens": 5, "output_tokens": 2, "cache_read_input_tokens": 0,
        "cache_creation_input_tokens": 0}})
    assert audit["status"] == "INCOMPLETE"
    assert "transcript_parse_error" in audit["reasons"]
    assert "transcript_output_incomplete" in audit["reasons"]


def test_tool_result_turns_do_not_require_distinct_assistant_ids():
    """Claude counts user-side tool results as turns; rejecting these made a complete run unreadable."""
    assistant = {"type": "assistant", "message": {"id": "m1", "model": "claude-sonnet-5",
                  "usage": {"input_tokens": 5, "output_tokens": 2,
                            "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0}}}
    lines = [runner.json.dumps({"type": "user", "message": {"content": "task"}}),
             runner.json.dumps(assistant),
             runner.json.dumps({"type": "user", "message": {"content": "tool result"}})]
    observed = runner.sum_transcript_usage(lines)
    audit = runner.usage_audit(observed, {"num_turns": 2, "usage": {
        "input_tokens": 5, "output_tokens": 2,
        "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0}})
    assert observed["userMessages"] == 2
    assert observed["assistantMessages"] == 1
    assert audit["status"] == "PASS"


def test_background_cli_model_does_not_replace_coding_model():
    """CLI title/utility usage can name Haiku while assistant code turns remain Sonnet."""
    model, all_models = runner.coding_model(
        {"claude-haiku-4-5-20251001": {}, "claude-sonnet-5": {}}, ["claude-sonnet-5"])
    assert model == "claude-sonnet-5"
    assert all_models == ["claude-haiku-4-5-20251001", "claude-sonnet-5"]
    assert runner.coding_model(None, ["claude-sonnet-5"])[0] is None
    assert runner.coding_model({}, ["claude-sonnet-5"])[0] is None
    assert runner.coding_model({"claude-sonnet-5": {}}, ["claude-opus-4-1"])[0] is None
    assert runner.coding_model({}, ["claude-sonnet-5", "claude-opus-4-1"])[0] is None


def test_cli_identity_binds_version_and_executable_bytes(tmp_path, monkeypatch):
    """A changed CLI can change transcript or cost semantics without changing the model name."""
    executable = tmp_path / "claude.exe"
    executable.write_bytes(b"pinned-cli")
    monkeypatch.setenv("TOKEN_BENCH_CLAUDE_CLI", str(executable))
    monkeypatch.setattr(runner.subprocess, "run", lambda *_args, **_kw: subprocess.CompletedProcess(
        [str(executable), "--version"], 0, stdout="2.1.269\n", stderr=""))
    identity = runner.claude_cli_identity()
    assert identity["version"] == "2.1.269"
    assert identity["sha256"] == runner.digest_file(executable)


def test_worktree_scratch_must_be_outside_repository(monkeypatch):
    """Catches generated checkouts being written into the source worktree tree."""
    monkeypatch.setenv("TOKEN_BENCH_SCRATCH", str(runner.REPO))
    try:
        runner.scratch_root()
    except RuntimeError as exc:
        assert "outside the repository" in str(exc)
    else:
        raise AssertionError("repository path was accepted as scratch")


def test_evaluator_snapshot_has_exact_source_without_git_metadata(tmp_path, monkeypatch):
    """Three preflight observers do not need costly git init/add/commit passes."""
    source = tmp_path / "source"
    source.mkdir()
    subprocess.run(["git", "init", "-q", "-b", "main"], cwd=source, check=True)
    (source / "proof.bin").write_bytes(b"historical\x00source")
    subprocess.run(["git", "add", "proof.bin"], cwd=source, check=True)
    subprocess.run(["git", "-c", "user.name=bench", "-c", "user.email=bench@invalid",
                    "commit", "-qm", "source"], cwd=source, check=True)
    sha = subprocess.run(["git", "rev-parse", "HEAD"], cwd=source, check=True,
                         capture_output=True, text=True).stdout.strip()
    committed = subprocess.run(["git", "show", f"{sha}:proof.bin"], cwd=source, check=True,
                               capture_output=True).stdout
    (source / "proof.bin").write_bytes(b"future fix")
    subprocess.run(["git", "commit", "-qam", "future fix"], cwd=source, check=True)
    future_sha = subprocess.run(["git", "rev-parse", "HEAD"], cwd=source, check=True,
                                capture_output=True, text=True).stdout.strip()
    monkeypatch.setattr(runner, "REPO", source)
    monkeypatch.setenv("TOKEN_BENCH_SCRATCH", str(tmp_path / "scratch"))
    snapshot = runner.make_snapshot("fixture", "preflight", sha)
    assert (snapshot / "proof.bin").read_bytes() == committed
    assert not (snapshot / ".git").exists()
    agent = runner.make_worktree("fixture", "a", sha)
    assert (agent / "proof.bin").read_bytes() == committed
    assert (agent / ".git").is_dir()
    assert subprocess.run(["git", "remote"], cwd=agent, check=True,
                          capture_output=True, text=True).stdout == ""
    assert subprocess.run(["git", "rev-parse", "HEAD"], cwd=agent, check=True,
                          capture_output=True, text=True).stdout.strip() == sha
    assert subprocess.run(["git", "rev-list", "--count", "HEAD"], cwd=agent, check=True,
                          capture_output=True, text=True).stdout.strip() == "1"
    assert subprocess.run(["git", "cat-file", "-e", future_sha], cwd=agent,
                          capture_output=True).returncode != 0
    assert not (agent / ".git" / "FETCH_HEAD").exists()


def test_missing_graphhelm_cli_stops_before_any_snapshot(tmp_path, monkeypatch):
    """A missing executable once wasted six minutes on checkouts before a model could start."""
    monkeypatch.setenv("TOKEN_BENCH_SCRATCH", str(tmp_path / "scratch"))
    monkeypatch.delenv("TOKEN_BENCH_GRAPHHELM_CLI", raising=False)
    monkeypatch.setattr(runner, "make_snapshot", lambda *_args: pytest.fail("snapshot started"))
    with pytest.raises(SystemExit, match="TOKEN_BENCH_GRAPHHELM_CLI"):
        runner.cmd_run(argparse.Namespace(task="1279", arm="b"))


def test_nonlaunchable_graphhelm_file_stops_before_any_snapshot(tmp_path, monkeypatch):
    """An existing but non-executable file must not trigger minutes of preflight work."""
    fake = tmp_path / "graphhelm.txt"
    fake.write_text("not an executable", encoding="utf-8")
    monkeypatch.setenv("TOKEN_BENCH_SCRATCH", str(tmp_path / "scratch"))
    monkeypatch.setenv("TOKEN_BENCH_GRAPHHELM_CLI", str(fake))
    monkeypatch.setattr(runner, "make_snapshot", lambda *_args: pytest.fail("snapshot started"))
    with pytest.raises(SystemExit, match="not launchable"):
        runner.cmd_run(argparse.Namespace(task="1279", arm="b"))


def test_methodology_observer_requires_calls_card_and_persisted_signal(tmp_path, monkeypatch):
    """A good patch alone cannot count as GraphHelm + Keel without the treatment occurring."""
    monkeypatch.setattr(runner.Path, "home", lambda: tmp_path)
    transcript = tmp_path / ".claude" / "projects" / "task" / "session.jsonl"
    transcript.parent.mkdir(parents=True)
    lines = []
    for stage in ("start", "briefing", "compile_context", "signal"):
        args = {"executionId": "exec-1"} if stage != "compile_context" else {}
        lines.append(runner.json.dumps({"type": "assistant", "message": {"content": [{
            "type": "tool_use", "id": stage, "name": f"mcp__graphhelm__{stage}", "input": args}]}}))
        lines.append(runner.json.dumps({"type": "user", "message": {"content": [{
            "type": "tool_result", "tool_use_id": stage, "content": '{"ok":true}'}]}}))
    transcript.write_text("\n".join(lines), encoding="utf-8")
    events = tmp_path / "events"
    events.mkdir()
    journal = events / "journal.jsonl"
    def event(kind):
        return {"actor": {"id": "bench-actor"}, "kind": {"type": kind,
                "data": {"executionId": "exec-1"}}}
    card = tmp_path / "card.json"
    card.write_text(runner.json.dumps({"paths": ["ci/gate.ps1"], "promise": "No wasted cluster",
                                       "defect": "Failed build starts cluster", "proofCommand": "test"}), encoding="utf-8")
    signal = tmp_path / "signal.json"
    signal.write_text(runner.json.dumps({"description": "Proof finished",
                                         "evidence": ["test exit 0"]}), encoding="utf-8")
    signal_event = event("signal_recorded")
    signal_event["kind"]["data"]["envelopeSha256"] = runner.digest_file(signal).split(":", 1)[1]
    journal.write_text(runner.json.dumps({"events": [event("execution_started"),
                                         signal_event]}) + "\n", encoding="utf-8")
    runtime = {"events": str(events), "actor": "bench-actor"}
    assert runner.methodology_observer("session", runtime, card, signal, "c")["status"] == "PASS"
    journal.write_text(runner.json.dumps({"events": [event("execution_started")]}) + "\n", encoding="utf-8")
    assert runner.methodology_observer("session", runtime, card, signal, "c")["status"] == "INCOMPLETE"
    journal.write_text(runner.json.dumps({"events": ["not an event"]}) + "\n", encoding="utf-8")
    assert runner.methodology_observer("session", runtime, card, signal, "c")["status"] == "INCOMPLETE"
    journal.write_text(runner.json.dumps({"events": [event("execution_started"),
                                         signal_event]}) + "\n", encoding="utf-8")
    card.write_text(runner.json.dumps({"paths": ["C:\\outside\\gate.ps1"], "promise": "x",
                                       "defect": "y", "proofCommand": "test"}), encoding="utf-8")
    assert runner.methodology_observer("session", runtime, card, signal, "c")["status"] == "INCOMPLETE"


def test_explicit_effort_reaches_cli_without_changing_legacy_commands(tmp_path, monkeypatch):
    """Catches silently dropping requested effort; offline process-boundary fixture (<1s)."""
    calls = []
    real_popen = subprocess.Popen
    def launch(cmd, **kw):
        calls.append(cmd)
        return real_popen([sys.executable, "-c", "print('{\"result\":\"ok\"}')"], **kw)
    monkeypatch.setattr(runner.subprocess, "Popen", launch)
    args = (tmp_path, "task", "a", "claude-test-exact", 1, {}, 1.0, None, {"path": str(tmp_path / "unlaunchable-agent-fixture")})
    runner.run_agent(*args, effort="high")
    assert calls[0][calls[0].index("--effort") + 1] == "high"
    runner.run_agent(*args)
    assert "--effort" not in calls[1]
