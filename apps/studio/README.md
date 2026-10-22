# GraphHelm Studio

The local control room for a GraphHelm project. One page, logged in to the project's Runtime,
with three views:

- **Graph**: each owner request with the issues opened for it, their pull requests and the
  result, or the same work as Kanban columns. "Merged" is not done: a card reads Proven only when
  its journeys passed their last replay. A card in review opens a large review screen that compares
  the journey's frames on main (Before) with the PR head's (Now).
- **Lanes**: one card per agent with its idle time, its request, its progress and its last action;
  a question an agent asks, and the reply; Wake, Reassign and Stop; the Needs action list; and the
  run's capacity.
- **Proof**: the project's journeys, Run with the browser being prepared and its log, step cards
  with captured frames, the compare modes (Frame, Side by side, Overlay, Changes), and Approve or
  Reject.

The Studio talks to the **public Runtime API** and nothing else. It imports no Rust crate, reads
no event file and holds no second copy of a rule: every action it takes is one the CLI and the MCP
server can also take.

## Run it

From a project that has a Runtime (`graphhelm init`, then `graphhelm serve`):

```bash
graphhelm studio open
```

`studio start` installs and starts the Studio from a managed clone (the launchers are in
[`tools/`](tools/)); `studio open` registers the project with it and opens a logged-in page.
`studio open --journey <id>` opens Proof on that journey.

For development inside this repository:

```bash
npm ci
npm run dev
```

The dev server serves on `127.0.0.1` and proxies `/v1` and `/health` to the Runtime
(`GRAPHHELM_RUNTIME_URL`), so the bearer token never rides a cross-origin request. Login is by
ticket: `/__studio/session` trades the run's nonce, a stable key derived from the token, or the
one-shot ticket `graphhelm studio open` writes, for the Runtime token. A second project joins the
same port with `/__studio/register` and is served under `/p/<id>`.

## Tests

```bash
npm run typecheck
npx vitest run --maxWorkers=4
```

The journeys that prove the Studio are `.graphhelm/journeys/studio2-*.journey.yaml`, replayed by
the fixture in `tools/studio-journey-fixture`.
