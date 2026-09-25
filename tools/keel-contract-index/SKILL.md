---
name: keel-contract-index
description: Build and consult a source-bound repository fact index when starting Keel work in a new or changed codebase. Use it to find relevant code facts before opening source, never to invent product promises.
---

# Keel contract index

The index is a map of **observed code facts**, not a specification of intended behavior. Use it to propose a Keel card's scope and cited symbols. Obtain the promise, acceptance criterion, and refusal cases from the task and accepted product contracts. A bug in the current code must not become a requirement merely because the scanner saw it.

## Build and read

1. Run `graphhelm keel index --repo <repo> --out <outside-dir>/index.json` before using any prior index. Choose a private output directory outside the repository. The scanner retains the output parent, refuses a changed path alias, and publishes relative to the retained directory. On Unix it uses a reusable owner-only `.keel-stage` directory; on Windows it holds the parent against direct rename and publishes from a temporary file handle. An actor who can relocate the retained directory itself into the repository is outside this boundary. The scanner inventories tracked paths and binds its entries to source content digests. Git inventory uses `ls-files` and disables the repository's fsmonitor command, avoiding the clean-filter execution path in `git status`. Do not insert raw source bodies, secrets, or ignored files into a card.
2. Run `graphhelm keel verify --repo <repo> --index <outside-dir>/index.json` immediately before retrieval. `graphhelm keel query` also checks freshness. A stale or unreadable index is not current evidence. Rebuild it or inspect exact source; record which fallback was used.
3. Query only the paths or symbols the task needs with `graphhelm keel query --repo <repo> --index <outside-dir>/index.json --term <name>`. For one unambiguous hit, the standalone scanner still offers `propose-card` through `cargo +1.97.1 run --locked --manifest-path <skill-dir>/Cargo.toml -- propose-card --repo <repo> --index <outside-dir>/index.json --term <name>`. This emits a source-bound **candidate** Keel card with empty acceptance criteria and refusal cases. Fill those from the task and accepted product contracts before asking a person to accept it. This first scanner extracts only top-level public declarations from Rust; other languages are inventoried but marked unsupported. Both query and candidate output include `coverageGaps` counts for unsupported files, invalid Rust files, and omitted paths by reason; inspect the index for their exact paths. Keep skipped, ignored, ambiguous, and untracked coverage visible in the task record. An ignored directory is recorded by name, but changes inside it are not observed by freshness checks; `ignoredDirectoryContentsUnobserved` marks this partial coverage. If any omission can affect the claim, inspect exact source and expand the card.
4. For a negative or exhaustive claim, require complete relevant coverage and exact-source confirmation. A zero-hit search is not proof that a symbol or dependency does not exist.
5. Hand the candidate to `code-contract` for acceptance criteria and refusal cases. Use `context-retrieval` to assemble citations when the task depends on facts beyond this index. Do not label a generated card accepted, and do not let this skill enforce or publish it.

## Evidence and cost

Record the scanner version, worktree snapshot digest, queried terms, returned paths, omissions, and source fallbacks with the task. Compare total cost per correct delivery and source-body reads against the normal workflow on paired tasks before claiming a saving. A shorter prompt or fewer reads alone is not a success measure.

If the scanner, verifier, or required source observer is unavailable, report `OBSERVER_MISSING` for the affected fact. Continue only with an explicit exact-source fallback that can actually prove it; never fill a missing field from model inference.
