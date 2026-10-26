"""Dependency-free transcript accounting shared by native and legacy runners."""
from __future__ import annotations

import json


def sum_transcript_usage(lines: list[str], path: str | None = None) -> dict:
    """Sum the largest usage record for each assistant message ID."""
    tot = {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0,
           "assistantMessages": 0, "userMessages": 0, "parseErrors": 0}
    by_id = {}
    models = set()
    tools_seen = {}
    for line in lines:
        try:
            d = json.loads(line)
        except json.JSONDecodeError:
            tot["parseErrors"] += 1
            continue
        if not isinstance(d, dict):
            tot["parseErrors"] += 1
            continue
        if d.get("type") == "user":
            tot["userMessages"] += 1
        if d.get("type") != "assistant":
            continue
        message = d.get("message") or {}
        if not isinstance(message, dict):
            tot["parseErrors"] += 1
            continue
        if isinstance(message.get("model"), str) and message["model"] not in {"<synthetic>", "unknown"}:
            models.add(message["model"])
        for block in message.get("content") or []:
            if isinstance(block, dict) and block.get("type") == "tool_use" and block.get("id"):
                tools_seen[block["id"]] = block.get("name", "unknown")
        message_id = message.get("id") or d.get("uuid")
        usage = message.get("usage") or {}
        if not isinstance(usage, dict):
            tot["parseErrors"] += 1
            continue
        counts = {"input": usage.get("input_tokens", 0), "output": usage.get("output_tokens", 0),
                  "cacheRead": usage.get("cache_read_input_tokens", 0),
                  "cacheWrite": usage.get("cache_creation_input_tokens", 0)}
        key = message_id or f"line-{len(by_id)}"
        if key not in by_id or sum(counts.values()) > sum(by_id[key].values()):
            by_id[key] = counts
    for counts in by_id.values():
        for name, value in counts.items():
            tot[name] += value
    tot["assistantMessages"] = len(by_id)
    tot["models"] = sorted(models)
    tot["toolCalls"] = len(tools_seen)
    tot["toolNames"] = {name: list(tools_seen.values()).count(name) for name in sorted(set(tools_seen.values()))}
    if path:
        tot["path"] = path
    return tot
