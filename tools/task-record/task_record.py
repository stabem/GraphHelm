#!/usr/bin/env python3
"""Record one `task.*` delivery step on a Runtime execution, as one lane (DELIVERY.md, "Task records").

One command per step, the actor set per command, so lanes that share one MCP registration never
need a per-lane environment or a session relaunch:

    python tools/task-record/task_record.py --lane gh-claude-2 claimed  --issue 355 --branch issue-355-x
    python tools/task-record/task_record.py --lane gh-claude-2 planned  --issue 355 --paths <card scope...> --summary "<one line>"
    python tools/task-record/task_record.py --lane gh-claude-2 released --issue 355 --claim-sequence <claim sequence> --release-reason "<why>"
    python tools/task-record/task_record.py --lane gh-claude-2 pr_opened --issue 355 --pr 357 --head <sha>
    python tools/task-record/task_record.py --lane gh-claude-2 review_assigned --issue 355 --pr 357 --head <sha> --reviewer gh-claude-6
    python tools/task-record/task_record.py --lane gh-claude-6 review_verdict --issue 355 --pr 357 --head <sha> --verdict APPROVE --comment-url <url>
    python tools/task-record/task_record.py --lane gh-claude-6 merged --issue 355 --pr 357 --merge-sha <sha>
    python tools/task-record/task_record.py --lane gh-claude-2 critic_verdict --issue 355 --round 1 --score 6 --design-ref <path> --reason "<why>"

It POSTs `/v1/executions/<execution>/signal` with the LANE'S OWN credential (#1148):
`<events>.agent.<lane>.token`, resolved beside `--token-file` (the shared `<events>.agent.token`
path, whose value is never sent for a task record), and `X-GraphHelm-Actor: <lane>`. The owner or
the orchestrator mints that file with `graphhelm tasks credential --lane <lane> --execution <exec>
--token-file <events.token>`; the Runtime verifies the lane by recomputing the credential and refuses
a `task.*` record under the shared token or in another lane's name (GHCLI038_ACTOR_MISMATCH).
Standard library only. `--dry-run` prints the request instead of sending it.
"""
import argparse
import datetime
import hashlib
import json
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
import os

# Owner rule: children run hidden in the calling terminal, never in a new console window.
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0

LOOPBACK = ("127.0.0.1", "localhost", "::1")
KINDS = ("claimed", "planned", "released", "heartbeat", "pr_opened", "review_assigned", "review_verdict", "merged", "closed", "critic_verdict")
# The default revision is the step's position in the delivery. `planned` (#480) came later and is
# the claim's companion, so the older steps keep their numbers (and their records' keys).
REVISIONS = {"claimed": 1, "planned": 1, "released": 2, "heartbeat": 1, "pr_opened": 2, "review_assigned": 3, "review_verdict": 4, "merged": 5, "closed": 6, "critic_verdict": 1}
CLASSES = ("docs", "code", "user_visible", "invariant")
PROOFS = ("none", "tests", "journey", "both")
# #477: what the naming standard asks (DELIVERY.md "Naming") and what the Runtime accepts.
STANDARD = {"claimed": 50, "pr_opened": 60, "summary": 100}
ACCEPTED = {"title": 200, "summary": 300}
MAX_HISTORY_EVENTS = 10_000
MAX_HISTORY_PAGES = MAX_HISTORY_EVENTS // 1000
MAX_HEARTBEAT_EVIDENCE_READS = 256
MAX_RESPONSE_BYTES = 2 * 1024 * 1024
MAX_TOTAL_EVIDENCE_BYTES = 2 * 1024 * 1024


def credential_file(token_file, lane):
    """The lane's credential beside `--token-file` (#1148): `events.agent.token` or `events.token`
    → `events.agent.<lane>.token`, the path `graphhelm tasks credential --lane <lane>` writes."""
    path = Path(token_file)
    stem = path.name
    for suffix in (".agent.token", ".token"):
        if stem.endswith(suffix):
            stem = stem[: -len(suffix)]
            break
    return path.with_name(f"{stem}.agent.{lane}.token")


def read_credential(args):
    """The lane's credential when its file exists; otherwise the shared token (#1148 rollout: a
    Runtime without `--require-lane-credential` still accepts it), with a note on stderr naming the
    mint command, so the missing file is visible without stopping the record."""
    path = credential_file(args.token_file, args.lane)
    try:
        value = path.read_text(encoding="utf-8").strip()
    except OSError:
        value = ""
    if value and "\n" not in value:
        return value
    print(f"task_record: no credential for lane {args.lane} at {path}; sending the shared agent token "
          f"(a Runtime with --require-lane-credential refuses it); the owner mints it with: "
          f"graphhelm tasks credential --lane {args.lane} --execution {args.execution} --token-file <events.token>",
          file=sys.stderr)
    try:
        token = Path(args.token_file).read_text(encoding="utf-8").strip()
    except OSError as error:
        sys.exit(f"task_record: cannot read the agent token ({error}); no record sent")
    if not token or "\n" in token:
        sys.exit("task_record: agent token is empty; no record sent")
    return token


def github_words(kind, number, repo):
    """The issue's (claimed) or the PR's (pr_opened) title and the body's `Summary:` line, read with
    `gh`. Returns (title, summary); either is None when gh is missing or the field is absent."""
    command = ["gh", "issue" if kind == "claimed" else "pr", "view", str(number), "--json", "title,body"]
    if repo:
        command += ["--repo", repo]
    try:
        # gh writes UTF-8; the platform default (cp1252 on Windows) cannot decode every byte of it (#526).
        reply = json.loads(subprocess.run(command, capture_output=True, text=True, encoding="utf-8", timeout=60, check=True, creationflags=NO_WINDOW).stdout)
    except (OSError, subprocess.SubprocessError, ValueError):
        return None, None
    summary = next((line.split(":", 1)[1].strip() for line in (reply.get("body") or "").splitlines()
                    if line.strip().lower().startswith("summary:")), None)
    return (reply.get("title") or "").strip() or None, summary or None


def merged_on_github(pr, sha, repo):
    """Refuse a merge record unless GitHub confirms its state and merge commit."""
    if not re.fullmatch(r"[0-9a-fA-F]{7,40}", sha):
        sys.exit("task_record: --merge-sha must be a full SHA or a prefix of at least 7 hex characters")
    command = ["gh", "pr", "view", str(pr), "--json", "state,mergeCommit"]
    if repo:
        command += ["--repo", repo]
    # Three reads, each with a 60 s subprocess timeout; only eventual state/commit absence retries.
    for attempt in range(3):
        try:
            reply = json.loads(subprocess.run(command, capture_output=True, text=True,
                                               encoding="utf-8", timeout=60, check=True, creationflags=NO_WINDOW).stdout)
        except (OSError, subprocess.SubprocessError, ValueError):
            sys.exit(f"task_record: could not confirm the merge for PR {pr}")
        if not isinstance(reply, dict):
            sys.exit(f"task_record: could not confirm the merge for PR {pr}: invalid GitHub response")
        state = reply.get("state")
        commit = reply.get("mergeCommit")
        oid = commit.get("oid") if isinstance(commit, dict) else None
        if state == "MERGED" and isinstance(oid, str) and re.fullmatch(r"[0-9a-fA-F]{40}", oid):
            if oid.lower().startswith(sha.lower()):
                return
            sys.exit(f"task_record: --merge-sha {sha} does not match PR {pr} merge commit {oid}")
        if attempt < 2 and (state == "OPEN" or commit is None):
            time.sleep(2)
            continue
        sys.exit(f"task_record: could not confirm the merge for PR {pr}: state={state}, mergeCommit={oid}")


def closed_on_github(pr, repo):
    """Refuse a close record unless GitHub confirms the PR is closed and unmerged."""
    command = ["gh", "pr", "view", str(pr), "--json", "state,mergedAt"]
    if repo:
        command += ["--repo", repo]
    for attempt in range(3):
        try:
            reply = json.loads(subprocess.run(command, capture_output=True, text=True,
                                               encoding="utf-8", timeout=60, check=True, creationflags=NO_WINDOW).stdout)
        except (OSError, subprocess.SubprocessError, ValueError):
            sys.exit(f"task_record: could not confirm the closure for PR {pr}")
        if not isinstance(reply, dict):
            sys.exit(f"task_record: could not confirm the closure for PR {pr}: invalid GitHub response")
        state, merged_at = reply.get("state"), reply.get("mergedAt")
        if state == "CLOSED" and "mergedAt" in reply and merged_at is None:
            return
        if attempt < 2 and state == "OPEN":
            time.sleep(2)
            continue
        sys.exit(f"task_record: could not confirm an unmerged closed PR {pr}: state={state}, mergedAt={merged_at}")


def refuse_merged_pr_record(pr, repo):
    """Refuse a follow-up opening record when GitHub says its PR is already merged."""
    command = ["gh", "pr", "view", str(pr), "--json", "state"]
    if repo:
        command += ["--repo", repo]
    try:
        reply = json.loads(subprocess.run(command, capture_output=True, text=True,
                                          encoding="utf-8", timeout=60, check=True, creationflags=NO_WINDOW).stdout)
    except (OSError, subprocess.SubprocessError, ValueError):
        sys.exit(f"task_record: could not confirm the state of PR {pr}")
    if not isinstance(reply, dict) or not isinstance(reply.get("state"), str):
        sys.exit(f"task_record: could not confirm the state of PR {pr}: invalid GitHub response")
    if reply["state"] == "MERGED":
        sys.exit(f"task_record: PR #{pr} is already merged")


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
    p.add_argument("--revision", type=int, help="defaults to the step's position: claimed 1 (planned 1) ... merged 5")
    p.add_argument("--branch")
    p.add_argument("--claim-sequence", type=int, help="released: exact sequence of your latest still-open pre-PR task.claimed record")
    p.add_argument("--release-reason", help="released: why this pre-PR claim is being released")
    p.add_argument("--parent", type=int, help="claimed: the issue whose work turned this task up (#514)")
    p.add_argument("--assigned-by", help="claimed: who ordered the work, as reported by this lane")
    p.add_argument("--pr", type=int)
    p.add_argument("--head", help="the PR head sha the step is about")
    p.add_argument("--reviewer")
    p.add_argument("--ordinal", type=int, default=1, choices=(1, 2))
    p.add_argument("--verdict", choices=("APPROVE", "APPROVE-WITH-RISK", "BLOCK"))
    p.add_argument("--comment-url")
    p.add_argument("--merge-sha")
    p.add_argument("--by", type=int, help="closed: the superseding PR number")
    p.add_argument("--closes", type=int, nargs="*", default=[],
                   help="exactly the issues the merge closed (what ci/closing-keywords.ps1 checked); none for a Refs PR")
    p.add_argument("--journeys", nargs="*", default=[],
                   help="claimed/pr_opened: the journey contract ids the issue serves (#577)")
    p.add_argument("--journeys-dir", default=str(JOURNEYS_DIR),
                   help="where <id>.journey.yaml contracts live; default: the repository's .graphhelm/journeys")
    p.add_argument("--round", type=int, help="critic_verdict: which round of the design critic this is, from 1")
    p.add_argument("--score", type=int, help="critic_verdict: the critic's grade, 0 to 10")
    p.add_argument("--pass-score", type=int, default=8)
    p.add_argument("--max-rounds", type=int, default=3)
    p.add_argument("--design-ref", help="critic_verdict: the design that was graded (a path or a URL)")
    p.add_argument("--reason", action="append", default=[], help="critic_verdict: one reason; repeat for more (1 to 8)")
    # planned (#480): the keel plan's fields, from `graphhelm keel plan` on --paths, from a saved
    # `keel plan --json` reply (--plan-file), or given one by one.
    p.add_argument("--paths", nargs="*", default=[], help="planned: the task's paths (its card scope); runs `graphhelm keel plan`")
    p.add_argument("--promise", default="", help="planned: the task's promise, passed to `keel plan`")
    p.add_argument("--plan-file", help="planned: a saved `graphhelm --json keel plan` reply to copy the fields from")
    p.add_argument("--graphhelm", default="graphhelm", help="planned: the graphhelm binary that runs `keel plan`")
    p.add_argument("--plan-repo", default=".", help="planned: the repository `keel plan` reads")
    p.add_argument("--classes", nargs="*", choices=CLASSES)
    p.add_argument("--reviews", type=int)
    p.add_argument("--proof", choices=PROOFS)
    p.add_argument("--critic-mode", choices=("none", "design"))
    p.add_argument("--repo", default="stabem/GraphHelm",
                   help="owner/name; pass '' to omit it (a Runtime older than #420 refuses the field)")
    p.add_argument("--execution", default="gh-team")
    p.add_argument("--url", default="http://127.0.0.1:8793")
    p.add_argument("--token-file", default=".graphhelm/events.agent.token",
                   help="the Runtime's agent session token path; the lane's own credential is read beside it, "
                        "at <events>.agent.<lane>.token (#1148), and the shared token's value is never sent")
    p.add_argument("--title", help="claimed/pr_opened: the issue's or PR's title; default: read with gh")
    p.add_argument("--summary", help="claimed/pr_opened: one sentence; default: the body's 'Summary:' line, read with gh."
                   " planned: the plan in one line (at most 300 characters), required")
    p.add_argument("--no-github", action="store_true", help="do not call gh for the title and summary")
    p.add_argument("--dry-run", action="store_true")
    return p.parse_args(argv)


JOURNEYS_DIR = Path(__file__).resolve().parents[2] / ".graphhelm" / "journeys"


def known_journeys(args):
    """#577: a journey id is the stem of an existing contract; an unknown one stops the step before any send."""
    if not args.journeys:
        return []
    folder = Path(args.journeys_dir)
    known = sorted(f.name[:-len(".journey.yaml")] for f in folder.glob("*.journey.yaml")) if folder.is_dir() else []
    unknown = [j for j in args.journeys if j not in known]
    if unknown:
        sys.exit(f"task_record: unknown journey {', '.join(unknown)}; no {folder}/<id>.journey.yaml. "
                 f"Known: {', '.join(known) or '(none)'}")
    return list(args.journeys)


def need(args, *names):
    missing = [n for n in names if getattr(args, n.replace("-", "_")) in (None, "", [])]
    if missing:
        sys.exit(f"task_record: {args.kind} needs --" + ", --".join(missing))


# #602: what a plan records when there is nothing to plan from yet (just claimed, no paths) or
# `keel plan` cannot run. Said on stderr each time; never a reason to skip the record.
DEFAULT_PLAN = {"classes": ["code"], "reviews": 1, "proof": "tests",
                "critic": {"mode": "none", "passScore": 8, "maxRounds": 3}}


def git_paths(repo):
    """#602: the task's paths when none are given: the branch's diff against `origin/main` plus
    uncommitted changes, in `repo`. Empty when git cannot answer."""
    def nul_paths(*command):
        try:
            run = subprocess.run(["git", "-C", repo, *command], capture_output=True, timeout=60, creationflags=NO_WINDOW)
        except (OSError, subprocess.TimeoutExpired):
            return []
        if run.returncode != 0:
            return []
        return [path.decode("utf-8", errors="surrogateescape")
                for path in run.stdout.split(b"\0") if path]

    paths = nul_paths("diff", "--name-only", "-z", "origin/main...HEAD")
    try:
        run = subprocess.run(["git", "-C", repo, "status", "--porcelain=v1", "-z", "--untracked-files=all"],
                             capture_output=True, timeout=60, creationflags=NO_WINDOW)
    except (OSError, subprocess.TimeoutExpired):
        run = None
    if run is not None and run.returncode == 0:
        records = run.stdout.split(b"\0")
        index = 0
        while index < len(records):
            record = records[index]
            index += 1
            if len(record) < 4:
                continue
            status, path = record[:2], record[3:]
            if path:
                paths.append(path.decode("utf-8", errors="surrogateescape"))
            # In NUL porcelain output the destination comes first; the next field is its source.
            if b"R" in status or b"C" in status:
                index += 1
    return sorted(dict.fromkeys(path for path in paths if path))


def keel_plan(args):
    """The `graphhelm-task-plan-v1` record the step copies its fields from, or `None` (with the
    reason on stderr) when there is nothing to plan from or `keel plan` cannot run."""
    if args.plan_file:
        reply = json.loads(Path(args.plan_file).read_text(encoding="utf-8"))
    else:
        paths = args.paths or git_paths(args.plan_repo)
        if not paths:
            print("task_record: planned: no --paths and no changed files yet; recording the default plan", file=sys.stderr)
            return None
        command = [args.graphhelm, "--json", "keel", "plan", "--task", f"issue-{args.issue}",
                   "--repo", args.plan_repo, "--promise", args.promise, "--paths", *paths]
        try:
            run = subprocess.run(command, capture_output=True, text=True, timeout=120, creationflags=NO_WINDOW)
            reply = json.loads(run.stdout)
        except (OSError, subprocess.TimeoutExpired, json.JSONDecodeError) as error:
            print(f"task_record: planned: `keel plan` did not answer ({error}); recording the default plan", file=sys.stderr)
            return None
        if reply.get("ok") is not True:
            codes = ", ".join(d.get("code", "?") for d in reply.get("diagnostics", []))
            print(f"task_record: planned: `keel plan` refused ({codes}); recording the default plan", file=sys.stderr)
            return None
    plan = reply.get("data", {}).get("plan", reply)
    if plan.get("schema") != "graphhelm-task-plan-v1":
        sys.exit("task_record: the plan is not a graphhelm-task-plan-v1 record")
    return plan


def planned_fields(args):
    # Explicit fields win, each replacing only its own part (#604 review): a partial set still plans
    # the rest. Only when all four are given is there nothing left to plan.
    complete = args.classes and args.reviews is not None and args.proof and args.critic_mode
    plan = None if complete and not args.plan_file else keel_plan(args)
    if plan is not None:
        # A plan recorded before #467 carries no critic: it asked for none.
        fields = {"classes": plan["classes"], "reviews": plan["reviews"], "proof": plan["proof"],
                  "critic": plan.get("critic") or DEFAULT_PLAN["critic"]}
    else:
        fields = json.loads(json.dumps(DEFAULT_PLAN))
    if args.classes:
        fields["classes"] = list(dict.fromkeys(args.classes))
    if args.reviews is not None:
        fields["reviews"] = args.reviews
    if args.proof:
        fields["proof"] = args.proof
    if args.critic_mode:
        fields["critic"] = {"mode": args.critic_mode, "passScore": args.pass_score, "maxRounds": args.max_rounds}
    return fields


def document(args, now):
    doc = {"schema": "graphhelm-task-event-v1", "taskId": f"issue-{args.issue}",
           "revision": args.revision or REVISIONS[args.kind], "at": now}
    if args.kind == "heartbeat":
        doc.update(lane=args.lane)
    elif args.kind == "claimed":
        need(args, "branch")
        doc.update(issue=args.issue, lane=args.lane, branch=args.branch)
        if args.assigned_by is not None:
            if not args.assigned_by or len(args.assigned_by) > 128 or not args.assigned_by.isprintable() or args.assigned_by == args.lane:
                sys.exit("task_record: --assigned-by must name someone other than the lane in 1-128 printable characters")
            doc["assignedBy"] = args.assigned_by
        if args.parent:
            doc["parent"] = args.parent
        journeys = known_journeys(args)
        if journeys:
            doc["journeys"] = journeys
    elif args.kind == "released":
        need(args, "claim-sequence", "release-reason")
        if args.claim_sequence <= 0:
            sys.exit("task_record: --claim-sequence must be positive")
        if not args.release_reason or len(args.release_reason) > 500 or not args.release_reason.isprintable():
            sys.exit("task_record: --release-reason must be nonempty printable text of at most 500 characters")
        doc.update(lane=args.lane, claimSequence=args.claim_sequence, reason=args.release_reason)
    elif args.kind == "planned":
        need(args, "summary")
        if len(args.summary) > 300 or not args.summary.isprintable():
            sys.exit("task_record: --summary is one line of at most 300 characters")
        doc.update(lane=args.lane, **planned_fields(args), summary=args.summary)
    elif args.kind == "pr_opened":
        # #508: the reviewer is part of opening the PR, so the Review step is never drawn unnamed.
        need(args, "pr", "head", "reviewer")
        doc.update(pr=args.pr, headSha=args.head, journeys=known_journeys(args), lane=args.lane)
    elif args.kind == "review_assigned":
        need(args, "pr", "head", "reviewer")
        doc.update(pr=args.pr, headSha=args.head, reviewer=args.reviewer, ordinal=args.ordinal)
    elif args.kind == "review_verdict":
        need(args, "pr", "head", "verdict", "comment-url")
        doc.update(pr=args.pr, headSha=args.head, reviewer=args.lane, verdict=args.verdict,
                   commentUrl=args.comment_url)
    elif args.kind == "closed":
        need(args, "pr", "reason", "by")
        if args.pr < 1 or args.by < 1:
            sys.exit("task_record: closed needs positive PR numbers for --pr and --by")
        if len(args.reason) != 1 or args.reason[0] not in ("superseded", "abandoned"):
            sys.exit("task_record: closed needs exactly one --reason superseded|abandoned")
        doc.update(pr=args.pr, reason=args.reason[0], by=args.by)
    elif args.kind == "critic_verdict":
        # #467: the verdict is not an argument. It follows from the score and the round, the way
        # the Runtime checks it, so running out of rounds can never be sent as a pass.
        need(args, "round", "score", "design-ref", "reason")
        verdict = ("pass" if args.score >= args.pass_score
                   else "revise" if args.round < args.max_rounds else "exhausted")
        doc.update(round=args.round, score=args.score, passScore=args.pass_score, maxRounds=args.max_rounds,
                   verdict=verdict, designRef=args.design_ref, lane=args.lane, reasons=args.reason)
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
    if args.kind == "heartbeat":
        reject_heartbeat_overrides(args)
        history = read_execution_history(args)
        doc = heartbeat_document(args, history, now)
    else:
        doc = document(args, now)
    if args.kind in ("pr_opened", "review_assigned") and not args.no_github:
        refuse_merged_pr_record(args.pr, args.repo)
    if args.kind == "merged" and not args.dry_run:
        if args.no_github:
            sys.exit("task_record: --no-github is refused for merged unless --dry-run is set")
        merged_on_github(args.pr, args.merge_sha, args.repo)
    if args.kind == "closed" and not args.dry_run:
        if args.no_github:
            sys.exit("task_record: --no-github is refused for closed unless --dry-run is set")
        closed_on_github(args.pr, args.repo)
    code, signal_id, reply = send_opening(args, doc, now)
    if args.kind == "heartbeat" and args.dry_run:
        print("task_record: dry-run only; Runtime admission and append were not observed", file=sys.stderr)
    outcome = report(code, signal_id, reply)
    if args.kind == "pr_opened" and outcome == 0:
        # #508: the assignment rides on the same call, the same head and PR, recorded as this lane.
        assigned = argparse.Namespace(**{**vars(args), "kind": "review_assigned", "revision": None})
        code, signal_id, reply = send(assigned, document(assigned, now), now)
        return report(code, signal_id, reply)
    return outcome


def reject_heartbeat_overrides(args):
    """Keep heartbeat payloads to the canonical lane/task activity fields."""
    forbidden = {
        "revision": args.revision, "branch": args.branch, "claim-sequence": args.claim_sequence,
        "release-reason": args.release_reason, "parent": args.parent, "assigned-by": args.assigned_by,
        "pr": args.pr, "head": args.head, "reviewer": args.reviewer, "ordinal": args.ordinal if args.ordinal != 1 else None,
        "verdict": args.verdict, "comment-url": args.comment_url, "merge-sha": args.merge_sha,
        "by": args.by, "closes": args.closes, "journeys": args.journeys, "round": args.round,
        "score": args.score, "design-ref": args.design_ref, "reason": args.reason,
        "pass-score": args.pass_score if args.pass_score != 8 else None,
        "max-rounds": args.max_rounds if args.max_rounds != 3 else None,
        "classes": args.classes, "reviews": args.reviews, "proof": args.proof,
        "critic-mode": args.critic_mode, "plan-file": args.plan_file, "promise": args.promise or None,
        "paths": args.paths, "title": args.title, "summary": args.summary,
    }
    supplied = [f"--{name}" for name, value in forbidden.items() if value not in (None, "", [])]
    if supplied:
        sys.exit("task_record: heartbeat refuses " + ", ".join(supplied))


def _runtime_json(args, path, token, *, max_bytes=MAX_RESPONSE_BYTES):
    url = f"{args.url.rstrip('/')}/v1/executions/{urllib.parse.quote(args.execution, safe='')}{path}"
    if urllib.parse.urlsplit(url).hostname not in LOOPBACK:
        sys.exit(f"task_record: refusing {args.url}: the agent token is sent only to a loopback Runtime")
    request = urllib.request.Request(url, method="GET", headers={"Authorization": f"Bearer {token}"})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            raw = response.read(max_bytes + 1)
    except (urllib.error.URLError, urllib.error.HTTPError, OSError) as error:
        sys.exit(f"task_record: heartbeat history is unavailable; no record sent ({error})")
    if len(raw) > max_bytes:
        sys.exit("task_record: heartbeat history response exceeded its byte bound; no record sent")
    try:
        reply = json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError):
        sys.exit("task_record: heartbeat history response is invalid; no record sent")
    if not isinstance(reply, dict) or reply.get("ok") is not True or not isinstance(reply.get("data"), dict):
        sys.exit("task_record: heartbeat history response is incomplete; no record sent")
    return reply["data"], len(raw)


def read_execution_history(args):
    """Read a complete bounded event tail and its referenced task evidence from the Runtime."""
    token = read_credential(args)

    events, _ = _runtime_json(args, "/events?after=0&limit=1000", token)
    head = events.get("head")
    first_page = events.get("events")
    if not isinstance(head, int) or isinstance(head, bool) or head < 0 or not isinstance(first_page, list):
        sys.exit("task_record: heartbeat event history is malformed; no record sent")
    if head > MAX_HISTORY_EVENTS:
        sys.exit("task_record: heartbeat event history exceeds its event bound; no record sent")
    history = list(first_page)
    after = history[-1].get("sequence") if history and isinstance(history[-1], dict) else 0
    if len(history) > 1000 or len(history) != min(head, 1000):
        sys.exit("task_record: heartbeat event history is incomplete; no record sent")
    if any(not isinstance(event, dict) or event.get("sequence") != expected
           for expected, event in enumerate(history, start=1)):
        sys.exit("task_record: heartbeat event history has a sequence gap; no record sent")
    for _ in range(MAX_HISTORY_PAGES - 1):
        if after == head:
            break
        if not isinstance(after, int) or after <= 0:
            sys.exit("task_record: heartbeat event history is incomplete; no record sent")
        page, _ = _runtime_json(args, f"/events?after={after}&limit=1000", token)
        if page.get("head") != head or not isinstance(page.get("events"), list) or not page["events"]:
            sys.exit("task_record: heartbeat event history changed or is incomplete; no record sent")
        next_page = page["events"]
        if len(history) + len(next_page) > MAX_HISTORY_EVENTS:
            sys.exit("task_record: heartbeat event history exceeds its event bound; no record sent")
        for expected, event in enumerate(next_page, start=after + 1):
            if not isinstance(event, dict) or event.get("sequence") != expected:
                sys.exit("task_record: heartbeat event history has a sequence gap; no record sent")
        history.extend(next_page)
        after = next_page[-1]["sequence"]
    if after != head or len(history) != head:
        sys.exit("task_record: heartbeat event history is incomplete; no record sent")

    relevant = []
    for event in history:
        if (not isinstance(event, dict) or not isinstance(event.get("kind"), dict)
                or not isinstance(event.get("actor"), dict)
                or not isinstance(event["actor"].get("id"), str)):
            sys.exit("task_record: heartbeat event history is malformed; no record sent")
        if event.get("actor", {}).get("id") != args.lane:
            continue
        kind = event["kind"].get("data", {}).get("kind") if event["kind"].get("type") == "signal_recorded" else None
        if kind in ("task.claimed", "task.pr_opened", "task.released", "task.heartbeat",
                    "task.merged", "task.closed"):
            relevant.append((event, kind))
    if len(relevant) > MAX_HEARTBEAT_EVIDENCE_READS:
        sys.exit("task_record: heartbeat history requires too many evidence reads; no record sent")
    evidence_docs = {}
    total_bytes = 0
    for event, kind in relevant:
        refs = event.get("evidenceRefs")
        if not isinstance(refs, list) or not refs or not isinstance(refs[0], dict) or not isinstance(refs[0].get("evidenceId"), str):
            sys.exit("task_record: heartbeat task evidence reference is missing; no record sent")
        evidence_id = refs[0]["evidenceId"]
        if evidence_id not in evidence_docs:
            data, size = _runtime_json(args, f"/evidence/{urllib.parse.quote(evidence_id, safe='')}", token)
            total_bytes += size
            if total_bytes > MAX_TOTAL_EVIDENCE_BYTES:
                sys.exit("task_record: heartbeat evidence exceeds its total byte bound; no record sent")
            content = data.get("content")
            try:
                evidence_docs[evidence_id] = json.loads(content) if isinstance(content, str) else None
            except json.JSONDecodeError:
                evidence_docs[evidence_id] = None
            if not isinstance(evidence_docs[evidence_id], dict):
                sys.exit("task_record: heartbeat task evidence is unreadable; no record sent")
    return [(event, kind, evidence_docs[event["evidenceRefs"][0]["evidenceId"]]) for event, kind in relevant]


def heartbeat_document(args, history, now):
    """The heartbeat record for the lane's open pre-PR claim. The 300 s cooldown is not checked
    here (#1147): the Runtime owns that rule (`validate_heartbeat`) and its refusal is the answer.
    Earlier heartbeats still count toward the revision."""
    task_id = f"issue-{args.issue}"
    open_claims = []
    opened_prs = []
    max_revision = 0
    for event, kind, envelope in history:
        if envelope.get("type") != kind or envelope.get("source", {}).get("id") != args.lane:
            sys.exit("task_record: heartbeat task history identity is invalid; no record sent")
        description = envelope.get("description")
        try:
            record = json.loads(description) if isinstance(description, str) else None
        except json.JSONDecodeError:
            record = None
        if not isinstance(record, dict) or record.get("schema") != "graphhelm-task-event-v1":
            sys.exit("task_record: heartbeat task history description is invalid; no record sent")
        if (not isinstance(record.get("taskId"), str) or not isinstance(record.get("lane"), str)
                or record["lane"] != args.lane):
            sys.exit("task_record: heartbeat task history identity is invalid; no record sent")
        if record.get("taskId") != task_id:
            continue
        revision = record.get("revision")
        if not isinstance(revision, int) or isinstance(revision, bool) or revision < 1:
            sys.exit("task_record: heartbeat task history revision is invalid; no record sent")
        max_revision = max(max_revision, revision)
        if kind == "task.claimed":
            open_claims.append(event["sequence"])
        elif kind == "task.pr_opened":
            pr = record.get("pr")
            if not isinstance(pr, int) or isinstance(pr, bool) or pr < 1:
                sys.exit("task_record: heartbeat PR history is invalid; no record sent")
            if pr not in opened_prs:
                opened_prs.append(pr)
                if open_claims:
                    open_claims.pop(0)
        elif kind == "task.released":
            sequence = record.get("claimSequence")
            if not isinstance(sequence, int) or isinstance(sequence, bool):
                sys.exit("task_record: heartbeat release history is invalid; no record sent")
            open_claims = [claim for claim in open_claims if claim != sequence]
        elif kind in ("task.merged", "task.closed"):
            pr = record.get("pr")
            if not isinstance(pr, int) or isinstance(pr, bool) or pr < 1:
                sys.exit("task_record: heartbeat terminal PR history is invalid; no record sent")
            if opened_prs:
                if pr not in opened_prs:
                    continue
                opened_prs.remove(pr)
            if open_claims:
                # A terminal record names the task's PR. If PR-open history is incomplete,
                # still fail closed for this task rather than heartbeat a terminal claim.
                open_claims.pop(0)
    if not open_claims:
        sys.exit("task_record: no open pre-PR claim for this lane/task; heartbeat not sent")
    return {"schema": "graphhelm-task-event-v1", "taskId": task_id,
            "revision": max_revision + 1, "at": now, "lane": args.lane}


def send_opening(args, doc, now):
    code, signal_id, reply = send(args, doc, now)
    errors = {d.get("code") for d in reply.get("diagnostics", []) if d.get("severity") == "error"}
    words = [key for key in ("title", "summary") if key in doc]
    if code == 1 and errors == {"GHCLI003_SIGNAL_INVALID"} and words:
        # #498: a Runtime built before #486 refuses these keys. The step itself matters more than
        # its words, so it is sent again without them, loudly: an old Runtime must be noticed.
        print(f"task_record: warning: the Runtime refused {signal_id} with GHCLI003. It may predate task "
              f"titles (#486), so the step is sent again without title/summary; if that lands, restart the "
              f"Runtime on a current build. GHCLI003 also covers other defects, so a second refusal is reported as is.",
              file=sys.stderr)
        code, signal_id, reply = send(args, {k: v for k, v in doc.items() if k not in words}, now)
    return code, signal_id, reply


def send(args, doc, now):
    """POSTs one record; returns (0 ok / 1 refused / 2 already recorded, signal id, reply)."""
    # The id and key are the record's content without its timestamp: the same step sent again is
    # the same key (a retry), and any change (a new head, a second reviewer, a changed verdict) is
    # a new key, so a GHE003 conflict on it can only be a retry of this exact record.
    content = json.dumps({k: v for k, v in doc.items() if k != "at"}, sort_keys=True, separators=(",", ":"))
    digest = hashlib.sha256("\n".join((args.lane, args.kind, content)).encode()).hexdigest()[:16]
    signal_id = f"{args.lane}-{doc['taskId']}-{args.kind}-{digest}"
    if len(signal_id) > 64:
        # Keep accepted keys stable; the digest still covers the full lane, kind and record.
        signal_id = f"{signal_id[:47]}-{digest}"
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
    token = read_credential(args)
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
