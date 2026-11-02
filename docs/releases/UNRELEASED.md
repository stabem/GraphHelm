# Unreleased

## Added
- [#2416](https://github.com/stabem/GraphHelm/pull/2416) The Studio marks an active run that needs your input in the project rail and the empty Graph view.
- [#2427](https://github.com/stabem/GraphHelm/pull/2427) The Studio shows where a detached run failed, the cause, and the CLI resume guidance.
- [#2428](https://github.com/stabem/GraphHelm/pull/2428) When leading context candidates are unreadable or appear to contain a secret, the Runtime can fill the result from later candidates within a fixed limit.
- [#2448](https://github.com/stabem/GraphHelm/pull/2448) Journey steps without a frame show a waiting indicator while a run is active; reduced-motion users see static text.

## Changed
- [#2407](https://github.com/stabem/GraphHelm/pull/2407) The Runtime records detached-run failures so status and briefing can show the cause.
- [#2412](https://github.com/stabem/GraphHelm/pull/2412) Most session-hook paths hand file work to a detached helper and report `UNOBSERVED` when it does not answer by its deadline; Codex SessionEnd is the exception (#2431).
- [#2411](https://github.com/stabem/GraphHelm/pull/2411) Getting Started explains where session hooks come from, lists `curl`, and shows the archive checksum command for one downloaded archive.
- [#2457](https://github.com/stabem/GraphHelm/pull/2457) On Windows, four Journey browser launchers request hidden browser consoles for `spawn` calls. The full Studio-to-driver window check remains unverified.
- [#2431](https://github.com/stabem/GraphHelm/pull/2431) A Codex session end skips the detached helper and handles bounded state work in-process under a 1 s watchdog, because Codex's 3 s cap leaves no room for a helper start. Spawn-to-exit can still exceed 3 s under load (#2436); this was not observed on a real Codex host.
- [#2453](https://github.com/stabem/GraphHelm/pull/2453) Getting Started explains the 1.0.0 archive steps for use once that release is published, and adds clearer setup and troubleshooting guidance. The #2443 follow-up findings were addressed; a clean Windows install and real chat-host flow remain unverified, and docs checks do not validate factual claims.

## Fixed
- [#2406](https://github.com/stabem/GraphHelm/pull/2406) Workspace release can clear a claim when its folder is gone, so the task can be claimed again; `git worktree prune` may also clear other sessions' prunable folder-missing entries, but does not touch their working trees or branches.
- [#2435](https://github.com/stabem/GraphHelm/pull/2435) A lane can reclaim its own task while a parallel task slice is active.

## For contributors
- [#2441](https://github.com/stabem/GraphHelm/pull/2441) Contributors run long builds and tests before starting a blind review.
- [#2446](https://github.com/stabem/GraphHelm/pull/2446) Before a blind review, contributors also run formatting, clippy, Keel checks, and journey replays.
- [#2438](https://github.com/stabem/GraphHelm/pull/2438) The MCP invalid-arguments test now waits for session completion instead of relying on a 30-second wall-clock timeout.
- [#2397](https://github.com/stabem/GraphHelm/pull/2397) The task recorder warns when a second reviewer is assigned to the same pull request head.
- [#2451](https://github.com/stabem/GraphHelm/pull/2451) The docs guard catches broken relative links to files and folders, including non-Markdown targets.
- [#2452](https://github.com/stabem/GraphHelm/pull/2452) The heartbeat and session-stop tests accept the measured timing race but still fail if cleanup leaves a waiter behind.

## Internal
- [#2460](https://github.com/stabem/GraphHelm/pull/2460) The release notes list 16 merged PRs since the v1.0.0 notes merge, with links to their source PRs.
- [#2463](https://github.com/stabem/GraphHelm/pull/2463) The notes add source-linked entries for #2457 and #2397 and correct the #2431 and #2453 claims after review.
- [#2456](https://github.com/stabem/GraphHelm/pull/2456) The Codex-end test records the helper-spawn thread start; one of 20 break-once runs passed, and the review did not establish why.
- [#2478](https://github.com/stabem/GraphHelm/pull/2478) The Codex-end guard now checks the helper marker before the timeout return and adds a watchdog-path test; the slow-briefing test no longer races interpreter startup.

## Removed
