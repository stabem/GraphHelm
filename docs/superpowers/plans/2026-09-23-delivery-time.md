# Delivery Time Report Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a bounded, offline PowerShell JSON report that accounts for gate receipt execution time and overlap per pull request while preserving malformed, unattributed, and unobserved evidence.

**Architecture:** `ci/delivery-time.ps1` will be the pure reader and aggregation layer. It will enumerate bounded local JSON receipts, validate timestamps and attribution, deduplicate exact content copies with provenance, and calculate sum, interval union, span, slot waits, statuses, and additional-run counts. `ci/report-delivery-time.ps1` will supply defaults and emit only JSON; it will never execute receipt content or call external services.

**Tech Stack:** Windows PowerShell 5.1-compatible scripts, `ConvertFrom-Json`, SHA-256 file hashing, deterministic ordered JSON output, existing PowerShell suite harness.

**Spec:** GitHub issue #1217.

## Global Constraints

- Read only bounded local files under `.factory/gate-runs` or an explicit directory.
- Do not change `ci/gate.ps1`, `ci/gate-runner.ps1`, or the existing overlap helper.
- Slot wait is the recorded `slotWaitSecs` field; queue/request wait and retry causality remain unknown unless a receipt explicitly records them.
- Invalid, reversed, missing, and unattributable receipts remain visible even when a PR filter is applied.
- Exact content duplicates are counted once and retain every source path as provenance.
- No external execution, network access, credentials, or writes from receipt input.
- Local path checks reject UNC/device/network and reparse-point components before enumeration; they do not claim to close races after validation.
- Timestamps require a complete ISO date-time and explicit offset; finite overflow in an aggregate produces an unavailable value with a reason instead of a non-JSON number.

## Review Focus

- Overlapping valid windows must contribute once to union wall time while execution sum remains additive; test overlapping and touching intervals.
- A later GREEN receipt must not erase earlier RED history; test status counts and run ordering.
- A receipt with no usable PR, malformed JSON, invalid timestamps, or reversed timestamps must appear in an indeterminate population under a PR filter; test explicit disclosure.
- Exact duplicate bytes must not inflate totals and must retain duplicate paths; test copied receipts.
- Missing queue and retry observers must serialize as unknown, and `slotWaitSecs` must remain separate; test absent fields and a present slot wait.
- Nested intervals must preserve the outer observed span, while empty input and missing slot waits remain unavailable (`null`) with explicit counts.
- File-count and byte limits must refuse or disclose bounded fixtures before untrusted content can grow without limit.
- Status maps and serialized reports must be byte-stable across separate processes, and UNC/device/network or reparse-point receipt roots must be rejected before enumeration.

### Task 1: Implement bounded receipt reader and aggregation

**Files:**
- Create: `ci/delivery-time.ps1`
- Test: `ci/delivery-time.tests.ps1`

**Interfaces:**
- `Read-DeliveryReceipts -Directory <string> -PullRequest <nullable int>` returns an object with `Receipts`, `Indeterminate`, `Duplicates`, and `Provenance` collections.
- `Measure-DeliveryTime -ReadResult <object>` returns ordered report data with `population`, `pullRequests`, `totals`, and `unknowns` fields.

- [ ] **Step 1: Add focused tests for the observable contracts.**
  The observable behavior is JSON-safe aggregate data and explicit indeterminate evidence. The plausible defects are double-counted overlaps, a nested interval shortening the span, lost RED history, filtered-out malformed files, duplicate inflation, invented queue/retry values, zero-valued missing observations, and unbounded input. Existing overlap tests do not cover per-PR accounting or these attribution/unknown rules, so the new suite will build small temporary receipts and assert those boundaries.
- [ ] **Step 2: Run `powershell -NoProfile -File ci/delivery-time.tests.ps1` and confirm the new cases fail for the missing reader.**
- [ ] **Step 3: Implement size and population bounds, safe JSON parsing, strict UTC timestamp parsing, PR attribution validation, exact-byte SHA-256 deduplication, and deterministic provenance.**
- [ ] **Step 4: Implement additive execution seconds, interval union and observed span, recorded slot wait totals, status counts, additional-run counts without classifying rework, and explicit unknown queue/retry/end-to-end fields.**
- [ ] **Step 5: Re-run the focused suite and inspect JSON for a real PR 1204 report.**

### Task 2: Add JSON report entrypoint and suite registration

**Files:**
- Create: `ci/report-delivery-time.ps1`
- Modify: `ci/run-ps-suites.ps1` (pinned suite registration only)

**Interfaces:**
- `report-delivery-time.ps1 -Directory <string> -PullRequest <nullable int>` emits one JSON document to stdout and uses the default `.factory/gate-runs` directory when omitted.

- [ ] **Step 1: Add entrypoint tests to the focused suite for JSON-only stdout, default directory, PR filtering, and nonzero refusal on unreadable population.**
- [ ] **Step 2: Run the focused suite to observe the missing entrypoint behavior.**
- [ ] **Step 3: Implement the thin entrypoint and add `delivery-time.tests.ps1` to the existing pinned suite set.**
- [ ] **Step 4: Run `powershell -NoProfile -File ci/delivery-time.tests.ps1`, `powershell -NoProfile -File ci/report-delivery-time.ps1 -PullRequest 1204`, and `git diff --check`.**

### Task 3: Validate the bounded deliverable

**Files:**
- Review: all files above

- [ ] **Step 1: Confirm only the five issue-scoped files changed.**
- [ ] **Step 2: Run the focused PowerShell suite through `ci/run-ps-suites.ps1` and record passed, failed, skipped, and unobserved states.**
- [ ] **Step 3: Run the repository’s local authoritative gate if the parent lane schedules it; do not claim full gate completion from focused tests alone.**
- [ ] **Step 4: Inspect the PR 1204 JSON output and report any missing end-to-end or retry-causality observers as unknown.**
