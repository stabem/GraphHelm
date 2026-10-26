"""Replay native subscription probes, including forked history and child accounting."""
import json
import copy
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
    previous = next(row for row in reversed(rows) if row.get("payload", {}).get("type") == "token_count")
    counter = copy.deepcopy(previous)
    counter["payload"]["info"]["total_token_usage"]["input_tokens"] += 100
    counter["payload"]["info"]["total_token_usage"]["output_tokens"] += 10
    rows.extend([counter, {"type": "event_msg", "payload": {"type": "task_complete", "turn_id": "followup"}}])
    completed = observe()
    assert completed["status"] == "observed"
    assert completed["total"]["input"] == 87312
    assert completed["total"]["output"] == 91
