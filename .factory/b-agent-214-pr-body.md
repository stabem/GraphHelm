## What and why

The MCP adapter composed every request by concatenating strings. This gives it a contract: path segments and query values are encoded, bases are validated, and ambiguous ones are refused rather than repaired.

Closes #214

## This is a fix, not a precaution

The issue frames the work around future reverse-proxy deployments. Measuring first changed that: **the loopback check inspects only the authority** — it splits on `/`, `?`, `#` and reads the first part — so every shape below is accepted today, and the concatenation then produces:

```
http://127.0.0.1:8080/graphhelm  -> .../graphhelm/v1/executions/abc     correct
http://127.0.0.1:8080/?tenant=a  -> .../?tenant=a/v1/executions/abc     the API path lands INSIDE the query
http://127.0.0.1:8080/#frag      -> .../#frag/v1/executions/abc         everything after # is never sent
```

And the sharper half is not about proxies at all. `tools.rs` had **fifteen** `/v1` interpolations, **twelve** of them placing a caller-supplied id straight into a path segment. Those ids arrive in MCP tool-call arguments:

```
id = a/../../admin  -> /v1/executions/a/../../admin    a different endpoint
id = a?x=1          -> /v1/executions/a?x=1            the id becomes a query
id = a#frag         -> /v1/executions/a                the rest is dropped before the wire
```

Five live breakages, none hypothetical, none needing a reverse proxy to reach.

## The contract

| base | result |
|---|---|
| with or without a trailing slash | identical — normalised, not concatenated |
| with a path prefix | preserved; the API path appends to it |
| carrying a query | merged, base parameters first |
| carrying a fragment | **refused** |
| not absolute, or not http(s) | **refused** |
| a path segment containing `/`, `?`, `#`, `%`, space | percent-encoded, so it cannot change what the URL addresses |
| a query value containing `&`, `=`, `+`, `#`, space | percent-encoded under a **narrower** allowlist, because each of those is structural inside a query |
| a path segment that is entirely `.` or `..` | **refused** — an HTTP stack removes it before sending, so it would choose the endpoint |

**A fragment is refused rather than stripped.** It is never transmitted, so a base carrying one describes a request that cannot happen — and repairing it quietly would leave the configured URL and the requested URL different with nothing saying so.

**The encoder is an allowlist** of RFC 3986 `pchar`, not a denylist. A denylist claims its author enumerated every dangerous byte; an allowlist claims they enumerated the safe ones. An incomplete allowlist over-encodes and breaks loudly; an incomplete denylist under-encodes and reaches the wrong endpoint quietly.

## Nine reds, against stubs that were today's behaviour

Both stubs reproduced today's behaviour exactly -- the composer concatenated, the query encoder returned its input unchanged -- so each red demonstrated a live defect rather than an invented one. Two cases passed from the start — the loopback root, and a path prefix, which concatenation happens to get right — and those two are the positive control.

The control has **two halves** on purpose: today's URL must still compose byte-identically, *and* an ordinary id must survive encoding unchanged. Without the second, an over-aggressive encoder passes every hostile case and breaks every real call.

## Substitution, counted rather than eyeballed

**Sixteen** interpolations were migrated: thirteen path expressions (10 plain, 1 events path with a separately assembled query, 2 wake-lease reads) and three query values in the two gateway tools. A partial substitution is the worst outcome available here -- the sites left behind stay vulnerable while the suite reports a contract in force, which is exactly what happened on the first pass.

The guard that enforces it now runs over the **wide** population: any `format!` naming a `/v1` path that still interpolates a bare variable, anywhere in the file. Its first version ran over the narrow one I had defined, and that is how two sites survived it.

The minimal-diff commitment from scope amendment 1 is measurable from the diff rather than promised in prose:

```
client.rs   1 changed
mod.rs      4 added
tools.rs   20 added, 13 removed
```

## Two design decisions worth arguing

**No new dependency.** The workspace declares 213 dependencies through one table and zero directly, so a direct dep here would be the only one in the repository, and the alternative — editing the workspace table — is outside this issue's scope. The sibling module already hand-parses on `://`, `/`, `?` and `#` deliberately; what is needed here is scheme, authority presence, fragment absence and a query split, none of which is the part of URL handling that deserves a parser.

**A redundant check was deleted, not kept.** Extracting `validate_base` left a second copy of the authority check inside `join`. With it in place, breaking the real guard leaves `join`'s tests green — so no sabotage could show whether the guard still cuts. A redundant blade adds no protection; it adds blindness.

**Validation happens once at startup**, beside the loopback check and before any protocol byte. The same error surfacing per request would read as an intermittent fault rather than a configuration mistake.

## What the black-box half can assert, and what it cannot

`apps/cli` is binary-only with no `[lib]`, so an integration test drives the process and nothing else — the composition table is unit-tested next to the code, in the pattern `is_loopback_url` already uses.

And **refusals are ordered**: loopback runs first, so a bad scheme or a missing host is refused by *that* check and never reaches this contract. Asserting those in the black-box file would be asserting a message this change never wrote — a case passing for a neighbouring reason. The file says so at the top, covers the fragment shape end to end, and carries its own positive control, because a command that refused every `--url` would satisfy the fragment case perfectly.

## L's two findings, closed in the same round

**The contract was written in the wrong unit.** `.` belongs in the segment allowlist — real identifiers carry dots — so the encoder is byte-correct and byte-blind. But an HTTP stack removes `.` and `..` **segments** before sending, so a caller-supplied id of `..` still chooses the endpoint **without containing a single illegal byte**. The allowlist prices bytes; the threat lives in segments. That is this contract's own failure mode, one level above where it was written.

The refusal sits in `join` rather than `segment_path`, and the reason is coverage rather than taste: `join` is the single chokepoint every composed URL passes through, so one rule covers the thirteen call sites *and* a configured base prefix. In `segment_path` it would have needed a `Result` at thirteen closures and still missed the prefix. Its positive control is what stops the fix becoming a ban on the character — a dot *inside* a segment is ordinary content and still composes.

**Three of the four refusals quoted the operator's whole base**, and #214's own invariants say no credential reaches a diagnostic. L then narrowed this himself — the operations doc already promises the GraphHelm token never enters a URL — leaving a proxy credential the operator puts in their own base.

This goes further than the narrowed ask and **closes it rather than documenting it**: the redaction costs four lines and does not depend on anyone reading a caveat. Scheme and host only, with **userinfo stripped as well**, because `user:password@` keeps the secret in the authority and "scheme plus authority" would still print it. An unparseable base has nothing safe to quote and is described instead.

Both sides of that tension carry their own case: one asserts the secret does not escape, the other asserts the refusal still names the host — because a refusal that said nothing would pass the first perfectly and leave the operator guessing which flag is wrong.

## What this does NOT establish

**An earlier revision of this PR claimed query values were "safe by type" and needed no encoder. That was false, and the retraction is the most useful thing in this description.**

Raised by L, who counted the wide population from the blob and asked why the PR said thirteen substituted sites when the file held fifteen `format!("/v1…` expressions. The two outside my pattern were `routes?manifest={manifest}` and `probe?route={route}` — both **caller-supplied strings**, declared `{"type": "string"}` in the tool schema, interpolated into a query with no encoding. A value carrying `&` appended a parameter the caller never wrote; one carrying `#` truncated the request before it left the process. Live query injection, not hypothetical.

`query_value` now encodes them, with a **narrower** allowlist than path segments — only `unreserved`, because inside a query `&`, `=`, `+` and `#` are all structural. Three reds first, against a stub that returned the value unchanged, plus a positive control so an ordinary value still passes through untouched.

**How the false claim survived to be written in three places is the part worth keeping.** The substitution carried a guard that refused to finish unless every un-migrated site was gone — and it ran to completion, exhaustively, over *the population it was given*: `format!("/v1/executions/{…}")`. **The population was the instrument.** The guard did not fail; it answered a narrow question with precision, and the question was mine.

The warning was already on my screen. An early measurement printed `format!("/v1` = 15 next to `/v1/executions/{id}` = 12, and I carried 13 forward without reconciling them. Two numbers answering the same question belong side by side, where the layout forces the comparison rather than leaving it to attention.

**Nothing here loosens the loopback rule**, and the adapter remains a public Runtime API client rather than a second operational path.

## Validation evidence

Base is `origin/main` at `8ba96a2`, merged in; ED-18's subject is the merge result.

```
cargo check --workspace --all-targets --locked   exit 0
cargo fmt -p graphhelm-cli -- --check            clean
cargo clippy -p graphhelm-cli -D warnings        0 errors

commands::mcp (unit)   23 passed     mcp_url    2 passed     mcp_stdio   19 passed
                                                    total    44 passed, 0 failed
```

The sweep fingerprints the tree before and after and reports contamination itself; this run reported it unmoved.

**`fmt` is in that sweep because of this lane's own miss.** #290's ED-18 ran check, clippy and tests, all green, and omitted the one stage that would have caught a formatting defect — which then landed on main and was hotfixed in #296. A sweep is a claim about what was verified, and the stages it omits are invisible in its output. The new stage caught its first defect on its first run, in this change.

## Shared files

`tools.rs`, `client.rs` and `mod.rs` are shared with #213. The order was declared at birth: **E publishes first**, this branch rebases onto their result and re-runs both suites; if the order inverts, the rule inverts with it.

**The union tool does not apply here.** It rebuilds append-only lists from main's blob; this is Rust with overlapping regions, and the discipline is an ordinary rebase with both suites green. Using it here would be trusting an instrument because it exists rather than because it answers the question.

## Security review

No new authority, no new surface, no dependency. **No credential appears in a URL**: the token is read from `--token-file` or the environment, travels in the `Authorization` header, and appears in no refusal message — including the ones that quote the operator's base URL back to them.

The change closes a path-traversal-shaped hole: before it, an id from a tool-call argument could redirect a request to a different endpoint on the Runtime API.

## Rollback

Revert the commits. The composition returns to concatenation and the five breakages return with it, so this is not a revert to reach for casually — the state restored is one where a tool-call argument can choose which endpoint the adapter reaches.
