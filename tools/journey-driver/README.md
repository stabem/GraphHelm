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
| `open` | `base`, `viewport: {width,height}`, `allowOrigins: []` | `url` |
| `snapshot` | `expect: [{role,name}]` (0–8) | `url`, `ariaYaml`, `fingerprint`, `controls` |
| `act` | `kind`, `role`, `name`; `text` or `secretEnv` for `enter_text`; optional `locator` | `url`, `locator` |
| `capture` | relative `.png` `path`, `maskSecrets: true` | relative `path`, `width`, `height`, `masked: true` |
| `close` | none | `closed: true` |

Success is `{protocol,requestId,ok:true,result}`. Failure is
`{protocol,requestId,ok:false,code,path:"/"}`. Rust maps that failure to the approved action's
stable path. EOF without `close` is a protocol failure and releases the browser. A failed
operation terminates the session; it cannot be retried as if an action had not happened.

Codes include `driver.protocol_invalid`, `driver.frame_too_large`, `driver.observer_missing`,
`driver.secret_missing`, `driver.secret_literal`, `driver.redaction_failed`,
`driver.locator_missing`, `driver.locator_ambiguous`, `driver.expectation_failed`,
`driver.host_refused`, `driver.unsupported_act`, `driver.snapshot_too_large`,
`driver.capture_refused`, `driver.timeout`, and `driver.action_failed`.

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

`activate`, `submit`, and edge `navigate` click the named control. `enter_text` fills its
explicit text or named secret. `wait_for` and `inspect` observe the named control. `select`,
`upload`, `download`, `recover`, and `approve` require later resource/value/recovery policy
and are refused. Viewports are bounded to 16,384 per dimension and 16,777,216 total pixels.

## Privacy and deterministic cache facts

Only referenced `GRAPHHELM_SECRET_<NAME>` values belong in the driver's environment. The
supervisor filters inherited environment; browser requests never receive event/provider keys.
Missing/empty named secrets and literals equal to supplied secrets fail before action.
Known values are redacted from URLs/ARIA/cache before serialization, and outputs are rescanned.
Capture masks all editable inputs, filled controls, and visible secret echoes with opaque
magenta before PNG creation. Images are temporary inputs to sealing, not a public screenshot
output. Output directories/targets reject symlinks and existing files.

ARIA is limited to 6 KiB without truncation. Fingerprints hash compact JSON
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
masking, a live network canary, redirect-chain refusal, and loopback alias resolution. No test
downloads a browser or calls a provider. An unavailable browser is `OBSERVER_MISSING`.
