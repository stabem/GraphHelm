"""Explicit, provider-neutral task handoff over the existing Runtime contracts.

This adapter records two typed Graph Signals.  It never assigns a node and never says that a
recipient understood or accepted the work.  The Runtime's sealed evidence is the source of truth;
the local hook cache is deliberately not used.
"""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
import sys
import urllib.parse

from session_hook import ID, PORTABLE_HOST, _briefing_digest, _validate_digest, binding, request, token_from_file

MAX_PAGES = 32
MAX_PAGE_SIZE = 256
MAX_OFFERS = 128
MAX_BRIEFING = 32 * 1024
MAX_DESCRIPTION = 8 * 1024
MAX_MCP_MESSAGE = 32 * 1024
HANDOFF_TIMEOUT = 5.0
PROTOCOL = "graphhelm-task-handoff-v1"
OFFER_KIND = "task_handoff_offer"
RECEIPT_KIND = "task_handoff_received"
MCP_PROTOCOL_VERSION = "2025-06-18"


def _host(value: str) -> str:
    if not isinstance(value, str) or not PORTABLE_HOST.fullmatch(value):
        raise ValueError("invalid host identity")
    return value


def _id(value: str, label: str) -> str:
    if not isinstance(value, str) or not ID.fullmatch(value):
        raise ValueError(f"invalid {label}")
    return value


def _session(value: str) -> str:
    configured = os.environ.get("GRAPHHELM_SESSION_ID")
    if configured and configured != value:
        raise ValueError("session identity mismatch")
    return _id(value, "session identity")


def _mcp_session_source() -> str:
    source = os.environ.get("GRAPHHELM_MCP_SESSION_SOURCE", "environment")
    if source not in {"environment", "codex_metadata"}:
        raise ValueError("unsupported MCP session source")
    return source


def _codex_metadata_session(metadata: object) -> str:
    if not isinstance(metadata, dict):
        raise ValueError("Codex session metadata is required")
    thread_id = metadata.get("threadId")
    session_id = metadata.get("sessionId")
    thread_id = _id(thread_id, "Codex thread identity")
    session_id = _id(session_id, "Codex session identity")
    if thread_id != session_id:
        raise ValueError("Codex thread and session identities conflict")
    configured = os.environ.get("GRAPHHELM_SESSION_ID")
    if configured and configured != session_id:
        raise ValueError("session identity mismatch")
    return session_id


def _binding() -> tuple[str, str, str, str, str | None]:
    bound = binding()
    if bound is None:
        raise ValueError("GRAPHHELM_EXECUTION_ID, GRAPHHELM_TOKEN_FILE and GRAPHHELM_RUNTIME_URL are required")
    return bound


def _actor(host: str, session: str) -> str:
    # Do not concatenate the two caller-controlled strings: ``a-session-b`` + ``c`` and
    # ``a`` + ``b-session-c`` would otherwise name the same recipient. This namespace is only
    # for the explicit handoff protocol; the established SessionEnd actor labels are untouched.
    identity = f"{PROTOCOL}\0actor\0{len(host)}\0{host}\0{len(session)}\0{session}"
    return f"handoff-agent-{hashlib.sha256(identity.encode()).hexdigest()[:48]}"


def _offer_id(execution: str, session: str, host: str, recipient_host: str,
              recipient_session: str, origin: str, handoff_id: str = "default") -> str:
    material = "\0".join((PROTOCOL, "offer", execution, session, host, recipient_host,
                           recipient_session, origin, handoff_id))
    return "handoff-offer-" + hashlib.sha256(material.encode()).hexdigest()[:48]


def _receipt_id(offer_id: str, recipient_host: str, recipient_session: str, origin: str) -> str:
    material = "\0".join((PROTOCOL, "receipt", offer_id, recipient_host, recipient_session, origin))
    return "handoff-receipt-" + hashlib.sha256(material.encode()).hexdigest()[:48]


def _briefing(url: str, token: str, execution: str) -> dict:
    reply = request(f"{url}/v1/executions/{urllib.parse.quote(execution, safe='')}/briefing", token, "GET", timeout=HANDOFF_TIMEOUT)
    data = reply.get("data")
    if not isinstance(data, dict):
        raise ValueError("invalid Runtime briefing")
    raw = json.dumps(data, separators=(",", ":"), sort_keys=True)
    if len(raw.encode()) > MAX_BRIEFING:
        raise ValueError("briefing too large")
    # Keep only the same typed resume digest as SessionStart. Objective text and transcript are
    # deliberately not transferred.
    try:
        digest = _briefing_digest(data)
    except (TypeError, ValueError, KeyError) as error:
        raise ValueError("invalid Runtime briefing") from error
    for field in ("pending", "unevaluated", "decisions", "workDone"):
        if not isinstance(data.get(field), list):
            raise ValueError("invalid Runtime briefing")
    return digest


def _events(url: str, token: str, execution: str) -> list[dict]:
    after = 0
    found: list[dict] = []
    for _ in range(MAX_PAGES):
        reply = request(
            f"{url}/v1/executions/{urllib.parse.quote(execution, safe='')}/events?after={after}&limit={MAX_PAGE_SIZE}",
            token, "GET", timeout=HANDOFF_TIMEOUT)
        data = reply.get("data")
        if not isinstance(data, dict) or not isinstance(data.get("events"), list) or type(data.get("head")) is not int:
            raise ValueError("invalid Runtime event page")
        page = data["events"]
        previous = after
        for event in page:
            if not isinstance(event, dict) or type(event.get("sequence")) is not int or event["sequence"] <= after:
                raise ValueError("event page did not advance")
            found.append(event)
            after = event["sequence"]
        head = data["head"]
        if not page:
            if head > after:
                raise ValueError("Runtime returned an incomplete event page")
            break
        if after == previous:
            raise ValueError("event cursor did not advance")
        if after >= head:
            break
    else:
        raise ValueError("event pagination exceeded bound")
    return found


def _signal_record(event: dict) -> dict:
    kind = event.get("kind")
    if not isinstance(kind, dict) or kind.get("type") != "signal_recorded" or not isinstance(kind.get("data"), dict):
        return {}
    return kind["data"]


def _signal_event(events: list[dict], signal_id: str) -> dict | None:
    for event in events:
        record = _signal_record(event)
        if record.get("signalId") == signal_id:
            return event
    return None


def _sealed_signal(url: str, token: str, execution: str, event: dict) -> dict:
    record = _signal_record(event)
    if not record:
        raise ValueError("handoff journal event is not a signal record")
    actor = event.get("actor")
    scope = event.get("scope")
    if (not isinstance(actor, dict) or actor.get("type") != "agent"
            or actor.get("id") != record.get("sourceId")
            or not isinstance(scope, dict) or scope.get("executionId") != execution
            or record.get("executionId") != execution):
        raise ValueError("handoff journal actor or execution does not match")
    refs = event.get("evidenceRefs")
    if not isinstance(refs, list) or len(refs) != 1 or not isinstance(refs[0], dict):
        raise ValueError("handoff signal has no single sealed evidence reference")
    evidence_id = refs[0].get("evidenceId")
    if not isinstance(evidence_id, str) or not ID.fullmatch(evidence_id):
        raise ValueError("invalid sealed evidence reference")
    reply = request(
        f"{url}/v1/executions/{urllib.parse.quote(execution, safe='')}/evidence/{urllib.parse.quote(evidence_id, safe='')}",
        token, "GET", timeout=HANDOFF_TIMEOUT)
    data = reply.get("data")
    if not isinstance(data, dict) or data.get("mediaType") != "application/json" or not isinstance(data.get("content"), str):
        raise ValueError("sealed handoff evidence is unavailable")
    if (data.get("evidenceId") != evidence_id
            or refs[0].get("contentSha256") != record.get("envelopeSha256")
            or record.get("sourceKind") != "tool"):
        raise ValueError("sealed evidence reference does not match journal")
    if data.get("contentSha256") != record.get("envelopeSha256"):
        raise ValueError("sealed evidence hash does not match journal envelope")
    if data.get("contentSha256") != hashlib.sha256(data["content"].encode("utf-8")).hexdigest():
        raise ValueError("sealed evidence content hash is invalid")
    try:
        signal = json.loads(data["content"])
    except json.JSONDecodeError as error:
        raise ValueError("sealed handoff evidence is not JSON") from error
    if not isinstance(signal, dict) or signal.get("id") != record.get("signalId"):
        raise ValueError("sealed evidence does not match journal envelope")
    if signal.get("source", {}).get("id") != record.get("sourceId"):
        raise ValueError("sealed evidence actor does not match journal envelope")
    if signal.get("source", {}).get("type") != "tool" or signal.get("type") not in {OFFER_KIND, RECEIPT_KIND}:
        raise ValueError("sealed evidence is not a handoff signal")
    if signal.get("type") != record.get("kind") or signal.get("severity") != record.get("severity"):
        raise ValueError("sealed evidence metadata does not match journal record")
    return signal


def _post_signal(url: str, token: str, execution: str, signal: dict, signal_id: str,
                 session: str) -> dict:
    actor = signal.get("source", {}).get("id")
    return request(
        f"{url}/v1/executions/{urllib.parse.quote(execution, safe='')}/signal", token, "POST",
        {"signal": signal},
        {"X-GraphHelm-Actor": actor, "X-GraphHelm-Actor-Type": "agent",
         "X-GraphHelm-Actor-Session": session, "Idempotency-Key": signal_id},
        timeout=HANDOFF_TIMEOUT)


def _make_offer(execution: str, host: str, session: str, recipient_host: str,
                recipient_session: str, origin: str, briefing: dict, emitted_at: str,
                handoff_id: str = "default") -> dict:
    signal_id = _offer_id(execution, session, host, recipient_host, recipient_session, origin, handoff_id)
    source = _actor(host, session)
    target = _actor(recipient_host, recipient_session)
    details = {"protocol": PROTOCOL, "executionId": execution, "handoffId": handoff_id, "offerId": signal_id,
               "sender": source, "recipient": target, "briefing": briefing}
    description = json.dumps(details, separators=(",", ":"), sort_keys=True)
    if len(description.encode()) > MAX_DESCRIPTION:
        raise ValueError("handoff offer too large")
    return {"id": signal_id, "source": {"type": "tool", "id": source}, "type": OFFER_KIND,
            "severity": "medium", "description": description, "evidence": [execution],
            "emittedAt": emitted_at, "to": target}


def _validate_receipt(receipt: dict, execution: str, offer_id: str,
                      recipient: str, sender: str) -> None:
    if (receipt.get("type") != RECEIPT_KIND
            or receipt.get("source", {}).get("type") != "tool"
            or receipt.get("source", {}).get("id") != recipient
            or receipt.get("to") != sender
            or receipt.get("replyTo") != offer_id):
        raise ValueError("handoff receipt identity does not match offer")
    try:
        details = json.loads(receipt.get("description", "{}"))
    except (TypeError, json.JSONDecodeError) as error:
        raise ValueError("handoff receipt metadata is invalid") from error
    if (not isinstance(details, dict) or details.get("protocol") != PROTOCOL
            or details.get("executionId") != execution or details.get("offerId") != offer_id
            or details.get("recipient") != recipient or details.get("sender") != sender):
        raise ValueError("handoff receipt metadata is invalid")


def offer(host: str, session: str, recipient_host: str, recipient_session: str,
          handoff_id: str = "default") -> dict:
    host = _host(host)
    session = _session(session)
    recipient_host = _host(recipient_host)
    recipient_session = _id(recipient_session, "recipient session identity")
    handoff_id = _id(handoff_id, "handoff id")
    execution, token_file, url, origin, _ = _binding()
    token = token_from_file(token_file)
    offer_id = _offer_id(execution, session, host, recipient_host, recipient_session, origin, handoff_id)
    events = _events(url, token, execution)
    existing = _signal_event(events, offer_id)
    if existing:
        signal = _sealed_signal(url, token, execution, existing)
        if signal.get("source", {}).get("id") != _actor(host, session) or signal.get("to") != _actor(recipient_host, recipient_session):
            raise ValueError("recorded handoff offer identity does not match this request")
        state = "already_recorded"
    else:
        briefing = _briefing(url, token, execution)
        emitted_at = datetime.now(timezone.utc).isoformat(timespec="seconds")
        signal = _make_offer(execution, host, session, recipient_host, recipient_session, origin, briefing, emitted_at, handoff_id)
        _post_signal(url, token, execution, signal, offer_id, session)
        recorded = _signal_event(_events(url, token, execution), offer_id)
        if not recorded:
            raise ValueError("Runtime did not expose the recorded handoff offer")
        _sealed_signal(url, token, execution, recorded)
        state = "recorded"
    return {"format": "graphhelm-portable-v1", "phase": "offer", "state": state,
            "executionId": execution, "offerId": offer_id, "recipient": signal.get("to"),
            "sealed": True, "activation": "unobserved"}


def receive(host: str, session: str, offer_id: str) -> dict:
    host = _host(host); session = _session(session); offer_id = _id(offer_id, "offer id")
    execution, token_file, url, origin, _ = _binding()
    token = token_from_file(token_file)
    events = _events(url, token, execution)
    event = _signal_event(events, offer_id)
    if not event:
        raise ValueError("original handoff offer is not recorded")
    offer_signal = _sealed_signal(url, token, execution, event)
    record = _signal_record(event)
    if record.get("executionId") != execution or record.get("sourceId") != offer_signal.get("source", {}).get("id"):
        raise ValueError("handoff journal actor or execution does not match sealed offer")
    target = offer_signal.get("to")
    actor = _actor(host, session)
    if target != actor or offer_signal.get("type") != OFFER_KIND:
        raise ValueError("handoff offer recipient does not match this adapter")
    details = json.loads(offer_signal.get("description", "{}"))
    if (not isinstance(details, dict) or details.get("protocol") != PROTOCOL
            or details.get("offerId") != offer_id
            or details.get("executionId") != execution
            or details.get("sender") != offer_signal.get("source", {}).get("id")
            or details.get("recipient") != actor):
        raise ValueError("handoff offer metadata is invalid")
    briefing = _briefing(url, token, execution)
    receipt_id = _receipt_id(offer_id, host, session, origin)
    existing = _signal_event(events, receipt_id)
    if existing:
        receipt = _sealed_signal(url, token, execution, existing)
        _validate_receipt(receipt, execution, offer_id, actor, offer_signal["source"]["id"])
        state = "already_recorded"
    else:
        receipt = {"id": receipt_id, "source": {"type": "tool", "id": actor}, "type": RECEIPT_KIND,
                   "severity": "low", "description": json.dumps({"protocol": PROTOCOL,
                   "executionId": execution, "offerId": offer_id, "recipient": actor,
                   "sender": offer_signal["source"]["id"], "briefing": briefing},
                   separators=(",", ":"), sort_keys=True),
                   "evidence": [execution], "emittedAt": datetime.now(timezone.utc).isoformat(timespec="seconds"),
                   "to": offer_signal["source"]["id"], "replyTo": offer_id}
        _post_signal(url, token, execution, receipt, receipt_id, session)
        recorded = _signal_event(_events(url, token, execution), receipt_id)
        if not recorded:
            raise ValueError("Runtime did not expose the recorded handoff receipt")
        _validate_receipt(_sealed_signal(url, token, execution, recorded), execution, offer_id,
                          actor, offer_signal["source"]["id"])
        state = "recorded"
    return {"format": "graphhelm-portable-v1", "phase": "receive", "state": state,
            "executionId": execution, "offerId": offer_id, "receiptId": receipt_id,
            "recipient": actor, "briefing": briefing, "accepted": False, "activation": "unobserved"}


def status(host: str, session: str, offer_id: str | None) -> dict:
    host = _host(host); session = _session(session)
    if offer_id is not None:
        offer_id = _id(offer_id, "offer id")
    execution, token_file, url, origin, _ = _binding()
    token = token_from_file(token_file)
    events = _events(url, token, execution)
    sealed_by_id = {}
    current_actor = _actor(host, session)
    for event in events:
        record = _signal_record(event)
        signal_id = record.get("signalId")
        if record.get("kind") in {OFFER_KIND, RECEIPT_KIND} and isinstance(signal_id, str):
            if record.get("kind") == RECEIPT_KIND and record.get("sourceId") != current_actor:
                continue
            sealed_by_id[signal_id] = _sealed_signal(url, token, execution, event)
    offers = []
    truncated = False
    for event in events:
        record = _signal_record(event)
        if record.get("kind") != OFFER_KIND:
            continue
        sid = record.get("signalId")
        if not isinstance(sid, str) or (offer_id and sid != offer_id):
            continue
        signal = sealed_by_id.get(sid)
        if signal is None:
            raise ValueError("handoff offer evidence is unavailable")
        if signal.get("to") != current_actor:
            continue
        if len(offers) >= MAX_OFFERS:
            truncated = True
            break
        details = json.loads(signal.get("description", "{}"))
        if (not isinstance(details, dict) or details.get("protocol") != PROTOCOL
                or details.get("executionId") != execution or details.get("offerId") != sid
                or details.get("sender") != signal.get("source", {}).get("id")
                or details.get("recipient") != signal.get("to")):
            raise ValueError("handoff offer metadata is invalid")
        try:
            briefing_reference = _validate_digest(details.get("briefing"))
        except (TypeError, ValueError, KeyError) as error:
            raise ValueError("handoff offer briefing reference is invalid") from error
        receipt_ids = []
        for candidate in events:
            candidate_record = _signal_record(candidate)
            if candidate_record.get("kind") != RECEIPT_KIND or candidate_record.get("sourceId") != current_actor:
                continue
            candidate_signal = sealed_by_id.get(candidate_record.get("signalId"))
            if candidate_signal is None:
                raise ValueError("handoff receipt evidence is unavailable")
            try:
                candidate_details = json.loads(candidate_signal.get("description", "{}"))
            except (TypeError, json.JSONDecodeError) as error:
                raise ValueError("handoff receipt metadata is invalid") from error
            if not isinstance(candidate_details, dict) or candidate_details.get("offerId") != sid:
                continue
            _validate_receipt(candidate_signal, execution, sid, current_actor, signal["source"]["id"])
            if candidate_signal.get("replyTo") == sid:
                receipt_ids.append(candidate_record.get("signalId"))
        offers.append({"offerId": sid, "sender": signal.get("source", {}).get("id"),
                       "recipient": signal.get("to"), "offeredAt": signal.get("emittedAt"),
                       "sealed": True, "receipts": receipt_ids,
                       "briefingReference": briefing_reference})
    return {"format": "graphhelm-portable-v1", "phase": "status", "executionId": execution,
            "offers": offers, "truncated": truncated, "activation": "unobserved"}


def _mcp_tools() -> list[dict]:
    """Return the deliberately small, closed MCP surface for this adapter."""
    identity = {"type": "object", "additionalProperties": False}
    return [
        {"name": "offer", "description": "Record an explicit sealed handoff offer.",
         "inputSchema": {**identity, "properties": {
             "recipientHost": {"type": "string", "maxLength": 64},
             "recipientSessionId": {"type": "string", "maxLength": 128},
             "handoffId": {"type": "string", "maxLength": 128},
         }, "required": ["recipientHost", "recipientSessionId"]}},
        {"name": "receive", "description": "Read and record one sealed handoff offer.",
         "inputSchema": {**identity, "properties": {
             "offerId": {"type": "string", "maxLength": 128},
         }, "required": ["offerId"]}},
        {"name": "status", "description": "List sealed handoffs addressed to this adapter.",
         "inputSchema": {**identity, "properties": {
             "offerId": {"type": "string", "maxLength": 128},
         }}},
    ]


def _mcp_error(request_id, code: int, message: str) -> dict:
    return {"jsonrpc": "2.0", "id": request_id, "error": {"code": code, "message": message}}


def _mcp_result(request_id, result: dict) -> dict:
    return {"jsonrpc": "2.0", "id": request_id, "result": result}


def _mcp_tool_call(name: object, arguments: object, host: str, session: str) -> dict:
    try:
        if not isinstance(name, str) or name not in {"offer", "receive", "status"}:
            raise ValueError("unknown handoff tool")
        if arguments is None:
            arguments = {}
        if not isinstance(arguments, dict):
            raise ValueError("tool arguments must be an object")
        allowed = {
            "offer": {"recipientHost", "recipientSessionId", "handoffId"},
            "receive": {"offerId"}, "status": {"offerId"},
        }[name]
        if set(arguments) - allowed:
            raise ValueError("tool arguments contain an unsupported field")
        if name == "offer":
            if "recipientHost" not in arguments or "recipientSessionId" not in arguments:
                raise ValueError("offer requires recipient host and session")
            value = offer(host, session, arguments["recipientHost"], arguments["recipientSessionId"],
                          arguments.get("handoffId", "default"))
        elif name == "receive":
            if "offerId" not in arguments:
                raise ValueError("receive requires offer id")
            value = receive(host, session, arguments["offerId"])
        else:
            value = status(host, session, arguments.get("offerId"))
        return {"content": [{"type": "text", "text": json.dumps(value, separators=(",", ":"))}],
                "isError": False}
    except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError, RecursionError):
        return {"content": [{"type": "text", "text": "handoff tool failed; result is unobserved"}],
                "isError": True}
    except Exception:
        return {"content": [{"type": "text", "text": "handoff tool failed; result is unobserved"}],
                "isError": True}


def _mcp_stdio() -> int:
    """Serve newline-delimited MCP JSON-RPC with credentials held by this process."""
    host = os.environ.get("GRAPHHELM_MCP_HOST")
    try:
        host = _host(host)
        source = _mcp_session_source()
        if source == "environment":
            session = _session(os.environ.get("GRAPHHELM_SESSION_ID"))
            _binding()  # Legacy mode keeps its startup validation contract.
        else:
            if host != "codex":
                raise ValueError("Codex metadata source requires the codex host")
            session = None
    except (TypeError, ValueError, OSError):
        print("graphhelm-task-handoff: invalid MCP server configuration", file=sys.stderr)
        return 1
    while True:
        raw = sys.stdin.buffer.readline(MAX_MCP_MESSAGE + 1)
        if not raw:
            break
        request_id = None
        if len(raw) > MAX_MCP_MESSAGE or not raw.endswith(b"\n"):
            print(json.dumps(_mcp_error(None, -32600, "invalid request"), separators=(",", ":")), flush=True)
            # This read may be only a prefix of one physical frame. Its remaining bytes
            # must never be interpreted as a second request with Runtime side effects.
            return 1
        try:
            request_value = json.loads(raw)
            if not isinstance(request_value, dict) or request_value.get("jsonrpc") != "2.0":
                raise ValueError
            request_id = request_value.get("id")
            method = request_value.get("method")
            if not isinstance(method, str):
                raise ValueError
            if "id" not in request_value:
                if method == "notifications/initialized":
                    continue
                continue
            if method == "initialize":
                result = {"protocolVersion": MCP_PROTOCOL_VERSION, "capabilities": {"tools": {}},
                          "serverInfo": {"name": "graphhelm-task-handoff", "version": "0.1.15"}}
                response = _mcp_result(request_id, result)
            elif method == "ping":
                response = _mcp_result(request_id, {})
            elif method == "tools/list":
                response = _mcp_result(request_id, {"tools": _mcp_tools()})
            elif method == "tools/call":
                params = request_value.get("params")
                if not isinstance(params, dict):
                    raise ValueError("tools/call params must be an object")
                if set(params) - {"name", "arguments", "_meta"}:
                    raise ValueError("tools/call params contain an unsupported field")
                if "_meta" in params and not isinstance(params["_meta"], dict):
                    raise ValueError("tools/call metadata must be an object")
                try:
                    call_session = (session if source == "environment"
                                    else _codex_metadata_session(params.get("_meta")))
                except (TypeError, ValueError):
                    call_session = None
                    response = _mcp_result(request_id, {
                        "content": [{"type": "text", "text": "handoff tool failed; result is unobserved"}],
                        "isError": True,
                    })
                    print(json.dumps(response, separators=(",", ":")), flush=True)
                    continue
                response = _mcp_result(request_id, _mcp_tool_call(params.get("name"), params.get("arguments"), host, call_session))
            else:
                response = _mcp_error(request_id, -32601, "method not found")
        except (ValueError, TypeError, KeyError, json.JSONDecodeError, RecursionError):
            response = _mcp_error(request_id, -32602, "invalid request")
        except Exception:
            response = _mcp_error(request_id, -32603, "internal error")
        print(json.dumps(response, separators=(",", ":")), flush=True)
    return 0


def main() -> int:
    if len(sys.argv) == 2 and sys.argv[1] == "--mcp-stdio":
        return _mcp_stdio()
    parser = argparse.ArgumentParser()
    parser.add_argument("phase", choices=("offer", "receive", "status"))
    parser.add_argument("--host", required=True, type=_host)
    parser.add_argument("--session-id")
    parser.add_argument("--recipient-host")
    parser.add_argument("--recipient-session-id")
    parser.add_argument("--offer-id")
    parser.add_argument("--handoff-id", default="default")
    args = parser.parse_args()
    try:
        args.session_id = args.session_id or os.environ.get("GRAPHHELM_SESSION_ID")
        if not args.session_id:
            raise ValueError("session identity is required")
        if args.phase == "offer":
            if not args.recipient_host or not args.recipient_session_id:
                raise ValueError("offer requires recipient host and session")
            result = offer(args.host, args.session_id, args.recipient_host, args.recipient_session_id, args.handoff_id)
        elif args.phase == "receive":
            if not args.offer_id:
                raise ValueError("receive requires offer id")
            result = receive(args.host, args.session_id, args.offer_id)
        else:
            result = status(args.host, args.session_id, args.offer_id)
        print(json.dumps(result, separators=(",", ":")))
        return 0
    except (OSError, ValueError, KeyError, RecursionError, json.JSONDecodeError) as error:
        print(f"graphhelm-task-handoff: {args.phase} unobserved ({type(error).__name__})", file=sys.stderr)
        print(json.dumps({"format": "graphhelm-portable-v1", "phase": args.phase,
                          "activation": "unobserved"}, separators=(",", ":")))
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
