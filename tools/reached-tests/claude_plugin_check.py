#!/usr/bin/env python3
"""Run `claude plugin validate` and `claude plugin test` on one plugin folder.

The reached plan runs this for `plugins/journey-review/**`. When the `claude` CLI is not on PATH
the plugin was not observed: the script says so on its last line, `UNOBSERVED: ...`, and exits 0
so the runner can report the step as unobserved. It is never a pass: `run_reached.py` reads that
line and counts the step as unobserved, and the run does not end `passed`.
"""
import os
import shutil
import subprocess
import sys

NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
UNOBSERVED = "UNOBSERVED:"


def main(argv):
    if len(argv) != 1:
        print("usage: claude_plugin_check.py <plugin folder>", file=sys.stderr)
        return 2
    claude = shutil.which("claude")
    if claude is None:
        print(f"{UNOBSERVED} the claude CLI is not on PATH, so `claude plugin validate` and "
              f"`claude plugin test` did not run for {argv[0]}")
        return 0
    for verb in ("validate", "test"):
        command = [claude, "plugin", verb, argv[0]]
        print("+ " + " ".join(command), flush=True)
        code = subprocess.run(command, creationflags=NO_WINDOW).returncode
        if code != 0:
            return code
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
