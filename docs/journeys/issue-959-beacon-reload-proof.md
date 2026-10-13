# Issue 959: beacon reload evidence

## Promise and decision rule

For the bus-only `gh-team` fixture run, a fresh Studio load must never show a lit owner-decision beacon or a phantom decision card. The accepted initial state is `Checking recorded agent work`; after the first WORK rail appears, the beacon should become `Nothing needs you` within about five seconds. A real owner question and a non-fixture wait must remain visible. Three independent read-only fresh loads of an identified build and store are required before recommending closure of #959.

This record separates prior observations from the three-load check reported by the [#1022 visual observer](https://github.com/stabem/GraphHelm/pull/1022#issuecomment-6136270370).

## Earlier RED and failed attempts

| Source | Observation | Result |
| --- | --- | --- |
| [#960 visual BLOCK on `05a7b736`](https://github.com/stabem/GraphHelm/pull/960#issuecomment-6130236988) | Two live-store reloads showed `1 decision needs you` and a phantom `Answer in chat` card. The status route omitted the proposed `fixture_scripted` reason. | Failed; this head was not merged. |
| [#960 visual BLOCK on `a5f60149`](https://github.com/stabem/GraphHelm/pull/960#issuecomment-6130706960) and [correction](https://github.com/stabem/GraphHelm/pull/960#issuecomment-6130712682) | The beacon lit for roughly 24 seconds before becoming dark; three manual reloads lit. The delay was tied to stored work messages, not the task-history read. | Failed; these three reloads do not prove the final head. |
| [#960 final visual check on `9e0ad987`](https://github.com/stabem/GraphHelm/pull/960#issuecomment-6131147684) | One 60-second sample showed `Checking recorded agent work` at 1.8 seconds and `Nothing needs you` at 16.0 seconds, without a lit beacon. | Passed that single sample; three final-head reloads remained unobserved. |
| [#959 follow-up on live `cd589ce8`](https://github.com/stabem/GraphHelm/issues/959#issuecomment-6133490804) | Three loads first showed checking at 2.4, 7.4 and 4.7 seconds. Load 1 became dark at 39 seconds; loads 2 and 3 still checked at 50 seconds. None showed the phantom lit beacon. | Failed the prompt-dark expectation; this preceded #996. |

The two [#996 author worker-start failures described in the latest card](https://github.com/stabem/GraphHelm/issues/959#issuecomment-6134965068) ran zero tests. They are incomplete runs, not a RED test. Original screenshots, DOM captures and hashes are not supplied in the linked comments; this record does not invent them.

## Delivered code and existing controls

[#960](https://github.com/stabem/GraphHelm/pull/960) merged as `ca183d96bf2732452366b771852ce78c8cfc7358`. It replaced the transient phantom decision with the checking state while agent evidence was pending. [#996](https://github.com/stabem/GraphHelm/pull/996) merged as `3739b75f581028eac5f40897a56c0610e0dc7dbe`. It derives recorded work from the loaded agent `signal_recorded` events and changes the waiting-step action to `Open run conversation`.

The [#996 reviewer](https://github.com/stabem/GraphHelm/pull/996#issuecomment-6134692118) reported 210 passing Vitest tests and passing TypeScript on `a90ac5a9`; its 271.031-second receipt used a 7200-second budget and does not meet the 180-second feedback target. The existing `App.test.tsx` deferred-evidence case and `needs-you.test.ts` question and non-fixture controls remain the offline checks. These are historical results, not tests run for this document.

The [#996 visual observer](https://github.com/stabem/GraphHelm/pull/996#issuecomment-6134912707) saw a connect screen at 1.8 seconds, then `Nothing needs you` at 8.8 seconds on a build of `a90ac5a9` against the live store. Desktop and mobile layouts were checked. The comment does not report three independent fresh-load timelines, the first WORK-rail time, screenshot hashes, or the served source of the owner's `:5197` instance. It also says no live owner question was available and the conversation action was not exercised.

## Three fresh loads of the owner's Studio

The [#1022 visual observer](https://github.com/stabem/GraphHelm/pull/1022#issuecomment-6136270370) made three independent read-only navigations on the owner's `:5197` Studio at 1280 px, polling every 200 ms for 42 seconds from navigation start. The served `wt-studio` main source was clean at `0f510bfe`; the `gh-team` store head was 6635 on the first load and at least 6635 on the later loads. The observer did not send, answer, approve, or seed anything. Screenshot files and a separate CSS-state trace were not supplied in the comment; the screenshot observations for loads 2 and 3 remain in the observer's session.

| Fresh load | URL / served SHA / store head | WORK rail | Beacon transitions and dark-minus-rail time | Phantom card | Screenshot / DOM hashes | Result |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Owner's `:5197`, `?session=2de63856…` / `0f510bfe` / 6635 | 4.5 s | First and only state `Nothing needs you` at 2.8 s; never lit; dark − rail = −1.7 s | None; `Answer in chat` never appeared | No hash recorded | Partial: dark and no phantom; checking state absent |
| 2 | Same URL and source / store head ≥6635 | 3.7 s | `Nothing needs you` at 2.0 s; never lit; dark − rail = −1.7 s | None | DOM text SHA-256 prefix `f880a8a5e74da48b`; screenshot in observer session | Partial: dark and no phantom; checking state absent |
| 3 | Same URL and source / store head ≥6635 | 3.4 s | `Nothing needs you` at 2.5 s; never lit; dark − rail = −0.9 s | None | DOM text SHA-256 prefix `ebd719bcb6fdee9b`; screenshot in observer session | Partial: dark and no phantom; checking state absent |

**Result: partial; the accepted three-load criterion was not met.** The first beacon text in all three loads was already `Nothing needs you`, before the WORK rail; none showed the required `Checking recorded agent work` transition. No load lit a phantom decision or showed an `Answer in chat` card, and all were dark within the approximately five-second after-rail bound. These observations support the no-phantom and prompt-dark parts only; they do not justify a closure recommendation under the latest #959 Keel card. The `Open run conversation` action and unopened-question behavior remain separate and were not exercised by these loads.
