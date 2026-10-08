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
import hashlib
import json
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

LOOPBACK = ("127.0.0.1", "localhost", "::1")
KINDS = ("claimed", "pr_opened", "review_assigned", "review_verdict", "merged")
# #477: what the naming standard asks (DELIVERY.md "Naming") and what the Runtime accepts.
STANDARD = {"claimed": 50, "pr_opened": 60, "summary": 100}
ACCEPTED = {"title": 200, "summary": 300}


def github_words(kind, number, repo):
    """The issue's (claimed) or the PR's (pr_opened) title and the body's `Summary:` line, read with
    `gh`. Returns (title, summary); either is None when gh is missing or the field is absent."""
    command = ["gh", "issue" if kind == "claimed" else "pr", "view", str(number), "--json", "title,body"]
    if repo:
        command += ["--repo", repo]
    try:
        reply = json.loads(subprocess.run(command, capture_output=True, text=True, timeout=60, check=True).stdout)
    except (OSError, subprocess.SubprocessError, ValueError):
        return None, None
    summary = next((line.split(":", 1)[1].strip() for line in (reply.get("body") or "").splitlines()
                    if line.strip().lower().startswith("summary:")), None)
    return (reply.get("title") or "").strip() or None, summary or None


def words(args):
    """#477: `title` and `summary` for an opening record: the flags win, else GitHub. Over the
    standard's length only warns; over what the Runtime accepts is clipped, so a long title never
    costs the record. Control characters become spaces."""
    title, summary = args.title, args.summary
    if (title is None or summary is None) and not args.no_github:
        number = args.issue if args.kind == "claimed" else args.pr
        fetched = github_words(args.kind, number, args.repo)
        title = title if title is not None else fetched[0]
        summary = summary if summary is not None else fetched[1]
    out = {}
    for key, value, standard in (("title", title, STANDARD[args.kind]), ("summary", summary, STANDARD["summary"])):
        if not value:
            continue
        value = " ".join("".join(c if c.isprintable() else " " for c in value).split())
        if len(value) > standard:
            print(f"task_record: warning: {key} is {len(value)} characters; the standard asks for {standard}", file=sys.stderr)
        if value:
            out[key] = value[:ACCEPTED[key]]
    return out


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
    p.add_argument("--closes", type=int, nargs="*", default=[],
                   help="exactly the issues the merge closed (what ci/closing-keywords.ps1 checked); none for a Refs PR")
    p.add_argument("--journeys", nargs="*", default=[])
    p.add_argument("--repo", default="stabem/GraphHelm",
                   help="owner/name; pass '' to omit it (a Runtime older than #420 refuses the field)")
    p.add_argument("--execution", default="gh-team")
    p.add_argument("--url", default="http://127.0.0.1:8793")
    p.add_argument("--token-file", default=".graphhelm/events.agent.token",
                   help="the Runtime's agent session token (never the owner's events.token)")
    p.add_argument("--title", help="claimed/pr_opened: the issue's or PR's title; default: read with gh")
    p.add_argument("--summary", help="claimed/pr_opened: one sentence; default: the body's 'Summary:' line, read with gh")
    p.add_argument("--no-github", action="store_true", help="do not call gh for the title and summary")
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
        doc.update(pr=args.pr, mergeSha=args.merge_sha, closes=args.closes,
                   merger=args.lane)
    if args.repo and args.kind in ("claimed", "pr_opened"):
        doc["repo"] = args.repo
    if args.kind in ("claimed", "pr_opened"):
        doc.update(words(args))
    return doc


def main(argv):
    args = parse(argv)
    now = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    doc = document(args, now)
    code, signal_id, reply = send(args, doc, now)
    errors = {d.get("code") for d in reply.get("diagnostics", []) if d.get("severity") == "error"}
    words = [key for key in ("title", "summary") if key in doc]
    if code == 1 and errors == {"GHCLI003_SIGNAL_INVALID"} and words:
        # #498: a Runtime built before #486 refuses these keys. The step itself matters more than
        # its words, so it is sent again without them, loudly: an old Runtime must be noticed.
        print(f"task_record: warning: the Runtime refused {signal_id} with GHCLI003; it predates task "
              f"titles (#486), so the step is sent again without title/summary. Restart it on a current build.",
              file=sys.stderr)
        code, signal_id, reply = send(args, {k: v for k, v in doc.items() if k not in words}, now)
    return report(code, signal_id, reply)


def send(args, doc, now):
    """POSTs one record; returns (0 ok / 1 refused / 2 already recorded, signal id, reply)."""
    # The id and key are the record's content without its timestamp: the same step sent again is
    # the same key (a retry), and any change (a new head, a second reviewer, a changed verdict) is
    # a new key, so a GHE003 conflict on it can only be a retry of this exact record.
    content = json.dumps({k: v for k, v in doc.items() if k != "at"}, sort_keys=True, separators=(",", ":"))
    digest = hashlib.sha256("\n".join((args.lane, args.kind, content)).encode()).hexdigest()[:16]
    signal_id = f"{args.lane}-{doc['taskId']}-{args.kind}-{digest}"
    evidence = args.comment_url or (f"https://github.com/{args.repo}/pull/{args.pr}" if args.pr
                                    else f"https://github.com/{args.repo}/issues/{args.issue}")
    body = {"signal": {"id": signal_id, "type": f"task.{args.kind}",
                       "source": {"type": "user", "id": args.lane}, "severity": "low",
                       "emittedAt": now, "evidence": [evidence],
                       "description": json.dumps(doc, separators=(",", ":"))}}
    url = f"{args.url.rstrip('/')}/v1/executions/{args.execution}/signal"
    headers = {"Content-Type": "application/json", "Idempotency-Key": signal_id,
               "X-GraphHelm-Actor": args.lane, "X-GraphHelm-Actor-Type": "agent"}
    if urllib.parse.urlsplit(url).hostname not in LOOPBACK:
        sys.exit(f"task_record: refusing {args.url}: the agent token is sent only to a loopback Runtime")
    if args.dry_run:
        print(json.dumps({"url": url, "headers": headers, "body": body}, indent=1))
        return 3, signal_id, {}
    token = Path(args.token_file).read_text(encoding="utf-8").strip()
    request = urllib.request.Request(url, data=json.dumps(body).encode(), method="POST",
                                     headers={**headers, "Authorization": f"Bearer {token}"})
    # GHE001: another writer appended between the Runtime's read and this append; the same body
    # under the same key is safe to send again.
    for _ in range(3):
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                reply = json.load(response)
        except urllib.error.HTTPError as error:
            reply = json.loads(error.read() or b"{}")
        except urllib.error.URLError as error:
            sys.exit(f"task_record: no Runtime at {args.url}: {error.reason}")
        if not any(d.get("code") == "GHE001_SEQUENCE_CONFLICT" for d in reply.get("diagnostics", [])):
            break
    ok = reply.get("ok") is True
    codes = {d.get("code") for d in reply.get("diagnostics", []) if d.get("severity") == "error"}
    if not ok and codes == {"GHE003_IDEMPOTENCY_CONFLICT"}:
        # The key is this record's content, so a conflict on it is this record already committed
        # with an earlier timestamp.
        return 2, signal_id, reply
    return (0 if ok else 1), signal_id, reply


def report(code, signal_id, reply):
    if code == 3:  # --dry-run printed the request
        return 0
    if code == 2:
        print(f"already recorded {signal_id}")
        return 0
    print(f"{'recorded' if code == 0 else 'REFUSED'} {signal_id}")
    for d in reply.get("diagnostics", []):
        if d.get("severity") == "error":
            print(f"  {d.get('code')} {d.get('path')}: {d.get('message')}")
    return code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
