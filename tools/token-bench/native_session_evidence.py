"""Read native subscription session evidence; never accept model-reported token totals."""
from __future__ import annotations

import hashlib
import json
import re
import uuid
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


def verified_archive(archive: dict, *, cli: str) -> tuple[Path, list[Path]]:
    """Bind every replayed native journal to its checked archive digest."""
    root_value = archive.get("archivedRoot")
    hashes = archive.get("archive")
    if not isinstance(root_value, str) or not isinstance(hashes, dict) or cli not in {"codex", "claude"}:
        raise ValueError("native archive inventory is missing")
    root = Path(root_value)
    directory = root.parent
    prefix = "child-" if cli == "codex" else "agent-"
    actual = {path.name for path in directory.glob("*.jsonl")}
    if (root.name != "root.jsonl" or not directory.is_dir() or
            not all(isinstance(name, str) and (name == "root.jsonl" or
                    (name.startswith(prefix) and name.endswith(".jsonl")))
                    for name in hashes) or set(hashes) != actual):
        raise ValueError("native archive journal inventory differs")
    for name, digest in hashes.items():
        path = directory / name
        if (path.is_symlink() or not path.is_file() or not isinstance(digest, str)
                or hashlib.sha256(path.read_bytes()).hexdigest() != digest):
            raise ValueError("native archive journal hash differs")
    return root, sorted(directory / name for name in hashes if name != "root.jsonl")


def native_review(path: Path) -> dict:
    """Read a terminal review from a real CLI envelope, never a standalone verdict row."""
    try:
        rows = records(path)
        codex = [r for r in rows if r.get("type") == "thread.started"]
        claude = [r for r in rows if r.get("type") == "system" and r.get("subtype") == "init"]
        if len(codex) == 1 and not claude:
            root = codex[0]["thread_id"]
            finals = [r for r in rows if r.get("type") == "turn.completed"]
            messages = [r["item"]["text"] for r in rows if r.get("type") == "item.completed"
                        and r.get("item", {}).get("type") == "agent_message"]
            if len(finals) != 1 or not messages or any(r.get("type") == "turn.failed" for r in rows):
                raise ValueError("native Codex review did not complete")
            body = messages[-1]
            cli = "codex"
        elif len(claude) == 1 and not codex:
            root = claude[0]["session_id"]
            finals = [r for r in rows if r.get("type") == "result"]
            if (len(finals) != 1 or finals[0].get("is_error") is not False
                    or finals[0].get("subtype") != "success" or finals[0].get("session_id") != root):
                raise ValueError("native Claude review did not complete")
            body = finals[0]["result"]
            cli = "claude"
        else:
            raise ValueError("native review root is missing or ambiguous")
        if str(uuid.UUID(root)) != root:
            raise ValueError("native review root is not a canonical UUID")
        if isinstance(body, str) and body.startswith("```json\n") and body.endswith("\n```"):
            body = body[8:-4]
        verdict = json.loads(body)
        if not isinstance(verdict, dict) or verdict.get("verdict") not in {"APPROVE", "APPROVE-WITH-RISK", "BLOCK"}:
            raise ValueError("native terminal response is not a review verdict")
        return {"status": "observed", "cli": cli, "rootSession": root, "verdict": verdict}
    except (KeyError, TypeError, OSError, ValueError) as error:
        return {"status": "unknown", "reason": str(error)}


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
            if meta.get("cli_version") != "0.157.1":
                raise ValueError("Codex thread inclusion semantics are not qualified for this version")
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
                    info = event.get("info")
                    if isinstance(info, dict) and isinstance(info.get("total_token_usage"), dict):
                        counters(info["total_token_usage"])
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
                    if item.get("type") == "tool_use" and item.get("name") in {"Bash", "PowerShell"}:
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
            if (not isinstance(requirement, dict) or not isinstance(requirement.get("commandPattern"), str)
                    or not isinstance(requirement.get("outputJson", {}), dict)):
                raise ValueError("native operation requires a command pattern and an optional JSON object")
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


def claude_owned_tree(stream: Path, child_logs: list[Path], *, project: Path,
                      session_id: str, version: str = "2.1.310") -> dict:
    """Reconcile recursive agents using final persisted native assistant usage.

    Forwarded assistant chunks and Agent hand-back usage are insufficient for a
    multi-turn child. Its private persisted transcript contains finalized usage
    for every unique native message. The root pipeline must equal the main loop
    plus those disjoint messages, and all reported children must have completed.
    """
    try:
        if version != "2.1.310":
            raise ValueError("Claude recursive inclusion semantics are not qualified for this version")
        rows = records(stream)
        finals = [r for r in rows if r.get("type") == "result"]
        if len(finals) != 1 or finals[0].get("session_id") != session_id:
            raise ValueError("native final session identity mismatch")
        final = finals[0]
        stats = final.get("subagent_stats")
        if not isinstance(stats, dict) or stats.get("spawned") != stats.get("completed"):
            raise ValueError("native child tree is incomplete")
        calls = set()
        answered = set()
        bindings = {}
        starts = {}
        notifications = {}
        for row in rows:
            if row.get("type") != "system" or row.get("session_id") != session_id:
                continue
            call = row.get("tool_use_id")
            if row.get("subtype") == "task_started" and row.get("task_type") == "local_agent":
                if call in starts:
                    raise ValueError("duplicate native background agent start")
                starts[call] = row
            if row.get("subtype") == "task_notification" and row.get("task_id", "").startswith("a"):
                if call in notifications:
                    raise ValueError("duplicate native background agent completion")
                notifications[call] = row
        for row in rows:
            if row.get("type") in {"user", "assistant"} and row.get("session_id") != session_id:
                raise ValueError("native linkage event belongs to another session")
            parent_call, agent = row.get("parent_tool_use_id"), row.get("agent_id")
            if isinstance(parent_call, str) and isinstance(agent, str):
                if parent_call in bindings and bindings[parent_call] != agent:
                    raise ValueError("native agent-call binding changed")
                bindings[parent_call] = agent
            message = row.get("message", {})
            content = message.get("content", []) if isinstance(message, dict) else []
            for item in content if isinstance(content, list) else []:
                if not isinstance(item, dict):
                    continue
                if item.get("type") == "tool_use" and item.get("name") == "Agent":
                    call = item.get("id")
                    if not isinstance(call, str) or call in calls:
                        raise ValueError("duplicate native Agent call")
                    calls.add(call)
                elif item.get("type") == "tool_result" and item.get("tool_use_id") in calls:
                    call = item["tool_use_id"]
                    if call in answered or item.get("is_error") is True:
                        raise ValueError("duplicate or failed native Agent result")
                    answered.add(call)
                    result = row.get("tool_use_result")
                    if isinstance(result, dict) and isinstance(result.get("agentId"), str):
                        if result.get("status") == "async_launched":
                            start = starts.get(call, {})
                            done = notifications.get(call, {})
                            if (result.get("isAsync") is not True or start.get("task_id") != result["agentId"]
                                    or done.get("task_id") != result["agentId"] or done.get("status") != "completed"
                                    or not start.get("run_id") or done.get("run_id") != start["run_id"]):
                                raise ValueError("native background agent has no matching completion")
                        elif result.get("status") != "completed":
                            raise ValueError("native agent has not completed")
                        if call in bindings and bindings[call] != result["agentId"]:
                            raise ValueError("native result agent identity mismatch")
                        bindings[call] = result["agentId"]
        if calls != answered or set(bindings) != calls or len(set(bindings.values())) != len(calls):
            raise ValueError("native Agent calls, results and child identities are not bijective")
        expected = set(bindings.values())
        if stats.get("spawned") != len(expected) or stats.get("failed", 0) != 0:
            raise ValueError("native child statistics do not match observed tree")
        children = {}
        for path in child_logs:
            native = records(path)
            metadata = [r for r in native if r.get("type") in {"user", "assistant"}]
            if not metadata:
                raise ValueError("native child transcript is empty")
            agent = metadata[0].get("agentId")
            if agent not in expected or agent in children:
                raise ValueError("unknown or duplicate native child transcript")
            messages = {}
            for row in metadata:
                if (row.get("agentId") != agent or row.get("sessionId") != session_id
                        or row.get("version") != version
                        or not Path(row.get("cwd", "")).resolve().is_relative_to(project.resolve())):
                    raise ValueError("native child provenance mismatch")
                if row["type"] == "assistant":
                    message = row.get("message", {})
                    identifier = message.get("id")
                    if not isinstance(identifier, str):
                        raise ValueError("native assistant message identity missing")
                    messages[identifier] = counters(message["usage"], claude=True)
            if not messages:
                raise ValueError("native child message counters missing")
            children[agent] = {"usage": _sum(list(messages.values())), "messages": len(messages),
                               "path": str(path), "sha256": _hash(path)}
        if set(children) != expected:
            raise ValueError("native child transcript missing")
        models = final.get("modelUsage")
        if not isinstance(models, dict) or not models:
            raise ValueError("native pipeline counters missing")
        pipeline = _sum([counters({"input_tokens": v.get("inputTokens"),
                                   "output_tokens": v.get("outputTokens"),
                                   "cache_read_input_tokens": v.get("cacheReadInputTokens"),
                                   "cache_creation_input_tokens": v.get("cacheCreationInputTokens")}, claude=True)
                         for v in models.values()])
        main = counters(final["usage"], claude=True)
        if _sum([main, *[v["usage"] for v in children.values()]]) != pipeline:
            raise ValueError("native persisted child messages do not reconcile with pipeline")
        return {"status": "observed", "total": pipeline, "mainLoop": main,
                "children": children, "sha256": _hash(stream),
                "semantics": "claude-pipeline-reconciled-persisted-recursive-messages"}
    except (KeyError, TypeError, OSError, ValueError) as error:
        return {"status": "unknown", "reason": str(error), "total": None}


def native_skill_load(native_log: Path, *, cli: str, expected_path: Path,
                      expected_sha256: str, frozen_prompt: str) -> dict:
    """Observe the native loader's expanded skill, not merely a present skill file.

    Codex records a separate native `<skill>` user message. Claude records a
    native metadata message with the installed base directory; its frontend
    removes YAML frontmatter and substitutes the slash command's arguments.
    Model claims, ordinary file reads and the initial task prompt are insufficient.
    """
    try:
        if "<skill>" in frozen_prompt or "Base directory for this skill:" in frozen_prompt:
            raise ValueError("frozen task prompt embeds a native loader marker")
        expected_path = expected_path.resolve()
        if expected_path.name != "SKILL.md" or _hash(expected_path) != expected_sha256:
            raise ValueError("selected native skill asset changed or is invalid")
        source = expected_path.read_text(encoding="utf-8")
        frontmatter = re.match(r"\A---\s*\n(.*?)\n---", source, re.S)
        if frontmatter is None:
            raise ValueError("selected skill has no native frontmatter")
        metadata_name = re.search(r"^name:\s*(.*?)\s*$", frontmatter[1], re.M)
        if metadata_name is None:
            raise ValueError("selected skill has no native name")
        expected_name = metadata_name[1].strip('"\'')
        native_rows = records(native_log)
        arguments_seen = {""}
        first, _, initial_arguments = frozen_prompt.partition(" ")
        if first.lstrip("/$").split(":")[-1] == expected_name:
            arguments_seen.add(initial_arguments)
        for row in native_rows:
            message = row.get("message", {})
            if row.get("type") != "assistant" or not isinstance(message, dict):
                continue
            for item in message.get("content", []):
                if not isinstance(item, dict) or item.get("type") != "tool_use" or item.get("name") != "Skill":
                    continue
                selected = item.get("input", {})
                if (isinstance(selected, dict) and isinstance(selected.get("skill"), str)
                        and selected["skill"].split(":")[-1] == expected_name
                        and isinstance(selected.get("args", ""), str)):
                    arguments_seen.add(selected.get("args", ""))
        loaded = []
        for row in native_rows:
            if cli == "codex":
                message = row.get("payload", {})
                if row.get("type") != "response_item" or message.get("type") != "message" or message.get("role") != "user":
                    continue
            elif cli == "claude":
                if row.get("type") != "user" or row.get("isMeta") is not True:
                    continue
                message = row.get("message", {})
            else:
                raise ValueError("unsupported native skill loader protocol")
            content = message.get("content", [])
            texts = [item.get("text", "") if isinstance(item, dict) else item
                     for item in content] if isinstance(content, list) else [content]
            for text in texts:
                if not isinstance(text, str):
                    continue
                if cli == "codex":
                    for match in re.finditer(r"<skill>\s*<name>(.*?)</name>\s*<path>(.*?)</path>\s*(.*?)</skill>", text, re.S):
                        path = Path(match[2]).resolve()
                        loaded.append(str(path))
                        if path == expected_path and match[1] == expected_name and match[3].strip() == source.strip():
                            return {"status": "observed", "skillPath": str(path), "skillSha256": expected_sha256,
                                    "nativeLogSha256": _hash(native_log), "loader": "codex-native-skill-message"}
                elif text.startswith("Base directory for this skill: "):
                    header, body = text.split("\n\n", 1)
                    path = (Path(header.removeprefix("Base directory for this skill: ")) / "SKILL.md").resolve()
                    loaded.append(str(path))
                    plain = re.sub(r"\A---\s*\n.*?\n---\s*\n", "", source, count=1, flags=re.S)
                    expected_bodies = {plain.replace("$ARGUMENTS", arguments).strip() for arguments in arguments_seen}
                    if "$ARGUMENTS" not in plain:
                        expected_bodies.update((plain + "\n\nARGUMENTS: " + arguments).strip()
                                               for arguments in arguments_seen if arguments)
                    normalized_body = body.replace("\r\n", "\n").replace("\r", "\n").strip()
                    if path == expected_path and normalized_body in expected_bodies:
                        return {"status": "observed", "skillPath": str(path), "skillSha256": expected_sha256,
                                "nativeLogSha256": _hash(native_log), "loader": "claude-native-skill-metadata"}
        return {"status": "incomplete", "reason": "selected skill was not expanded by the native loader",
                "observedSkillPaths": loaded}
    except (OSError, ValueError, TypeError, AttributeError) as error:
        return {"status": "incomplete", "reason": str(error)}
