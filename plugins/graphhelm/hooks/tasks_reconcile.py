"""Reconcile finished GitHub PRs with sealed Runtime task records.

Runs on every Stop of a bound lane (`lane_stop_hook.py` -> `graphhelm tasks reconcile`). Besides
recording merges, closes and releases, it heartbeats the lane's open pre-PR claims (#1147): a live
lane needs no reminder to say it is alive, and the Runtime alone decides whether a heartbeat is due
(`validate_heartbeat`: one per 300 s); its refusal is reported, never retried or pre-empted here.

It also runs with no lane at all (#1628): a hidden scheduled task calls it every few minutes as
`--lane graphhelm --adopt-opened`, so the board follows GitHub even when no lane ever stops again.
As `graphhelm` it records merges and closes of every lane's PR, and `--adopt-opened` records the
`task.pr_opened` a lane forgot for an open PR whose `Closes #N` names that lane's open claim (signed
with that lane's own credential file; a lane without one is reported, never impersonated with the
shared token). `--log` appends the reply to a file, for `pythonw` which has no console.

The CLI embeds this file, so it also works from an installed GraphHelm binary.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request
import os

# Owner rule: children run hidden in the calling terminal, never in a new console window.
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
MAX_PRS_PER_STOP = 20


def cursor_path(token_file, lane):
    path = Path(token_file)
    return path.with_name(f"{path.stem}.reconcile.{lane or 'shared'}.cursor")


def rotate(pending, cursor):
    """#1544: the backlog in a stable order, starting after the last PR the previous Stop checked,
    so a window of unreadable or still-open PRs cannot hide the newer ones behind it."""
    pending = sorted(pending)
    after = [pr for pr in pending if pr > cursor]
    return after + [pr for pr in pending if pr <= cursor]


def read_cursor(path):
    try:
        value = int(path.read_text(encoding="utf-8").strip())
    except (OSError, ValueError):
        return 0
    return value if value > 0 else 0


def write_cursor(path, value):
    try:
        path.write_text(str(value), encoding="utf-8")
    except OSError:
        pass  # local state only: a lost cursor restarts the rotation, it never blocks a Stop


def get(url, token, lane=None):
    headers = {"Authorization": "Bearer " + token}
    if lane is not None:
        headers["X-GraphHelm-Actor"] = lane
    request = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(request, timeout=2) as reply:
        return json.load(reply)["data"]


def records(base, token, execution, lane=None):
    path = f"{base}/v1/executions/{urllib.parse.quote(execution, safe='')}"
    after = 0
    events = []
    while True:
        page = get(f"{path}/events?after={after}&limit=512", token, lane)
        batch = page["events"]
        if not batch:
            if after < page["head"]:
                raise ValueError("incomplete event page")
            break
        for event in batch:
            kind = event.get("kind", {})
            detail = kind.get("data", {})
            if kind.get("type") == "signal_recorded" and detail.get("kind") in (
                    "task.claimed", "task.released", "task.pr_opened", "task.merged", "task.closed"):
                refs = event.get("evidenceRefs", [])
                if len(refs) == 1:
                    events.append((event, detail, refs[0]["evidenceId"]))
        last = batch[-1]["sequence"]
        if last <= after:
            raise ValueError("event cursor did not advance")
        after = last
        if after >= page["head"]:
            break

    def open_record(item):
        event, detail, evidence_id = item
        evidence = get(f"{path}/evidence/{urllib.parse.quote(evidence_id, safe='')}", token, lane)
        content = evidence["content"]
        digest = hashlib.sha256(content.encode()).hexdigest()
        if digest != detail.get("envelopeSha256") or digest != evidence.get("contentSha256"):
            raise ValueError("task evidence digest mismatch")
        envelope = json.loads(content)
        if envelope.get("type") != detail["kind"] or envelope.get("source", {}).get("id") != event.get("actor", {}).get("id"):
            raise ValueError("task evidence actor mismatch")
        document = json.loads(envelope["description"])
        if document.get("schema") != "graphhelm-task-event-v1":
            raise ValueError("invalid task document")
        if detail["kind"] in ("task.merged", "task.closed"):
            refs = envelope.get("evidence", [])
            if len(refs) == 1:
                match = re.fullmatch(r"https://github\.com/([^/]+/[^/]+)/pull/([1-9][0-9]*)", refs[0])
                if match and int(match.group(2)) == document.get("pr"):
                    document["repo"] = match.group(1)
        elif detail["kind"] == "task.released":
            refs = envelope.get("evidence", [])
            if len(refs) == 1:
                match = re.fullmatch(r"https://github\.com/([^/]+/[^/]+)/issues/([1-9][0-9]*)", refs[0])
                if match and f"issue-{match.group(2)}" == document.get("taskId"):
                    document["repo"] = match.group(1)
        return detail["kind"], document, event

    with ThreadPoolExecutor(max_workers=4) as pool:
        return list(pool.map(open_record, events))


def github(pr, repo):
    result = subprocess.run(["gh", "pr", "view", str(pr), "--repo", repo,
                             "--json", "state,mergeCommit,mergedAt,closingIssuesReferences,body"],
                            capture_output=True, text=True, encoding="utf-8", timeout=3, check=True, creationflags=NO_WINDOW)
    return json.loads(result.stdout)


def github_issue(issue, repo):
    result = subprocess.run(["gh", "issue", "view", str(issue), "--repo", repo,
                             "--json", "state,assignees"], capture_output=True, text=True,
                            encoding="utf-8", timeout=3, check=True, creationflags=NO_WINDOW)
    return json.loads(result.stdout)


def post(base, token, execution, pr, task_id, repo, sha, closes):
    now = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    doc = {"schema": "graphhelm-task-event-v1", "taskId": task_id,
           "revision": 5, "at": now, "pr": pr, "mergeSha": sha,
           "closes": closes, "merger": "graphhelm"}
    content = json.dumps({k: v for k, v in doc.items() if k != "at"}, sort_keys=True, separators=(",", ":"))
    digest = hashlib.sha256(("graphhelm\nmerged\n" + content).encode()).hexdigest()[:16]
    signal_id = f"graphhelm-{task_id}-merged-{digest}"
    signal = {"id": signal_id, "type": "task.merged", "source": {"type": "user", "id": "graphhelm"},
              "severity": "low", "emittedAt": now,
              "evidence": [f"https://github.com/{repo}/pull/{pr}"],
              "description": json.dumps(doc, separators=(",", ":"))}
    url = f"{base}/v1/executions/{urllib.parse.quote(execution, safe='')}/signal"
    headers = {"Authorization": "Bearer " + token, "Content-Type": "application/json",
               "Idempotency-Key": signal_id, "X-GraphHelm-Actor": "graphhelm",
               "X-GraphHelm-Actor-Type": "agent"}
    request = urllib.request.Request(url, data=json.dumps({"signal": signal}).encode(), headers=headers, method="POST")
    with urllib.request.urlopen(request, timeout=3) as reply:
        answer = json.load(reply)
    if answer.get("ok") is not True:
        raise ValueError("Runtime refused task.merged")


def github_open_prs(repo):
    """#1628: the repository's open PRs with their head and the issues their body closes."""
    result = subprocess.run(["gh", "pr", "list", "--repo", repo, "--state", "open", "--limit", "200",
                             "--json", "number,headRefOid,closingIssuesReferences"],
                            capture_output=True, text=True, encoding="utf-8", timeout=10, check=True, creationflags=NO_WINDOW)
    return json.loads(result.stdout)


def lane_credential(token_file, lane):
    """The lane's own credential beside --token-file, or None. Never the shared token: a record in
    a lane's name is written only with that lane's credential (#1148)."""
    path = Path(token_file)
    stem = path.name
    for suffix in (".agent.token", ".token"):
        if stem.endswith(suffix):
            stem = stem[: -len(suffix)]
            break
    try:
        value = path.with_name(f"{stem}.agent.{lane}.token").read_text(encoding="utf-8").strip()
    except OSError:
        return None
    return value if value and "\n" not in value else None


def adopt_opened(base, token_file, execution, repo, unopened, prs):
    """#1628: for each open claim with no `task.pr_opened` (`unopened`: taskId -> lane), the open PR
    whose `Closes #N` names its issue is recorded as opened, as the claim's lane. Returns
    {"opened": [pr...], "refused": [{"pr", "taskId", "message"}...]}."""
    out = {"opened": [], "refused": []}
    for pr in sorted(prs, key=lambda item: item.get("number", 0)):
        number, sha = pr.get("number"), pr.get("headRefOid")
        if type(number) is not int or number < 1 or not isinstance(sha, str) or not re.fullmatch(r"[0-9a-f]{40}", sha):
            continue
        for issue in pr.get("closingIssuesReferences") or []:
            if issue.get("repository", {}).get("nameWithOwner", repo) != repo:
                continue
            task_id = f"issue-{issue.get('number')}"
            lane = unopened.pop(task_id, None)
            if lane is None:
                continue
            credential = lane_credential(token_file, lane)
            if credential is None:
                out["refused"].append({"pr": number, "taskId": task_id,
                                       "message": f"no credential for {lane}; the lane records it"})
                continue
            try:
                post_end(base, credential, execution, "pr_opened", task_id, repo, lane,
                         {"pr": number, "headSha": sha, "journeys": [], "lane": lane, "repo": repo})
                out["opened"].append(number)
            except (OSError, ValueError, KeyError) as error:
                out["refused"].append({"pr": number, "taskId": task_id, "message": f"{type(error).__name__}: {error}"})
    return out


def head(base, token, execution):
    """The execution's event head: the state a heartbeat observed, and so its revision."""
    path = f"{base}/v1/executions/{urllib.parse.quote(execution, safe='')}"
    value = get(f"{path}/events?after=0&limit=1", token)["head"]
    if type(value) is not int or value < 1:
        raise ValueError("invalid event head")
    return value


def post_end(base, token, execution, kind, task_id, repo, lane, fields, revision=None):
    """POST one `task.<kind>` record as the lane. Returns the Runtime's reply for a `heartbeat`
    (refusal included: the Runtime owns that rule); every other kind raises on refusal."""
    now = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    if revision is None:
        revision = 6 if kind == "closed" else 2
    doc = {"schema": "graphhelm-task-event-v1", "taskId": task_id,
           "revision": revision, "at": now, **fields}
    content = json.dumps({k: v for k, v in doc.items() if k != "at"}, sort_keys=True, separators=(",", ":"))
    digest = hashlib.sha256((lane + "\n" + kind + "\n" + content).encode()).hexdigest()[:16]
    signal_id = f"{lane}-{task_id}-{kind}-{digest}"
    if len(signal_id) > 64:
        signal_id = f"{signal_id[:47]}-{digest}"
    issue = int(task_id.removeprefix("issue-"))
    evidence = (f"https://github.com/{repo}/pull/{fields['pr']}" if kind == "closed"
                else f"https://github.com/{repo}/issues/{issue}")
    signal = {"id": signal_id, "type": "task." + kind,
              "source": {"type": "user", "id": lane}, "severity": "low",
              "emittedAt": now, "evidence": [evidence],
              "description": json.dumps(doc, separators=(",", ":"))}
    url = f"{base}/v1/executions/{urllib.parse.quote(execution, safe='')}/signal"
    headers = {"Authorization": "Bearer " + token, "Content-Type": "application/json",
               "Idempotency-Key": signal_id, "X-GraphHelm-Actor": lane,
               "X-GraphHelm-Actor-Type": "agent"}
    request = urllib.request.Request(url, data=json.dumps({"signal": signal}).encode(),
                                     headers=headers, method="POST")
    try:
        with urllib.request.urlopen(request, timeout=3) as reply:
            answer = json.load(reply)
    except urllib.error.HTTPError as error:
        if kind != "heartbeat":
            raise
        answer = json.loads(error.read() or b"{}")
    if kind == "heartbeat":
        return answer
    if answer.get("ok") is not True:
        raise ValueError("Runtime refused task." + kind)


def post_closed(base, token, execution, task_id, repo, lane, fields):
    """#1544: record a close this lane only observed on GitHub as `observed: true`. A Runtime older
    than the field refuses it as an invalid signal (HTTP 400); the close is then recorded without
    it, as before, so a plugin update never stops a lane from reconciling against an older Runtime."""
    try:
        post_end(base, token, execution, "closed", task_id, repo, lane, {**fields, "observed": True})
    except urllib.error.HTTPError as error:
        if error.code != 400:
            raise ValueError(f"Runtime refused task.closed: HTTP {error.code}") from error
        post_end(base, token, execution, "closed", task_id, repo, lane, fields)


def heartbeat(base, token, credential, execution, task_id, repo, lane):
    """One `task.heartbeat` for an open claim. Returns None when it landed (or was already
    recorded), else the Runtime's refusal message: the cooldown lives there, not here.
    The head is read with the shared token; the record is written with the lane's credential (#1148)."""
    answer = post_end(base, credential, execution, "heartbeat", task_id, repo, lane, {"lane": lane},
                      revision=head(base, token, execution))
    if answer.get("ok") is True:
        return None
    errors = [d for d in answer.get("diagnostics", []) if isinstance(d, dict) and d.get("severity") == "error"]
    codes = {d.get("code") for d in errors}
    if codes == {"GHE003_IDEMPOTENCY_CONFLICT"}:
        return None
    return "; ".join(f"{d.get('code')}: {d.get('message')}" for d in errors) or "Runtime refused task.heartbeat"


def reconcile(args):
    parsed = urllib.parse.urlsplit(args.url)
    if parsed.scheme != "http" or parsed.hostname not in ("127.0.0.1", "localhost", "::1"):
        raise ValueError("agent token may only be sent to loopback Runtime")
    token = Path(args.token_file).read_text(encoding="utf-8").strip()
    if not token or "\n" in token:
        raise ValueError("invalid agent token")
    # #1148: reads use the shared agent token; a task.* record is written only with the credential
    # of the actor it names (`graphhelm` for merged, the lane for closed/released), read beside
    # --token-file at <events>.agent.<actor>.token, never the shared token.
    credentials = {}

    def credential(actor):
        if actor not in credentials:
            path = Path(args.token_file)
            stem = path.name
            for suffix in (".agent.token", ".token"):
                if stem.endswith(suffix):
                    stem = stem[: -len(suffix)]
                    break
            path = path.with_name(f"{stem}.agent.{actor}.token")
            try:
                value = path.read_text(encoding="utf-8").strip()
            except OSError:
                value = ""
            if not value or "\n" in value:
                # Rollout: a Runtime without --require-lane-credential still accepts the shared token.
                print(f"tasks reconcile: no credential for {actor} at {path}; sending the shared agent token; "
                      f"mint it with: graphhelm tasks credential --lane {actor} --execution {args.execution} "
                      f"--token-file <events.token>", file=sys.stderr)
                value = token
            credentials[actor] = value
        return credentials[actor]
    opened = {}
    finished = set()
    claims = {}
    releases = {}
    # #1628: every lane's claims, for --adopt-opened: taskId -> [(sequence, lane)], released
    # sequences, and the tasks that already have a PR.
    all_claims, all_released, with_pr = {}, set(), set()
    for kind, doc, event in records(args.url.rstrip("/"), token, args.execution, args.lane):
        task_id = doc.get("taskId")
        lane = doc.get("lane")
        if doc.get("repo", args.repo) == args.repo and re.fullmatch(r"issue-[1-9][0-9]*", str(task_id)):
            if kind == "task.claimed" and doc.get("repo") == args.repo and isinstance(lane, str):
                all_claims.setdefault(task_id, []).append((event.get("sequence"), lane))
            elif kind == "task.released":
                all_released.add(doc.get("claimSequence"))
            elif kind == "task.pr_opened":
                with_pr.add(task_id)
        if kind == "task.claimed" and lane == args.lane and doc.get("repo") == args.repo and re.fullmatch(r"issue-[1-9][0-9]*", str(task_id)):
            claims.setdefault(task_id, []).append(event["sequence"])
        elif kind == "task.released" and lane == args.lane and doc.get("repo") == args.repo and re.fullmatch(r"issue-[1-9][0-9]*", str(task_id)):
            releases.setdefault(task_id, set()).add(doc.get("claimSequence"))
        pr = doc.get("pr")
        if type(pr) is not int or pr < 1:
            continue
        if kind in ("task.merged", "task.closed"):
            finished.add((doc.get("repo"), pr))
        elif kind == "task.pr_opened" and doc.get("repo") == args.repo and re.fullmatch(r"issue-[1-9][0-9]*", str(task_id)):
            # #1520: every lane's open PR is reconciled, not only the stopping lane's: a PR whose
            # lane stopped, was retired or never stops again would otherwise stay open forever.
            opened[pr] = (doc["taskId"], args.repo)
            if task_id in claims:
                for sequence in claims[task_id]:
                    if sequence not in releases.get(task_id, set()):
                        releases.setdefault(task_id, set()).add(sequence)
                        break
    done = []
    closed = []
    unreadable = []
    cursor_file = cursor_path(args.token_file, args.lane)
    pending = [pr for pr, (_, repo) in opened.items() if (repo, pr) not in finished]
    # #1520/#1544: at most MAX_PRS_PER_STOP unfinished PRs per Stop, rotating through the backlog
    # from a cursor kept beside the token, so every PR is reached; one unreadable PR is reported,
    # never fatal to the rest.
    batch = rotate(pending, read_cursor(cursor_file))[:MAX_PRS_PER_STOP]
    if batch:
        write_cursor(cursor_file, batch[-1])
    for pr in batch:
        task_id, repo = opened[pr]
        try:
            result = github(pr, repo)
        except (OSError, ValueError, subprocess.SubprocessError):
            unreadable.append(pr)
            continue
        commit = result.get("mergeCommit") or {}
        sha = commit.get("oid")
        if result.get("state") == "CLOSED" and not result.get("mergedAt"):
            body = result.get("body") or ""
            match = re.search(r"(?i)\bsuperseded\s+by\s+(?:PR\s+)?(?:https://github\.com/[^\s]+/pull/)?#?([1-9][0-9]*)\b", body)
            by = int(match.group(1)) if match else pr
            if args.lane is None:
                continue
            post_closed(args.url.rstrip("/"), credential(args.lane), args.execution, task_id, repo,
                        args.lane, {"pr": pr, "reason": "superseded" if match else "abandoned", "by": by})
            closed.append(pr)
            continue
        if result.get("state") != "MERGED":
            continue
        if not isinstance(sha, str) or not re.fullmatch(r"[0-9a-fA-F]{40}", sha):
            raise ValueError(f"PR {pr} has no confirmed merge commit")
        closes = [issue["number"] for issue in result.get("closingIssuesReferences") or []
                  if issue.get("repository", {}).get("nameWithOwner", repo) == repo]
        post(args.url.rstrip("/"), credential("graphhelm"), args.execution, pr, task_id, repo, sha, closes)
        done.append(pr)
    released = []
    beats = {"sent": [], "refused": []}
    for task_id, sequences in claims.items():
        latest_release = max((sequence for sequence in releases.get(task_id, set())
                              if type(sequence) is int), default=0)
        outstanding = [sequence for sequence in sequences if sequence > latest_release]
        if not outstanding:
            continue
        issue = int(task_id.removeprefix("issue-"))
        result = github_issue(issue, args.repo)
        assignees = result.get("assignees") or []
        reassigned = bool(assignees) and args.lane not in [person.get("login") for person in assignees]
        if result.get("state") != "CLOSED" and not reassigned:
            # The claim stays open and this lane is alive (it is stopping a turn): heartbeat it.
            refusal = heartbeat(args.url.rstrip("/"), token, credential(args.lane), args.execution, task_id, args.repo, args.lane)
            if refusal is None:
                beats["sent"].append(issue)
            else:
                beats["refused"].append({"issue": issue, "message": refusal})
            continue
        post_end(args.url.rstrip("/"), credential(args.lane), args.execution, "released", task_id, args.repo,
                 args.lane, {"lane": args.lane, "claimSequence": outstanding[-1],
                             "reason": "Issue closed" if result.get("state") == "CLOSED" else "Issue reassigned"})
        released.append(issue)
    # #1193: every open pre-PR claim the Runtime's tasks fold reads as past its lease is released
    # as graphhelm, whichever lane holds it; the claim's lane is told by `releasedBy`.
    base = args.url.rstrip("/")
    try:
        view = get(f"{base}/v1/executions/{urllib.parse.quote(args.execution, safe='')}/tasks", token)
    except (OSError, ValueError, KeyError):
        view = None
    lapsed = release_overdue(view, base, credential, args.execution, args.repo)
    reply = {}
    if getattr(args, "adopt_opened", False):
        unopened = {}
        for task_id, items in all_claims.items():
            live = [(sequence, lane) for sequence, lane in items if sequence not in all_released]
            if live and task_id not in with_pr:
                unopened[task_id] = live[-1][1]
        try:
            prs = github_open_prs(args.repo) if unopened else []
        except (OSError, ValueError, subprocess.SubprocessError) as error:
            prs = []
            reply["adoptError"] = f"{type(error).__name__}: {error}"
        reply["adopted"] = adopt_opened(base, args.token_file, args.execution, args.repo, unopened, prs)
    return {**reply, "merged": done, "closed": closed, "released": released, "heartbeat": beats,
            "lapsed": lapsed, "stalled": stalled(view), "inspected": len(opened),
            "unreadable": unreadable}


def stalled(view):
    """#1235: the open PRs the tasks route reports idle past its stall budget, each with the lane
    that owes the next step. Recorded with the reconcile reply (`tasks.reconciled`), so a BLOCK
    behind a dead fixer is on the log for the Studio and the coordinator, not silent. An older
    Runtime without `stalls` yields an empty list."""
    stalls = view.get("stalls") if isinstance(view, dict) else None
    if not isinstance(stalls, list):
        return []
    keep = ("taskId", "pr", "reason", "owner", "idleSince", "idleSeconds")
    return [{key: item.get(key) for key in keep} for item in stalls if isinstance(item, dict)]


def release_overdue(view, base, credential, execution, repo):
    """Releases, as graphhelm, each lease the tasks route reports overdue (#1193). Returns
    {"released": [...], "refused": [...]}; a route that does not serve leases (older Runtime)
    or could not be read yields empty lists, never an error, so the rest of the reconcile stands."""
    out = {"released": [], "refused": []}
    leases = view.get("leases") if isinstance(view, dict) else None
    if not isinstance(leases, list):
        return out
    for lease in leases:
        if not isinstance(lease, dict) or lease.get("overdue") is not True:
            continue
        lane, task_id, sequence = lease.get("lane"), lease.get("taskId"), lease.get("claimSequence")
        if not (isinstance(lane, str) and isinstance(task_id, str) and type(sequence) is int and sequence > 0):
            continue
        entry = {"taskId": task_id, "lane": lane, "claimSequence": sequence}
        try:
            post_end(base, credential("graphhelm"), execution, "released", task_id, repo, "graphhelm",
                     {"lane": lane, "claimSequence": sequence, "releasedBy": "graphhelm",
                      "reason": f"lease of {lease.get('leaseSeconds')} s lapsed at {lease.get('expiresAt')} without a heartbeat"})
            out["released"].append(entry)
        except (OSError, ValueError, KeyError) as error:
            out["refused"].append({**entry, "message": f"{type(error).__name__}: {error}"})
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--execution", required=True)
    parser.add_argument("--url", required=True)
    parser.add_argument("--token-file", required=True)
    parser.add_argument("--lane")
    parser.add_argument("--repo", required=True)
    parser.add_argument("--adopt-opened", action="store_true",
                        help="#1628: record pr_opened for an open claim whose issue an open PR closes")
    parser.add_argument("--log", help="#1628: append the reply (or the error) to this file, for pythonw")
    args = parser.parse_args()

    def emit(line, stream):
        if args.log:
            stamp = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
            try:
                with open(args.log, "a", encoding="utf-8") as log:
                    log.write(f"{stamp} {line}\n")
            except OSError:
                pass
        if stream is not None:
            print(line, file=stream)
    try:
        emit(json.dumps(reconcile(args)), sys.stdout)
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        emit(f"tasks reconcile: {error}", sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
