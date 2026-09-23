#!/usr/bin/env python3
"""Split one bench run's spend into agent / MCP / gate, from the session transcript.

Attribution is per assistant message, by what that message did:
  mcp    the message issued a `mcp__graphhelm__*` call, OR it is the first message after such a
         call's result (the turn that reads the reply) - both are cost the methodology added
  gate   the message ran `ci/gate.ps1` or a repository-wide cargo test/build (the v2 arm-b run
         spent ~25 min here), or is the turn reading that output
  agent  everything else: reading code, editing, targeted tests, writing the answer

Each message's cost is its own tokens (input + output + cache read + cache write) priced at the
model's published rates when known, else reported as tokens only. Cache-read tokens dominate every
run, so the split is shown in tokens AND in dollars; the dollars are the session's own
`total_cost_usd` apportioned by token share, which is an approximation - the real per-message
price depends on cache hits the transcript does not itemise.

Usage: python tools/token-bench/split.py [--task 1044] [--version 4]
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
RESULTS = HERE / "results.jsonl"

READERS = ("grep", "sed", "cat", "head", "tail", "awk", "wc", "less", "rg", "type", "Get-Content", "Select-String")


def runs_the_gate(command: str) -> bool:
    """The gate RUNNING, not the file being read: a command segment that invokes ci/gate.ps1
    (by -File, by ./ci/gate.ps1, or by a PowerShell call operator), or a workspace-wide cargo
    run. `grep -n clippy ci/gate.ps1` is reading, and reading is agent work."""
    for raw in command.replace("&&", ";").replace("||", ";").replace("|", ";").split(";"):
        segment = raw.strip()
        if not segment:
            continue
        first = segment.split()[0].rsplit("/", 1)[-1]
        if first in READERS:
            continue
        if "gate.ps1" in segment and ("-File" in segment or "./ci/gate.ps1" in segment or first.startswith("powershell") or segment.startswith("&")):
            return True
        if segment.startswith("cargo") and "--workspace" in segment and any(verb in segment for verb in (" test", " nextest run", " build")):
            return True
    return False


def classify(message: dict, previous_kind: str, previous_was_call: bool) -> tuple[str, bool]:
    """Return (kind, issued_a_tool_call) for one assistant message."""
    kind = "agent"
    issued = False
    for block in (message.get("content") or []):
        if not isinstance(block, dict) or block.get("type") != "tool_use":
            continue
        issued = True
        name = block.get("name", "")
        command = str((block.get("input") or {}).get("command", ""))
        if name.startswith("mcp__graphhelm__"):
            kind = "mcp"
        elif runs_the_gate(command) and kind != "mcp":
            kind = "gate"
    if kind == "agent" and previous_kind in ("mcp", "gate") and (previous_was_call or not issued):
        # the turn that reads the reply belongs to what asked for it; so does a text-only streak
        # after it ("Waiting." x10 while the gate ran is the gate's cost, not the agent's)
        kind = previous_kind
    return kind, issued


def split_transcript(path: Path) -> dict:
    totals = {k: {"tokens": 0, "messages": 0} for k in ("agent", "mcp", "gate")}
    previous_kind, previous_was_call = "agent", False
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if row.get("type") != "assistant":
            continue
        message = row.get("message") or {}
        usage = message.get("usage") or {}
        tokens = sum(usage.get(k, 0) for k in ("input_tokens", "output_tokens", "cache_read_input_tokens", "cache_creation_input_tokens"))
        kind, issued = classify(message, previous_kind, previous_was_call)
        totals[kind]["tokens"] += tokens
        totals[kind]["messages"] += 1
        previous_kind, previous_was_call = kind, issued
    return totals


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--task")
    parser.add_argument("--version", type=int)
    args = parser.parse_args()
    rows = [json.loads(l) for l in RESULTS.read_text(encoding="utf-8").splitlines() if l.strip()]
    print("at | v | task | arm | verdict | USD total | agent | mcp | gate  (tokens, then USD apportioned)")
    for r in rows:
        if args.task and r["task"] != args.task:
            continue
        if args.version and r.get("benchVersion") != args.version:
            continue
        path = (r.get("transcript") or {}).get("path")
        if not path or not Path(path).exists():
            print(f"{r['at'][:16]} | {r.get('benchVersion')} | {r['task']} | {r['arm']} | no transcript")
            continue
        t = split_transcript(Path(path))
        total_tokens = sum(v["tokens"] for v in t.values()) or 1
        usd = r.get("costUsd") or 0.0
        cells = []
        for k in ("agent", "mcp", "gate"):
            share = t[k]["tokens"] / total_tokens
            cells.append(f"{t[k]['tokens']/1e6:.1f}M/${usd*share:.2f} ({t[k]['messages']} msgs)")
        print(f"{r['at'][:16]} | {r.get('benchVersion')} | {r['task']} | {r['arm']} | {r['verdict']} | ${usd:.2f} | " + " | ".join(cells))


if __name__ == "__main__":
    main()
