# Ticket-free Studio acceptance (#2449)

Contract: https://github.com/stabem/GraphHelm/issues/2449#issuecomment-6577572269

## Observed browser behavior

On 2026-11-02, source commit `5092634443ff7fc997620304dd541a38aa14fd67`, an isolated synthetic project was served by the real Runtime on loopback port 8852 and the real Studio on port 62399. No owner project or owner screenshot is included.

- A first visit to a new loopback origin opened `?view=proof&journey=runtime-health` without a ticket and displayed **Project demo** with **Proof** selected.
- Reload retained the selected project, view and journey. Adding `run=demo` retained that deep-link parameter.
- A new tab using `localhost:62399` opened the same project and deep link.
- An unknown project id showed **Project unavailable** and did not fall back to demo.

[Real Studio capture](../design/studio/2449/proof.png). The small historical frame inside the proof viewer belongs to an older synthetic run; it is not the current application's login screen. The current shell has the demo project open.

Limits: this was a first visit to a new origin in OpenBot's browser, not a separate fresh browser profile. The HTTP integration starts without a cookie and proves session issuance. This is navigation/authentication evidence, not execution evidence: no Run action was taken, and copied fixture approvals were not treated as current approvals. Changed journey sources are draft, with no fabricated approval.

## Regression and security proof

`node node_modules/vitest/vitest.mjs run loopback-session.test.ts --reporter=verbose` uses a real Vite HTTP server and a temporary project; Runtime I/O is replaced at the HTTP boundary. All 11 cases pass.

The same observer against parent `e9db8dc6573ad23cc134fe8a141c28f61c8e95cd` fails. Removing the explicit Origin comparison makes four cases fail. Removing the explicit Host allowlist makes the wrong-port case fail. Restoring the guards gives 11/11. The foreign-host cases also exercise Vite's independent host guard; they do not alone prove the custom allowlist.

Cases cover ticket-free cookie issuance, no credential disclosure, foreign/null Origin, foreign Host and wrong port, cross-site loads, absent cookie, mutation Origin, originless reads, unknown project, server-injected Runtime credentials and machine registration without a browser cookie.

The complete Studio suite passed 215/215 on the implementation commit. The help golden initially failed because it still described ticket URLs; its six Studio entries now match the new documented behavior and its targeted test passes. A launcher unit test exceeded its unrelated two-second deadline during the broad run; it passes alone (0.25 seconds). These isolated results do not make the broad run green.

## Full-plan receipt

The PR body records the final reached-test report and head. The first broad run encountered stale generated journey contracts, which were recompiled and validated with `graphhelm journey validate --all` (passed). Do not interpret this document as a completed full-plan receipt.
