"""Verification handoff gate: the agent proves the journey, it never asks the user to test.

Runs on the host's Stop event. When the turn's last assistant message hands verification to the
user ("try it in the app", "please test", "vai lá e testa", "me avisa se funcionou"), the hook
blocks once and tells the agent to run the journey observer itself (Playwright by default; after
a deploy, against the deployed site) and record the proof, or to say `OBSERVER_MISSING` with what
is missing. A product decision (approve a design, pick an option) is not a handoff and passes.

The hook blocks once; when the host reports `stop_hook_active` it lets the turn end, so an agent
can never loop on it. Any failure of the hook itself lets the turn end.
"""

from __future__ import annotations

import json
from pathlib import Path
import re
import sys

# Imperatives aimed at the user that hand over a check the agent can run. English and Portuguese.
# A bare "vai testar" (the agent or the observer will test) and a request to choose are not handoffs.
HANDOFF = re.compile(
    r"\b(?:"
    r"try it (?:in|on|from) the (?:app|site|browser|page|ui)"
    r"|(?:please|can you|could you|go(?: and)?|now) (?:test|try|verify) (?:it|this|the (?:app|site|fix|page))\b"
    r"|test it (?:yourself|again|out)"
    r"|let me know (?:if|whether) it works"
    r"|(?:vai|v[áa]|pode ir) l[áa] e (?:test|tent|verific|confer)\w*"
    r"|(?:testa|tenta|confere|verifica) (?:a[íi]|l[áa]|no app|no site|voc[êe]|vc)\b"
    r"|test[ae] (?:voc[êe]|vc|no app|no site|l[áa]|a[íi])"
    r"|me (?:diz|diga|avisa|avise|fala|fale) se (?:funcion|deu certo)\w*"
    r")",
    re.IGNORECASE,
)

REASON = (
    "Keel (JPD): your reply hands verification to the user ({phrase!r}). Do not ask the user to "
    "test. Run the journey yourself with the observer (Playwright by default: "
    "tools/playwright-observer/playwright_observe.py --journey <contractId>; after a deploy, point "
    "it at the deployed site), record the proof in GraphHelm, and report what the observer saw. "
    "If no observer can reach it, say OBSERVER_MISSING and what is missing. Only a product "
    "decision (approve a design, choose an option) goes to the user."
)


def _text(content) -> str:
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(part.get("text", "") for part in content
                         if isinstance(part, dict) and part.get("type") in ("text", "output_text"))
    return ""


def last_message(payload: dict) -> str:
    """The turn's last assistant text: from the payload, else from the transcript's tail."""
    direct = payload.get("last_assistant_message")
    if isinstance(direct, str) and direct:
        return direct
    path = payload.get("transcript_path")
    if not path:
        return ""
    try:
        lines = Path(path).read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return ""
    for line in reversed(lines[-200:]):
        try:
            entry = json.loads(line)
        except ValueError:
            continue
        message = entry.get("message") if isinstance(entry, dict) else None
        if isinstance(message, dict) and message.get("role") == "assistant":
            text = _text(message.get("content"))
            if text.strip():
                return text
    return ""


def decide(message: str) -> str | None:
    """The reason to block, or None to let the turn end."""
    if "OBSERVER_MISSING" in message:
        return None
    match = HANDOFF.search(message)
    return REASON.format(phrase=match.group(0)) if match else None


def main() -> int:
    try:
        payload = json.load(sys.stdin)
    except ValueError:
        return 0
    if not isinstance(payload, dict) or payload.get("stop_hook_active"):
        return 0
    try:
        reason = decide(last_message(payload))
    except Exception:  # noqa: BLE001 - a broken gate never traps the turn
        return 0
    if reason:
        print(json.dumps({"decision": "block", "reason": reason}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
