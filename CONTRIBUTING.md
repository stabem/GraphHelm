# Contributing to GraphHelm

Thank you for helping improve GraphHelm. The project is MIT licensed. Contributions are accepted
under the same MIT terms, and GraphHelm does not require a CLA, ICLA, or CCLA.

## Before you start

Start every change with an issue, including small documentation fixes. Check the repository's decision register,
schemas, and subsystem specifications before changing a contract. Write repository documentation
in English.

## Make a change

Create an `issue-<N>-<short-description>` branch from `main`, keep the change focused, and explain the user-visible promise in the
pull request. Name the paths in scope, the command that proves the promise, and any remaining
risks. Do not include secrets, credentials, private data, or generated local state.

Tests should observe a real behavior at the boundary that owns it. Add a test when it catches a
plausible defect that existing coverage does not observe; do not add tests only to increase a
count. Keep deterministic tests independent of network access, credentials, Docker, browsers, and
production services.

## Validate locally

GraphHelm does not use hosted GitHub Actions as its CI system. Run the tests the change reaches
and list each command and result in the pull request. For a documentation-only change, inspect
the changed text and check links and whitespace. A Rust code change also requires formatting
and Clippy on the touched crates; see [the delivery process](docs/process/DELIVERY.md) for the
exact commands and review rules. `ci/gate.ps1` is an optional full local check, not a merge
requirement.

Do not enable or run GitHub Actions. Local, change-directed evidence is the source of truth for
validation.

## Pull requests

Explain what changed, why, validation evidence, security impact, and rollback steps. Keep claims
bound to the commit that was checked. A pull request is delivered only after it is merged into
`main` and the merged revision is verified.

Maintainers may request extra review for changes to authentication, secrets, permissions, policy,
the tool broker, persistence, schemas, or public compatibility.

## Security

Do not report a suspected vulnerability in a public issue. Follow [SECURITY.md](SECURITY.md).

## License and authorship

By submitting a contribution, you confirm that you have the right to submit it under the MIT
license. You keep copyright in your work; no rights assignment or separate agreement is required.
