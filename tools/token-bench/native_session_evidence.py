"""Read native subscription session evidence; never accept model-reported token totals."""
from __future__ import annotations

import hashlib
import json
import re
from pathlib import Path


def records(path: Path) -> list[dict]:
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 128 * 1024 * 1024:
        raise ValueError("native session file is missing, linked or too large")
    result = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip():
            row = json.loads(line)
            if not isinstance(row, dict):
                raise ValueError("invalid native event")
            result.append(row)
    return result


def counters(value: dict, *, claude: bool = False) -> dict:
    names = ({"input": "input_tokens", "output": "output_tokens",
              "cacheRead": "cache_read_input_tokens", "cacheWrite": "cache_creation_input_tokens"}
             if claude else {"input": "input_tokens", "output": "output_tokens",
                            "cacheRead": "cached_input_tokens", "cacheWrite": "cache_write_input_tokens"})
    result = {}
    for name, wire in names.items():
        number = value.get(wire, 0 if name == "cacheWrite" and not claude else None)
        if type(number) is not int or number < 0:
            raise ValueError("missing or invalid native counter: " + wire)
        result[name] = number
    if not claude and result["cacheRead"] > result["input"]:
        raise ValueError("cached input exceeds input")
    return result


def _sum(rows: list[dict]) -> dict:
    return {key: sum(row[key] for row in rows) for key in ("input", "output", "cacheRead", "cacheWrite")}


def _hash(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def codex_tree(root_log: Path, child_logs: list[Path], *, project: Path,
               root_thread: str, stdout_usage: dict) -> dict:
    """Validate linked completed threads, then add each final cumulative counter once.

    Callers supply only logs discovered under their owned root. This function checks
    the native linkage, cwd and successful spawn receipts instead of trusting that list.
    """
    try:
        nodes = {}
        for path in [root_log, *child_logs]:
            rows = records(path)
            metadata = [r["payload"] for r in rows if r.get("type") == "session_meta"]
            if not metadata or rows[0].get("type") != "session_meta":
                raise ValueError("native session metadata must be first")
            meta = metadata[0]
            thread = meta.get("id")
            if not isinstance(thread, str) or thread in nodes:
                raise ValueError("missing or duplicate thread")
            if Path(meta.get("cwd", "")).resolve() != project.resolve():
                raise ValueError("native session belongs to another project")
            # fork_turns=all embeds ancestor metadata and history after the new
            # thread's leading metadata. Only its own settings boundary begins
            # fresh execution; inherited calls/counters must never count again.
            if len(metadata) > 1:
                boundaries = [i for i, r in enumerate(rows)
                              if r.get("type") == "event_msg"
                              and r.get("payload", {}).get("type") == "thread_settings_applied"
                              and r["payload"].get("thread_id") == thread]
                if len(boundaries) != 1:
                    raise ValueError("forked native history has no unique execution boundary")
                rows = rows[boundaries[0]:]
            started = {}
            completed = set()
            last_counter = -1
            for index, row in enumerate(rows):
                if row.get("type") != "event_msg":
                    continue
                event = row.get("payload", {})
                turn = event.get("turn_id")
                if event.get("type") == "task_started":
                    if not isinstance(turn, str) or turn in started:
                        raise ValueError("missing or duplicate native turn start")
                    started[turn] = index
                elif event.get("type") == "task_complete":
                    if turn not in started or turn in completed or last_counter < started[turn]:
                        raise ValueError("native completion lacks its own turn counters")
                    completed.add(turn)
                elif event.get("type") == "token_count":
                    last_counter = index
            if not started or set(started) != completed:
                raise ValueError("latest native task has not completed")
            totals = [r["payload"]["info"]["total_token_usage"] for r in rows
                      if r.get("type") == "event_msg" and r.get("payload", {}).get("type") == "token_count"
                      and isinstance(r["payload"].get("info"), dict)]
            if not totals:
                raise ValueError("native final cumulative counters missing")
            parsed = [counters(total) for total in totals]
            for previous, current in zip(parsed, parsed[1:]):
                if any(current[key] < previous[key] for key in current):
                    raise ValueError("native cumulative counters moved backwards")
            calls = {}
            spawned = []
            for row in rows:
                if row.get("type") != "response_item":
                    continue
                payload = row.get("payload", {})
                if payload.get("type") == "function_call" and payload.get("name") == "spawn_agent":
                    calls[payload.get("call_id")] = True
                if payload.get("type") == "function_call_output" and payload.get("call_id") in calls:
                    output = payload.get("output")
                    if isinstance(output, str):
                        try:
                            output = json.loads(output)
                        except ValueError:
                            output = None
                    if isinstance(output, dict) and isinstance(output.get("task_name"), str):
                        spawned.append(output["task_name"])
            nodes[thread] = {"session": thread, "parent": meta.get("parent_thread_id"),
                             "agentPath": meta.get("agent_path"), "spawned": spawned,
                             "usage": parsed[-1], "path": str(path), "sha256": _hash(path)}
        if root_thread not in nodes or nodes[root_thread]["parent"]:
            raise ValueError("native root identity mismatch")
        if nodes[root_thread]["usage"] != stdout_usage:
            raise ValueError("root rollout and stdout counters disagree")
        visited = set()

        def visit(thread: str) -> None:
            if thread in visited:
                raise ValueError("cycle or repeated native thread")
            visited.add(thread)
            node = nodes[thread]
            children = [n for n in nodes.values() if n["parent"] == thread]
            if sorted(c["agentPath"] for c in children) != sorted(node["spawned"]):
                raise ValueError("successful native spawn lacks exactly one child receipt")
            for child in children:
                visit(child["session"])

        visit(root_thread)
        if visited != set(nodes):
            raise ValueError("unrelated native session supplied")
        return {"status": "observed", "total": _sum([n["usage"] for n in nodes.values()]),
                "sessions": list(nodes.values()), "semantics": "codex-thread-cumulative-disjoint"}
    except (KeyError, TypeError, OSError, ValueError) as error:
        return {"status": "unknown", "reason": str(error), "total": None}


def claude_pipeline(stream: Path) -> dict:
    """Reconcile the actual Agent tool results against pipeline minus main-loop usage.

    This pinned observer supports the qualified one-level native Agent protocol.
    Deeper trees stay unknown until their inclusion semantics are independently proven.
    """
    try:
        rows = records(stream)
        finals = [r for r in rows if r.get("type") == "result"]
        if len(finals) != 1:
            raise ValueError("expected exactly one native final result")
        final = finals[0]
        model_usage = final.get("modelUsage")
        if not isinstance(model_usage, dict) or not model_usage:
            raise ValueError("native pipeline counters missing")
        pipeline_rows = []
        for value in model_usage.values():
            normalized = {"input_tokens": value.get("inputTokens"), "output_tokens": value.get("outputTokens"),
                          "cache_read_input_tokens": value.get("cacheReadInputTokens"),
                          "cache_creation_input_tokens": value.get("cacheCreationInputTokens")}
            pipeline_rows.append(counters(normalized, claude=True))
        pipeline = _sum(pipeline_rows)
        main = counters(final["usage"], claude=True)
        agent_calls = set()
        children = {}
        answered_calls = set()
        for row in rows:
            message = row.get("message", {})
            content = message.get("content", []) if isinstance(message, dict) else []
            for item in content if isinstance(content, list) else []:
                if isinstance(item, dict) and item.get("type") == "tool_use" and item.get("name") == "Agent":
                    if not isinstance(item.get("id"), str) or item["id"] in agent_calls:
                        raise ValueError("missing or duplicate native Agent call ID")
                    agent_calls.add(item.get("id"))
            result = row.get("tool_use_result")
            if isinstance(result, dict) and "agentId" in result:
                if result.get("status") != "completed" or not isinstance(result.get("usage"), dict):
                    raise ValueError("native child incomplete or counters missing")
                child = result["agentId"]
                if child in children:
                    raise ValueError("duplicate child result")
                linked = [item["tool_use_id"] for item in content if isinstance(item, dict)
                          and item.get("type") == "tool_result" and item.get("tool_use_id") in agent_calls]
                if len(linked) != 1 or linked[0] in answered_calls:
                    raise ValueError("child result has no native Agent call")
                answered_calls.add(linked[0])
                children[child] = counters(result["usage"], claude=True)
        stats = final.get("subagent_stats", {})
        if agent_calls or stats:
            if not isinstance(stats, dict) or stats.get("spawned") != len(children) or stats.get("completed") != len(children):
                raise ValueError("native child count mismatch")
            if stats.get("max_depth") != (1 if children else 0) or stats.get("spawned_by_subagents") != 0:
                raise ValueError("unqualified nested-child inclusion semantics")
        if agent_calls != answered_calls:
            raise ValueError("Agent call has no completed child usage")
        expected = _sum([main, *children.values()])
        if expected != pipeline:
            raise ValueError("pipeline counters do not reconcile with native children")
        return {"status": "observed", "total": pipeline, "mainLoop": main,
                "children": children, "sha256": _hash(stream),
                "semantics": "claude-pipeline-includes-children"}
    except (KeyError, TypeError, OSError, ValueError) as error:
        return {"status": "unknown", "reason": str(error), "total": None}


def required_operations(stream: Path, *, cli: str, requirements: list[dict]) -> dict:
    """Match frozen mandatory commands to successful native results by tool ID.

    Requirements contain a command regex and optionally a JSON output object
    whose listed key/value pairs must occur in the actual command result.
    Artifact hashes are collected separately by the controller after execution.
    """
    try:
        rows = records(stream)
        calls = {}
        completed = []
        if cli == "codex":
            for row in rows:
                item = row.get("item", {})
                if row.get("type") == "item.completed" and item.get("type") == "command_execution":
                    completed.append({"id": item.get("id"), "command": item.get("command", ""),
                                      "success": item.get("exit_code") == 0 and item.get("status") == "completed",
                                      "output": item.get("aggregated_output", "")})
        elif cli == "claude":
            for row in rows:
                message = row.get("message", {})
                content = message.get("content", []) if isinstance(message, dict) else []
                for item in content if isinstance(content, list) else []:
                    if not isinstance(item, dict):
                        continue
                    if item.get("type") == "tool_use" and item.get("name") == "Bash":
                        calls[item.get("id")] = item.get("input", {}).get("command", "")
                    if item.get("type") == "tool_result" and item.get("tool_use_id") in calls:
                        output = item.get("content", "")
                        if isinstance(output, list):
                            output = "\n".join(c.get("text", "") for c in output if isinstance(c, dict))
                        completed.append({"id": item["tool_use_id"], "command": calls[item["tool_use_id"]],
                                          "success": item.get("is_error") is False,
                                          "output": output})
        else:
            raise ValueError("unsupported native command protocol")
        matched = []
        for requirement in requirements:
            candidates = [c for c in completed if re.search(requirement["commandPattern"], c["command"])]
            accepted = []
            for candidate in candidates:
                if not candidate["success"]:
                    continue
                if any(text not in candidate["output"] for text in requirement.get("outputContains", [])):
                    continue
                expected = requirement.get("outputJson", {})
                if expected:
                    try:
                        actual = json.loads(candidate["output"].strip())
                    except (TypeError, ValueError):
                        continue
                    if not isinstance(actual, dict) or any(actual.get(k) != v for k, v in expected.items()):
                        continue
                accepted.append(candidate)
            if not accepted:
                raise ValueError("mandatory native operation missing or failed: " + requirement["commandPattern"])
            matched.append({"requirement": requirement, "toolIds": [c["id"] for c in accepted]})
        return {"status": "observed", "operations": matched, "sha256": _hash(stream)}
    except (KeyError, TypeError, OSError, ValueError, re.error) as error:
        return {"status": "incomplete", "reason": str(error)}
