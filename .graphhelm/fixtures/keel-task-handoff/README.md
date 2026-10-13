# Isolated task handoff observer

This scenario runs the checked-in Claude and Codex task handoff adapter against a disposable
GraphHelm Runtime. It needs Python 3 and a prebuilt GraphHelm CLI. It does not call a model, use
the user's hook cache, or connect to the configured live Runtime.

Run it from the repository root:

```powershell
$env:GRAPHHELM_CLI = 'C:/Users/gabri/.graphhelm/versions/5fb91a36-dev/bin/graphhelm.exe'
python .graphhelm/fixtures/keel-task-handoff/scenario.py
```

The CLI starts on `127.0.0.1:0`, so Windows assigns an unused port. The scenario rejects port
8793. Its event store, keyring, owner token and agent token are created inside one temporary
directory, and only the child Runtime started by this scenario is stopped.

The negative control first asks a receiver to read a nonexistent offer. It must report the
adapter's specific unobserved receive result. Then the Claude-labelled adapter offers to the
Codex-labelled adapter. Separate processes receive the same execution and token but start with
no shared local hook state. The observer checks the stable offer and receipt identities, sealed
references recovered through the Runtime event log, repeat-receive idempotency, pending duty
references, and rejection of another Codex session. Direct scenario output is not a journey replay
or proof of Claude/Codex host activation.

Child processes are hidden on Windows and have bounded startup/request/cleanup waits. The Runtime
server command owns its event directory; the scenario deletes only its `TemporaryDirectory` after
the process exits. No process search or unrelated cleanup is used.

Cost: one short local Runtime and a fixed graph/event set; no build, network access, model call,
browser or production infrastructure.
