# Deterministic journey browser driver

This observer executes approved semantic actions. It has no model executor, gateway lease,
provider SDK, event key, or GraphHelm recording API. The replay supervisor owns recording and
process containment. The Node process is not an OS sandbox.

The standalone driver requires Node.js 24 or later and the tested project's `@playwright/test`
with Chromium installed explicitly. Resolve that package from the project's `package.json`,
not from this checkout. The companion installed location is
`.graphhelm/observers/journey_driver.mjs`; explicit observer setup owns installation.

```text
node journey_driver.mjs --project <project-root> --output-dir <supervisor-owned-directory>
```

## Closed wire protocol

One outstanding request, one UTF-8 JSON object plus LF per line, at most 65,536 bytes. Every
request has `protocol: "graphhelm-journey-driver/1"`, `requestId` (integer starting at 1,
increasing by 1), and `op`. Unknown fields, invalid UTF-8, partial EOF, sequence mismatch,
oversized frames, and unsupported operations are refused before browser I/O. Stdout carries
only replies. No raw exception, input, stack, or secret is logged to stderr.

| `op` | Additional request fields | `result` on success |
|---|---|---|
| `open` | `base`, `viewport: {width,height}`, `allowOrigins: []`; optional `headed: boolean` (default false), `coverage: boolean` (default false), `webmcp: boolean` (default false; launches the browser with the WebMCP feature, #2837) | `url` |
| `snapshot` | `expect: [{role,name}]` (0–8) | `url`, `ariaYaml`, `fingerprint`, `controls` |
| `act` | `kind`, `role`, `name`; `text` or `secretEnv` for `enter_text`; for `tool`, `role: webmcp` and optional `arguments` (a JSON object of at most 4096 bytes of compact `JSON.stringify` and 8 levels (the flow validator measures its own, longer text form, so a flow it passes fits here); never `text` or a secret); optional `locator` | `url`, `locator`; a `tool` act also `toolResult` |
| `capture` | relative `.png` `path`, `maskSecrets: true` | relative `path`, `width`, `height`, `masked: true` (secrets are masked; see Privacy) |
| `coverage` | none; only after `open`, once per session | `path: "coverage.json"`, `schema: "graphhelm-js-coverage/1"` |
| `close` | none | `closed: true` |

Success is `{protocol,requestId,ok:true,result}`. Failure is
`{protocol,requestId,ok:false,code,path:"/"}`. Rust maps that failure to the approved action's
stable path. EOF without `close` is a protocol failure and releases the browser. A failed
operation terminates the session; it cannot be retried as if an action had not happened.
Exception (#398, `journey open`): a session opened with `headed: true` survives
`driver.locator_missing`, `driver.locator_ambiguous`, `driver.expectation_failed`,
`driver.action_failed` and `driver.timeout`, so the visible browser stays where the journey
broke. Every other failure (protocol, privacy, host, capture) still ends a headed session.

Codes include `driver.protocol_invalid`, `driver.frame_too_large`, `driver.observer_missing`,
`driver.secret_missing`, `driver.secret_literal`, `driver.redaction_failed`,
`driver.locator_missing`, `driver.locator_ambiguous`, `driver.expectation_failed`,
`driver.host_refused`, `driver.unsupported_act`, `driver.snapshot_too_large`,
`driver.capture_refused`, `driver.timeout`, `driver.action_failed`, and, for the `tool` act (#2837),
`driver.tool_missing`, `driver.tool_ambiguous`, `driver.tool_failed` and `driver.webmcp_unavailable`.

When `open` includes `coverage: true`, the Chromium JavaScript coverage observer starts before
the initial navigation. `coverage` stops it once and writes an exclusively-created,
supervisor-owned `coverage.json`. The bounded artifact uses schema `graphhelm-js-coverage/1` and
reports `collection: "captured"`, `navigationCount`, `negativeEvidenceEligible`, `complete`, and machine-readable
`completeness`, alongside generated script SHA-256 hashes, generated function ranges and counts,
and hashes of inline source-map `sourcesContent` only. It never stores source text, source
map text, source-map paths, hosts, queries, or secrets. A missing or unsupported coverage API is
reported as `driver.observer_missing`; a session opened without coverage produces an explicit
`collection: "unavailable"` artifact rather than zero coverage. This is page JavaScript evidence:
backend, CSS, worker coverage, and source attribution beyond the recorded hashes remain unknown.
Main-frame navigations after the initial load are recorded as a limitation because Chromium may
lose coverage data across them.

## Browser and locator boundaries

Each session opens a fresh nonpersistent context, blocks service workers, and registers
context-wide HTTP and WebSocket routes before creating a page. Initial/navigation destinations
stay on the local base origin. Extra allowed origins permit subresources/WebSockets only.
Hosts are `localhost`, `127.0.0.1`, `[::1]`, `*.localhost`, or `*.test`; userinfo, encoded hosts,
lookalikes, and non-HTTP URLs are refused. Chromium resolution and the separate HTTP transport
both pin local aliases to loopback. Arbitrary `.test` DNS is never trusted.

**HTTP redirect chains are refused in v1**, including local redirects. Playwright routes only
the first request of a chain; releasing a local first hop can otherwise leak a later remote
request. The driver fetches each initial response with redirects disabled and refuses redirect
responses before releasing them. Ordinary same-origin links and JavaScript navigation work.
An application needing HTTP redirect chains has an unresolved capability with this driver.

All matches use exact accessible names and must be unique and visible. Cache tiers are test
ID, unique nearest-landmark context (`role "name"`), then global role/name. Only absence may
fall back; ambiguity and a mismatched test-ID hit fail. `exact` must be true and `nth` null.
There is no first-match selection or generic JavaScript-evaluation request.

A `tool` act calls a page tool: in the page, from `document.modelContext` (or `navigator.modelContext`),
it keeps the tools whose `name` equals the act's name exactly and calls the one that is left with
the object in `arguments` (`{}` when absent). No such tool is `driver.tool_missing`, more than one is
`driver.tool_ambiguous` (no first match, as for locators), a handler that throws is
`driver.tool_failed`, and a browser without WebMCP is `driver.webmcp_unavailable` (the observer
cannot do this act; the replay maps it to a missing observer, never a pass). The reply's `locator`
is `{role: "webmcp", name, ...}`, so the cache keeps one entry per act, and `toolResult` is what the
tool returned as text, masked and cut to 512 bytes, for the reader only: it never judges a step, the
snapshot of the reached screen does. A page tool runs the page's own code; the browser has
`--enable-features=WebMCP` only in a session opened with `webmcp: true`, which the Rust callers send
only for a flow that has a `tool` act.

`activate`, `submit`, and edge `navigate` click the named control. `enter_text` fills its
explicit text or named secret. `wait_for` waits for its exact control to become visible; `inspect` observes it immediately. `select`,
`upload`, `download`, `recover`, and `approve` require later resource/value/recovery policy
and are refused. Viewports are bounded to 16,384 per dimension and 16,777,216 total pixels.

## Privacy and deterministic cache facts

Only referenced `GRAPHHELM_SECRET_<NAME>` values belong in the driver's environment. The
supervisor filters inherited environment; browser requests never receive event/provider keys.
Missing/empty named secrets and literals equal to supplied secrets fail before action.
Known values are redacted from URLs/ARIA/cache before serialization, and outputs are rescanned.
Capture paints only secrets, in opaque magenta (`#FF00FF`) before PNG creation: password
fields (`input[type=password]`), fields the flow filled from a `secret`, fields whose
`autocomplete` names a credential or payment value (`current-password`, `new-password`,
`one-time-code`, `cc-number`, `cc-csc`, `cc-exp`, `cc-exp-month`, `cc-exp-year`; a guard for a
secret typed into a `type=text` field), any editable field that shows a known secret (see below), and any visible
non-editable text equal to a known secret (`getByText`, as the browser matches it). Every editable field (`input`, `textarea`, `[contenteditable="true"]`), painted or not,
gets a 2px solid magenta outline with 1px offset instead of a cover, so a frame shows what a
plain field holds (a search box, an e-mail address). The outline is set inline for the
screenshot only and put back exactly as it was; it takes no space and moves nothing. Accepted
trade (#2848): a value typed into a plain field is visible in the kept frame; a flow that
handles something sensitive must name it a `secret`. `masked: true` means secrets are masked,
not that every input is.

Author decisions on top of the approved rule (#2848 review, labelled as such). An editable field (`input`, `textarea`, `[contenteditable="true"]`) shows a known secret when what it draws (its `value`; its `placeholder` while the value is empty, since a placeholder is drawn only then; the text for a contenteditable) and a declared secret, each folded to lowercase letters and digits only (every other character dropped), share a contiguous run of at least min(4, folded secret length) characters. That one rule covers an exact copy, other case, spacing, separators (`4242-4242-4242-4242`, `sk_PROBE_123456`), an ellipsis at either end and a label plus a cut (`key: sk-PROBE-12…`). A folded secret of length 0 never matches. Second condition: a field whose whole folded drawn text (non-empty) is a contiguous part of a folded declared secret is painted too, at any length down to one character, so a secret split across boxes (six one-digit code boxes, pairs, 3-character pieces) is painted piece by piece without looking at neighbours. That over-paints short plain fields in a flow that declares secrets (a search box showing `re` is painted when a secret contains `re`); a flow that declares no secret paints nothing by this rule. Over-painting is the accepted direction: a plain value that shares 4 letters or digits with a secret is painted too (for example the value `be 2848` for a secret ending `probe-2848`). Field text is compared up to 4096 characters; a longer field is painted, not skipped. Such a field is painted. The paint is computed for the editable fields read in one pass and applied to those very elements (a temporary `data-graphhelm-paint` attribute, removed with the outline), never by re-resolving a position. A field whose value cannot be read is painted. Around the screenshot the editable-field list is taken again, and the same read is repeated: if the list differs, or any field that was outlined now shows a secret (a page wrote it in after the read), the whole read-and-capture is repeated once on the new DOM, and a second difference refuses with `driver.capture_refused` and keeps no image. One retry is the bound: a page that keeps changing cannot be given a mask known to fit it, and an endless retry could hang a flow; a refusal is the safe failure because the flow fails loudly where an unpainted secret would pass silently. Known limits, stated and not solved: a page that overrides the value getter to lie can hide a value from this check; a transformed echo (upper-cased, truncated) in a non-editable element is matched only as `getByText` matches it, as before this change; and a piece of a secret shown together with other characters in a field, where the shared run is under 4 (for example `x48`); a value the page draws that is not what the field holds (a masked or formatted overlay) is outside what this reads. Entire rendered iframe/frame elements are also covered, including
isolated/cross-origin frames; their contents are intentionally unobserved in
the image. Main-document locators cannot establish privacy inside a frame. Images are temporary inputs to sealing, not a public screenshot
output. Output directories/targets reject symlinks and existing files.
If the enumerated browser frames do not match the frame hosts reachable by page selectors
(for example, nested documents or closed shadow roots), capture refuses with
`driver.capture_refused` rather than claiming `masked: true`.

ARIA is limited without truncation: 32 KiB for a replay snapshot (a real page, well inside the
64 KiB frame; #434), 6 KiB for a `discover` snapshot, which feeds a model. Fingerprints hash compact JSON
`{"controls":[...],"lists":[...]}`: sorted unique landmark/heading/interactive role/name
pairs, free text discarded, number runs replaced by `#`, and repeated list sizes bucketed as
`1`, `2–5`, `6+`. Exact names remain separate locator facts. No similarity/drift/healing
threshold is implemented. These are observations, not generic JPD acceptance receipts.

## Observer commands

```powershell
node --test tools/journey-driver/driver.test.mjs
$env:GRAPHHELM_JOURNEY_TOOLCHAIN_PROJECT = '<explicit-validation-project>'
node --test tools/journey-driver/driver.browser.test.mjs
```

The first command needs only Node and runs no browser. The second fails explicitly if its
declared validation toolchain is missing. It observes real exact-name controls, text/PNG secret
masking (painted secrets, outlined plain fields), a live network canary, redirect-chain refusal, and loopback alias resolution. No test
downloads a browser or calls a provider. An unavailable browser is `OBSERVER_MISSING`.
