# Node deliveries and owner document edits

**Issue:** #1093. Related gap: #112.

**Goal:** A node explains its reported deliverables and reasons. Linked project documents open in an adjacent editor. An owner save changes the main project directory and records change notices for associated runs.

**Architecture:** Reuse sealed signal evidence for progressive delivery records. Bind records to an opaque canonical project identity and the producing node. The editor accepts only a registered document reference in the requested execution and requires an explicitly configured matching project. File access belongs in the filesystem adapter; CLI and HTTP share command behavior. Studio uses public HTTP contracts only.

## Global constraints

- Owner selected the main project directory as the save target, including effects on other runs. A run's Git output ref is not silently substituted for this target.
- Delivery descriptions and journey/rule associations are reported provenance, not proof that a file was created or a requirement was satisfied.
- Legacy or demonstration successes must not acquire invented deliverables.
- Preserve append-only sealed evidence, paused/completed execution state, and Governor-only operational mutations.
- A recorded notice is not agent acknowledgment or replanning. Already executing model calls cannot be retroactively changed.
- Reject unbound projects, unrelated evidence, secret-bearing or non-text documents, unsafe paths and stale editor revisions. Keep owner drafts on conflict.
- Serialize Runtime document writers and replace files atomically. Ordinary filesystem replacement cannot offer compare-and-swap against arbitrary non-cooperating external editors; do not claim that guarantee.

## Implementation and evidence

1. Add a bounded `execution delivery` command and reserved sealed `node_delivery` validation. Verify node membership, project identity, malformed records and progressive appends.
2. Add project-bound bounded text reads and revision-checked saves in the filesystem adapter. Exercise traversal, links, Windows aliases, binary/secret content, stale saves and successful replacement.
3. Expose registered document reads/saves through shared CLI/HTTP commands. Check same-run evidence and configured project before filesystem access. Preserve a durable retry path for change notices and distinguish saved-but-notification-pending.
4. Surface deliveries above lifecycle history, with explicit file/rule/journey references. Open the main-project editor to the right. Exercise stale responses, draft retention, retry and narrow screens.
5. Route change notices to associated runs without resuming them. Any claim that a node received a notice requires an observer of its actual prompt/input, separate from the append receipt.
6. Run focused Rust and Studio tests, typecheck/build, browser desktop/mobile journeys, and an independent review. Record incomplete obligations explicitly; a component test is not end-to-end delivery evidence.

## Security and rollback

The bearer-authorized editor must not become a general filesystem browser. Clients send evidence references and document indices, never arbitrary project roots. Screen both existing and proposed text. Do not render HTML from reports or persist editor drafts in browser storage. Rollback reverts the feature commit; owner document changes are independent project edits and require explicit revision recovery rather than silently reverting user files.
