# Isolated task handoff observer

This scenario runs the checked-in Claude and Codex task handoff adapter against a disposable
GraphHelm Runtime. It needs Python 3 and an explicitly pinned prebuilt GraphHelm CLI. It does not
call a model, use the user's hook cache, or connect to the configured live Runtime.

Run it from the repository root:

```powershell
$env:GRAPHHELM_CLI = 'C:/path/to/prebuilt/graphhelm.exe'
$env:Path = (Split-Path $env:GRAPHHELM_CLI) + ';' + $env:Path
where.exe graphhelm
& $env:GRAPHHELM_CLI --version
python .graphhelm/fixtures/keel-task-handoff/scenario.py
```

The CLI starts on `127.0.0.1:0`, so Windows assigns an unused port. The scenario rejects port
8793. Its event stores, keyrings, owner tokens, agent tokens and hook state path are created inside
a temporary directory under this fixture. It clears inherited `GRAPHHELM_*` bindings before
starting child processes, and only the child Runtimes started by this scenario are stopped.
Successful output includes the CLI version and executable SHA-256 so the observer binary is
traceable.

The Claude-labelled adapter first offers to the Codex-labelled adapter. The RED-only command below
then asks the receiver to recover that exact offer from a separate healthy but empty Runtime. It
must exit 1 with `continuity assertion failed`; setup or authentication failures are not an
acceptable result. This demonstrates that the continuity observer detects lost source evidence.
The normal command runs the same missing-offer control, restores the receiver to the source
Runtime after clearing only fixture-owned hook state, and checks the stable offer and receipt
identities, sealed references recovered through the Runtime event log, repeat-receive idempotency,
pending duty references, and rejection of another Codex session without a new receipt. Direct
scenario output is not a journey replay or proof of Claude/Codex host activation.

Run the RED-only sensitivity control once during implementation; it is expected to exit 1:

```powershell
python .graphhelm/fixtures/keel-task-handoff/scenario.py --wrong-store
```

Child processes are hidden on Windows and have bounded startup/request/cleanup waits. The Runtime
server command owns its event directory; the scenario deletes only its fixture-local
`TemporaryDirectory` after the process exits. No process search or unrelated cleanup is used.

Cost: two short local Runtimes and a fixed graph/event set; no build, network access, model call,
browser or production infrastructure.

The journey contract is draft and its approval is null. `journey validate` checks the flow's
shape and semantics; it does not run this scenario. Running the scenario directly proves the
bounded offline adapter/Runtime continuity described above. Journey replay remains pending and
must be observed in an isolated journey-enabled environment before this flow can be called proven.
