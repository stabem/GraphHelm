# Red-first Keel proof fixture

This draft journey checks the real `graphhelm keel check --prove-new-tests` observer against a
small Python specimen. The scenario creates a temporary Git repository with a defective parent,
then asks the CLI to prove two candidate heads:

1. A vacuous test must report `passed` on both revisions, `green_on_parent`, and the diagnostic
   `keel.test.green_on_parent`.
2. The named sum regression must report exactly `failed`, `passed`, `earned` after changing only
   the specimen implementation.

The scenario uses Python's standard-library `unittest`; its runner writes JUnit from the actual
test result and includes only the exact requested test name. It checks the structured
`data.testProof.proofs` and diagnostic IDs. It never compiles Rust, uses a provider, writes to
Runtime, or creates a symlink. The specimen Git repository and prover worktrees live in a
temporary directory under this fixture and are removed on exit. Child processes are hidden on
Windows and have bounded deadlines.

Set `GRAPHHELM_CLI` to the pinned prebuilt CLI and run from the repository root:

```powershell
$env:GRAPHHELM_CLI = 'C:/Users/gabri/.graphhelm/versions/d8751570-dev/bin/graphhelm.exe'
python .graphhelm/fixtures/keel-red-first-proof/scenario.py
```

The journey stays draft. The direct scenario is its available observer; an approved journey
replay is not claimed.
