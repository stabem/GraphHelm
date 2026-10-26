"""Replay native subscription probes, including forked history and child accounting."""
import json
import copy
import hashlib
from pathlib import Path

import native_session_evidence as evidence

FIXTURES = Path(__file__).parent / "fixtures" / "native-sessions"


def codex(fork=False, children=True, usage=None):
    prefix = "codex-fork" if fork else "codex"
    root = "01a17678-5676-7271-b89b-70392d5016a5" if fork else "01a17672-2f62-7c41-b135-b94ea4c887c0"
    suffix = "002" if fork else "001"
    counts = {"input": 61932 if fork else 65656, "output": 116 if fork else 72,
              "cacheRead": 40832 if fork else 43392, "cacheWrite": 0}
    return evidence.codex_tree(FIXTURES / (prefix + "-root.jsonl"),
                               [FIXTURES / (prefix + "-child.jsonl")] if children else [],
                               project=Path("D:/gh/codex-scope-cleanup/native-2074-probes/codex-child-" + suffix),
                               root_thread=root, stdout_usage=usage or counts)


def test_codex_native_child_is_not_in_parent_stdout_total():
    result = codex()
    assert result["status"] == "observed"
    assert result["total"] == {"input": 87212, "output": 81, "cacheRead": 43392, "cacheWrite": 0}
    assert len(result["sessions"]) == 2


def test_forked_ancestor_history_is_not_a_second_execution():
    result = codex(fork=True)
    assert result["status"] == "observed"
    assert result["total"] == {"input": 88885, "output": 125, "cacheRead": 40832, "cacheWrite": 0}


def test_codex_successful_spawn_without_child_receipt_is_unknown():
    assert codex(children=False)["status"] == "unknown"


def test_codex_root_rollout_must_agree_with_captured_stream():
    assert codex(usage={"input": 1, "output": 1, "cacheRead": 0, "cacheWrite": 0})["status"] == "unknown"


def test_claude_pipeline_includes_child_once():
    result = evidence.claude_pipeline(FIXTURES / "claude-child.jsonl")
    assert result["status"] == "observed"
    assert result["total"] == {"input": 6, "output": 270, "cacheRead": 5773, "cacheWrite": 7381}
    assert list(result["children"].values())[0]["output"] == 21


def test_claude_unobserved_child_never_becomes_zero(tmp_path):
    rows = evidence.records(FIXTURES / "claude-child.jsonl")
    rows = [row for row in rows if "tool_use_result" not in row]
    path = tmp_path / "missing-child.jsonl"
    path.write_text("".join(json.dumps(row) + "\n" for row in rows), encoding="utf-8")
    assert evidence.claude_pipeline(path)["status"] == "unknown"


def test_claude_placeholder_child_counters_fail_reconciliation(tmp_path):
    rows = evidence.records(FIXTURES / "claude-child.jsonl")
    for row in rows:
        if "tool_use_result" in row:
            row["tool_use_result"]["usage"]["output_tokens"] = 8
    path = tmp_path / "placeholder.jsonl"
    path.write_text("".join(json.dumps(row) + "\n" for row in rows), encoding="utf-8")
    assert evidence.claude_pipeline(path)["status"] == "unknown"


def test_denied_native_resolver_is_incomplete_despite_written_constitution():
    requirement = [{"commandPattern": r"resolve-template\.ps1.*constitution-template"}]
    result = evidence.required_operations(FIXTURES / "claude-speckit-001-operation.jsonl",
                                          cli="claude", requirements=requirement)
    assert result["status"] == "incomplete"


def test_actual_native_resolvers_return_the_official_template():
    requirement = [{"commandPattern": r"resolve-template\.ps1.*constitution-template",
                    "outputContains": ['"TEMPLATE_NAME":"constitution-template"', '"TEMPLATE_CONTENT":']}]
    for cli, name in [("claude", "claude-speckit-002"), ("codex", "codex-speckit-001")]:
        result = evidence.required_operations(FIXTURES / (name + "-operation.jsonl"),
                                              cli=cli, requirements=requirement)
        assert result["status"] == "observed", result


def test_claude_powershell_operation_is_matched_to_its_native_result(tmp_path):
    call = {"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "PowerShell",
            "id": "tool-1", "input": {"command": ".specify/scripts/powershell/setup-tasks.ps1 -Json"}}]}}
    reply = {"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "tool-1",
             "is_error": False, "content": '{"FEATURE_DIR":"specs/001"}'}]}}
    stream = tmp_path / "native.jsonl"
    stream.write_text(json.dumps(call) + "\n" + json.dumps(reply) + "\n", encoding="utf-8")
    requirement = [{"commandPattern": r"setup-tasks\.ps1\s+-Json",
                    "outputContains": ['"FEATURE_DIR"']}]
    assert evidence.required_operations(stream, cli="claude", requirements=requirement)["status"] == "observed"
    reply["message"]["content"][0]["is_error"] = True
    stream.write_text(json.dumps(call) + "\n" + json.dumps(reply) + "\n", encoding="utf-8")
    assert evidence.required_operations(stream, cli="claude", requirements=requirement)["status"] == "incomplete"


def test_final_child_stats_cannot_be_ignored_when_agent_events_are_missing(tmp_path):
    rows = evidence.records(FIXTURES / "claude-child.jsonl")
    final = rows[-1]
    main = final["usage"]
    final["modelUsage"] = {"native": {"inputTokens": main["input_tokens"],
                                      "outputTokens": main["output_tokens"],
                                      "cacheReadInputTokens": main["cache_read_input_tokens"],
                                      "cacheCreationInputTokens": main["cache_creation_input_tokens"]}}
    path = tmp_path / "missing-events.jsonl"
    path.write_text(json.dumps(final) + "\n", encoding="utf-8")
    assert evidence.claude_pipeline(path)["status"] == "unknown"


def test_each_native_agent_call_needs_its_own_completed_child(tmp_path):
    rows = evidence.records(FIXTURES / "claude-child.jsonl")
    call = next(row for row in rows if row["type"] == "assistant")
    extra_call = copy.deepcopy(call)
    extra_call["message"]["content"][0]["id"] = "unanswered-agent-call"
    child = next(row for row in rows if "tool_use_result" in row)
    extra_child = copy.deepcopy(child)
    extra_child["tool_use_result"]["agentId"] = "second-child-same-call"
    final = rows[-1]
    final["subagent_stats"]["spawned"] = final["subagent_stats"]["completed"] = 2
    usage = child["tool_use_result"]["usage"]
    for value in final["modelUsage"].values():
        for wire, native in [("inputTokens", "input_tokens"), ("outputTokens", "output_tokens"),
                             ("cacheReadInputTokens", "cache_read_input_tokens"),
                             ("cacheCreationInputTokens", "cache_creation_input_tokens")]:
            value[wire] += usage[native]
    path = tmp_path / "duplicate-call-result.jsonl"
    path.write_text("".join(json.dumps(row) + "\n" for row in [call, extra_call, child, extra_child, final]), encoding="utf-8")
    assert evidence.claude_pipeline(path)["status"] == "unknown"


def test_reused_codex_child_must_finish_its_latest_turn(tmp_path):
    rows = evidence.records(FIXTURES / "codex-child.jsonl")
    rows.append({"type": "event_msg", "payload": {"type": "task_started", "turn_id": "followup"}})
    child = tmp_path / "child.jsonl"

    def observe():
        child.write_text("".join(json.dumps(row) + "\n" for row in rows), encoding="utf-8")
        return evidence.codex_tree(FIXTURES / "codex-root.jsonl", [child],
                                   project=Path("D:/gh/codex-scope-cleanup/native-2074-probes/codex-child-001"),
                                   root_thread="01a17672-2f62-7c41-b135-b94ea4c887c0",
                                   stdout_usage={"input": 65656, "output": 72, "cacheRead": 43392, "cacheWrite": 0})

    assert observe()["status"] == "unknown"
    rows.extend([{"type": "event_msg", "payload": {"type": "token_count", "info": None}},
                 {"type": "event_msg", "payload": {"type": "task_complete", "turn_id": "followup"}}])
    assert observe()["status"] == "unknown"
    rows.pop()
    rows.pop()
    previous = next(row for row in reversed(rows) if row.get("payload", {}).get("type") == "token_count")
    counter = copy.deepcopy(previous)
    counter["payload"]["info"]["total_token_usage"]["input_tokens"] += 100
    counter["payload"]["info"]["total_token_usage"]["output_tokens"] += 10
    rows.extend([counter, {"type": "event_msg", "payload": {"type": "task_complete", "turn_id": "followup"}}])
    completed = observe()
    assert completed["status"] == "observed"
    assert completed["total"]["input"] == 87312
    assert completed["total"]["output"] == 91


def test_claude_recursive_persisted_messages_reconcile_every_child_once():
    children = list(FIXTURES.glob("claude-depth2-agent-*.jsonl"))
    result = evidence.claude_owned_tree(FIXTURES / "claude-depth2-stream.jsonl", children,
                                       project=Path("D:/gh/codex-scope-cleanup/native-2074-probes/claude-child-depth2-001"),
                                       session_id="4fb1b27b-870f-4a50-9989-754a959cfb5e")
    assert result["status"] == "observed", result
    assert result["total"] == {"input": 10, "output": 530, "cacheRead": 9256, "cacheWrite": 11868}
    assert sorted(child["messages"] for child in result["children"].values()) == [1, 2]
    missing = evidence.claude_owned_tree(FIXTURES / "claude-depth2-stream.jsonl", children[:1],
                                        project=Path("D:/gh/codex-scope-cleanup/native-2074-probes/claude-child-depth2-001"),
                                        session_id="4fb1b27b-870f-4a50-9989-754a959cfb5e")
    assert missing["status"] == "unknown"


def test_recursive_claude_linkage_rejects_foreign_session_and_unqualified_version(tmp_path):
    rows = evidence.records(FIXTURES / "claude-depth2-stream.jsonl")
    rows[0]["session_id"] = "FOREIGN-SESSION"
    path = tmp_path / "foreign.jsonl"
    path.write_text("".join(json.dumps(row) + "\n" for row in rows), encoding="utf-8")
    arguments = {"child_logs": list(FIXTURES.glob("claude-depth2-agent-*.jsonl")),
                 "project": Path("D:/gh/codex-scope-cleanup/native-2074-probes/claude-child-depth2-001"),
                 "session_id": "4fb1b27b-870f-4a50-9989-754a959cfb5e"}
    assert evidence.claude_owned_tree(path, **arguments)["status"] == "unknown"
    assert evidence.claude_owned_tree(FIXTURES / "claude-depth2-stream.jsonl", version="9.9.999",
                                      **arguments)["status"] == "unknown"


def test_native_background_completion_and_project_subdirectory_usage_are_counted_once(tmp_path):
    children = list(FIXTURES.glob("claude-background-agent-*.jsonl"))
    stream = FIXTURES / "claude-background-stream.jsonl"
    arguments = {"child_logs": children, "project": Path("D:/native-fixtures/graphhelm-planning"),
                 "session_id": "4eb1be7a-8dbb-4da9-b31b-6fccfb55420c"}
    result = evidence.claude_owned_tree(stream, **arguments)
    assert result["status"] == "observed", result
    assert result["total"] == {"input": 84, "output": 42160, "cacheRead": 3033050, "cacheWrite": 214194}
    rows = evidence.records(stream)
    # An asynchronous hand-back is not completion: the native callback must
    # identify the same agent, tool call and run before its counters qualify.
    missing = [r for r in rows if not (r.get("type") == "system" and r.get("subtype") == "task_notification")]
    path = tmp_path / "missing.jsonl"
    path.write_text("".join(json.dumps(r)+"\n" for r in missing), encoding="utf-8")
    assert evidence.claude_owned_tree(path, **arguments)["status"] == "unknown"
    child_rows = evidence.records(children[0])
    child_rows[-1]["cwd"] = "D:/native-fixtures/unrelated-project"
    foreign = tmp_path / "foreign-child.jsonl"
    foreign.write_text("".join(json.dumps(r)+"\n" for r in child_rows), encoding="utf-8")
    assert evidence.claude_owned_tree(stream, **{**arguments, "child_logs": [foreign, children[1]]})["status"] == "unknown"


def test_native_skill_loader_requires_native_metadata_and_pinned_body(tmp_path):
    skill = tmp_path / "SKILL.md"
    source = "---\nname: selected-skill\n---\nFollow the actual task. Input: $ARGUMENTS\n"
    skill.write_text(source, encoding="utf-8")
    pinned = hashlib.sha256(skill.read_bytes()).hexdigest()
    log = tmp_path / "native.jsonl"
    codex = {"type": "response_item", "payload": {"type": "message", "role": "user", "content": [
        {"type": "input_text", "text": f"<skill>\n<name>selected-skill</name>\n<path>{skill}</path>\n{source}</skill>"}]}}
    log.write_text(json.dumps(codex) + "\n", encoding="utf-8")
    arguments = {"cli": "codex", "expected_path": skill, "expected_sha256": pinned,
                 "frozen_prompt": "$selected-skill public task"}
    assert evidence.native_skill_load(log, **arguments)["status"] == "observed"
    injected = {**arguments, "frozen_prompt": codex["payload"]["content"][0]["text"]}
    assert evidence.native_skill_load(log, **injected)["status"] == "incomplete"
    codex["payload"]["role"] = "assistant"
    log.write_text(json.dumps(codex) + "\n", encoding="utf-8")
    assert evidence.native_skill_load(log, **arguments)["status"] == "incomplete"
    claude = {"type": "user", "isMeta": True, "message": {"content": [
        {"type": "text", "text": f"Base directory for this skill: {tmp_path}\n\nFollow the actual task. Input: public task\n"}]}}
    log.write_text(json.dumps(claude) + "\n", encoding="utf-8")
    arguments.update(cli="claude", frozen_prompt="/selected-skill public task")
    assert evidence.native_skill_load(log, **arguments)["status"] == "observed"
    claude["isMeta"] = False
    log.write_text(json.dumps(claude) + "\n", encoding="utf-8")
    assert evidence.native_skill_load(log, **arguments)["status"] == "incomplete"
    skill.write_text(source + "Unpinned amendment.\n", encoding="utf-8")
    assert evidence.native_skill_load(log, **arguments)["status"] == "incomplete"


def test_claude_native_skill_without_argument_macro_appends_exact_footer(tmp_path):
    skill = tmp_path / "SKILL.md"
    skill.write_text("---\nname: selected-skill\n---\nReal instructions.\n", encoding="utf-8")
    pinned = hashlib.sha256(skill.read_bytes()).hexdigest()
    log = tmp_path / "native.jsonl"
    row = {"type": "user", "isMeta": True, "message": {"content": [{"type": "text",
        "text": f"Base directory for this skill: {tmp_path}\n\nReal instructions.\n\n\nARGUMENTS: task-a"}]}}
    log.write_text(json.dumps(row) + "\n", encoding="utf-8")
    args = {"cli": "claude", "expected_path": skill, "expected_sha256": pinned,
            "frozen_prompt": "/selected-skill task-a"}
    assert evidence.native_skill_load(log, **args)["status"] == "observed"
    row["message"]["content"][0]["text"] = row["message"]["content"][0]["text"].replace("task-a", "task-b")
    log.write_text(json.dumps(row) + "\n", encoding="utf-8")
    assert evidence.native_skill_load(log, **args)["status"] == "incomplete"


def test_claude_initial_slash_skill_receives_full_multiline_arguments(tmp_path):
    skill = tmp_path / "SKILL.md"
    skill.write_text("---\nname: graphhelm-resume\n---\nResume $ARGUMENTS\n", encoding="utf-8")
    pinned = hashlib.sha256(skill.read_bytes()).hexdigest()
    log = tmp_path / "native.jsonl"
    row = {"type": "user", "isMeta": True, "message": {"content": [{"type": "text",
        "text": f"Base directory for this skill: {tmp_path}\n\nResume task-record-binding\nRead the task context."}]}}
    log.write_text(json.dumps(row) + "\n", encoding="utf-8")
    args = {"cli": "claude", "expected_path": skill, "expected_sha256": pinned,
            "frozen_prompt": "/graphhelm:graphhelm-resume task-record-binding\nRead the task context."}
    assert evidence.native_skill_load(log, **args)["status"] == "observed"


def test_claude_native_skill_crlf_expansion_matches_pinned_lf_source(tmp_path):
    skill = tmp_path / "SKILL.md"
    skill.write_text("---\nname: selected-skill\n---\nRead task context.\n", encoding="utf-8")
    pinned = hashlib.sha256(skill.read_bytes()).hexdigest()
    log = tmp_path / "native.jsonl"
    row = {"type": "user", "isMeta": True, "message": {"content": [{"type": "text",
        "text": f"Base directory for this skill: {tmp_path}\n\nRead task context.\r\n\r\n\r\nARGUMENTS: public task"}]}}
    log.write_text(json.dumps(row) + "\n", encoding="utf-8")
    args = {"cli": "claude", "expected_path": skill, "expected_sha256": pinned,
            "frozen_prompt": "/selected-skill public task"}
    assert evidence.native_skill_load(log, **args)["status"] == "observed"
