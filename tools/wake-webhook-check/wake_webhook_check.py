"""Checks one lane's wake webhook setup (#2714), without ever printing a secret.

    python tools/wake-webhook-check/wake_webhook_check.py --dir <project>/.graphhelm --lane openbot-neji
    python tools/wake-webhook-check/wake_webhook_check.py --dir ... --lane openbot-neji --send

Default is a dry run: it reads `wake-webhooks.json` and `wake-secrets/<lane>.secret` exactly as the
Runtime does, says what is missing, and shows what it WOULD send (type, delivery id, header names).
`--send` makes the one real POST of a `selftest` step to that lane's webhook, signed the way the
Runtime signs, and prints only the HTTP status. The lane's routine wakes if the setup is right.
The secret value, the url's path and the signature are never printed or logged.
"""
import argparse
import datetime
import hashlib
import hmac
import json
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

PINNED_HOST = "signal.openbot.run"
LANE = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]*")


def signature(secret: str, timestamp: int, delivery_id: str, body: str) -> str:
    """`sha256=<hex HMAC-SHA256("<timestamp>.<delivery-id>.<body>", secret)>`, the OpenBot contract."""
    message = f"{timestamp}.{delivery_id}.{body}".encode()
    return "sha256=" + hmac.new(secret.encode(), message, hashlib.sha256).hexdigest()


def url_problem(url: str) -> str | None:
    """Why `url` is not an https url on the pinned host with a path, or None."""
    parts = urllib.parse.urlsplit(url)
    if parts.scheme != "https" or parts.hostname != PINNED_HOST:
        return f"must be https on {PINNED_HOST}"
    if parts.username or parts.password or parts.fragment or parts.port not in (None, 443):
        return "must have no credentials, fragment or port"
    if not parts.path.startswith("/"):
        return "must have a path"
    return None


def inspect(directory: Path, lane: str) -> tuple[list[str], str | None, str | None]:
    """(problems, url, secret) for `lane` under `directory`; the secret is returned, never printed."""
    problems: list[str] = []
    if not LANE.fullmatch(lane):
        return [f"lane '{lane}' is not a file-name-safe lane name"], None, None
    config = directory / "wake-webhooks.json"
    url = None
    try:
        urls = json.loads(config.read_text(encoding="utf-8"))
        if not isinstance(urls, dict):
            raise ValueError("not an object")
        bad = [name for name, value in urls.items() if not isinstance(value, str) or url_problem(value)]
        if bad:
            # The Runtime refuses the WHOLE file when any lane is off the pin: say which lanes.
            problems.append(f"{config.name}: lane(s) {', '.join(sorted(bad))} are not https urls on {PINNED_HOST}: "
                            "the Runtime turns the whole file off")
        url = urls.get(lane)
        if url is None:
            problems.append(f"{config.name} has no url for {lane}")
    except FileNotFoundError:
        problems.append(f"{config} does not exist")
    except (ValueError, OSError):
        problems.append(f"{config} is not a JSON object of lane -> url")
    secret_file = directory / "wake-secrets" / f"{lane}.secret"
    secret = None
    try:
        secret = secret_file.read_text(encoding="utf-8").strip()
        if not secret:
            problems.append(f"{secret_file} is empty")
            secret = None
    except OSError:
        problems.append(f"{secret_file} does not exist")
    return problems, url, secret


def body_for(lane: str, now: datetime.datetime) -> tuple[str, str]:
    stamp = now.strftime("%Y-%m-%dT%H:%M:%SZ")
    delivery_id = f"{lane}:selftest:{now.strftime('%Y%m%dT%H%M%SZ')}"
    body = json.dumps({"type": "graphhelm.step.ready", "occurredAt": stamp,
                       "data": {"lane": lane, "step": "selftest", "skill": "graphhelm-resume", "issue": 0,
                                "pr": None, "head": "selftest", "because": "selftest"}},
                      separators=(",", ":"))
    return delivery_id, body


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--dir", required=True, type=Path, help="the project's .graphhelm directory")
    parser.add_argument("--lane", required=True)
    parser.add_argument("--send", action="store_true", help="make the one real signed POST (default: dry run)")
    args = parser.parse_args(argv)
    problems, url, secret = inspect(args.dir, args.lane)
    for problem in problems:
        print("PROBLEM:", problem)
    if problems or url is None or secret is None:
        print("NOT READY:", args.lane)
        return 1
    now = datetime.datetime.now(datetime.timezone.utc)
    delivery_id, body = body_for(args.lane, now)
    print(f"ready: url on {PINNED_HOST} ({len(url)} chars), secret file ({len(secret)} chars)")
    print(f"would send: POST type=graphhelm.step.ready delivery-id={delivery_id}")
    print("headers: x-openbot-timestamp, x-openbot-delivery-id, x-openbot-signature")
    if not args.send:
        print("dry run: nothing was sent. Add --send to wake the lane once.")
        return 0
    timestamp = int(time.time())
    request = urllib.request.Request(url, data=body.encode(), method="POST", headers={
        "content-type": "application/json", "x-openbot-timestamp": str(timestamp),
        "x-openbot-delivery-id": delivery_id, "x-openbot-signature": signature(secret, timestamp, delivery_id, body)})

    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *_args, **_kwargs):
            return None

    try:
        status = urllib.request.build_opener(NoRedirect).open(request, timeout=10).status
    except urllib.error.HTTPError as error:
        status = error.code
    except (urllib.error.URLError, OSError) as error:
        print(f"FAILED: no answer ({type(error).__name__})")
        return 2
    meaning = {202: "accepted: the routine was started", 200: "duplicate delivery id", 401: "bad signature: the secret does not match this routine",
               404: "the routine is gone: wrong url", 503: "the host is busy: try again"}.get(status, "unexpected")
    print(f"HTTP {status}: {meaning}")
    return 0 if status in (200, 202) else 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
