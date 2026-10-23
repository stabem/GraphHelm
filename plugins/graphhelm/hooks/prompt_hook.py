"""Opt-in UserPromptSubmit hook: record each owner prompt as a candidate ask (#1921).

Off by default. A project opts in with `"recordOwnerPrompts": true` in `.graphhelm/team.json`.
Each prompt the owner submits in a title-bound lane session is then posted to the team execution
as a `task.request_recorded` record whose requestId starts with `candidate-` and whose title starts
with `candidate:`, so the orchestrator links it to an issue (`request_linked`) or leaves it.

Rules: the hook never blocks the prompt and never prints (exit 0, no output, on every path); the
text is capped; a prompt that looks like a secret is skipped whole; the record is sent only with the
session lane's own credential (`events.agent.<lane>.token` beside the team's token file), never the
shared or the owner token, and only to a loopback Runtime.
"""

from __future__ import annotations

from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import sys
import urllib.parse

sys.path.insert(0, str(Path(__file__).resolve().parent))
import session_hook  # noqa: E402

FLAG = "recordOwnerPrompts"
MAX_ASK = 1000
MAX_TITLE = 80
LOOPBACK = {"127.0.0.1", "localhost", "::1"}
# Shapes that read as credentials: known token prefixes, PEM blocks, key=value secrets, and any
# long unbroken run of base64/hex characters.
SECRET = re.compile(
    r"(gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|sk-[A-Za-z0-9_-]{16,}|xox[abprs]-[A-Za-z0-9-]{10,}"
    r"|AKIA[0-9A-Z]{16}|eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}|-----BEGIN [A-Z ]*PRIVATE KEY-----"
    r"|(?i:password|passwd|secret|api[_-]?key|token|bearer)\s*[:=]\s*\S{6,}"
    r"|[A-Za-z0-9+/_=-]{40,})")


def team_config(cwd: str) -> tuple[Path, dict] | None:
    """The nearest `.graphhelm/team.json` above `cwd` and its object, or None."""
    repo = Path(cwd)
    if not repo.is_absolute():
        return None
    for directory in (repo, *repo.parents):
        path = directory / ".graphhelm" / "team.json"
        if path.is_file():
            raw = path.read_bytes()
            if len(raw) > 4096:
                return None
            value = json.loads(raw)
            return (path, value) if isinstance(value, dict) else None
    return None


def lane_credential(token_file: str, lane: str) -> Path | None:
    """The lane's own credential beside the team's token file; never the shared or owner token."""
    path = Path(token_file)
    stem = path.name
    for suffix in (f".agent.{lane}.token", ".agent.token", ".token"):
        if stem.endswith(suffix):
            stem = stem[: -len(suffix)]
            break
    candidate = path.with_name(f"{stem}.agent.{lane}.token")
    if "owner" in candidate.name.lower() or not candidate.is_file():
        return None
    return candidate


def candidate_ask(prompt: object) -> str | None:
    """The prompt as a candidate ask: collapsed whitespace, capped; None to skip it."""
    if not isinstance(prompt, str):
        return None
    text = " ".join("".join(c if c.isprintable() else " " for c in prompt).split())
    if not text or text.startswith("/") or SECRET.search(text):
        return None
    return text[:MAX_ASK]


def record(payload: dict, now: datetime | None = None) -> dict | None:
    """Build and send the record; returns the sent body, or None when nothing was sent."""
    found = team_config(str(payload.get("cwd") or ""))
    if found is None:
        return None
    _path, team = found
    if team.get(FLAG) is not True:
        return None
    ask = candidate_ask(payload.get("prompt"))
    if ask is None:
        return None
    lane = session_hook._lane_actor(payload)
    execution = team.get("executionId")
    url = str(team.get("runtimeUrl") or "").rstrip("/")
    if not lane or not isinstance(execution, str) or not session_hook.ID.fullmatch(execution):
        return None
    if urllib.parse.urlsplit(url).hostname not in LOOPBACK or urllib.parse.urlsplit(url).scheme != "http":
        return None
    credential = lane_credential(str(team.get("tokenFile") or ""), lane)
    if credential is None:
        return None
    at = (now or datetime.now(timezone.utc)).strftime("%Y-%m-%dT%H:%M:%SZ")
    session = str(payload.get("session_id") or "")
    digest = hashlib.sha256(f"{session}\0{at}\0{ask}".encode()).hexdigest()[:16]
    request_id = f"candidate-{digest}"
    title = f"candidate: {ask}"
    doc = {"schema": "graphhelm-task-event-v1", "taskId": f"request-{request_id}", "revision": 1, "at": at,
           "requestId": request_id, "ask": ask, "title": title[:MAX_TITLE], "recordedBy": lane}
    signal_id = f"{lane}-request-{request_id}"[:64]
    body = {"signal": {"id": signal_id, "type": "task.request_recorded", "source": {"type": "user", "id": lane},
                       "severity": "low", "emittedAt": at, "evidence": [f"owner prompt in session {session[:64]}"],
                       "description": json.dumps(doc, separators=(",", ":"))}}
    session_hook.request(f"{url}/v1/executions/{urllib.parse.quote(execution, safe='')}/signal",
                         session_hook.token_from_file(str(credential)), "POST", body,
                         {"X-GraphHelm-Actor": lane, "X-GraphHelm-Actor-Type": "agent", "Idempotency-Key": signal_id},
                         timeout=2.0)
    return body


def main() -> int:
    try:
        record(session_hook.hook_input())
    except Exception:  # noqa: BLE001 - the prompt is never blocked; recording fails quietly
        pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
