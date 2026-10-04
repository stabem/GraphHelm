"""Record a Keel lock's refusal on the graph, so the Studio shows a skipped step was stopped.

Best effort and silent: inside a GraphHelm execution (`GRAPHHELM_EXECUTION_ID` bound, the same
binding the session hook uses) a refusal is posted as a `keel.blocked` signal; outside one, or on
any failure, nothing happens and the lock's own decision stands unchanged.
"""

from __future__ import annotations

from datetime import datetime, timezone
import hashlib
import urllib.parse

import session_hook


def record_block(gate: str, reason: str, payload: dict) -> bool:
    try:
        bound = session_hook.binding()
        if bound is None:
            return False
        execution, token_file, url, _origin, node_id = bound
        session = str(payload.get("session_id") or "unknown")[:128]
        emitted_at = datetime.now(timezone.utc).isoformat(timespec="seconds")
        digest = hashlib.sha256(f"{execution}\0{session}\0{gate}\0{reason}\0{emitted_at}".encode()).hexdigest()[:24]
        signal_id = f"keel-blocked-{digest}"
        source = {"type": "node", "id": node_id} if node_id else {"type": "tool", "id": f"keel-{gate}"}
        signal = {"id": signal_id, "source": source, "type": "keel.blocked", "severity": "medium",
                  "description": f"Keel {gate} lock refused: {reason}"[:2000],
                  "evidence": [f"gate: {gate}", f"session: {session}"], "emittedAt": emitted_at}
        session_hook.request(f"{url}/v1/executions/{urllib.parse.quote(execution, safe='')}/signal",
                             session_hook.token_from_file(token_file), "POST", {"signal": signal},
                             {"X-GraphHelm-Actor": source["id"], "X-GraphHelm-Actor-Type": "agent",
                              "X-GraphHelm-Actor-Session": session, "Idempotency-Key": signal_id},
                             timeout=2.0)
        return True
    except Exception:  # noqa: BLE001 - recording must never change the lock's decision
        return False
