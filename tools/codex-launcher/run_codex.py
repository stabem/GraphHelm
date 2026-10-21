"""Admit a pre-PR implementation claim before starting a bounded Codex process."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
BINDING = ("execution", "lane", "issue", "branch", "repo", "claim_sequence")


def executable(name):
    found = shutil.which(name)
    if found is None:
        raise ValueError(f"executable unavailable: {name}")
    return found


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("lane")
    parser.add_argument("tag")
    parser.add_argument("prompt", type=Path)
    for flag in ("execution", "issue", "branch", "repo", "claim-sequence", "url", "token-file"):
        parser.add_argument(f"--{flag}", required=True)
    parser.add_argument("--result-file", type=Path, required=True)
    parser.add_argument("--graphhelm", default="graphhelm")
    parser.add_argument("--codex", default="codex")
    parser.add_argument("--timeout-seconds", type=int, default=3600)
    args = parser.parse_args(argv)
    try:
        if args.timeout_seconds <= 0 or not args.tag or not args.prompt.is_file():
            raise ValueError("a positive timeout, run tag and readable prompt file are required")
        graphhelm, codex = executable(args.graphhelm), executable(args.codex)
        if os.name == "nt":
            # Windows may run a discovered batch shim through cmd even with shell=False.
            # Refuse shell syntax instead of attempting a second quoting language.
            values = [str(value) for value in vars(args).values()] + [graphhelm, codex]
            if any(any(char in value for char in "\"%!&|<>^\r\n") for value in values):
                raise ValueError("Windows launcher arguments must not contain shell metacharacters")
            if Path(graphhelm).suffix.lower() in {".cmd", ".bat"}:
                raise ValueError("GraphHelm admission requires a native executable, not a batch shim")
        admission = [graphhelm, "--json", "tasks", "admit"]
        for field in BINDING:
            admission += ["--" + field.replace("_", "-"), str(getattr(args, field))]
        admission += ["--url", args.url, "--token-file", args.token_file]
        check = subprocess.run(admission, capture_output=True, text=True, timeout=45, creationflags=NO_WINDOW)
        if check.returncode != 0:
            # Map only known diagnostic codes to local guidance; never echo remote prose,
            # arbitrary stdout/stderr, token paths or credential values.
            guidance = {
                "GHCLI041_TASK_ADMISSION_REFUSED": "recheck the current own-lane pre-PR claim and exact binding",
                "GHCLI007_SERVE_UNAUTHORIZED": "restore the existing authorized read binding",
                "GHCLI016_DRIVER_FAILURE": "check the Runtime read binding and connectivity before retrying admission",
            }
            try:
                failure = json.loads(check.stdout)
                diagnostics = failure.get("diagnostics", []) if isinstance(failure, dict) else []
                code = next((item.get("code") for item in diagnostics if isinstance(item, dict)
                             and item.get("code") in guidance), None)
            except (ValueError, TypeError):
                code = None
            detail = f"{code}: {guidance[code]}" if code else "task admission refused; check the existing Runtime and claim binding"
            print(f"launcher blocked: {detail}", file=sys.stderr)
            return 2
        reply = json.loads(check.stdout)
        data = reply.get("data")
        expected = {"executionId": args.execution, "lane": args.lane, "issue": int(args.issue),
                    "branch": args.branch, "repo": args.repo, "claimSequence": int(args.claim_sequence)}
        if (reply.get("ok") is not True or reply.get("command") != "tasks.admit"
                or not isinstance(data, dict) or data.get("admitted") is not True
                or any(data.get(key) != value for key, value in expected.items())):
            raise ValueError("task admission receipt does not match the requested binding")
        # The receipt is a current read, not a process reservation or a worker-start claim.
        with args.prompt.open("rb") as prompt:
            try:
                child = subprocess.run([codex, "exec", "-o", str(args.result_file), "-"], stdin=prompt,
                                       timeout=args.timeout_seconds, creationflags=NO_WINDOW)
                code = child.returncode
            except subprocess.TimeoutExpired:
                print("launcher: Codex process exceeded its deadline", file=sys.stderr)
                code = 124
        try:
            ended = subprocess.run([graphhelm, "--json", "tasks", "run-ended", "--lane", args.lane,
                            "--execution", args.execution, "--url", args.url, "--exit", str(code),
                            "--run-id", args.tag, "--result-file", str(args.result_file),
                            "--token-file", args.token_file], capture_output=True, text=True, timeout=45,
                                   creationflags=NO_WINDOW)
            try:
                receipt = json.loads(ended.stdout)
                data = receipt.get("data") if isinstance(receipt, dict) else None
                recorded = (ended.returncode == 0 and isinstance(receipt, dict)
                            and receipt.get("ok") is True and receipt.get("command") == "tasks.run-ended"
                            and isinstance(data, dict) and data.get("recorded") is True)
            except (ValueError, TypeError):
                recorded = False
            if not recorded:
                print("launcher: run-end recording failed", file=sys.stderr)
        except (OSError, subprocess.TimeoutExpired):
            print("launcher: run-end recording unavailable", file=sys.stderr)
        return code
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"launcher blocked: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
