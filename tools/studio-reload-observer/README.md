# Studio board reload observer

`reload.mjs` (#1703) times serial authenticated reloads of the Studio's Lanes board and writes one
JSON receipt per condition. It reads only: it never writes to the Runtime. The measurement rule
was pre-registered on the issue before any number was taken.

```sh
tools/studio-journey-fixture/fixture.sh up <fixture dir> <runtime port> <studio port>
node tools/studio-reload-observer/reload.mjs --studio http://127.0.0.1:<studio port> \
  --runtime http://127.0.0.1:<runtime port> --session <file holding the session nonce> \
  --token <fixture project>/.graphhelm/events.token --condition idle|builds \
  --playwright <fixture project> --out <receipt.json> [--attempts 20] [--timeout-ms 30000]
tools/studio-journey-fixture/fixture.sh down <fixture dir>
```

- **Attempt:** one uncounted warm-up navigation, then N serial `page.reload()` in one headless
  Chromium context. Usable = `section#panel-lanes.lanes` (the loaded board, not its placeholder)
  is visible; time is reload start to that state.
- **Outcome:** `success`, `timeout` (not usable at the timeout), `error` (navigation threw, a
  `/v1/*` answer >= 400 or none, or the page shows "could not be read"), `excluded` (a reason
  named before the next attempt). p50/p95 (nearest rank) and max use `success` samples only.
- **Recorded per attempt:** Navigation Timing phases, every `/v1/*` route (path only: the
  query, where the session nonce rides, is dropped) with status, ms and body bytes, and the
  cargo/rustc processes on the host (crate or subcommand names, no paths). Per receipt: source
  SHA, machine class, event head before/after.
- **Refused:** non-loopback hosts and the live ports 5183/8793. The nonce and token are read
  from files and the receipt is not written if either would appear in it.

Tests: `node --test tools/studio-reload-observer/reload.test.mjs`.

## Receipts (`receipts/`)

| Condition | Accounted | Valid | p50 | p95 | max | Builds seen |
|---|---|---|---|---|---|---|
| idle | 20/20 | 20 | 106 ms | 117 ms | 120 ms | 0 cargo, 0 rustc |
| builds | 20/20 | 20 | 104 ms | 110 ms | 117 ms | one `cargo build` (2 cargo processes), 1–6 rustc |

Limits, stated plainly:

- **One host, load not controlled.** Other work on the machine was not stopped or measured.
- **One build, not four.** The `builds` receipt ran beside one workspace `cargo build`; the lane
  did not start four, so as not to load a shared machine. `fourActiveThroughout` is `false`, and
  the issue's four-build criterion is not met by these receipts.
- **Fixture, not a real project.** The subject is the studio journey fixture's disposable
  Runtime and run `demo` (about 1.3 KB of tasks). A real project's board reads more data;
  these numbers are a floor, not that project's baseline.
- Headless Chromium, `vite` dev server (not a production build). Event head is the highest
  sequence in `/v1/executions/{id}/tasks`; it did not move during either run (39 → 39).
- Machine is recorded as platform, arch, Node and Chromium versions only: no CPU, memory or host
  name in public text.
- `source.dirty` is `true` because the receipts were taken from the uncommitted tool itself.
- A first `builds` run started after its build had already finished and saw 0 builds; it was
  discarded as mislabelled (not a builds condition) and re-run while the build compiled.
