# Item 5, new project from scratch — 2026-11-01

Receipt for the "new project" line of item 5 of the 1.0 body (#2302): the CLI and the Studio, used in
a project that is not GraphHelm. Summary only; the screenshots were kept outside the repository.
**Result: every step passed.** The two private-project receipts (a Python project and a Vue project)
stay outside this repository; this page writes only their outcome: private Python project, local
receipt ok; private Vue project, local receipt ok.

| | |
|---|---|
| CLI under test | built from release head `b1a9f299330ffac8423cace95c43b7b0b71951c1` with `graphhelm workspace slot ... -- cargo +1.97.1 build --locked -p graphhelm-cli` (dev profile, jobs 2). `graphhelm --version` printed `graphhelm 1.0.0` and `commit b1a9f299330ffac8423cace95c43b7b0b71951c1`. The installed 0.1.1 binary was not used for any step |
| Studio | the same checkout, `npm --prefix apps/studio ci`, then `npm --prefix apps/studio run dev` (section 4 of [GETTING_STARTED](../install/GETTING_STARTED.md)) |
| Project | an empty git repository with a README, in a throwaway directory; its own events store, token, Runtime port and `GRAPHHELM_RUNTIME_DIR` |
| Host | Windows 11, all processes hidden, one terminal; browser: Playwright driving headless Edge |

| Step | Command (short) | Result |
|---|---|---|
| 1 setup | `graphhelm init --project <dir> --bind 127.0.0.1:<port> --harness claude-code` | passed, exit 0; events, token, key, keyring, `.gitignore` and `.mcp.json` created |
| 2 Runtime | `graphhelm serve --events ... --project <dir> --keyring ... --key-id studio` | passed; `serve.started`, `/health` ok, `401` without a token, `execution.list` with the token ok and empty |
| 3 Studio login | `graphhelm studio open --project <dir> --no-browser`, URL opened in the browser | passed; the page connected, the project shown, no connect gate |
| 4 task | `graphhelm execution start --file examples/graphs/manual-override-deploy.yaml --fixtures ... --mode supervised --execution demo` | passed; `attention: needs_you`, `blocked_node implementation`; the Graph tab reads `Status: waiting for you` with the `Waits on:` row, the Lanes tab lists it under Needs action |
| 5 briefing | `graphhelm execution briefing --events ... --execution demo` | passed; objective, `pending: blocked_node implementation`, `nextStep` approve |
| 6 stop | `taskkill /T /F` on the two recorded PIDs | passed; nothing left listening on the Runtime or Studio ports |

Not observed: the MCP `briefing` tool through a real chat host (`OBSERVER_MISSING`); the CLI briefing
above is the same read from the store. The run used dev-profile binaries on a loaded machine, so the
timings are not a benchmark.
