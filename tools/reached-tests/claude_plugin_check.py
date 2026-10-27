#!/usr/bin/env python3
"""Run `claude plugin validate` and `claude plugin test` on one plugin folder.

The reached plan runs this for `plugins/journey-review/**`; `plugin test` runs only for a plugin that has `hooks/hooks.json`. When the `claude` CLI is not on PATH
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
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    if len(argv) != 1:
        print("usage: claude_plugin_check.py <plugin folder>", file=sys.stderr)
        return 2
    claude = shutil.which("claude")
    if claude is None:
        print(f"{UNOBSERVED} the claude CLI is not on PATH, so `claude plugin validate` and "
              f"`claude plugin test` did not run for {argv[0]}")
        return 0
    for verb in ("validate", "test"):
        if verb == "test" and not os.path.isfile(os.path.join(argv[0], "hooks", "hooks.json")):
            # `claude plugin test` loads a hooks module; a plugin without one has nothing for it to test.
            print(f"NOT APPLICABLE: {argv[0]} has no hooks/hooks.json, so `claude plugin test` has no module to load")
            continue
        command = [claude, "plugin", verb, argv[0]]
        print("+ " + " ".join(command), flush=True)
        done = subprocess.run(command, creationflags=NO_WINDOW, capture_output=True, text=True,
                              encoding="utf-8", errors="replace")
        # Print what the CLI said: a step that exits non-zero must show why, not just the exit code.
        print(((done.stdout or "") + (done.stderr or "")).rstrip(), flush=True)
        if done.returncode != 0:
            print(f"FAILED: `claude plugin {verb}` exited {done.returncode} for {argv[0]}", flush=True)
            return done.returncode
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
