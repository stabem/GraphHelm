"""Reconcile merged GitHub PRs with sealed Runtime task records.

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
import urllib.parse
import urllib.request


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
            if kind.get("type") == "signal_recorded" and detail.get("kind") in ("task.pr_opened", "task.merged"):
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
        if detail["kind"] == "task.merged":
            refs = envelope.get("evidence", [])
            if len(refs) == 1:
                match = re.fullmatch(r"https://github\.com/([^/]+/[^/]+)/pull/([1-9][0-9]*)", refs[0])
                if match and int(match.group(2)) == document.get("pr"):
                    document["repo"] = match.group(1)
        return detail["kind"], document, event

    with ThreadPoolExecutor(max_workers=4) as pool:
        return list(pool.map(open_record, events))


def github(pr, repo):
    result = subprocess.run(["gh", "pr", "view", str(pr), "--repo", repo,
                             "--json", "state,mergeCommit,closingIssuesReferences"],
                            capture_output=True, text=True, encoding="utf-8", timeout=3, check=True)
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


def reconcile(args):
    parsed = urllib.parse.urlsplit(args.url)
    if parsed.scheme != "http" or parsed.hostname not in ("127.0.0.1", "localhost", "::1"):
        raise ValueError("agent token may only be sent to loopback Runtime")
    token = Path(args.token_file).read_text(encoding="utf-8").strip()
    if not token or "\n" in token:
        raise ValueError("invalid agent token")
    # TODO(#750, #741): fold task.closed and task.released after those records land.
    opened = {}
    merged = set()
    for kind, doc, event in records(args.url.rstrip("/"), token, args.execution):
        pr = doc.get("pr")
        if type(pr) is not int or pr < 1:
            continue
        if kind == "task.merged":
            merged.add((doc.get("repo"), pr))
        elif (args.lane is None or doc.get("lane") == args.lane) and doc.get("repo") == args.repo and re.fullmatch(r"issue-[1-9][0-9]*", str(doc.get("taskId"))):
            opened[pr] = (doc["taskId"], args.repo)
    done = []
    for pr, (task_id, repo) in opened.items():
        if (repo, pr) in merged:
            continue
        result = github(pr, repo)
        commit = result.get("mergeCommit") or {}
        sha = commit.get("oid")
        if result.get("state") != "MERGED":
            continue
        if not isinstance(sha, str) or not re.fullmatch(r"[0-9a-fA-F]{40}", sha):
            raise ValueError(f"PR {pr} has no confirmed merge commit")
        closes = [issue["number"] for issue in result.get("closingIssuesReferences") or []
                  if issue.get("repository", {}).get("nameWithOwner", repo) == repo]
        post(args.url.rstrip("/"), token, args.execution, pr, task_id, repo, sha, closes)
        done.append(pr)
    return {"merged": done, "inspected": len(opened), "pendingKinds": ["closed", "released"]}


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
