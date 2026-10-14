"""Reconcile finished GitHub PRs with sealed Runtime task records.

Runs on every Stop of a bound lane (`lane_stop_hook.py` -> `graphhelm tasks reconcile`). Besides
recording merges, closes and releases, it heartbeats the lane's open pre-PR claims (#1147): a live
lane needs no reminder to say it is alive, and the Runtime alone decides whether a heartbeat is due
(`validate_heartbeat`: one per 300 s); its refusal is reported, never retried or pre-empted here.

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


def get(url, token):
    request = urllib.request.Request(url, headers={"Authorization": "Bearer " + token})
    with urllib.request.urlopen(request, timeout=2) as reply:
        return json.load(reply)["data"]


def records(base, token, execution):
    path = f"{base}/v1/executions/{urllib.parse.quote(execution, safe='')}"
    after = 0
    events = []
    while True:
        page = get(f"{path}/events?after={after}&limit=512", token)
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
        evidence = get(f"{path}/evidence/{urllib.parse.quote(evidence_id, safe='')}", token)
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
    for kind, doc, event in records(args.url.rstrip("/"), token, args.execution):
        task_id = doc.get("taskId")
        lane = doc.get("lane")
        if kind == "task.claimed" and lane == args.lane and doc.get("repo") == args.repo and re.fullmatch(r"issue-[1-9][0-9]*", str(task_id)):
            claims.setdefault(task_id, []).append(event["sequence"])
        elif kind == "task.released" and lane == args.lane and doc.get("repo") == args.repo and re.fullmatch(r"issue-[1-9][0-9]*", str(task_id)):
            releases.setdefault(task_id, set()).add(doc.get("claimSequence"))
        pr = doc.get("pr")
        if type(pr) is not int or pr < 1:
            continue
        if kind in ("task.merged", "task.closed"):
            finished.add((doc.get("repo"), pr))
        elif kind == "task.pr_opened" and (args.lane is None or lane == args.lane) and doc.get("repo") == args.repo and re.fullmatch(r"issue-[1-9][0-9]*", str(task_id)):
            opened[pr] = (doc["taskId"], args.repo)
            if task_id in claims:
                for sequence in claims[task_id]:
                    if sequence not in releases.get(task_id, set()):
                        releases.setdefault(task_id, set()).add(sequence)
                        break
    done = []
    closed = []
    for pr, (task_id, repo) in opened.items():
        if (repo, pr) in finished:
            continue
        result = github(pr, repo)
        commit = result.get("mergeCommit") or {}
        sha = commit.get("oid")
        if result.get("state") == "CLOSED" and not result.get("mergedAt"):
            body = result.get("body") or ""
            match = re.search(r"(?i)\bsuperseded\s+by\s+(?:PR\s+)?(?:https://github\.com/[^\s]+/pull/)?#?([1-9][0-9]*)\b", body)
            by = int(match.group(1)) if match else pr
            if args.lane is None:
                continue
            post_end(args.url.rstrip("/"), credential(args.lane), args.execution, "closed", task_id, repo,
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
    lapsed = release_overdue(args.url.rstrip("/"), token, credential, args.execution, args.repo)
    return {"merged": done, "closed": closed, "released": released, "heartbeat": beats,
            "lapsed": lapsed, "inspected": len(opened)}


def release_overdue(base, token, credential, execution, repo):
    """Releases, as graphhelm, each lease the tasks route reports overdue (#1193). Returns
    {"released": [...], "refused": [...]}; a route that does not serve leases (older Runtime)
    yields empty lists, never an error, so the rest of the reconcile stands."""
    out = {"released": [], "refused": []}
    try:
        view = get(f"{base}/v1/executions/{urllib.parse.quote(execution, safe='')}/tasks", token)
    except (OSError, ValueError, KeyError):
        return out
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
    args = parser.parse_args()
    try:
        print(json.dumps(reconcile(args)))
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"tasks reconcile: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
