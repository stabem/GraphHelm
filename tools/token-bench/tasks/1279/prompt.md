The token benchmark runner fails on some Windows hosts before the agent starts.

Reproduce and fix the bug in `tools/token-bench/run.py` where a prompt containing normal
non-ASCII text cannot be sent to the child Claude process when the host uses the legacy
Windows cp1252 code page. The prompt must arrive at the child process as the same Unicode
text that the runner received. Keep the existing command-line behavior and error handling.

Add or update focused coverage if needed. Run the focused token-bench tests and show the
commands and results in your final response.
