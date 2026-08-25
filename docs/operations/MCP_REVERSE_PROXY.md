# Deploying the MCP adapter behind a reverse proxy

The MCP adapter is a client of the Public Runtime API. It composes every request from a **base URL**
you configure with `--url` and an API path it builds itself. This page states what that composition
guarantees, because "the URL is joined correctly" is not a specification.

Everything below is enforced by tests in `apps/cli/src/commands/mcp/url.rs` and
`apps/cli/tests/mcp_url.rs`. Where a rule is not enforced, this page says so rather than implying it.

## The base URL contract

| base you configure | what the adapter does |
|---|---|
| `http://127.0.0.1:8080` | requests land at `/v1/...` |
| `http://127.0.0.1:8080/` | identical — the trailing slash makes no difference |
| `http://127.0.0.1:8080/graphhelm` | prefix preserved: requests land at `/graphhelm/v1/...` |
| `http://127.0.0.1:8080/graphhelm/` | identical to the line above |
| `http://127.0.0.1:8080/?tenant=a` | `tenant=a` is kept and travels on every request, ahead of any parameter the call adds |
| `http://127.0.0.1:8080/#anything` | **refused at startup** |
| `ftp://…`, `127.0.0.1:8080`, `/v1` | **refused at startup** |

### Why a fragment is refused rather than cleaned

A fragment is never transmitted to a server — it is a client-side concept. A base carrying one
therefore describes a request that cannot happen. The adapter refuses it instead of stripping it,
because stripping it silently would leave the URL you configured and the URL actually requested
different from each other, with nothing saying so.

The refusal happens **at startup**, beside the loopback check and before any protocol byte. A
configuration mistake that surfaced once per request would read as an intermittent fault.

### Refusal order matters when you are reading an error

`build_client` checks **loopback first**. A base with a bad scheme, or without a host, is refused by
that check and its message names loopback rather than the URL contract. This is not a bug and the
tests are written to respect it: asserting the contract's message for an input the loopback check
rejects would be asserting something this code never says.

## Path segments are encoded, and why that is the security-relevant half

Every API path is built from segments, and each segment is percent-encoded before it is joined. This
matters because segments carry **caller-supplied identifiers** that arrive in MCP tool-call
arguments:

| an id containing | before this contract | now |
|---|---|---|
| `a/../../admin` | the request reached a different endpoint | encoded to one segment |
| `a?x=1` | the id opened a query the caller never wrote | encoded to one segment |
| `a#frag` | the request was truncated before it left the process | encoded to one segment |

The encoder is an **allowlist** of RFC 3986 `pchar`, not a denylist of dangerous bytes. A denylist
is a claim that its author enumerated every dangerous byte; an allowlist is a claim they enumerated
the safe ones. An incomplete allowlist over-encodes and breaks visibly. An incomplete denylist
under-encodes and reaches the wrong endpoint quietly.

## What this does NOT cover

**Query values are encoded, and an earlier draft of this page said they did not need to be.**

That earlier claim — that query values were "safe by type" because the adapter built only `after=`
and `limit=`, both read as integers — was **false**, and it is retracted here rather than quietly
edited away. Two gateway tools interpolate caller-supplied **strings** into a query: `routes` takes
a `manifest` argument and `probe` takes a `route`, both declared `{"type": "string"}` in the tool
schema. A value containing `&` appended a parameter the caller never wrote; one containing `#`
truncated the request before it left the process.

Query values are now percent-encoded by the same contract that encodes path segments, with a
narrower allowlist: `&`, `=`, `#`, `+` and space are all encoded, because each of them means
something structural inside a query.

**How the false claim survived long enough to be written down three times** is worth more than the
fix. The substitution that routed paths through this contract carried a guard that refused to
finish if any un-migrated site remained — and it ran to completion, exhaustively, over the
population *it had been given*. That population was `format!("/v1/executions/{…}")`. The file held
fifteen `/v1` sites, not thirteen, and the two outside the pattern were exactly the two that put
strings into a query. The guard did not fail; it answered a narrow question precisely.

**Nothing here loosens the loopback rule.** The adapter still refuses any base that does not name a
loopback authority. A reverse proxy in front of the Runtime is deployed on the same host as far as
this client is concerned; exposing the Runtime API beyond loopback is a different decision, made
elsewhere, and this page is not it.

**No GraphHelm credential ever appears in a URL.** The token is read from `--token-file` or
`GRAPHHELM_API_TOKEN` and travels in the `Authorization` header. It is not accepted in argv and is
not placed in a query parameter.

**That guarantee is about the GraphHelm token, and your base URL is a different thing.** If your
proxy needs its own credential and you put it in the base — `http://127.0.0.1:8080/gh?proxy_token=…`,
or as `user:password@` — that value is yours, not ours, and no promise above covers it.

So the refusals on this page **do not quote your base back to you**. They name the scheme and the
host and nothing else: not the path, not the query, not the userinfo. A refusal still tells you
which host is misconfigured, which is what you need to act, and it cannot carry a proxy credential
into a log on its way there.

This is narrower than "we never leak secrets" and that is deliberate: the honest statement is that
the one credential this adapter owns never enters a URL, and the one you supply is kept out of the
messages it prints.

## Configuring a prefix

Point `--url` at the prefix your proxy serves:

```
graphhelm mcp --url http://127.0.0.1:8080/graphhelm --actor my-chat --token-file /run/secrets/token
```

Your proxy should forward `/graphhelm/v1/...` to the Runtime's `/v1/...`. The adapter never strips
the prefix, so whatever you configure is what the proxy receives.
