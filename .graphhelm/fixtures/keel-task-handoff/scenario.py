"""Exercise the Claude-to-Codex handoff adapter against an isolated Runtime."""

from __future__ import annotations

import json
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[3]
ADAPTER = ROOT / "plugins" / "graphhelm" / "hooks" / "task_handoff.py"
SESSION_HOOKS = ROOT / "plugins" / "graphhelm" / "hooks"
EXECUTION = "keel-handoff-fixture"
CHILD_TIMEOUT = 10
RUNTIME_TIMEOUT = 30
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0


class ScenarioFailure(RuntimeError):
    pass


def _runtime_binary() -> Path:
    raw = os.environ.get("GRAPHHELM_CLI") or shutil.which("graphhelm")
    if not raw:
        raise ScenarioFailure("OBSERVER_MISSING: set GRAPHHELM_RUNTIME_BINARY to a local prebuilt Runtime executable")
    binary = Path(raw).expanduser().resolve()
    if not binary.is_file():
        raise ScenarioFailure(f"OBSERVER_MISSING: Runtime executable does not exist: {binary}")
    return binary


def _http(url: str, token: str, method: str = "GET", body: dict | None = None) -> dict:
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(url, data=data, method=method, headers={
        "Authorization": f"Bearer {token}",
        "Content-Type": "application/json",
    })
    with urllib.request.urlopen(request, timeout=2) as response:
        raw = response.read(256 * 1024 + 1)
    if len(raw) > 256 * 1024:
        raise ScenarioFailure("Runtime response exceeded fixture bound")
    try:
        value = json.loads(raw)
    except ValueError as error:
        raise ScenarioFailure("Runtime returned invalid JSON") from error
    if not isinstance(value, dict):
        raise ScenarioFailure("Runtime response was not an object")
    return value


def _wait_started(log_path: Path, child: subprocess.Popen) -> str:
    deadline = time.monotonic() + RUNTIME_TIMEOUT
    while time.monotonic() < deadline:
        if child.poll() is not None:
            raise ScenarioFailure(f"isolated Runtime exited early ({child.returncode})")
        try:
            line = log_path.read_text(encoding="utf-8").splitlines()[0]
            started = json.loads(line)
            address = started["data"]["address"]
            if started.get("command") != "serve.started" or not address.startswith("127.0.0.1:"):
                raise ScenarioFailure("Runtime did not bind an isolated loopback address")
            if address.rsplit(":", 1)[-1] == "8793":
                raise ScenarioFailure("refusing reserved live Runtime port 8793")
            return "http://" + address
        except (OSError, IndexError, KeyError, ValueError):
            time.sleep(0.1)
    raise ScenarioFailure("isolated Runtime did not report serve.started")


def _run_adapter(phase: str, host: str, session: str, token_file: Path, base_url: str,
                 recipient_host: str | None = None, recipient_session: str | None = None,
                 offer_id: str | None = None, owned: Path | None = None) -> dict:
    env = os.environ.copy()
    env.update({
        "GRAPHHELM_EXECUTION_ID": EXECUTION,
        "GRAPHHELM_RUNTIME_URL": base_url,
        "GRAPHHELM_TOKEN_FILE": str(token_file),
        "GRAPHHELM_MCP_HOST": host,
        "GRAPHHELM_SESSION_ID": session,
        "GRAPHHELM_MCP_SESSION_SOURCE": "environment",
        "PYTHONPATH": str(SESSION_HOOKS),
    })
    command = [sys.executable, str(ADAPTER), phase, "--host", host,
               "--session-id", session]
    if phase == "offer":
        command.extend(["--recipient-host", recipient_host or "codex",
                        "--recipient-session-id", recipient_session or "codex-fixture"])
    if phase == "receive":
        command.extend(["--offer-id", offer_id or "missing-fixture-offer"])
    launcher = None
    if phase == "receive":
        # The adapter's CLI deliberately redacts operational errors. The child launcher invokes
        # its real public receive() function but exposes only the stable missing-offer reason to
        # this observer; other setup/runtime exceptions keep distinct types.
        if owned is None:
            raise ScenarioFailure("fixture-owned launcher directory was not supplied")
        launcher = owned / f"adapter-receive-{session}.py"
        launcher.write_text(
            "import json, sys\n"
            "from task_handoff import receive\n"
            "try:\n"
            " value = receive(sys.argv[1], sys.argv[2], sys.argv[3])\n"
            " print(json.dumps(value))\n"
            "except ValueError as error:\n"
            " print(json.dumps({'phase':'receive','activation':'unobserved',"
            "'missingOffer':str(error) == 'original handoff offer is not recorded',"
            "'reason':str(error)}))\n",
            encoding="utf-8")
        command = [sys.executable, str(launcher), host, session, offer_id or ""]
    try:
        result = subprocess.run(command, cwd=ROOT, env=env, capture_output=True, text=True,
                                timeout=CHILD_TIMEOUT, creationflags=NO_WINDOW,
                                stdin=subprocess.DEVNULL)
    except subprocess.TimeoutExpired as error:
        raise ScenarioFailure(f"adapter {phase} exceeded {CHILD_TIMEOUT}s") from error
    finally:
        if launcher is not None:
            launcher.unlink(missing_ok=True)
    if result.returncode != 0:
        try:
            value = json.loads(result.stdout)
        except ValueError:
            detail = (result.stderr or result.stdout).strip().splitlines()[-1:] or ["no output"]
            raise ScenarioFailure(f"adapter {phase} failed without structured output: {detail[0][:300]}")
        if isinstance(value, dict) and value.get("phase") in {"offer", "receive", "status"}:
            return value
        raise ScenarioFailure(f"adapter {phase} failed without a phase result")
    try:
        value = json.loads(result.stdout)
    except ValueError as error:
        raise ScenarioFailure(f"adapter {phase} did not return JSON") from error
    if not isinstance(value, dict):
        raise ScenarioFailure(f"adapter {phase} returned a non-object")
    return value


def _briefing_references(briefing: dict) -> list[str]:
    refs: list[str] = []
    for group in ("pending", "unevaluated", "decisions", "workDone"):
        for item in briefing.get(group, []):
            if isinstance(item, dict):
                for key in ("id", "nodeId", "evidenceId", "signalId"):
                    value = item.get(key)
                    if isinstance(value, str) and value:
                        refs.append(value)
    return refs


def run() -> dict:
    binary = _runtime_binary()
    with tempfile.TemporaryDirectory(prefix="graphhelm-keel-handoff-") as temp:
        owned = Path(temp)
        store = owned / "store"
        keyring = owned / "keyring"
        store.mkdir()
        keyring.mkdir()
        runtime_env = os.environ.copy()
        runtime_env["GRAPHHELM_EVENTS_KEY"] = "42" * 32
        key_setup = subprocess.run([str(binary), "gateway", "keyring", "init", "--keyring",
                                    str(keyring), "--key-id", "handoff-fixture"],
                                   cwd=ROOT, env=runtime_env, capture_output=True, text=True,
                                   timeout=CHILD_TIMEOUT, creationflags=NO_WINDOW)
        if key_setup.returncode != 0:
            raise ScenarioFailure("could not initialize the fixture-only evidence keyring")
        log_path = owned / "runtime.log"
        log = log_path.open("w+", encoding="utf-8")
        command = [str(binary), "serve", "--events", str(store), "--bind", "127.0.0.1:0",
                   "--keyring", str(keyring), "--key-id", "handoff-fixture"]
        child = subprocess.Popen(command, cwd=owned, env=runtime_env, stdin=subprocess.DEVNULL,
                                 stdout=log, stderr=subprocess.STDOUT,
                                 creationflags=NO_WINDOW)
        try:
            base_url = _wait_started(log_path, child)
            log.flush()
            env = runtime_env.copy()
            env["GRAPHHELM_CLI"] = str(binary)
            setup = subprocess.run([str(binary), "execution", "start", "--file",
                                    str(ROOT / "examples/graphs/software-feature.yaml"),
                                    "--events", str(store), "--execution", EXECUTION,
                                    "--mode", "manual", "--held"],
                                   cwd=ROOT, env=env, capture_output=True, text=True,
                                   timeout=CHILD_TIMEOUT, creationflags=NO_WINDOW)
            if setup.returncode != 0:
                raise ScenarioFailure("could not initialize the owned Runtime fixture execution")
            owner_token_file = store.with_name(store.name + ".token")
            agent_token_file = store.with_name(store.name + ".agent.token")
            if not owner_token_file.is_file() or not agent_token_file.is_file():
                raise ScenarioFailure("isolated Runtime did not create its owner and agent token files")
            agent_token = agent_token_file.read_text(encoding="utf-8").strip()
            token_file = owned / "claude.agent.token"
            token_file.write_text(agent_token + "\n", encoding="utf-8")
            token_file.chmod(0o600)
            token = agent_token

            # Negative control: the same adapter points at a separate empty event store.
            empty = owned / "empty-store"
            empty.mkdir()
            empty_keyring = owned / "empty-keyring"
            empty_keyring.mkdir()
            empty_key_setup = subprocess.run([str(binary), "gateway", "keyring", "init", "--keyring",
                                              str(empty_keyring), "--key-id", "handoff-fixture"],
                                             cwd=ROOT, env=runtime_env, capture_output=True, text=True,
                                             timeout=CHILD_TIMEOUT, creationflags=NO_WINDOW)
            if empty_key_setup.returncode != 0:
                raise ScenarioFailure("could not initialize the empty-store fixture keyring")
            empty_log = (owned / "empty-runtime.log").open("w+", encoding="utf-8")
            empty_child = subprocess.Popen([str(binary), "serve", "--events", str(empty),
                                            "--bind", "127.0.0.1:0", "--keyring", str(empty_keyring),
                                            "--key-id", "handoff-fixture"], cwd=owned, env=runtime_env,
                                           stdin=subprocess.DEVNULL, stdout=empty_log,
                                           stderr=subprocess.STDOUT, creationflags=NO_WINDOW)
            try:
                empty_url = _wait_started(owned / "empty-runtime.log", empty_child)
                empty_token_file = empty.with_name(empty.name + ".agent.token")
                empty_token = empty_token_file.read_text(encoding="utf-8").strip()
                isolated_empty_token = owned / "codex-empty.agent.token"
                isolated_empty_token.write_text(empty_token + "\n", encoding="utf-8")
                isolated_empty_token.chmod(0o600)
                empty_owner_token = empty.with_name(empty.name + ".token").read_text(encoding="utf-8").strip()
                empty_history = _http(f"{empty_url}/v1/executions/{EXECUTION}/events?after=0&limit=256",
                                      empty_owner_token).get("data", {})
                if empty_history.get("events") != [] or empty_history.get("head") != 0:
                    raise ScenarioFailure("negative-control Runtime store was not empty")
                negative = _run_adapter("receive", "codex", "codex-empty", isolated_empty_token, empty_url,
                                        offer_id="handoff-offer-" + "0" * 48, owned=owned)
                if negative.get("phase") != "receive" or negative.get("activation") != "unobserved":
                    raise ScenarioFailure("empty-store negative control unexpectedly received a handoff")
                if negative.get("missingOffer") is not True:
                    raise ScenarioFailure("empty-store rejection did not identify its missing offer")
            finally:
                empty_child.terminate()
                try:
                    empty_child.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    empty_child.kill()
                    empty_child.wait(timeout=2)
                empty_log.close()

            offered = _run_adapter("offer", "claude", "claude-fixture", token_file, base_url)
            offer_id = offered.get("offerId")
            if not isinstance(offer_id, str) or not offer_id.startswith("handoff-offer-"):
                raise ScenarioFailure(f"Claude adapter did not expose a durable offer id: {offered}")
            first = _run_adapter("receive", "codex", "codex-fixture", token_file, base_url,
                                 offer_id=offer_id, owned=owned)
            second = _run_adapter("receive", "codex", "codex-fixture", token_file, base_url,
                                  offer_id=offer_id, owned=owned)
            if first.get("offerId") != offer_id or second.get("offerId") != offer_id:
                raise ScenarioFailure("offer identity did not survive receiver cache loss")
            if first.get("receiptId") != second.get("receiptId") or second.get("state") != "already_recorded":
                raise ScenarioFailure("repeat receive did not recover the same receipt identity")
            if not first.get("offeredBriefing", {}).get("asOfSequence", 0):
                raise ScenarioFailure("offer has no source Runtime briefing reference")
            status = _run_adapter("status", "codex", "codex-fixture", token_file, base_url, offer_id=offer_id)
            matching = [item for item in status.get("offers", []) if item.get("offerId") == offer_id]
            if len(matching) != 1 or first.get("receiptId") not in matching[0].get("receipts", []):
                raise ScenarioFailure("original Runtime event history did not recover the receipt")

            owner_token = owner_token_file.read_text(encoding="utf-8").strip()
            original_events = _http(f"{base_url}/v1/executions/{EXECUTION}/events?after=0&limit=256",
                                    owner_token).get("data", {}).get("events", [])
            required_ids = {offer_id, first["receiptId"]}
            journal = {}
            for event in original_events:
                data = event.get("kind", {}).get("data", {})
                signal_id = data.get("signalId")
                if signal_id in required_ids:
                    journal[signal_id] = event
            if set(journal) != required_ids:
                raise ScenarioFailure("the original Runtime event history lacks the exact offer and receipt")
            evidence_refs = {}
            for signal_id, event in journal.items():
                refs = event.get("evidenceRefs", [])
                if len(refs) != 1 or not isinstance(refs[0].get("evidenceId"), str):
                    raise ScenarioFailure("a handoff journal event lacks its single sealed evidence reference")
                evidence_id = refs[0]["evidenceId"]
                evidence = _http(f"{base_url}/v1/executions/{EXECUTION}/evidence/{evidence_id}", owner_token).get("data", {})
                content = evidence.get("content")
                if (evidence.get("evidenceId") != evidence_id or not isinstance(content, str)
                        or hashlib.sha256(content.encode("utf-8")).hexdigest() != refs[0].get("contentSha256")):
                    raise ScenarioFailure("Runtime evidence did not match the original journal reference")
                envelope = json.loads(content)
                if envelope.get("id") != signal_id:
                    raise ScenarioFailure("sealed Runtime evidence does not carry the journal signal identity")
                evidence_refs[signal_id] = evidence_id
            # A held execution carries a concrete resume duty, while the offer and receipt
            # themselves remain available through the event and sealed-evidence references.
            original_briefing = _http(f"{base_url}/v1/executions/{EXECUTION}/briefing", owner_token).get("data", {})
            offered_briefing = first.get("briefing", {})
            if (not isinstance(original_briefing.get("nextStep"), dict)
                    or not isinstance(offered_briefing.get("asOfSequence"), int)
                    or offered_briefing.get("asOfSequence", 0) > original_briefing.get("asOfSequence", -1)):
                raise ScenarioFailure("the original Runtime briefing duty did not survive the adapter handoff")
            pending_duty = original_briefing["nextStep"]

            wrong = _run_adapter("receive", "codex", "wrong-codex", token_file, base_url,
                                 offer_id=offer_id, owned=owned)
            if wrong.get("phase") != "receive" or wrong.get("activation") != "unobserved":
                raise ScenarioFailure("wrong recipient unexpectedly received the offer")
            return {"verdict": "passed", "offerId": offer_id,
                    "receiptId": first["receiptId"], "negativeControl": "missing offer rejected",
                    "repeatReceive": second["state"], "wrongRecipient": "rejected",
                    "sourceBriefingSequence": first["offeredBriefing"]["asOfSequence"],
                    "pendingDuty": pending_duty,
                    "pendingEvidenceReferences": _briefing_references(offered_briefing),
                    "sealedEvidenceReferences": evidence_refs}
        finally:
            child.terminate()
            try:
                child.wait(timeout=3)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait(timeout=2)
            log.close()


if __name__ == "__main__":
    try:
        print(json.dumps(run(), indent=2))
    except (OSError, ScenarioFailure, urllib.error.URLError) as error:
        print(json.dumps({"verdict": "observer_missing", "evidence": [str(error)]}, indent=2))
        sys.exit(2)
