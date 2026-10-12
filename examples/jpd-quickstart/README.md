# JPD quickstart: a backend journey, proven and sabotaged

A tiny backend project that adopts [Journey-Proven Development](../../docs/jpd/README.md):

- `hello.sh` promises to greet the user and exit 0 (a `cli` act observes it).
- `server.py` serves `site/` over HTTP; `/health.json` must say `status=ok` and `/items.json`
  must list `apple` first (`http` acts observe them).
- `launch.sh` and `.graphhelm/journey-fixture.json` tell GraphHelm how to start the app on an
  isolated port.
- `.graphhelm/journeys/quickstart.journey.yaml` is the journey, shipped as a **draft**: approval
  binds your own commit and your owner token, so it cannot be shipped pre-approved.
- `sabotage/break-items.diff` breaks the product (`apple` becomes `pear`).

Needs `graphhelm`, `git`, `sh` and `python3`. No browser, Node or model.
`apps/cli/tests/jpd_quickstart_example.rs` runs these exact steps.

```sh
cp -r examples/jpd-quickstart /tmp/qs && cd /tmp/qs
git init -q && git config user.email you@example.invalid && git config user.name You
graphhelm --json init --harness claude-code --project .
graphhelm --json journey compile quickstart --fmt --include-draft --project .
git add -A && git commit -qm "app and draft journey"

# A draft never runs its cli act: validation exits 2 (reason cli_needs_approval).
graphhelm --json keel test validation --repo .

# Review the argv and expectations, then approve as the owner and commit the approval.
graphhelm --json journey approve quickstart --token-file .graphhelm/events.token --project .
git add -A && git commit -qm "approve quickstart"

graphhelm --json keel test validation --repo .
# exit 0; data.failures == [], data.paths[0].execution == "completed"

graphhelm --json keel test sabotage --repo . --patch sabotage/break-items.diff
# exit 0; data.verdict == "caught", data.failing[0].execution == "drift"
```

The example's `.gitignore` keeps `.graphhelm/journeys/` and `.graphhelm/journey-fixture.json`
tracked while the owner token and event store stay private. `graphhelm init` keeps an existing
`.gitignore`.
