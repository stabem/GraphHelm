"""Exercise the Claude-to-Codex handoff adapter against an isolated Runtime."""

from __future__ import annotations

import argparse
import hashlib
import json
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
ADAPTERS = {
    "claude": ROOT / "plugins" / "graphhelm" / "hooks" / "task_handoff.py",
    "codex": ROOT / "plugins" / "graphhelm-codex-hooks" / "hooks" / "task_handoff.py",
}
EXECUTION = "keel-handoff-fixture"
HANDOFF_PROTOCOL = "graphhelm-task-handoff-v1"
CHILD_TIMEOUT = 10
RUNTIME_TIMEOUT = 30
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0


def _handoff_actor_identity(host: str, session: str) -> str:
    identity = f"{HANDOFF_PROTOCOL}\0actor\0{len(host)}\0{host}\0{len(session)}\0{session}"
    return "handoff-agent-" + hashlib.sha256(identity.encode("utf-8")).hexdigest()[:48]


class ScenarioFailure(RuntimeError):
    pass


def _runtime_binary() -> Path:
    raw = os.environ.get("GRAPHHELM_CLI")
    if not raw:
        raise ScenarioFailure("OBSERVER_MISSING: set GRAPHHELM_CLI to a pinned prebuilt GraphHelm executable")
    binary = Path(raw).expanduser().resolve()
    if not binary.is_file():
        raise ScenarioFailure(f"OBSERVER_MISSING: Runtime executable does not exist: {binary}")
    return binary


def _runtime_identity(binary: Path) -> dict:
    identity_env = {key: value for key, value in os.environ.items()
                    if not key.startswith("GRAPHHELM_")}
    identity_env["GRAPHHELM_CLI"] = str(binary)
    try:
        version = subprocess.run([str(binary), "--version"], cwd=ROOT, capture_output=True,
                                 text=True, timeout=CHILD_TIMEOUT, creationflags=NO_WINDOW,
                                 stdin=subprocess.DEVNULL, env=identity_env)
    except subprocess.TimeoutExpired as error:
        raise ScenarioFailure("pinned GraphHelm version query timed out") from error
    if version.returncode != 0 or not version.stdout.strip():
        raise ScenarioFailure("pinned GraphHelm executable did not report its version")
    digest = hashlib.sha256()
    with binary.open("rb") as executable:
        for block in iter(lambda: executable.read(1024 * 1024), b""):
            digest.update(block)
    return {"version": version.stdout.strip(), "sha256": digest.hexdigest()}


def _http(url: str, token: str, method: str = "GET", body: dict | None = None,
          extra_headers: dict | None = None) -> dict:
    data = json.dumps(body).encode() if body is not None else None
    headers = {
        "Authorization": f"Bearer {token}",
        "Content-Type": "application/json",
    }
    headers.update(extra_headers or {})
    request = urllib.request.Request(url, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=5) as response:
            raw = response.read(256 * 1024 + 1)
    except urllib.error.HTTPError as error:
        detail = error.read(2048).decode("utf-8", errors="replace")
        raise ScenarioFailure(f"Runtime HTTP {error.code}: {detail[:1000]}") from error
    if len(raw) > 256 * 1024:
        raise ScenarioFailure("Runtime response exceeded fixture bound")
    try:
        value = json.loads(raw)
    except ValueError as error:
        raise ScenarioFailure("Runtime returned invalid JSON") from error
    if not isinstance(value, dict):
        raise ScenarioFailure("Runtime response was not an object")
    return value


def _record_synthetic_task_evidence(base_url: str, token: str, actor: str, session: str,
                                    signal_id: str, objective: str, keel_card: str,
                                    prior_proof: str) -> dict:
    task_document = {
        "schema": "graphhelm-task-event-v1",
        "taskId": signal_id,
        "revision": 1,
        "at": "2026-10-01T00:00:00Z",
        "issue": 999999,
        "lane": actor,
        "branch": f"fixture-{actor}-synthetic-prior-work",
        "plan": {"keelCard": keel_card, "priorProof": prior_proof},
        "summary": objective,
    }
    signal = {
        "id": signal_id,
        "source": {"type": "user", "id": actor},
        "type": "task.claimed",
        "severity": "low",
        "description": json.dumps(task_document, separators=(",", ":")),
        "evidence": [prior_proof],
        "emittedAt": "2026-10-01T00:00:00Z",
    }
    return _http(
        f"{base_url}/v1/executions/{EXECUTION}/signal", token, "POST", {"signal": signal},
        {"X-GraphHelm-Actor": actor, "X-GraphHelm-Actor-Type": "agent",
         "X-GraphHelm-Actor-Session": session, "Idempotency-Key": signal_id},
    )


def _signal_event(base_url: str, token: str, signal_id: str) -> dict:
    history = _http(f"{base_url}/v1/executions/{EXECUTION}/events?after=0&limit=256", token)
    data = history.get("data", {})
    events = data.get("events", [])
    matches = []
    for event in events:
        kind = event.get("kind", {})
        record = kind.get("data", {}) if kind.get("type") == "signal_recorded" else {}
        if record.get("signalId") == signal_id:
            matches.append(event)
    if len(matches) != 1:
        raise ScenarioFailure(f"Runtime history did not identify exactly one signal {signal_id}")
    return matches[0]


def _run_prior_task_evidence_reader(base_url: str, token_file: Path, host: str,
                                    session: str, owned: Path) -> dict:
    """Start a fresh Codex-bound reader with no sender IDs or cached evidence."""
    launcher = owned / f"prior-task-evidence-reader-{session}.py"
    launcher.write_text(
        "import hashlib, json, os, sys, urllib.parse, urllib.request\n"
        "import task_handoff\n"
        "execution = os.environ.get('GRAPHHELM_EXECUTION_ID')\n"
        "runtime = os.environ.get('GRAPHHELM_RUNTIME_URL')\n"
        "token_path = os.environ.get('GRAPHHELM_TOKEN_FILE')\n"
        "host = os.environ.get('GRAPHHELM_MCP_HOST')\n"
        "session = os.environ.get('GRAPHHELM_SESSION_ID')\n"
        "if (not execution or not runtime or not token_path or host != 'codex' or not session):\n"
        " raise SystemExit('fixture receiver binding is incomplete')\n"
        "token = open(token_path, encoding='utf-8').read().strip()\n"
        "def get_json(url):\n"
        " request = urllib.request.Request(url, headers={'Authorization': 'Bearer ' + token})\n"
        " with urllib.request.urlopen(request, timeout=2) as response:\n"
        "  raw = response.read(256 * 1024 + 1)\n"
        " if len(raw) > 256 * 1024:\n"
        "  raise ValueError('Runtime response exceeded fixture bound')\n"
        " value = json.loads(raw)\n"
        " if not isinstance(value, dict):\n"
        "  raise ValueError('Runtime response was not an object')\n"
        " return value\n"
        "encoded = urllib.parse.quote(execution, safe='')\n"
        "history = get_json(runtime + '/v1/executions/' + encoded + '/events?after=0&limit=256').get('data', {})\n"
        "events = history.get('events')\n"
        "if not isinstance(events, list) or (history.get('head', 0) and (not events or events[-1].get('sequence', 0) < history['head'])):\n"
        " raise ValueError('Runtime event history was incomplete')\n"
        "matches = []\n"
        "for event in events:\n"
        " kind = event.get('kind', {})\n"
        " record = kind.get('data', {}) if kind.get('type') == 'signal_recorded' else {}\n"
        " if record.get('kind') == 'task.claimed' and record.get('sourceKind') == 'user':\n"
        "  matches.append((event, record))\n"
        "if len(matches) > 1:\n"
        " raise ValueError('Runtime contains ambiguous synthetic task evidence')\n"
        "if not matches:\n"
        " print(json.dumps({'priorTaskEvidence': None, 'executionId': execution, 'receiver': host}))\n"
        " sys.exit(0)\n"
        "event, record = matches[0]\n"
        "scope = event.get('scope', {})\n"
        "actor = event.get('actor', {})\n"
        "if (record.get('executionId') != execution or scope.get('executionId') != execution\n"
        "    or actor.get('type') != 'agent' or actor.get('id') != record.get('sourceId')):\n"
        " raise ValueError('task evidence actor or execution does not match its Runtime event')\n"
        "refs = event.get('evidenceRefs')\n"
        "if not isinstance(refs, list) or len(refs) != 1 or not isinstance(refs[0].get('evidenceId'), str):\n"
        " raise ValueError('task evidence event lacks one sealed reference')\n"
        "reference = refs[0]\n"
        "evidence_id = urllib.parse.quote(reference['evidenceId'], safe='')\n"
        "evidence = get_json(runtime + '/v1/executions/' + encoded + '/evidence/' + evidence_id).get('data', {})\n"
        "content = evidence.get('content')\n"
        "digest = hashlib.sha256(content.encode('utf-8')).hexdigest() if isinstance(content, str) else None\n"
        "if (evidence.get('evidenceId') != reference['evidenceId']\n"
        "    or reference.get('contentSha256') != record.get('envelopeSha256')\n"
        "    or evidence.get('contentSha256') != record.get('envelopeSha256')\n"
        "    or digest != record.get('envelopeSha256')):\n"
        " raise ValueError('sealed task evidence identity or digest did not match its Runtime event')\n"
        "envelope = json.loads(content)\n"
        "source = envelope.get('source', {})\n"
        "if (envelope.get('id') != record.get('signalId') or envelope.get('type') != record.get('kind')\n"
        "    or source.get('type') != 'user' or source.get('id') != record.get('sourceId')):\n"
        " raise ValueError('sealed task evidence envelope did not match its Runtime event')\n"
        "task = json.loads(envelope.get('description', '{}'))\n"
        "if (task.get('schema') != 'graphhelm-task-event-v1' or task.get('taskId') != record.get('signalId')\n"
        "    or task.get('lane') != actor.get('id') or not isinstance(task.get('plan'), dict)):\n"
        " raise ValueError('sealed task record did not match its Runtime actor')\n"
        "print(json.dumps({'priorTaskEvidence': {'recordType': event.get('kind', {}).get('type'), 'signalType': record['kind'],\n"
        " 'executionId': record['executionId'], 'signalId': record['signalId'], 'sourceKind': record['sourceKind'],\n"
        " 'sourceId': record['sourceId'], 'actor': actor, 'sequence': event['sequence'],\n"
        " 'eventEvidenceId': reference['evidenceId'], 'evidenceId': evidence['evidenceId'],\n"
        " 'envelopeSha256': record['envelopeSha256'], 'contentSha256': evidence['contentSha256'],\n"
        " 'sealedContent': content, 'objective': task['summary'],\n"
        " 'keelCard': task['plan']['keelCard'], 'priorProof': task['plan']['priorProof']},\n"
        " 'executionId': execution, 'receiver': host}))\n",
        encoding="utf-8")
    env = {key: value for key, value in os.environ.items()
           if not key.startswith("GRAPHHELM_")}
    env.update({
        "GRAPHHELM_EXECUTION_ID": EXECUTION,
        "GRAPHHELM_RUNTIME_URL": base_url,
        "GRAPHHELM_TOKEN_FILE": str(token_file),
        "GRAPHHELM_MCP_HOST": host,
        "GRAPHHELM_SESSION_ID": session,
        "GRAPHHELM_MCP_SESSION_SOURCE": "environment",
        "PYTHONPATH": str(ADAPTERS[host].parent),
    })
    try:
        result = subprocess.run([sys.executable, str(launcher)], cwd=ROOT, env=env,
                                capture_output=True, text=True, timeout=CHILD_TIMEOUT,
                                creationflags=NO_WINDOW, stdin=subprocess.DEVNULL)
    except subprocess.TimeoutExpired as error:
        raise ScenarioFailure("fresh task evidence receiver exceeded its child budget") from error
    finally:
        launcher.unlink(missing_ok=True)
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip().splitlines()[-1:] or ["no output"]
        raise ScenarioFailure(f"fresh task evidence receiver failed: {detail[0][:300]}")
    try:
        value = json.loads(result.stdout)
    except ValueError as error:
        raise ScenarioFailure("fresh task evidence receiver returned invalid JSON") from error
    if not isinstance(value, dict) or value.get("receiver") != host:
        raise ScenarioFailure("fresh task evidence receiver returned an invalid result")
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


def _stop_owned_runtime(child: subprocess.Popen, log) -> None:
    if child.poll() is None:
        child.terminate()
    try:
        child.wait(timeout=3)
    except subprocess.TimeoutExpired:
        child.kill()
        child.wait(timeout=2)
    log.close()


def _event_snapshot(base_url: str, token: str) -> dict:
    data = _http(f"{base_url}/v1/executions/{EXECUTION}/events?after=0&limit=256", token).get("data", {})
    events = data.get("events")
    head = data.get("head")
    if not isinstance(events, list) or not isinstance(head, int):
        raise ScenarioFailure("Runtime returned an incomplete event snapshot")
    return {"head": head, "events": events}


def _run_adapter(phase: str, host: str, session: str, token_file: Path, base_url: str,
                 recipient_host: str | None = None, recipient_session: str | None = None,
                 offer_id: str | None = None, owned: Path | None = None) -> dict:
    if owned is None:
        raise ScenarioFailure("fixture-owned adapter directory was not supplied")
    adapter = ADAPTERS.get(host)
    if adapter is None:
        raise ScenarioFailure(f"unsupported adapter host: {host}")
    session_hooks = adapter.parent
    env = {key: value for key, value in os.environ.items()
           if not key.startswith("GRAPHHELM_")}
    env.update({
        "GRAPHHELM_EXECUTION_ID": EXECUTION,
        "GRAPHHELM_RUNTIME_URL": base_url,
        "GRAPHHELM_TOKEN_FILE": str(token_file),
        "GRAPHHELM_MCP_HOST": host,
        "GRAPHHELM_SESSION_ID": session,
        "GRAPHHELM_MCP_SESSION_SOURCE": "environment",
        "GRAPHHELM_HOOK_STATE_DIR": str(owned / "hook-state"),
        "PYTHONPATH": str(session_hooks),
    })
    command = [sys.executable, str(adapter), phase, "--host", host,
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


def _discard_fixture_hook_cache(owned: Path) -> None:
    cache = owned / "hook-state"
    if cache.is_symlink() or not cache.resolve().is_relative_to(owned.resolve()):
        raise ScenarioFailure("refusing to remove hook state outside the owned fixture directory")
    if cache.exists():
        shutil.rmtree(cache)


def run(wrong_store_only: bool = False) -> dict:
    binary = _runtime_binary()
    runtime_identity = _runtime_identity(binary)
    fixture_dir = Path(__file__).resolve().parent
    with tempfile.TemporaryDirectory(prefix="graphhelm-keel-handoff-", dir=fixture_dir) as temp:
        owned = Path(temp)
        store = owned / "store"
        keyring = owned / "keyring"
        store.mkdir()
        keyring.mkdir()
        runtime_env = {key: value for key, value in os.environ.items()
                       if not key.startswith("GRAPHHELM_")}
        runtime_env["GRAPHHELM_EVENTS_KEY"] = "42" * 32
        runtime_env["GRAPHHELM_CLI"] = str(binary)
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
        source_processes = [(child, store.resolve(), keyring.resolve())]
        source_logs = [log]
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

            objective = "Synthetic objective: restore the fixture search path without changing production state."
            keel_card = ("Scope: fixture-only. Promise: prior evidence remains readable after handoff. "
                         "Proof: recover this record from Runtime history.")
            prior_proof = "Synthetic prior proof: the isolated dry-run observed the fixture search path."
            prior_signal_id = "handoff-prior-evidence-1"
            prior_actor = _handoff_actor_identity("claude", "claude-fixture")
            _record_synthetic_task_evidence(base_url, token, prior_actor, "claude-fixture",
                                            prior_signal_id, objective, keel_card, prior_proof)
            prior_event = _signal_event(base_url, token, prior_signal_id)

            offered = _run_adapter("offer", "claude", "claude-fixture", token_file, base_url,
                                   owned=owned)
            offer_id = offered.get("offerId")
            if not isinstance(offer_id, str) or not offer_id.startswith("handoff-offer-"):
                raise ScenarioFailure(f"Claude adapter did not expose a durable offer id: {offered}")
            offer_event = _signal_event(base_url, token, offer_id)
            if prior_event.get("sequence", 0) >= offer_event.get("sequence", 0):
                raise ScenarioFailure("synthetic task evidence was not recorded before the handoff offer")
            _discard_fixture_hook_cache(owned)

            # Negative control: the actual source offer is absent from this healthy empty store.
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
                empty_prior = _run_prior_task_evidence_reader(
                    empty_url, isolated_empty_token, "codex", "codex-empty", owned)
                if empty_prior.get("priorTaskEvidence") is not None:
                    raise ScenarioFailure("healthy empty Runtime satisfied the prior task evidence read")
                negative = _run_adapter("receive", "codex", "codex-empty", isolated_empty_token, empty_url,
                                        offer_id=offer_id, owned=owned)
                if negative.get("phase") != "receive" or negative.get("activation") != "unobserved":
                    raise ScenarioFailure("empty-store negative control unexpectedly received a handoff")
                if negative.get("missingOffer") is not True:
                    raise ScenarioFailure("empty-store rejection did not identify the original offer as missing")
                if wrong_store_only:
                    raise ScenarioFailure(
                        f"continuity assertion failed: original offer {offer_id} and its sealed evidence "
                        "are absent from the healthy empty fixture Runtime"
                    )
                empty_setup = subprocess.run(
                    [str(binary), "execution", "start", "--file",
                     str(ROOT / "examples/graphs/software-feature.yaml"), "--events", str(empty),
                     "--execution", EXECUTION, "--mode", "manual", "--held"],
                    cwd=ROOT, env=runtime_env, capture_output=True, text=True,
                    timeout=CHILD_TIMEOUT, creationflags=NO_WINDOW)
                if empty_setup.returncode != 0:
                    raise ScenarioFailure("could not initialize the different-record fixture execution")
                different_signal_id = "handoff-other-prior-evidence-1"
                _record_synthetic_task_evidence(
                    empty_url, empty_token,
                    _handoff_actor_identity("claude", "different-fixture"), "different-fixture",
                    different_signal_id,
                    "Synthetic objective: update a different fixture document.",
                    "Scope: fixture-only. Promise: a different record cannot impersonate prior work. "
                    "Proof: its literal contents remain distinct.",
                    "Synthetic prior proof: a separate dry-run observed a different document.")
                different = _run_prior_task_evidence_reader(
                    empty_url, isolated_empty_token, "codex", "codex-empty", owned)
                different_record = different.get("priorTaskEvidence")
                if (not isinstance(different_record, dict)
                        or different_record.get("signalId") == prior_signal_id
                        or different_record.get("objective") == objective
                        or different_record.get("keelCard") == keel_card
                        or different_record.get("priorProof") == prior_proof):
                    raise ScenarioFailure("a different valid synthetic record satisfied the original content obligation")
            finally:
                empty_child.terminate()
                try:
                    empty_child.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    empty_child.kill()
                    empty_child.wait(timeout=2)
                empty_log.close()

            _discard_fixture_hook_cache(owned)
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
            status = _run_adapter("status", "codex", "codex-fixture", token_file, base_url,
                                  offer_id=offer_id, owned=owned)
            matching = [item for item in status.get("offers", []) if item.get("offerId") == offer_id]
            receipts = matching[0].get("receipts", []) if len(matching) == 1 else []
            if len(matching) != 1 or len(receipts) != 1 or receipts[0] != first.get("receiptId"):
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
            expected_duty = {
                "kind": "resume_held",
                "command": pending_duty.get("command"),
                "nodes": ["docs", "implement", "map_repository", "plan", "review", "tests"],
            }
            if (pending_duty.get("kind") != expected_duty["kind"]
                    or not isinstance(expected_duty["command"], str)
                    or "execution resume" not in expected_duty["command"]
                    or EXECUTION not in expected_duty["command"]
                    or pending_duty.get("nodes") != expected_duty["nodes"]):
                raise ScenarioFailure(f"the fixture briefing did not identify the expected held task duty: {pending_duty}")
            recovered_briefing = second.get("briefing", {})
            recovered_duty = recovered_briefing.get("nextStep", {})
            if (second.get("executionId") != EXECUTION
                    or recovered_duty != {"kind": "resume_held", "nodeCount": len(expected_duty["nodes"])}):
                raise ScenarioFailure(
                    f"the receiver briefing did not recover the same execution and open duty: "
                    f"execution={second.get('executionId')!r}, briefing={recovered_briefing!r}"
                )

            # Preserve the exact journal before replacing the source Runtime process.
            owner_token = owner_token_file.read_text(encoding="utf-8").strip()
            before_restart = _event_snapshot(base_url, owner_token)
            _stop_owned_runtime(child, log)
            if child.poll() is None:
                raise ScenarioFailure("owned source Runtime did not exit before restart")
            restart_log_path = owned / "runtime-restarted.log"
            restart_log = restart_log_path.open("w+", encoding="utf-8")
            source_logs.append(restart_log)
            restart_command = command.copy()
            restart_command[restart_command.index("--bind") + 1] = base_url.removeprefix("http://")
            restarted_child = subprocess.Popen(restart_command, cwd=owned, env=runtime_env,
                                               stdin=subprocess.DEVNULL, stdout=restart_log,
                                               stderr=subprocess.STDOUT, creationflags=NO_WINDOW)
            source_processes.append((restarted_child, store.resolve(), keyring.resolve()))
            restarted_url = _wait_started(restart_log_path, restarted_child)
            _discard_fixture_hook_cache(owned)
            restart_receive = _run_adapter("receive", "codex", "codex-fixture", token_file,
                                           restarted_url, offer_id=offer_id, owned=owned)
            if (restart_receive.get("receiptId") != first.get("receiptId")
                    or restart_receive.get("state") != "already_recorded"):
                raise ScenarioFailure(
                    "replacement Runtime did not recover the same recorded receipt: "
                    f"expected id/state={first.get('receiptId')!r}/'already_recorded', "
                    f"got id/state={restart_receive.get('receiptId')!r}/{restart_receive.get('state')!r}"
                )
            restart_prior = _run_prior_task_evidence_reader(
                restarted_url, token_file, "codex", "codex-fixture", owned)
            restart_evidence = restart_prior.get("priorTaskEvidence")
            if (not isinstance(restart_evidence, dict)
                    or restart_evidence.get("signalId") != prior_signal_id
                    or restart_evidence.get("objective") != objective
                    or restart_evidence.get("keelCard") != keel_card
                    or restart_evidence.get("priorProof") != prior_proof
                    or restart_evidence.get("actor") != {"type": "agent", "id": prior_actor}
                    or restart_evidence.get("executionId") != EXECUTION
                    or restart_evidence.get("evidenceId") != restart_evidence.get("eventEvidenceId")
                    or restart_evidence.get("contentSha256") != restart_evidence.get("envelopeSha256")
                    or hashlib.sha256(restart_evidence.get("sealedContent", "").encode("utf-8")).hexdigest()
                    != restart_evidence.get("envelopeSha256")):
                raise ScenarioFailure("replacement Runtime did not recover the original prior task evidence")
            if (restart_evidence.get("sequence", 0)
                    >= _signal_event(restarted_url, token, offer_id).get("sequence", 0)):
                raise ScenarioFailure("replacement Runtime recovered task evidence without its pre-offer ordering")
            prior_task_evidence = restart_evidence
            after_restart = _event_snapshot(restarted_url, owner_token)
            if before_restart != after_restart:
                raise ScenarioFailure("Runtime journal changed during restart recovery reads")
            base_url = restarted_url

            wrong = _run_adapter("receive", "codex", "wrong-codex", token_file, base_url,
                                 offer_id=offer_id, owned=owned)
            if (wrong.get("phase") != "receive" or wrong.get("activation") != "unobserved"
                    or wrong.get("reason") != "handoff offer recipient does not match this adapter"):
                raise ScenarioFailure("wrong recipient unexpectedly received the offer")
            after_wrong = _run_adapter("status", "codex", "codex-fixture", token_file, base_url,
                                       offer_id=offer_id, owned=owned)
            after_wrong_match = [item for item in after_wrong.get("offers", [])
                                 if item.get("offerId") == offer_id]
            if (len(after_wrong_match) != 1
                    or after_wrong_match[0].get("receipts") != [first.get("receiptId")]):
                raise ScenarioFailure("wrong recipient changed the original Runtime receipt history")
            return {"verdict": "passed", "offerId": offer_id,
                    "runtimeCli": runtime_identity,
                    "receiptId": first["receiptId"], "negativeControl": "missing offer rejected",
                    "repeatReceive": second["state"], "wrongRecipient": "rejected",
                    "sourceBriefingSequence": first["offeredBriefing"]["asOfSequence"],
                    "offerSequence": offer_event.get("sequence"),
                    "pendingDuty": pending_duty,
                    "recoveredExecutionId": second["executionId"],
                    "recoveredDuty": recovered_duty,
                    "pendingEvidenceReferences": _briefing_references(offered_briefing),
                    "sealedEvidenceReferences": evidence_refs,
                    "priorTaskEvidence": prior_task_evidence,
                    "journalStableAcrossRestart": before_restart == after_restart,
                    "restartReceiptId": restart_receive.get("receiptId"),
                    "restartReceiptState": restart_receive.get("state"),
                    "priorTaskActor": prior_actor,
                    "priorTaskEvidenceNegativeControls": {
                        "healthyEmpty": "no matching record discovered",
                        "differentSynthetic": "different record and literal content rejected as the original",
                    }}
        finally:
            for owned_child, _, _ in reversed(source_processes):
                if owned_child.poll() is None:
                    owned_child.terminate()
                try:
                    owned_child.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    owned_child.kill()
                    owned_child.wait(timeout=2)
            for source_log in source_logs:
                if not source_log.closed:
                    source_log.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wrong-store", action="store_true",
                        help="RED-only control: require continuity to fail for an empty Runtime")
    args = parser.parse_args()
    try:
        print(json.dumps(run(wrong_store_only=args.wrong_store), indent=2))
    except (OSError, ScenarioFailure, urllib.error.URLError) as error:
        verdict = "failed" if args.wrong_store and "continuity assertion failed" in str(error) else "observer_missing"
        print(json.dumps({"verdict": verdict, "evidence": [str(error)]}, indent=2))
        sys.exit(1 if verdict == "failed" else 2)
