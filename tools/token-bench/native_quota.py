"""Read included Codex allowance without inference, reset consumption or account edits."""
from __future__ import annotations

import json
import queue
import subprocess
import threading
import time
from pathlib import Path

import methodology as base

NO_WINDOW = base.NO_WINDOW


def claude_subscription_observation(stdout: str) -> dict:
    """Require native first-party authentication and explicit rejection of overage."""
    try:
        rows = [json.loads(line) for line in stdout.splitlines() if line.strip()]
        initialized = [r for r in rows if r.get("type") == "system" and r.get("subtype") == "init"]
        rates = [r.get("rate_limit_info") for r in rows if r.get("type") == "rate_limit_event"]
        finals = [r for r in rows if r.get("type") == "result"]
        if len(initialized) != 1 or initialized[0].get("apiKeySource") != "none":
            raise ValueError("native API-key-free Claude initialization missing")
        if not rates or any(not isinstance(rate, dict) or rate.get("isUsingOverage") is not False
                            or rate.get("overageStatus") != "rejected" or rate.get("status") != "allowed"
                            for rate in rates):
            raise ValueError("native subscription overage rejection missing or allowance refused")
        if len(finals) != 1 or finals[0].get("usage", {}).get("fallback_credit") is not None:
            raise ValueError("native subscription final usage missing or fallback credit used")
        models = finals[0].get("modelUsage")
        if not isinstance(models, dict) or not models or any(m.get("provider") != "firstParty" for m in models.values()):
            raise ValueError("native first-party accounting provider missing")
        return {"allowed": True, "reason": "native first-party subscription with overage rejected",
                "paidFallbackAuthorized": False, "isUsingOverage": False}
    except (ValueError, TypeError, AttributeError) as error:
        return {"allowed": False, "reason": str(error), "paidFallbackAuthorized": False}


def check_snapshot(account: dict, limits: dict, *, cutoff: int = 80) -> dict:
    """Unknown and exhausted included allowance refuse a new native phase."""
    if type(cutoff) is not int or not 0 < cutoff <= 80:
        return {"allowed": False, "reason": "unsafe included allowance cutoff"}
    user = account.get("account")
    if not isinstance(user, dict) or user.get("type") != "chatgpt" or not user.get("planType"):
        return {"allowed": False, "reason": "ChatGPT subscription identity unavailable"}
    if limits.get("ordinaryUsageAllowed") is not True:
        return {"allowed": False, "reason": "included allowance permission unavailable or refused"}
    snapshots = limits.get("rateLimitsByLimitId")
    snapshot = snapshots.get("codex") if isinstance(snapshots, dict) else limits.get("rateLimits")
    if not isinstance(snapshot, dict) or snapshot.get("rateLimitReachedType") or snapshot.get("spendControlReached") is True:
        return {"allowed": False, "reason": "native Codex quota unavailable or reached"}
    windows = []
    for name in ("primary", "secondary"):
        window = snapshot.get(name)
        if window is None and name == "secondary":
            continue
        if not isinstance(window, dict):
            return {"allowed": False, "reason": "native quota window missing"}
        percent = window.get("usedPercent")
        if type(percent) not in (int, float) or not 0 <= percent <= 100:
            return {"allowed": False, "reason": "native quota percentage invalid"}
        reset = window.get("resetsAt")
        if type(reset) not in (int, float) or reset <= time.time():
            return {"allowed": False, "reason": "native quota window expired or reset missing"}
        windows.append({"window": name, "usedPercent": percent,
                        "resetsAt": window.get("resetsAt")})
    return {"allowed": all(w["usedPercent"] < cutoff for w in windows),
            "reason": "included allowance below conservative cutoff" if all(w["usedPercent"] < cutoff for w in windows)
            else "conservative included allowance cutoff reached",
            "cutoffPercent": cutoff, "windows": windows,
            "paidFallbackAuthorized": False}


def codex_allowance(executable: Path, cwd: Path, *, timeout: float = 20, cutoff: int = 80) -> dict:
    """Start one owned stdio server and close it after a fresh read-only snapshot.

    Only sanitized quota/plan information is returned. Authentication, balances,
    account identifiers and raw server events never enter benchmark artifacts.
    """
    process = None
    messages: queue.Queue = queue.Queue()
    deadline = time.monotonic() + timeout
    try:
        process = subprocess.Popen([str(executable), "app-server", "--stdio"], cwd=cwd,
                                   env=base.child_environment(), stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                   text=True, encoding="utf-8", creationflags=NO_WINDOW)

        def read() -> None:
            try:
                for line in process.stdout:
                    try:
                        messages.put(json.loads(line))
                    except ValueError:
                        messages.put({"protocolError": True})
            finally:
                messages.put({"eof": True})

        reader = threading.Thread(target=read, daemon=True)
        reader.start()

        def send(value: dict) -> None:
            process.stdin.write(json.dumps(value) + "\n")
            process.stdin.flush()

        def request(identifier: int, method: str, params: dict) -> dict:
            send({"id": identifier, "method": method, "params": params})
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError("native quota snapshot timed out")
                response = messages.get(timeout=remaining)
                if response.get("eof") or response.get("protocolError"):
                    raise ValueError("native quota server ended or returned malformed data")
                if response.get("id") == identifier:
                    if "error" in response or not isinstance(response.get("result"), dict):
                        raise ValueError("native quota request refused")
                    return response["result"]

        request(1, "initialize", {"clientInfo": {"name": "graphhelm-quota-guard", "version": "1.0"},
                                  "capabilities": {"experimentalApi": True}})
        send({"method": "initialized", "params": {}})
        account = request(2, "account/read", {"refreshToken": False})
        limits = request(3, "account/rateLimits/read", {"supportsLunaReserve": False,
                                                         "excludeResetCreditDetails": False})
        result = check_snapshot(account, limits, cutoff=cutoff)
        result["observedAtUnix"] = time.time()
        return result
    except (OSError, ValueError, TimeoutError, queue.Empty) as error:
        return {"allowed": False, "reason": str(error), "paidFallbackAuthorized": False}
    finally:
        if process is not None:
            if process.stdin:
                try:
                    process.stdin.close()
                except OSError:
                    pass
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                process.terminate()
                try:
                    process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=3)
