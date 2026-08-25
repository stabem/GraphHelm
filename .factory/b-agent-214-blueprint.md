# B — #214 blueprint (harden MCP URL path and query encoding)

Branch `issue-214-mcp-url-encoding`, off `f390989`. Short by intent.

## Measured before deciding

**Composition is string concatenation.** `apps/cli/src/commands/mcp/client.rs`:

```rust
base_url: base_url.trim_end_matches('/').to_owned(),   // :101
url: format!("{}{path}", self.base_url),               // :141
```

No encoding, no query merge, no fragment handling. That is the deliverable the issue names.

**The sharper defect is one layer up, and it is not about proxies.** `tools.rs` builds paths by
interpolating caller-supplied data: **15** `/v1` interpolations, **12** of which place an `id`
straight into a path segment. Those ids arrive in the MCP tool-call `arguments`, so an id
containing `/`, `?` or `#` **changes which endpoint the request reaches**. That is endpoint
confusion driven by data, and it is what gets guarded first.

**~~The query half is currently safe by TYPE, not by care.~~ RETRACTED — the claim was false.**

What this said: that the only query values built were `after` and `limit`, both `u64`, plus an
internal `sessionId`, so no query encoder was needed. It is kept here struck through rather than
deleted, because the reason it survived matters more than the sentence.

**Two gateway tools interpolate caller-supplied STRINGS into a query** — `routes` takes a
`manifest`, `probe` takes a `route`, both declared `{"type": "string"}` in the tool schema. A value
carrying `&` appended a parameter the caller never wrote; one carrying `#` truncated the request.

**How it survived: the population was the instrument.** The measurement behind the claim enumerated
`/v1/executions/...` sites, and the substitution guard then refused to finish unless that population
was fully migrated — which it was, exhaustively. The file held **fifteen** `/v1` sites, not
thirteen, and the two outside the pattern were precisely the two that put strings into a query. The
guard never failed; it answered a narrow question with precision.

Worse, the warning was already printed in my own output: an early measurement showed
`format!("/v1` = 15 beside `/v1/executions/{id}` = 12, and I carried 13 forward without reconciling
them. Two numbers answering the same question belong side by side, where the layout forces the
comparison.

**The base-shape half is live too, and correcting my own first reading is why.** I had it down as
a future state, because `mod.rs:36` refuses any URL that is not loopback. Measured: `is_loopback_url`
inspects only the AUTHORITY -- it splits on `/`, `?`, `#` and reads the first part -- so every one of
these is accepted today, and the concatenation then produces:

```
http://127.0.0.1:8080/graphhelm  -> .../graphhelm/v1/executions/abc     correct
http://127.0.0.1:8080/?tenant=a  -> .../?tenant=a/v1/executions/abc     the API path lands INSIDE the query
http://127.0.0.1:8080/#frag      -> .../#frag/v1/executions/abc         everything after # is never sent
```

And with a caller-supplied id on the plain loopback root:

```
id = a/../../admin  -> /v1/executions/a/../../admin    a different endpoint
id = a?x=1          -> /v1/executions/a?x=1            the id becomes a query
id = a#frag         -> /v1/executions/a                the rest is dropped before the wire
```

Five live breakages, none of them hypothetical, none needing a reverse proxy to reach.

**Dependencies already exist.** `url` and `percent-encoding` are in `Cargo.lock` as transitive
deps, so a direct dependency is one line, not vendoring.

## The contract, all of it in one new file

`apps/cli/src/commands/mcp/url.rs` owns joining, encoding and refusal. The three shared files get
**minimal substitutions only** — one expression in `client.rs`, the `format!` calls in `tools.rs`
replaced by builder calls, module wiring in `mod.rs`. That is the conflict-surface commitment made
in scope amendment 1, and it is verifiable from the diff rather than promised in prose.

Behaviour to define, since "correct URL joining" is not a specification:

| input | contract |
|---|---|
| base with trailing slash, base without | same result — the join is normalised, not concatenated |
| base with a path prefix (`https://host/graphhelm`) | prefix preserved; the API path appends to it |
| base carrying a query | merged, base's parameters kept |
| base carrying a fragment | **refused** — a fragment is never sent to a server, so a base with one is a configuration error, not a thing to strip silently |
| path segment containing `/`, `?`, `#`, space, `%` | percent-encoded as a segment, so it cannot change the endpoint |
| base that is not absolute, or not http/https | refused |

## Order of work, red first

**Where the tests can live, measured rather than assumed.** `apps/cli` is a binary-only crate with
no `[lib]`, so `apps/cli/tests/` can drive the built binary and nothing else — an integration test
cannot call a `pub(crate)` composer. The suite is therefore in two halves, and saying which is which
matters because "black-box" is a deliverable of this issue:

* the composition table lives as unit tests **inside `url.rs`**, the same pattern `is_loopback_url`
  already uses in `client.rs`;
* `apps/cli/tests/mcp_url.rs` is genuinely black-box: it runs `graphhelm mcp --url <base>` and
  asserts the CLI **refuses** the bases this contract refuses, before contacting anything. That half
  is cross-platform because it asserts on the process's own output, not on a socket.

1. Both halves written first, with the loopback-root positive control **first in report order** —
   the root URL the built-in package uses today must still compose byte-identically, or every
   refusal below is satisfied by a builder that refuses everything.
2. Observe each red land on its own case.
3. The smallest `url.rs` that turns them green.
4. Substitute at the shared call sites, then re-run the MCP suites.
5. `docs/operations/MCP_REVERSE_PROXY.md` last, describing what the tests already prove.

## Shared-file order, declared at birth

`tools.rs`, `client.rs` and `mod.rs` are shared with #213 (E). **E publishes first** — that lane is
already in flight; this one rebases onto their result and re-runs both suites. If the order inverts,
the rule inverts with it and is named in the PR.

**The union tool does not apply here and must not be reached for.** It rebuilds append-only lists
from main's blob. This is Rust with overlapping regions; the discipline is an ordinary rebase with
both suites green. Using it here would be trusting an instrument because it exists rather than
because it answers the question.

## What this will NOT establish

No claim that query encoding was broken — it was not, for a reason the types happened to enforce.
No change to what the MCP adapter is permitted to do: it stays a public Runtime API client, and
nothing here adds a surface, a credential path, or a second operational route.
