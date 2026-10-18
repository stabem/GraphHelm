"""Run each eval case headless and record which skill the agent picked (#184).

Selection needs no judge: whether an agent invoked a skill is a `Skill` tool call in its session
transcript. Each run is one hidden `claude -p` process with its own `--session-id`, a whole-run
timeout and a running cost cap. A failed run, a timeout or a missing transcript is `infra_error`
(INCOMPLETE in the report), never a negative. Output is the observations JSON that
`selection_report.py` reads; completion is `unjudged` (no rubric was applied).

usage: python tools/evals/run_selection.py --workdir <repo copy> --out <observations.json>
       [--repeats 3] [--case <id>]... [--model sonnet] [--max-turns 12] [--timeout 600]
       [--budget-usd 20]
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CASES = ROOT / "evals/graphhelm-development-contracts"
NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)
RUBRIC = "selection-only"


def split_case(text):
    """A case file is optional `---` frontmatter (`max_turns`, `allowed_tools: [A, B]`) then the
    prompt body. Returns (body, max_turns or None, allowed_tools or None)."""
    text = text.replace("\r\n", "\n")
    turns = tools = None
    if text.startswith("---\n"):
        head, sep, rest = text[4:].partition("\n---\n")
        if sep:
            text = rest
            for row in head.splitlines():
                key, _, value = row.partition(":")
                key, value = key.strip(), value.strip()
                if key == "max_turns" and value.isdigit():
                    turns = int(value)
                elif key == "allowed_tools" and value.startswith("[") and value.endswith("]"):
                    tools = [t.strip() for t in value[1:-1].split(",") if t.strip()]
    return text.strip(), turns, tools


def invoked_skills(lines):
    """Skill names from `Skill` tool calls in a transcript, plugin prefix dropped, in call order."""
    found = []
    for raw in lines:
        try:
            entry = json.loads(raw)
        except (TypeError, ValueError):
            continue
        content = (entry.get("message") or {}).get("content") if isinstance(entry, dict) else None
        for block in content if isinstance(content, list) else []:
            if isinstance(block, dict) and block.get("type") == "tool_use" and block.get("name") == "Skill":
                name = str((block.get("input") or {}).get("skill", "")).rsplit(":", 1)[-1].strip()
                if name and name not in found:
                    found.append(name)
    return found


def observation(case_id, run, session_id, exited_ok, transcript_lines, cost_usd):
    """One observation; a failed process or an absent transcript is `infra_error`."""
    ok = exited_ok and transcript_lines is not None
    return {"case_id": case_id, "run": run, "session_id": session_id,
            "status": "completed" if ok else "infra_error",
            "invoked_skills": invoked_skills(transcript_lines) if ok else [],
            "completion": "unjudged", "rubric": RUBRIC,
            **({"cost_usd": cost_usd} if cost_usd is not None else {})}


def find_transcript(session_id):
    projects = Path.home() / ".claude" / "projects"
    for path in projects.glob(f"*/{session_id}.jsonl"):
        return path.read_text(encoding="utf-8", errors="replace").splitlines()
    return None


def run_case(claude, case_id, run, workdir, model, max_turns, timeout):
    prompt, turns, tools = split_case((CASES / case_id / "prompt.md").read_text(encoding="utf-8"))
    session_id = str(uuid.uuid4())
    # The prompt goes on stdin: a body starting with "-" must never be read as an option.
    cmd = [claude, "-p", "--session-id", session_id, "--model", model,
           "--max-turns", str(turns or max_turns), "--output-format", "json"]
    if tools:
        cmd += ["--allowedTools", ",".join(tools)]
    cost, ok = None, False
    try:
        done = subprocess.run(cmd, cwd=workdir, input=prompt, capture_output=True, text=True,
                              encoding="utf-8", timeout=timeout, creationflags=NO_WINDOW)
        ok = done.returncode == 0
        try:
            cost = float(json.loads(done.stdout).get("total_cost_usd"))
        except (TypeError, ValueError, AttributeError):
            cost = None
    except subprocess.TimeoutExpired:
        ok = False
    return observation(case_id, run, session_id, ok, find_transcript(session_id), cost)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--workdir", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--case", action="append", default=[])
    parser.add_argument("--model", default="sonnet")
    parser.add_argument("--max-turns", type=int, default=12)
    parser.add_argument("--timeout", type=int, default=600)
    parser.add_argument("--budget-usd", type=float, default=20.0)
    args = parser.parse_args(argv)
    claude = shutil.which("claude") or os.environ.get("CLAUDE_CLI")
    if not claude:
        print("claude CLI not found", file=sys.stderr)
        return 2
    cases = args.case or sorted(p.parent.name for p in CASES.glob("*/prompt.md"))
    observations, spent = [], 0.0
    for run in range(1, args.repeats + 1):
        for case_id in cases:
            if spent >= args.budget_usd:
                print(f"budget reached: ${spent:.2f} >= ${args.budget_usd:.2f}; stopping", file=sys.stderr)
                args.out.write_text(json.dumps(observations, indent=1), encoding="utf-8")
                return 3
            obs = run_case(claude, case_id, run, args.workdir, args.model, args.max_turns, args.timeout)
            spent += obs.get("cost_usd") or 0.0
            observations.append(obs)
            args.out.write_text(json.dumps(observations, indent=1), encoding="utf-8")
            print(f"{run}/{case_id}: {obs['status']} {obs['invoked_skills']} spent=${spent:.2f}", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
