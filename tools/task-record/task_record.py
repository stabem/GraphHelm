#!/usr/bin/env python3
"""Record one `task.*` delivery step on a Runtime execution, as one lane (DELIVERY.md, "Task records").

One command per step, the actor set per command, so lanes that share one MCP registration never
need a per-lane environment or a session relaunch:

    python tools/task-record/task_record.py --lane gh-claude-2 claimed  --issue 355 --branch issue-355-x
    python tools/task-record/task_record.py --lane gh-claude-2 pr_opened --issue 355 --pr 357 --head <sha>
    python tools/task-record/task_record.py --lane gh-claude-2 review_assigned --issue 355 --pr 357 --head <sha> --reviewer gh-claude-6
    python tools/task-record/task_record.py --lane gh-claude-6 review_verdict --issue 355 --pr 357 --head <sha> --verdict APPROVE --comment-url <url>
    python tools/task-record/task_record.py --lane gh-claude-6 merged --issue 355 --pr 357 --merge-sha <sha>

It POSTs `/v1/executions/<execution>/signal` with the Runtime's AGENT SESSION token
(`<events>.agent.token`, beside the Runtime's events directory; D-058) and
`X-GraphHelm-Actor: <lane>`. The Runtime forces the actor type to `agent` for that token and refuses
a record whose `lane` / `reviewer` / `merger` is not the recording actor (GHCLI038_ACTOR_MISMATCH).
Standard library only. `--dry-run` prints the request instead of sending it.
"""
import argparse
import datetime
import json
import sys
import urllib.error
import urllib.request
from pathlib import Path

KINDS = ("claimed", "pr_opened", "review_assigned", "review_verdict", "merged")


def parse(argv):
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("kind", choices=KINDS)
    p.add_argument("--lane", required=True, help="the recording actor (your ListAgents name)")
    p.add_argument("--issue", type=int, required=True, help="the task is issue-<N> for its whole life")
    p.add_argument("--revision", type=int, help="defaults to the step's position: claimed 1 ... merged 5")
    p.add_argument("--branch")
    p.add_argument("--pr", type=int)
    p.add_argument("--head", help="the PR head sha the step is about")
    p.add_argument("--reviewer")
    p.add_argument("--ordinal", type=int, default=1, choices=(1, 2))
    p.add_argument("--verdict", choices=("APPROVE", "APPROVE-WITH-RISK", "BLOCK"))
    p.add_argument("--comment-url")
    p.add_argument("--merge-sha")
    p.add_argument("--closes", type=int, nargs="*", help="issues the merge closed; defaults to --issue")
    p.add_argument("--journeys", nargs="*", default=[])
    p.add_argument("--repo", default="stabem/GraphHelm",
                   help="owner/name; pass '' to omit it (a Runtime older than #420 refuses the field)")
    p.add_argument("--execution", default="gh-team")
    p.add_argument("--url", default="http://127.0.0.1:8793")
    p.add_argument("--token-file", default=".graphhelm/events.agent.token",
                   help="the Runtime's agent session token (never the owner's events.token)")
    p.add_argument("--dry-run", action="store_true")
    return p.parse_args(argv)


def need(args, *names):
    missing = [n for n in names if getattr(args, n.replace("-", "_")) in (None, "")]
    if missing:
        sys.exit(f"task_record: {args.kind} needs --" + ", --".join(missing))


def document(args, now):
    doc = {"schema": "graphhelm-task-event-v1", "taskId": f"issue-{args.issue}",
           "revision": args.revision or KINDS.index(args.kind) + 1, "at": now}
    if args.kind == "claimed":
        need(args, "branch")
        doc.update(issue=args.issue, lane=args.lane, branch=args.branch)
    elif args.kind == "pr_opened":
        need(args, "pr", "head")
        doc.update(pr=args.pr, headSha=args.head, journeys=args.journeys, lane=args.lane)
    elif args.kind == "review_assigned":
        need(args, "pr", "head", "reviewer")
        doc.update(pr=args.pr, headSha=args.head, reviewer=args.reviewer, ordinal=args.ordinal)
    elif args.kind == "review_verdict":
        need(args, "pr", "head", "verdict", "comment-url")
        doc.update(pr=args.pr, headSha=args.head, reviewer=args.lane, verdict=args.verdict,
                   commentUrl=args.comment_url)
    else:
        need(args, "pr", "merge-sha")
        doc.update(pr=args.pr, mergeSha=args.merge_sha, closes=args.closes or [args.issue],
                   merger=args.lane)
    if args.repo and args.kind in ("claimed", "pr_opened"):
        doc["repo"] = args.repo
    return doc


def main(argv):
    args = parse(argv)
    now = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    doc = document(args, now)
    signal_id = f"{args.lane}-{doc['taskId']}-{args.kind}-r{doc['revision']}"
    evidence = args.comment_url or (f"https://github.com/{args.repo}/pull/{args.pr}" if args.pr
                                    else f"https://github.com/{args.repo}/issues/{args.issue}")
    body = {"signal": {"id": signal_id, "type": f"task.{args.kind}",
                       "source": {"type": "user", "id": args.lane}, "severity": "low",
                       "emittedAt": now, "evidence": [evidence],
                       "description": json.dumps(doc, separators=(",", ":"))}}
    url = f"{args.url.rstrip('/')}/v1/executions/{args.execution}/signal"
    headers = {"Content-Type": "application/json", "Idempotency-Key": signal_id,
               "X-GraphHelm-Actor": args.lane, "X-GraphHelm-Actor-Type": "agent"}
    if args.dry_run:
        print(json.dumps({"url": url, "headers": headers, "body": body}, indent=1))
        return 0
    token = Path(args.token_file).read_text(encoding="utf-8").strip()
    request = urllib.request.Request(url, data=json.dumps(body).encode(), method="POST",
                                     headers={**headers, "Authorization": f"Bearer {token}"})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            reply = json.load(response)
    except urllib.error.HTTPError as error:
        reply = json.loads(error.read() or b"{}")
    except urllib.error.URLError as error:
        sys.exit(f"task_record: no Runtime at {args.url}: {error.reason}")
    ok = reply.get("ok") is True
    print(f"{'recorded' if ok else 'REFUSED'} {signal_id}")
    for d in reply.get("diagnostics", []):
        if d.get("severity") == "error":
            print(f"  {d.get('code')} {d.get('path')}: {d.get('message')}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
