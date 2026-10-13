# Isolated task handoff observer

This scenario runs the checked-in Claude sender adapter and Codex companion receiver adapter
against a disposable GraphHelm Runtime. The Codex-labelled receiver loads
`plugins/graphhelm-codex-hooks/hooks/task_handoff.py`; a missing or broken companion must fail
the observer instead of silently using the sender package. It needs Python 3 and an explicitly
pinned prebuilt GraphHelm CLI. It does not
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

Before the offer, the scenario records one synthetic task-evidence signal with a literal objective,
bounded Keel card and synthetic prior proof. After the Codex companion receives the offer and the
fixture hook cache is discarded, the scenario stops the exact source Runtime process and waits for
it to exit. It starts the same pinned executable against the same event store and keyring on a new
ephemeral loopback port, without reinitializing or reseeding the store. After clearing only fixture
hook state, the actual Codex companion must recover the same receipt as `already_recorded`; a fresh
bounded Codex-bound process must discover the earlier signal through the replacement Runtime's
events endpoint and open its sealed evidence through the public evidence endpoint. That process
receives only fixture Runtime binding and execution identity; it is not given expected
signal/evidence IDs or sender-cached content. The observer checks literal content, actor,
execution, evidence identity and digest, and that the event precedes the offer. It compares the
complete bounded event snapshot immediately before shutdown and after recovery to ensure reads did
not append or alter journal records. It also checks the read against a healthy empty store and a
valid but different synthetic record.

The RED-only command below asks the receiver to recover the original offer from a separate healthy
but empty Runtime. It must exit 1 with `continuity assertion failed`; setup or authentication
failures are not an acceptable result. The normal command runs the same missing-offer control,
restores the receiver to the source Runtime after clearing only fixture-owned hook state, and checks
the stable offer and receipt identities, sealed references recovered through the Runtime event log,
repeat-receive idempotency, pending duty references, prior task-evidence recovery and rejection of
another Codex session without a new receipt. Direct scenario output is not a journey replay or proof
of Claude/Codex host activation.

Run the RED-only sensitivity control once during implementation; it is expected to exit 1:

```powershell
python .graphhelm/fixtures/keel-task-handoff/scenario.py --wrong-store
```

Child processes are hidden on Windows and have bounded startup/request/cleanup waits. The Runtime
server command owns its event directory; the scenario deletes only its fixture-local
`TemporaryDirectory` after the process exits. No process search or unrelated cleanup is used.

Cost: three sequential short local Runtime starts (source, empty-store control, source restart),
one fixed graph/event set, and bounded hidden child processes; no
build, network access, model call, browser or production infrastructure.

The journey contract is draft and its approval is null. `journey validate` checks the flow's
shape and semantics; it does not run this scenario. Running the scenario directly proves the
bounded offline adapter/Runtime continuity described above. Journey replay remains pending and
must be observed in an isolated journey-enabled environment before this flow can be called proven.

The package-selection regression test runs without a Runtime and proves that a unique refusal
from a copied Codex companion reaches the scenario as `ScenarioFailure`:

```powershell
python .graphhelm/fixtures/keel-task-handoff/test_scenario.py
```
