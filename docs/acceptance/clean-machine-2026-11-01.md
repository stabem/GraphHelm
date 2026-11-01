# Clean-machine run of GETTING_STARTED, final — 2026-11-01

One top-to-bottom run of [`docs/install/GETTING_STARTED.md`](../install/GETTING_STARTED.md) on a
machine that held only the operating system, for promise 1 of the 1.0 body (#2301, #2302). It
repeats the earlier run on `3d543d69`, whose product steps passed and whose three page defects
(#2386 Studio gate port, #2387 section 5 blocked-node text, #2388 section 4 PowerShell claim) are now
closed. **Result: every step passed, and the three fixes read as written.** This record carries a
summary only; screenshots were kept outside the repository.

| | |
|---|---|
| Commit under test | `d377d29f89fcdebe9fe5242632e1a61de4cd5987` (origin/main when the anonymous clone ran; `graphhelm --version` printed the same sha). origin/main has since gained `f815276c` (a docs-guard test, #2395), which touches neither the page, the CLI nor the Studio |
| Machine | fresh container `gh-clean-2301d`, Docker 29.8.2 on a Windows 11 host, `sleep infinity`, Ubuntu 24.04.5 LTS, `nproc` 8 (builds capped with `CARGO_BUILD_JOBS=2`, a load limit, not a page step) |
| Image | `ubuntu@sha256:fe81560990fae86dc4743202b2cb4d3df98d0bcf949173cecef15ce1127ef852`, the same digest as the earlier run's container `gh-clean-2301c` |
| User | `apt-get` as root (a root container has no `sudo`, as the page says); every other page command as the non-root user `tester` |
| Source | anonymous `git clone https://github.com/stabem/GraphHelm.git` (no token, no substitution) |
| Started / ended | `2026-11-01T15:14:44Z` / `2026-11-01T15:44:44Z` (container clock) |
| Ports | container `5183` (Studio, `-- --host`) published to host `127.0.0.1:15184`; the Runtime stayed on container loopback `8791` |
| Browser | Playwright 1.54.1 driving headless Chromium 1179 on the host, against the published port |

## Steps

| Page step | Command (abridged) | Exit | Time | Observed |
|---|---|---|---|---|
| Prereq, apt | `apt-get update`; `apt-get install --yes --no-install-recommends build-essential ca-certificates curl git pkg-config` | 0 | 5.9 s; 29.8 s | packages installed; `git version 2.43.0` |
| Prereq, rustup | `curl ... sh.rustup.rs \| sh -s -- -y --profile minimal --default-toolchain 1.97.1` | 0 | 11.8 s | `cargo 1.97.1 (c980f4866 2026-06-30)` |
| Prereq, Node | SHASUMS256 + tarball, `sha256sum -c`, extract | 0 | 5.0 s | `node-v22.23.4-linux-x64.tar.gz: OK`; `v22.23.4` |
| 1 clone | `git clone https://github.com/stabem/GraphHelm.git` | 0 | 4.9 s | `HEAD` = `d377d29f...` |
| 1 build | `cargo +1.97.1 install --locked --path apps/cli` | 0 | 429.9 s | `Installed package graphhelm-cli v0.1.1` |
| 1 version | `graphhelm --version` | 0 | 0.0 s | two lines: `graphhelm 0.1.1`, `commit d377d29f89fcdebe9fe5242632e1a61de4cd5987` |
| 2 init | `graphhelm init --pretty --harness claude-code` (empty folder) | 0 | 0.1 s | `ok`; events, `events.token`, `serve.key` (both 64 bytes, mode 0600), keyring, `.mcp.json` created |
| 3 serve | `graphhelm serve --events ... --bind 127.0.0.1:8791 --project ... --keyring ... --key-id studio` (background) | 0 | 2.0 s | first line `{"ok":true,"command":"serve.started",...}`, no warning |
| 3 health | `curl --fail http://127.0.0.1:8791/health` | 0 | 0.0 s | `serve.health`, with `projectId` and `instance` |
| 3 401 | `curl` without a token on `/v1/executions` | 0 | 0.0 s | `401` |
| 3 list | `curl` with the bearer token | 0 | 0.0 s | `execution.list`, `executions: []` |
| 4 studio start | `graphhelm studio start` (project folder, Linux, no PowerShell installed) | 0 | ready in 90 s window | `[up] Runtime already answering ... reusing it`; `[up] Studio starting at http://127.0.0.1:5183`; Vite ready. #2388 fixed: no PowerShell needed on Linux |
| 4 studio open | `graphhelm studio open --no-browser` | 0 | 0.0 s | `data.url` = `http://127.0.0.1:5183/?session=<nonce>` |
| 4 manual dev | `npm --prefix apps/studio ci`; `npm --prefix apps/studio run dev -- --host` | 0 | 2.4 s (ci, cached) | `Studio login: http://127.0.0.1:5183/?session=<nonce>` |
| 4 gate | browser, URL without `?session=` | n/a | n/a | "Open the Studio from your project ... Run graphhelm studio open in the project folder". #2386 fixed: no field, no wrong port |
| 5 start | `graphhelm execution start --file examples/graphs/manual-override-deploy.yaml ... --mode supervised --execution demo` | 0 | 0.2 s | `execution.start`, `needs_you`, `blocked_node: implementation` |
| 5 Studio Graph | browser, login URL, Graph tab | n/a | n/a | `STATUS waiting for you`; `WAITS ON implementation is blocked. Approve it from the CLI: graphhelm execution approve --events <events> --execution demo --node implementation`; "No owner request is recorded on this run yet." #2387 fixed |
| 5 Studio Lanes | Lanes tab | n/a | n/a | `NEEDS ACTION ... 1`, "The run waits for you: implementation is blocked. Approve it from the CLI: ..." |
| 5 Studio Proof | Proof tab | n/a | n/a | loads: "JOURNEYS 0 ... No journeys yet"; no console errors |
| 5 pause | `graphhelm execution pause` | 0 | 0.1 s | `paused`, `needs_you`, `blocked_node: implementation` |
| 5 approve | `graphhelm execution approve --node implementation` | 0 | 0.1 s | `paused`, `can_sleep`, `[]` |
| 5 resume | `graphhelm execution resume --file ... --fixtures ...` | 0 | 0.2 s | `running`, `needs_you`, `blocked_node: implementation` |
| 5 status | `graphhelm execution status` | 0 | 0.0 s | `running`, `needs_you`, `blocked_node: implementation` |
| 6 MCP | JSON-RPC `initialize`, `tools/list`, `tools/call list` over stdio to `graphhelm mcp --discover --project ... --actor agent-chat` | 0 | 3.0 s | tools listed; `list` returned one row, `demo`, `attention: needs_you`, `status: running` |

All four CLI verb results match the page's "Measured output" block.

## OBSERVER_MISSING

- A real chat host (Claude Code or Codex) reading `.mcp.json` and answering "use the graphhelm MCP
  tool list": not run. Step 6 above is a protocol-level probe of the same server, not a host.
- Windows, Windows Sandbox and any VM: not run.
- `graphhelm update`, `gateway setup` and a real model provider: out of scope for this run.

## Notes

- The container, the Runtime and the dev server were left running for the reviewer; they are the
  author's processes and are stopped by their recorded PIDs.
- The `studio start` clone used `~/.graphhelm/source`; the manual dev server used `~/GraphHelm`.
