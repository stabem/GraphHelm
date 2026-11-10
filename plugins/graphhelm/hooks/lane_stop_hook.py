"""Remind a bound lane once about confirmed unanswered addressed operator notes.

Read-only: a lane acknowledges handling a note by recording its own operator_note
with replyTo. Re-entry and unreadable state allow Stop without acknowledging notes.
"""
from __future__ import annotations

from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import threading
import time
from urllib.parse import quote

import urllib.error

from session_hook import _lane_actor, binding, hook_input, request, token_from_file, start_lane_waiter, state_path, ID

# Owner rule: children run hidden in the calling terminal, never in a new console window.
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
SCAN_BUDGET_SECONDS = 5.0
SCAN_CHILD_TIMEOUT_SECONDS = 6.0
# #2684: a note this old is no longer pending work for the lane: the hook skips it and names it,
# instead of blocking every Stop on a request whose issue closed long ago. Env override for tests.
NOTE_MAX_AGE_SECONDS = 3 * 24 * 3600
STALE_NOTES: list[str] = []  # ids skipped as too old by the latest scan in this process


def _note_max_age() -> float:
    try:
        return float(os.environ.get("GRAPHHELM_NOTE_MAX_AGE_SECONDS", NOTE_MAX_AGE_SECONDS))
    except ValueError:
        return NOTE_MAX_AGE_SECONDS


def note_is_stale(event: dict, now: datetime | None = None) -> bool:
    """True when the note's journal event is older than the bounded age. An unreadable time is not
    stale: unknown never hides a note."""
    stamp = event.get("occurredAt")
    if not isinstance(stamp, str):
        return False
    try:
        # The Runtime writes nanoseconds; datetime reads microseconds.
        at = datetime.fromisoformat(re.sub(r"(\.\d{6})\d*", r"", stamp).replace("Z", "+00:00"))
    except ValueError:
        return False
    if at.tzinfo is None:
        return False
    return ((now or datetime.now(timezone.utc)) - at).total_seconds() > _note_max_age()


def pending(payload: dict | None = None) -> bool:
    return bool(unanswered(payload))


def unanswered(payload: dict | None = None) -> set[str]:
    """The signal ids of operator notes addressed to this lane that it has not answered."""
    return set(unanswered_events(payload))


# #2697: task records after which a lane may have a ready step (an assigned review, a BLOCK to fix).
READY_HINT_KINDS = {"task.review_assigned", "task.review_verdict", "task.pr_opened"}


def unanswered_events(payload: dict | None = None) -> dict[str, dict]:
    """#2152: the same scan as `unanswered`, keeping each unanswered note's journal event, so a
    rewake can open the note's sealed text."""
    return scan_notes(payload, {})[0]


# #2699: the rewake is a background hook that never holds the host, so it gets a long budget, and its
# first cursor is taken at the head (a short look-back keeps a note that arrived a moment ago).
REWAKE_SCAN_BUDGET_SECONDS = 30.0
REWAKE_LOOKBACK_EVENTS = 64


def scan_notes(payload: dict | None, state: dict, budget: float = SCAN_BUDGET_SECONDS,
               from_head: bool = False) -> tuple[dict[str, dict], bool]:
    """#2697: the note scan, incremental. `state` ({} for a first scan) remembers the last read
    sequence and the unanswered notes, so a later scan reads only what is new: one small page and no
    head call, where the first scan costs the head call plus up to 8 pages. Returns the unanswered
    notes and whether a task record that can make a step ready for this lane arrived since the
    previous scan."""
    bound = binding(payload)
    if bound is None:
        return {}, False
    execution, token_file, url, _origin, _node = bound
    lane = os.environ.get("GRAPHHELM_ACTOR", "")
    if not lane and payload is not None:
        lane = _lane_actor(payload) or ""
    if not ID.fullmatch(lane):
        raise ValueError("lane identity missing")
    token = token_from_file(token_file)
    base = f"{url}/v1/executions/{quote(execution, safe='')}"
    deadline = time.monotonic() + budget
    first = "after" not in state
    if first:
        head = request(f"{base}/events?after=0&limit=1", token, "GET",
                       timeout=max(0.001, deadline - time.monotonic()))["data"]["head"]
        if type(head) is not int or head < 0:
            raise ValueError("invalid event head")
        if from_head:
            # #2699: a rewake reads only what arrives from now on, in one request.
            state["after"], state["notes"] = max(0, head - REWAKE_LOOKBACK_EVENTS), {}
        else:
            # Bound the window, not the execution's lifetime. A reply follows its note,
            # so replies to notes inside this window cannot precede the window.
            state["after"], state["notes"] = max(0, head - 4096), {}
    after, unanswered = state["after"], state["notes"]
    STALE_NOTES.clear()
    # Size bound; the parent process imposes the wall-time bound even if a server
    # trickles bytes. request's timeout alone only bounds socket inactivity.
    for _ in range(8):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("note scan exceeded time budget")
        data = request(f"{base}/events?after={after}&limit=512", token, "GET",
                       timeout=remaining, max_reply=1024 * 1024)["data"]
        page, head = data["events"], data["head"]
        if not isinstance(page, list) or type(head) is not int or head < after or len(page) > 512:
            raise ValueError("invalid event page")
        if not page and head > after:
            raise ValueError("incomplete event page")
        for event in page:
            sequence = event["sequence"]
            if type(sequence) is not int or sequence <= after:
                raise ValueError("event cursor did not advance")
            after = sequence
            kind = event.get("kind", {})
            record = kind.get("data", {})
            if kind.get("type") == "signal_recorded" and record.get("kind") in READY_HINT_KINDS:
                state["hint"] = True  # sticky: a failed scan must not lose it
            if kind.get("type") != "signal_recorded" or record.get("kind") != "operator_note":
                continue
            if record.get("executionId") != execution:
                raise ValueError("wrong execution")
            # The Governor exposes only opaque routing hints. Legacy notes have no
            # hints and remain in the full inbox; never open N sealed envelopes here.
            signal_id = record["signalId"]
            to, reply_to = record.get("to"), record.get("replyTo")
            if not isinstance(signal_id, str) or not ID.fullmatch(signal_id):
                raise ValueError("invalid signal identity")
            if any(value is not None and (not isinstance(value, str) or not ID.fullmatch(value))
                   for value in (to, reply_to)):
                raise ValueError("invalid note routing")
            actor = event.get("actor", {})
            if actor.get("type") == "agent" and actor.get("id") == lane:
                unanswered.pop(reply_to, None)
            if to == lane and reply_to is None:  # #2697: a reply is an answer, not a request to answer
                unanswered[signal_id] = event
        state["after"] = after  # a failure after this keeps what was read; the next scan goes on from here
        if after >= head:
            fresh = {key: event for key, event in unanswered.items() if not note_is_stale(event)}
            for signal_id in unanswered.keys() - fresh.keys():
                STALE_NOTES.append(signal_id)
            return fresh, bool(state.pop("hint", False))
    raise ValueError("note scan exceeded bound")


def record_reconciled(bound: tuple, lane: str, data: dict, memory: Path | None = None) -> None:
    """#1181: the reconciler's reply is a record, not noise. One `tasks.reconciled` signal by the
    lane per distinct reply: the key is the reply's content. #1195: the body carries a fresh
    `emittedAt`, so re-sending an unchanged reply under the same key earns a 409 on every quiet
    Stop; `memory` (a file beside the session's hook state) keeps the digest of the last reply this
    session recorded, and an unchanged reply costs no request. A 409 for a key the Runtime already
    holds is "recorded" and is remembered the same way."""
    execution, token_file, url, _origin, _node = bound
    content = json.dumps(data, sort_keys=True, separators=(",", ":"))
    if len(content) > 4096:
        raise ValueError("reconcile reply too large to record")
    digest = hashlib.sha256(f"{execution}\0{lane}\0{content}".encode()).hexdigest()[:24]
    if memory is not None:
        try:
            if memory.read_text(encoding="ascii").strip() == digest:
                return
        except (OSError, ValueError):
            pass
    signal_id = f"tasks-reconciled-{digest}"
    signal = {"id": signal_id, "source": {"type": "tool", "id": lane}, "type": "tasks.reconciled",
              "severity": "low", "description": content, "evidence": [execution],
              "emittedAt": datetime.now(timezone.utc).isoformat(timespec="seconds")}
    try:
        request(f"{url}/v1/executions/{quote(execution, safe='')}/signal", token_from_file(token_file),
                "POST", {"signal": signal},
                {"X-GraphHelm-Actor": lane, "X-GraphHelm-Actor-Type": "agent", "Idempotency-Key": signal_id},
                timeout=2.0)
    except urllib.error.HTTPError as error:
        if error.code != 409:
            raise
    if memory is not None:
        memory.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        memory.write_text(digest, encoding="ascii")


def scan_status(payload: dict) -> int:
    """#1278: the note scan, in this process. 1 = unanswered notes, 0 = none, 2 = unknown. The
    scan runs on a daemon thread joined for `SCAN_CHILD_TIMEOUT_SECONDS`: the same wall-time bound
    the `--scan` child process gave (a server that trickles bytes cannot hold the Stop), without
    starting a second interpreter."""
    box: list[int] = []

    def run() -> None:
        try:
            box.append(1 if pending(payload) else 0)
        except Exception:  # noqa: BLE001 - unknown state never blocks the turn
            box.append(2)
    thread = threading.Thread(target=run, name="lane-note-scan", daemon=True)
    thread.start()
    thread.join(SCAN_CHILD_TIMEOUT_SECONDS)
    return box[0] if box else 2


def _common_dir(cwd: str) -> Path | None:
    """The repository's common git dir, read from `.git` (and a worktree's `commondir`)."""
    try:
        start = Path(cwd).resolve()
        for top in (start, *start.parents):
            dot = top / ".git"
            if dot.is_dir():
                return dot
            if dot.is_file():
                text = dot.read_text(encoding="utf-8").strip()
                if not text.startswith("gitdir:"):
                    return None
                gitdir = Path(text[len("gitdir:"):].strip())
                gitdir = gitdir if gitdir.is_absolute() else (top / gitdir).resolve()
                pointer = gitdir / "commondir"
                return (gitdir / pointer.read_text(encoding="utf-8").strip()).resolve() if pointer.is_file() else gitdir
    except (OSError, ValueError):
        return None
    return None


def origin_url(cwd: str) -> str:
    """`remote.origin.url`, read from the repository's config file (#1278: no git process); else
    `git remote get-url origin`."""
    common = _common_dir(cwd)
    if common is not None:
        section = None
        try:
            for raw in (common / "config").read_text(encoding="utf-8").splitlines():
                line = raw.strip()
                if line.startswith("["):
                    section = re.sub(r"\s+", " ", line.strip("[]").strip())
                elif section == 'remote "origin"' and "=" in line:
                    key, _, value = line.partition("=")
                    if key.strip().lower() == "url":
                        return value.strip()
        except OSError:
            pass
    return subprocess.run(["git", "-C", cwd, "remote", "get-url", "origin"],
                          capture_output=True, text=True, timeout=1, check=True, creationflags=NO_WINDOW).stdout.strip()


# #2643: a ready step (assigned review, BLOCK to fix, approved merge, next item) blocks the stop
# once per step key; the guard is inline below so both hook copies stay self-contained.
MAX_STEP_BLOCKS_PER_TURN = 3


def new_turn_state() -> dict:
    """The per-turn ready-step state: reset at each new Stop cycle."""
    return _new_step_state()


def _new_step_state() -> dict:
    return {"injected": set(), "blocks": 0}


def _step_key(issue: int, step: str, head: str) -> tuple:
    return (issue, step, head)


def _should_block_step(state: dict, key: tuple) -> bool:
    return key not in state["injected"] and state["blocks"] < MAX_STEP_BLOCKS_PER_TURN


def _record_step_block(state: dict, key: tuple) -> None:
    state["injected"].add(key)
    state["blocks"] += 1


def ready_step_reason(state: dict, ready: list[dict]) -> str | None:
    """The reason to inject for the first ready step not yet injected, or None."""
    for step in ready:
        key = _step_key(step["issue"], step["step"], step["head"])
        if _should_block_step(state, key):
            _record_step_block(state, key)
            return f"GraphHelm: {step['reason']} on issue {step['issue']} at head {step['head'][:8]}."
    return None


# #2643 slice 2: a new review, verdict or merge for this lane rings its waiter, so the rewake
# scans now. The ring is the file rewake reads by mtime (base.with_suffix(".waiter.rang")).
def ring_lane(base: Path) -> None:
    """Touch the lane's waiter ring so an idle rewake sees a new event."""
    rang = base.with_suffix(".waiter.rang")
    rang.parent.mkdir(parents=True, exist_ok=True)
    rang.touch()
    os.utime(rang, None)


NOTES_REASON = ("GraphHelm: read the notes addressed to you on the team execution, act on them, "
                "then record your operator_note reply with replyTo before stopping.")


def _same_lane(name, lane) -> bool:
    """A reviewer name matches a lane by its id, or by its name without the `openbot-` prefix
    (the projection holds both forms, e.g. `Hinata` and `openbot-neji`)."""
    def norm(value) -> str:
        value = str(value or "").strip().lower()
        return value[len("openbot-"):] if value.startswith("openbot-") else value
    return bool(name) and norm(name) == norm(lane)


STALE_AFTER_SECONDS = 3 * 24 * 3600


def _stale(item: dict) -> bool:
    """A task idle for days is not a ready step: the projection can keep a task whose PR merged
    with no merge record. No readable last-event time means not known stale."""
    clock = item.get("clock")
    last = clock.get("lastAt") if isinstance(clock, dict) else None
    try:
        at = datetime.fromisoformat(str(last).replace("Z", "+00:00"))
        if at.tzinfo is None:
            return False
        return (datetime.now(timezone.utc) - at).total_seconds() > STALE_AFTER_SECONDS
    except (ValueError, TypeError):
        return False


def ready_steps(data: dict, lane: str) -> list[dict]:
    """#2643 slice 3: the steps ready for `lane` in the Runtime's tasks projection: a review
    assigned to it, or a BLOCK on its own PR to fix. A merged task is never ready."""
    steps: list[dict] = []
    for item in data.get("tasks") or []:
        if not isinstance(item, dict) or item.get("mergeSha") or _stale(item):
            continue
        issue, head = item.get("issue"), item.get("headSha")
        if not isinstance(issue, int) or not isinstance(head, str) or not head:
            continue
        reviewers = [item.get("currentReviewer"), *(item.get("assignedReviewers") or [])]
        if item.get("step") == "review" and any(_same_lane(name, lane) for name in reviewers):
            steps.append({"issue": issue, "step": "review", "head": head,
                          "reason": f"assigned review of PR {item.get('pr')}"})
        elif _same_lane(item.get("lane"), lane) and item.get("blockedBy"):
            steps.append({"issue": issue, "step": "fix", "head": head,
                          "reason": f"BLOCK to fix on PR {item.get('pr')}"})
    return steps


# #2643 slice 3: the lookup is one more Runtime call on every Stop, so it gets its own short,
# hard wall-time bound: the notes scan already uses up to SCAN_CHILD_TIMEOUT_SECONDS of the host's
# Stop time, and a stalled Runtime must not hold the stop for a second budget.
LOOKUP_BUDGET_SECONDS = 1.5


def lookup_ready(payload: dict) -> list[dict]:
    """#2643 slice 3: the lane's ready steps, within LOOKUP_BUDGET_SECONDS. A lookup that is not
    done in time finds no step: the stop is never held for it."""
    found: list[dict] = []

    def run() -> None:
        found.extend(_lookup_ready(payload))

    thread = threading.Thread(target=run, daemon=True)
    thread.start()
    thread.join(LOOKUP_BUDGET_SECONDS)
    return [] if thread.is_alive() else list(found)


def _lookup_ready(payload: dict, budget: float = LOOKUP_BUDGET_SECONDS) -> list[dict]:
    """Ask the Runtime (existing tasks endpoint, this lane's own token) for the lane's ready
    steps. Any failure means no ready step: an unreadable lookup never blocks a stop."""
    try:
        bound = binding(payload)
        if bound is None:
            return []
        execution, token_file, url, _origin, _node = bound
        lane = os.environ.get("GRAPHHELM_ACTOR") or _lane_actor(payload) or ""
        if not ID.fullmatch(lane):
            return []
        token = token_from_file(token_file)
        reply = request(f"{url}/v1/executions/{quote(execution, safe='')}/tasks", token, "GET",
                        timeout=budget)
        data = reply.get("data") if isinstance(reply, dict) else None
        return ready_steps(data, lane) if isinstance(data, dict) else []
    except Exception:  # noqa: BLE001
        return []


def evaluate(payload: dict, host: str = "claude", ready: list[dict] | None = None,
             turn_state: dict | None = None, new_event: bool = False,
             ring_base: Path | None = None) -> str | None:
    """#1278: one Stop for this lane, callable in-process by `stop_hook.py`: reconcile, then the
    reminder reason (or None)."""
    # Every Stop refreshes this lane's finished PRs and released claims. An offline
    # Runtime or GitHub must never prevent the host from ending its turn.
    try:
        bound = binding(payload)
        lane = os.environ.get("GRAPHHELM_ACTOR") or _lane_actor(payload)
        if bound is not None and lane and ID.fullmatch(lane):
            start_lane_waiter(payload, bound, host)
            execution, token_file, url, _origin, _node = bound
            cwd = payload.get("cwd")
            if isinstance(cwd, str) and Path(cwd).is_dir():
                remote = origin_url(cwd)
                match = re.fullmatch(r"(?:git@github\.com:|https://github\.com/)([^/\s]+/[^/\s]+?)(?:\.git)?", remote)
                if match is not None:
                    cli = os.environ.get("GRAPHHELM_CLI", "graphhelm")
                    result = subprocess.run([cli, "tasks", "reconcile", "--execution", execution,
                                             "--token-file", token_file, "--url", url, "--lane", lane,
                                             "--repo", match.group(1)],
                                            capture_output=True, timeout=5, creationflags=NO_WINDOW)
                    if result.returncode == 0:
                        reply = json.loads(result.stdout or b"{}")
                        data = reply.get("data") if isinstance(reply, dict) and reply.get("ok") is True else None
                        if isinstance(data, dict):
                            session = payload.get("session_id")
                            memory = (state_path(execution, session, host, bound[3]).with_suffix(".reconciled")
                                      if isinstance(session, str) and ID.fullmatch(session) else None)
                            record_reconciled(bound, lane, data, memory)
    except (OSError, ValueError, subprocess.SubprocessError):
        pass
    # The host sets this on the continuation caused by a Stop block. One reminder
    # per stop cycle is enough, even when the lane cannot send its reply.
    if payload.get("stop_hook_active") is True:
        return None
    # #2643: a ready step is injected first; the per-turn state is passed by main() and reset
    # at the start of each Stop cycle, so a stop with a step already injected does not re-block.
    # #2643 slice 2: a new review, verdict or merge rings this lane before the Stop decides.
    if new_event and ring_base is not None:
        ring_lane(ring_base)
    if ready and turn_state is not None:
        reason = ready_step_reason(turn_state, ready)
        if reason is not None:
            return reason
    if scan_status(payload) != 1:
        if STALE_NOTES:
            print(f"graphhelm-lane-stop: ignored {len(STALE_NOTES)} note(s) older than "
                  f"{_note_max_age() / 86400:g} days: {', '.join(STALE_NOTES[:3])}", file=sys.stderr)
        return None
    return NOTES_REASON + (f" (ignored {len(STALE_NOTES)} note(s) older than {_note_max_age() / 86400:g} days: "
                           + ", ".join(STALE_NOTES[:3]) + ")" if STALE_NOTES else "")


# #1058: how often an idle lane's rewake looks for its waiter's ring, and how long it goes
# without one before it scans the notes anyway (a waiter that died must not make it deaf).
REWAKE_POLL_SECONDS = 0.25
REWAKE_FALLBACK_SECONDS = 30.0
# #2152: a scan that failed is retried this soon, not at the next fallback; a failed scan after
# the ring used to leave the lane deaf for a further 30 s.
REWAKE_RETRY_SECONDS = 2.0
# #2152: one line per scan beside the rewake's state, kept under this size.
REWAKE_LOG_MAX_BYTES = 16 * 1024
# #2152: the wake carries the notes' own text, bounded.
REWAKE_NOTES_SHOWN = 3
REWAKE_NOTE_CHARS = 400
REWAKE_REASON = ("GraphHelm: a note addressed to your lane arrived while you were idle. Read the notes "
                 "addressed to you, act on them, then record your operator_note reply with replyTo.")


def _mtime(path: Path) -> int | None:
    try:
        return path.stat().st_mtime_ns
    except OSError:
        return None


def _read_notified(path: Path) -> set[str]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return set()
    return {item for item in value if isinstance(item, str)} if isinstance(value, list) else set()


def _log_scan(path: Path, trigger: str, outcome: str, started: float, error: BaseException | None = None) -> None:
    """#2152: one bounded line per rewake scan: when, what started it, what it found, and the
    class of the exception when it failed (never its message, which may carry a URL)."""
    line = (f"{datetime.now(timezone.utc).isoformat(timespec='milliseconds')} trigger={trigger} "
            f"outcome={outcome} ms={int((time.monotonic() - started) * 1000)}"
            + (f" error={type(error).__name__}" if error is not None else "") + "\n")
    try:
        try:
            if path.stat().st_size > REWAKE_LOG_MAX_BYTES:
                kept = path.read_bytes()[-REWAKE_LOG_MAX_BYTES // 2:]
                path.write_bytes(kept[kept.find(b"\n") + 1:])
        except FileNotFoundError:
            pass
        with path.open("a", encoding="utf-8") as output:
            output.write(line)
    except OSError:
        pass


def _note_text(bound: tuple, event: dict) -> str | None:
    """#2152: the description of one addressed note, read from its sealed evidence and checked
    against the journal's envelope hash; None when it cannot be read."""
    execution, token_file, url, _origin, _node = bound
    refs = event.get("evidenceRefs")
    record = event.get("kind", {}).get("data", {})
    if not isinstance(refs, list) or len(refs) != 1 or not isinstance(refs[0], dict):
        return None
    evidence_id = refs[0].get("evidenceId")
    if not isinstance(evidence_id, str) or not ID.fullmatch(evidence_id):
        return None
    data = request(f"{url}/v1/executions/{quote(execution, safe='')}/evidence/{quote(evidence_id, safe='')}",
                   token_from_file(token_file), "GET", timeout=2.0, max_reply=256 * 1024).get("data")
    if not isinstance(data, dict) or not isinstance(data.get("content"), str):
        return None
    content = data["content"]
    if hashlib.sha256(content.encode("utf-8")).hexdigest() != record.get("envelopeSha256"):
        return None
    signal = json.loads(content)
    description = signal.get("description") if isinstance(signal, dict) else None
    if not isinstance(description, str):
        return None
    text = " ".join(description.split())
    return text if len(text) <= REWAKE_NOTE_CHARS else text[:REWAKE_NOTE_CHARS - 3] + "..."


def wake_reason(bound: tuple, events: dict[str, dict]) -> str:
    """#2152: the wake message, carrying the new notes' text (bounded) so the lane can act at once."""
    lines = []
    for signal_id in sorted(events)[:REWAKE_NOTES_SHOWN]:
        try:
            text = _note_text(bound, events[signal_id])
        except Exception:  # noqa: BLE001 - the wake itself must not depend on reading the text
            text = None
        lines.append(f"- {signal_id}: {text}" if text else f"- {signal_id}: (text unavailable; read it from the notes)")
    if len(events) > REWAKE_NOTES_SHOWN:
        lines.append(f"- and {len(events) - REWAKE_NOTES_SHOWN} more")
    return REWAKE_REASON + "\n" + "\n".join(lines)


REWAKE_STEP_REASON = "GraphHelm: a step is ready for your lane while you were idle."
REWAKE_RETRY_MAX_SECONDS = 60.0


def rewake_retry_delay(failures: int) -> float:
    """#2697: a failed scan is retried 2 s, 4 s, 8 s ... up to 60 s later. A fixed 2 s retry from a
    few dozen lanes kept a slow Runtime saturated, and every scan kept timing out."""
    return min(REWAKE_RETRY_MAX_SECONDS, REWAKE_RETRY_SECONDS * (2 ** max(0, failures - 1)))


def _rewake_step_id(step: dict) -> str:
    return f"step:{step['issue']}:{step['step']}:{step['head']}"


def _lookup_ready_for_rewake(payload: dict) -> list[dict]:
    """#2699: the rewake waits for the tasks projection as long as its own scan budget; a failure is
    no ready step, and the next ring or fallback asks again."""
    return _lookup_ready(payload, REWAKE_SCAN_BUDGET_SECONDS)


def webhook_woken(lane) -> bool:
    """#2714: an OpenBot lane (`openbot-*`) is woken by the Runtime's webhook, and a waiter running
    outside OpenBot cannot start its turn, so it must not start at all: it would only leak. Lanes
    outside OpenBot keep the waiter; `GRAPHHELM_NO_WAITER=1` (from settings.local.json `env`) turns
    it off for any other lane the same way."""
    return os.environ.get("GRAPHHELM_NO_WAITER") == "1" or str(lane or "").startswith("openbot-")


def rewake(payload: dict, host: str = "claude") -> int:
    """#1058: the host runs this in the background after every Stop (`asyncRewake`); exit 2 wakes
    the idle host. It waits for the lane waiter's ring (or the fallback interval), then scans the
    notes and wakes for an unanswered note addressed to this lane that no earlier rewake already
    woke it for, so a note the lane chose to leave is not repeated every interval. The Stop that
    started it has already reminded the lane of what was waiting then. A newer Stop's rewake
    supersedes this one. #2152: every scan leaves one line in `.rewake.log`, and a failed scan is
    retried after `REWAKE_RETRY_SECONDS` instead of waiting for the next ring or fallback."""
    bound = binding(payload)
    lane = os.environ.get("GRAPHHELM_ACTOR") or _lane_actor(payload)
    session = payload.get("session_id")
    if (bound is None or not lane or not ID.fullmatch(lane)
            or not isinstance(session, str) or not ID.fullmatch(session)):
        return 0
    if webhook_woken(lane):  # #2714: the webhook wakes this lane; no background process to leak
        return 0
    base = state_path(bound[0], session, host, bound[3])
    owner, rang = base.with_suffix(".rewake.pid"), base.with_suffix(".waiter.rang")
    notified, log = base.with_suffix(".rewake.notified"), base.with_suffix(".rewake.log")
    me = str(os.getpid())
    owner.parent.mkdir(parents=True, exist_ok=True)
    owner.write_text(me, encoding="ascii")
    seen = _mtime(rang)
    next_scan = time.monotonic()  # #2699: the first scan, which takes the cursor, runs at once
    retrying = False
    failures = 0
    scan_state: dict = {}
    while True:
        time.sleep(REWAKE_POLL_SECONDS)
        try:
            if owner.read_text(encoding="ascii") != me:
                return 0
        except OSError:
            return 0
        now = _mtime(rang)
        if now == seen and time.monotonic() < next_scan:
            continue
        trigger = "ring" if now != seen else ("retry" if retrying else "fallback")
        seen, started = now, time.monotonic()
        try:
            waiting, ready_hint = scan_notes(payload, scan_state, budget=REWAKE_SCAN_BUDGET_SECONDS, from_head=True)
        except Exception as error:  # noqa: BLE001 - an unreadable scan is logged and retried, backing off
            _log_scan(log, trigger, "error", started, error)
            failures += 1
            retrying, next_scan = True, time.monotonic() + rewake_retry_delay(failures)
            continue
        failures = 0
        retrying, next_scan = False, time.monotonic() + REWAKE_FALLBACK_SECONDS
        fresh = set(waiting) - _read_notified(notified)
        # #2697: a task record that can make a step ready (an assigned review, a BLOCK to fix) is not
        # a note; the ready lookup (one /tasks call) runs only when such a record has arrived.
        steps = [step for step in (_lookup_ready_for_rewake(payload) if ready_hint else [])
                 if _rewake_step_id(step) not in _read_notified(notified)]
        _log_scan(log, trigger, "wake" if (fresh or steps) else ("seen" if waiting else "none"), started)
        if fresh or steps:
            try:
                notified.write_text(json.dumps(sorted(set(waiting) | {_rewake_step_id(step) for step in steps}
                                                    | (_read_notified(notified) - set(waiting)))), encoding="utf-8")
            except OSError:
                pass
            reason = wake_reason(bound, {key: waiting[key] for key in fresh}) if fresh else REWAKE_STEP_REASON
            for step in steps:
                reason += f"\n- {step['reason']} (issue {step['issue']}, head {step['head'][:8]}); read origin/main skill for the step, then act."
            print(reason, file=sys.stderr)
            return 2


def main() -> int:
    if sys.argv[1:] == ["--rewake"]:
        try:
            payload = hook_input()
        except (ValueError, OSError):
            return 0
        return rewake(payload, "claude")
    if sys.argv[1:] == ["--scan"]:  # kept for callers of the old child form
        try:
            payload = hook_input()
            return 1 if pending(payload) else 0
        except Exception:
            return 2
    try:
        payload = hook_input()
    except (ValueError, OSError):
        return 0
    # #2643: the ready list comes from the Runtime's tasks projection; the turn state is fresh for
    # this Stop cycle.
    reason = evaluate(payload, "codex" if "--host=codex" in sys.argv else "claude",
                      ready=lookup_ready(payload), turn_state=new_turn_state())
    if reason:
        print(json.dumps({"decision": "block", "reason": reason}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
