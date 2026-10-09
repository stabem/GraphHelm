Installed Codex clamps a `SessionEnd` hook command to three seconds, but both Codex hook files
(`plugins/graphhelm/hooks/codex-hooks.json` and `plugins/graphhelm-codex-hooks/hooks/codex-hooks.json`)
declare 5, and the end request in `session_hook.py` waits up to 3.0 seconds of socket inactivity,
so Codex can kill the hook while it is still waiting. Declare 3 for Codex's SessionEnd and make the
end acknowledgement wait fit inside it (both copies of `session_hook.py`). Claude's `hooks.json`
keeps its 5-second budget. Correct the budget paragraph in `plugins/graphhelm/README.md`. The
hook tests must stay green.
