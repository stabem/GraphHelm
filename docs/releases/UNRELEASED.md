# Unreleased

## Added
- [#2416](https://github.com/stabem/GraphHelm/pull/2416) The Studio marks an active run that needs your input in the project rail and the empty Graph view.
- [#2427](https://github.com/stabem/GraphHelm/pull/2427) The Studio shows where a detached run failed, the cause, and the CLI resume guidance.
- [#2428](https://github.com/stabem/GraphHelm/pull/2428) When leading context candidates are unreadable or appear to contain a secret, the Runtime can fill the result from later candidates within a fixed limit.
- [#2448](https://github.com/stabem/GraphHelm/pull/2448) Journey steps without a frame show a waiting indicator while a run is active; reduced-motion users see static text.

## Changed
- [#2407](https://github.com/stabem/GraphHelm/pull/2407) The Runtime records detached-run failures so status and briefing can show the cause.
- [#2412](https://github.com/stabem/GraphHelm/pull/2412) Session hooks hand file work to a detached helper and report `UNOBSERVED` when it does not answer in time.
- [#2411](https://github.com/stabem/GraphHelm/pull/2411) Getting Started explains where session hooks come from, lists `curl`, and shows the archive checksum command for one downloaded archive.
- [#2431](https://github.com/stabem/GraphHelm/pull/2431) The Codex session-end hook now starts its detached helper so the session work can finish.
- [#2453](https://github.com/stabem/GraphHelm/pull/2453) Getting Started adds archive download, checksum, extraction, and PATH commands, plus clearer setup and troubleshooting steps.

## Fixed
- [#2406](https://github.com/stabem/GraphHelm/pull/2406) Workspace release can clear a claim when its folder is gone, so the task can be claimed again.
- [#2435](https://github.com/stabem/GraphHelm/pull/2435) A lane can reclaim its own task while a parallel task slice is active.
- [#2438](https://github.com/stabem/GraphHelm/pull/2438) The MCP invalid-arguments test now ends with its session instead of a 30-second wall-clock timeout.

## For contributors
- [#2441](https://github.com/stabem/GraphHelm/pull/2441) Contributors run long builds and tests before starting a blind review.
- [#2446](https://github.com/stabem/GraphHelm/pull/2446) Before a blind review, contributors also run formatting, clippy, Keel checks, and journey replays.
- [#2451](https://github.com/stabem/GraphHelm/pull/2451) The docs guard catches broken relative links to files and folders, including non-Markdown targets.
- [#2452](https://github.com/stabem/GraphHelm/pull/2452) The heartbeat and session-stop tests accept the measured timing race but still fail if cleanup leaves a waiter behind.
- [#2460](https://github.com/stabem/GraphHelm/pull/2460) The Unreleased notes now summarize recent user-facing and contributor changes with links to their source PRs.

## Removed
