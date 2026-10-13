"""Record Keel lock refusals and check recommendations on the bound execution.

Best effort and silent: inside a GraphHelm execution (`GRAPHHELM_EXECUTION_ID` bound, the same
binding the session hook uses) a refusal is posted as a `keel.blocked` signal; outside one, or on
any failure, nothing happens and the lock's own decision stands unchanged.
"""

from __future__ import annotations

from datetime import datetime, timezone
import hashlib
import urllib.parse

import session_hook


def record_check(envelope: dict, payload: dict) -> bool:
    """Best-effort signal for a valid keel check; never apply a task transition."""
    try:
        recommendation = envelope["data"]["recommendation"]
        step = recommendation["step"]
        head = recommendation["head"]
        if step not in ("implement", "review") or not isinstance(head, str) or not head:
            return False
        if recommendation.get("applied") is not False:
            return False
        findings = recommendation["findingIds"]
        paths = recommendation["paths"]
        if not isinstance(findings, list) or not isinstance(paths, list):
            return False
        if any(not isinstance(item, str) for item in findings + paths):
            return False
        bound = session_hook.binding()
        if bound is None:
            return False
        execution, token_file, url, _origin, node_id = bound
        session = str(payload.get("session_id") or "unknown")[:128]
        emitted_at = datetime.now(timezone.utc).isoformat(timespec="seconds")
        evidence = [f"head: {head}", *(f"finding: {item}" for item in sorted(set(findings))),
                    *(f"path: {item}" for item in sorted(set(paths)))]
        kind = "keel.blocked" if step == "implement" else "keel.passed"
        digest = hashlib.sha256(f"{execution}\0{session}\0{kind}\0{evidence}\0{emitted_at}".encode()).hexdigest()[:24]
        signal_id = f"keel-check-{digest}"
        source = {"type": "node", "id": node_id} if node_id else {"type": "tool", "id": "keel-check"}
        signal = {"id": signal_id, "source": source, "type": kind,
                  "severity": "medium" if step == "implement" else "low",
                  "description": f"Keel check recommends {step}; no task transition applied.",
                  "evidence": evidence, "emittedAt": emitted_at}
        session_hook.request(f"{url}/v1/executions/{urllib.parse.quote(execution, safe='')}/signal",
                             session_hook.token_from_file(token_file), "POST", {"signal": signal},
                             {"X-GraphHelm-Actor": source["id"], "X-GraphHelm-Actor-Type": "agent",
                              "X-GraphHelm-Actor-Session": session, "Idempotency-Key": signal_id},
                             timeout=2.0)
        return True
    except Exception:  # noqa: BLE001 - recording must never change the check's decision
        return False


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
