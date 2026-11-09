#!/usr/bin/env python3
"""Answer an operator note addressed to a lane (#2684), then read the reply back.

    python tools/task-record/note_reply.py --lane <lane> --execution <exec> --url http://127.0.0.1:8793 \
        --token-file <workspace>/.graphhelm-lane.token --reply-to <note signal id> --to <who asked> \
        --text "<what you did, or why you did not; never claim unperformed work>"

It records one `operator_note` as the lane with `replyTo` set to the note, which clears the note for
that lane (LANES.md, "Notes"). The Runtime may label an informational note `rejected /
signal_not_actionable`; the record still lands, so the tool reads the journal back and reports the
sequence of the reply, or fails when it is not there. The token value is never printed.
Standard library only; `--dry-run` prints the request without sending it.
"""
import argparse
import datetime
import json
import re
import sys
import urllib.error
import urllib.parse
import urllib.request

ID = re.compile(r"[A-Za-z0-9._:-]{1,128}")
LOOPBACK = {"127.0.0.1", "localhost", "::1"}


def _call(url, token, lane, method="GET", body=None, key=None):
    if urllib.parse.urlsplit(url).hostname not in LOOPBACK:
        raise SystemExit("note_reply: the Runtime URL must be loopback")
    headers = {"Authorization": f"Bearer {token}", "X-GraphHelm-Actor": lane, "X-GraphHelm-Actor-Type": "agent"}
    if body is not None:
        headers["Content-Type"] = "application/json"
        headers["Idempotency-Key"] = key
    request = urllib.request.Request(url, data=None if body is None else json.dumps(body).encode(),
                                     method=method, headers=headers)
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def build_signal(args, now=None):
    stamp = (now or datetime.datetime.now(datetime.timezone.utc)).strftime("%Y-%m-%dT%H:%M:%SZ")
    signal = {"id": f"{args.lane}-reply-{args.reply_to}"[:128], "source": {"type": "tool", "id": args.lane},
              "type": "operator_note", "severity": "low", "replyTo": args.reply_to,
              "description": args.text, "evidence": [f"reply to note {args.reply_to}"], "emittedAt": stamp}
    if args.to:
        signal["to"] = args.to
    return signal


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("--lane", required=True)
    p.add_argument("--execution", required=True)
    p.add_argument("--url", default="http://127.0.0.1:8793")
    p.add_argument("--token-file", required=True)
    p.add_argument("--reply-to", required=True, help="the signal id of the note you answer")
    p.add_argument("--to", help="who asked (the note's sender), optional")
    p.add_argument("--text", required=True)
    p.add_argument("--dry-run", action="store_true")
    args = p.parse_args(argv)
    for value in (args.lane, args.execution, args.reply_to):
        if not ID.fullmatch(value):
            raise SystemExit("note_reply: invalid identifier")
    signal = build_signal(args)
    base = f"{args.url.rstrip('/')}/v1/executions/{urllib.parse.quote(args.execution, safe='')}"
    if args.dry_run:
        print(json.dumps({"url": base + "/signal", "signal": signal}, indent=2))
        return 0
    token = open(args.token_file, encoding="utf-8").read().strip()
    try:
        _call(base + "/signal", token, args.lane, "POST", {"signal": signal}, signal["id"])
    except urllib.error.HTTPError as error:
        if error.code != 409:  # 409: this reply was already committed
            raise SystemExit(f"note_reply: the Runtime refused the reply (HTTP {error.code})")
    head = _call(base + "/events?after=0&limit=1", token, args.lane)["data"]["head"]
    after = max(0, head - 512)
    while True:
        data = _call(f"{base}/events?after={after}&limit=512", token, args.lane)["data"]
        for event in data["events"]:
            after = event["sequence"]
            record = event.get("kind", {}).get("data", {})
            if (event.get("actor", {}).get("id") == args.lane and record.get("signalId") == signal["id"]
                    and record.get("replyTo") == args.reply_to):
                print(f"note_reply: recorded at sequence {event['sequence']} (replyTo {args.reply_to})")
                return 0
        if not data["events"] or after >= data["head"]:
            raise SystemExit("note_reply: the reply was not found on read-back")


if __name__ == "__main__":
    sys.exit(main())
