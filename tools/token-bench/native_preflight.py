"""Refuse a native benchmark session before it spends tokens on a broken environment.

Every check reads local state or asks a CLI about itself; none starts a model turn.
The receipt names each check, what it saw and whether it passed. Any failure exits 2,
and the cell is recorded as `config_error` (or `OBSERVER_MISSING` for observer checks)
instead of being launched.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)
PAID_KEYS = {"claude": ("ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"),
             "codex": ("OPENAI_API_KEY", "CODEX_API_KEY")}


def run(argv: list[str], cwd: Path | None = None) -> tuple[int, str]:
    # npm CLIs are .cmd shims on Windows; resolve them as the shell would.
    argv = [shutil.which(argv[0]) or argv[0], *argv[1:]]
    try:
        done = subprocess.run(argv, cwd=cwd, capture_output=True, text=True, timeout=90,
                              encoding="utf-8", errors="replace", creationflags=NO_WINDOW)
    except (OSError, subprocess.SubprocessError) as error:
        return -1, str(error)
    return done.returncode, done.stdout + done.stderr


def permission_problems(project: Path, tools: list[str]) -> list[str]:
    """Project settings that deny every tool, or a tool the method needs, stop the cell."""
    problems = []
    for name in ("settings.json", "settings.local.json"):
        path = project / ".claude" / name
        if not path.is_file():
            continue
        try:
            deny = json.loads(path.read_text(encoding="utf-8-sig")).get("permissions", {}).get("deny", [])
        except (OSError, ValueError, AttributeError) as error:
            problems.append(f"{name}: unreadable ({error})")
            continue
        for rule in deny if isinstance(deny, list) else [deny]:
            rule = str(rule)
            if rule in ("*", "**") or any(rule == tool or rule.startswith(tool + "(") and rule.endswith("(*)")
                                          for tool in tools):
                problems.append(f"{name}: deny rule {rule!r} blocks a required tool")
    return problems


def auth_problems(provider: str, status_output: str, environ: dict) -> list[str]:
    """Only a logged-in subscription counts; an API key in the environment could bill."""
    problems = [f"{key} is set; paid API fallback is forbidden" for key in PAID_KEYS[provider]
                if environ.get(key)]
    if provider == "claude":
        try:
            status = json.loads(status_output)
        except ValueError:
            return problems + ["claude auth status is not JSON"]
        if status.get("loggedIn") is not True or status.get("authMethod") != "claude.ai":
            problems.append("claude is not logged in with a claude.ai subscription")
    elif "Logged in using ChatGPT" not in status_output:
        problems.append("codex is not logged in with a ChatGPT subscription")
    return problems


def mcp_problems(listing: str, servers: list[str]) -> list[str]:
    # Names may contain colons (plugin:graphhelm:graphhelm); the name ends at the first ": ".
    connected = {line.split(": ", 1)[0].strip() for line in listing.splitlines()
                 if line.rstrip().endswith("Connected") and ": " in line}
    return [f"MCP server {name!r} is not connected" for name in servers if name not in connected]


def mirror_problems(mirror: Path, head: str) -> list[str]:
    code, out = run(["git", "-C", str(mirror), "rev-parse", "HEAD"])
    if code != 0:
        return [f"observer mirror is not a Git checkout: {out.strip()[:200]}"]
    problems = [] if out.strip() == head else [f"observer mirror is at {out.strip()[:12]}, not {head[:12]}"]
    code, out = run(["git", "-C", str(mirror), "status", "--porcelain", "--untracked-files=no"])
    if code != 0 or out.strip():
        problems.append("observer mirror has tracked changes")
    return problems


def logs_problems(logs_root: Path) -> list[str]:
    try:
        logs_root.mkdir(parents=True, exist_ok=True)
        probe = logs_root / ".preflight-write"
        probe.write_text("ok", encoding="utf-8")
        probe.unlink()
    except OSError as error:
        return [f"logs root is not writable: {error}"]
    return []


def runner_problems(runner: Path, sha256: str) -> list[str]:
    try:
        actual = hashlib.sha256(runner.read_bytes()).hexdigest()
    except OSError as error:
        return [f"reached runner unreadable: {error}"]
    return [] if actual == sha256 else [f"reached runner is {actual[:12]}, not the frozen {sha256[:12]}"]


def preflight(args: argparse.Namespace, environ: dict | None = None) -> dict:
    environ = dict(os.environ if environ is None else environ)
    status_cmd = ["claude", "auth", "status"] if args.provider == "claude" else ["codex", "login", "status"]
    _, status_out = run(status_cmd)
    checks = {"permissions": permission_problems(args.project, args.tool),
              "auth": auth_problems(args.provider, status_out, environ)}
    if args.mcp:
        list_cmd = ["claude", "mcp", "list"] if args.provider == "claude" else ["codex", "mcp", "list"]
        _, listing = run(list_cmd, cwd=args.project)
        checks["mcp"] = mcp_problems(listing, args.mcp)
    observer = {"logs": logs_problems(args.logs_root),
                "mirror": mirror_problems(args.mirror, args.head),
                "runner": runner_problems(args.runner, args.runner_sha256)}
    checks.update(observer)
    failed = [name for name, problems in checks.items() if problems]
    verdict = "ready" if not failed else (
        "OBSERVER_MISSING" if set(failed) <= set(observer) else "config_error")
    return {"schema": "graphhelm-native-preflight/1", "provider": args.provider,
            "head": args.head, "verdict": verdict, "failed": failed, "checks": checks}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--provider", choices=sorted(PAID_KEYS), required=True)
    parser.add_argument("--project", type=Path, required=True)
    parser.add_argument("--tool", action="append", default=[], help="tool the method must be able to call")
    parser.add_argument("--mcp", action="append", default=[], help="MCP server that must be connected")
    parser.add_argument("--logs-root", type=Path, required=True)
    parser.add_argument("--mirror", type=Path, required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--runner", type=Path, required=True)
    parser.add_argument("--runner-sha256", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = preflight(args)
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"verdict": result["verdict"], "failed": result["failed"]}))
    return 0 if result["verdict"] == "ready" else 2


if __name__ == "__main__":
    raise SystemExit(main())
